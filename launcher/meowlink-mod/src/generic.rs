//! Thin native adapter for the frozen MEOWSIMPLE/1 contract.
//! No platform accounts, HTTP fallback, database access or credential logging.
use crate::{error, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
};
use tokio_rustls::{client::TlsStream, TlsConnector};
use zeroize::Zeroizing;

const MAGIC: &[u8] = b"MEOWSIMPLE/1\n";
const LIMIT: usize = 4096;
fn rejected() -> crate::AppError {
    error(
        "simple_rejected",
        "注册或登录未完成，请检查服务器与账号信息。",
    )
}
fn invalid() -> crate::AppError {
    error("simple_protocol", "服务器认证响应无效，未进入游戏。")
}

#[derive(Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub server_name: String,
    pub authority_id: String,
    pub realm_id: String,
    pub save_profile_id: String,
    /// Optional DER CA certificate. Never a private key or a TLS bypass.
    #[serde(default)]
    pub ca_certificate: Option<String>,
}
impl Target {
    pub fn validate(&self) -> Result<()> {
        if self.port == 0
            || self.host.is_empty()
            || self.host.len() > 253
            || self
                .host
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || "/\\?@".contains(c))
        {
            return Err(error("simple_config", "通用版服务器地址无效。"));
        }
        rustls::pki_types::ServerName::try_from(self.server_name.clone())
            .map_err(|_| error("simple_config", "TLS 服务器名称无效。"))?;
        for field in [&self.authority_id, &self.realm_id, &self.save_profile_id] {
            if field.is_empty() || field.len() > 256 || field.chars().any(char::is_control) {
                return Err(error("simple_config", "服务器身份配置不完整。"));
            }
        }
        if let Some(path) = &self.ca_certificate {
            if !Path::new(path).is_absolute() || path.len() > 4096 {
                return Err(error("simple_config", "CA 证书须为本地绝对路径。"));
            }
        }
        Ok(())
    }
    fn tls(&self) -> Result<TlsConnector> {
        self.validate()?;
        let mut roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        if let Some(path) = &self.ca_certificate {
            let mut file = std::fs::File::open(path)
                .map_err(|_| error("simple_config", "无法读取服务器 CA 证书。"))?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut std::io::Read::take(&mut file, 65537), &mut bytes)
                .map_err(|_| rejected())?;
            if bytes.is_empty() || bytes.len() > 65536 {
                return Err(error("simple_config", "服务器 CA 证书大小无效。"));
            }
            roots
                .add(rustls::pki_types::CertificateDer::from(bytes))
                .map_err(|_| error("simple_config", "服务器 CA 证书须为有效 DER 格式。"))?;
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| rejected())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(TlsConnector::from(Arc::new(config)))
    }
    pub(crate) async fn connect(&self) -> Result<TlsStream<TcpStream>> {
        let connector = self.tls()?;
        let tcp = TcpStream::connect((self.host.as_str(), self.port))
            .await
            .map_err(|_| rejected())?;
        let name = rustls::pki_types::ServerName::try_from(self.server_name.clone())
            .map_err(|_| rejected())?;
        connector.connect(name, tcp).await.map_err(|_| rejected())
    }
}
pub fn validate_credentials(username: &str, password: &str) -> Result<()> {
    if username.is_empty()
        || username.len() > 32
        || !username
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err(error(
            "simple_input",
            "用户名须为 1–32 位英文字母、数字、下划线或连字符。",
        ));
    }
    if !(1..=72).contains(&password.len()) {
        return Err(error("simple_input", "密码须为 1–72 个 UTF-8 字节。"));
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Challenge {
    r#type: String,
    mode: String,
    authority_id: String,
    realm_id: String,
    save_profile_id: String,
    connection_id: String,
    nonce: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registered {
    r#type: String,
    mode: String,
    authority_id: String,
    realm_id: String,
    save_profile_id: String,
    connection_id: String,
    registered: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    r#type: String,
    mode: String,
    authority_id: String,
    realm_id: String,
    save_profile_id: String,
    connection_id: String,
    ready: bool,
    native_account_id: String,
    username: String,
}
#[derive(Serialize)]
struct Credentials<'a> {
    r#type: &'static str,
    operation: &'static str,
    username: &'a str,
    password: &'a str,
    nonce: &'a str,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub authority_id: String,
    pub realm_id: String,
    pub save_profile_id: String,
    pub connection_id: String,
    pub native_account_id: String,
    pub username: String,
}
pub struct Session {
    pub identity: Identity,
    pub stream: TlsStream<TcpStream>,
}

async fn frame<S: AsyncRead + Unpin, T: DeserializeOwned>(stream: &mut S) -> Result<T> {
    let size = stream.read_u32().await.map_err(|_| rejected())? as usize;
    if size == 0 || size > LIMIT {
        return Err(invalid());
    }
    let mut data = Zeroizing::new(vec![0u8; size]);
    stream.read_exact(&mut data).await.map_err(|_| rejected())?;
    // Derived structs reject duplicate fields as well as unknown fields.
    serde_json::from_slice(&data).map_err(|_| invalid())
}
fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn scope(target: &Target, mode: &str, authority: &str, realm: &str, profile: &str) -> bool {
    mode == "simple"
        && authority == target.authority_id
        && realm == target.realm_id
        && profile == target.save_profile_id
}
async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    target: &Target,
    operation: &'static str,
    username: &str,
    password: &str,
) -> Result<Option<Identity>> {
    validate_credentials(username, password)?;
    let mut magic = [0u8; 13];
    stream
        .read_exact(&mut magic[..MAGIC.len()])
        .await
        .map_err(|_| rejected())?;
    if &magic[..MAGIC.len()] != MAGIC {
        return Err(invalid());
    }
    let challenge: Challenge = frame(stream).await?;
    if challenge.r#type != "simple-challenge-v1"
        || !scope(
            target,
            &challenge.mode,
            &challenge.authority_id,
            &challenge.realm_id,
            &challenge.save_profile_id,
        )
        || !hex(&challenge.connection_id, 32)
        || !hex(&challenge.nonce, 64)
    {
        return Err(invalid());
    }
    let payload = Zeroizing::new(
        serde_json::to_vec(&Credentials {
            r#type: "simple-credentials-v1",
            operation,
            username,
            password,
            nonce: &challenge.nonce,
        })
        .map_err(|_| invalid())?,
    );
    if payload.len() > LIMIT {
        return Err(invalid());
    }
    stream
        .write_u32(payload.len() as u32)
        .await
        .map_err(|_| rejected())?;
    stream.write_all(&payload).await.map_err(|_| rejected())?;
    stream.flush().await.map_err(|_| rejected())?;
    drop(payload);
    if operation == "register" {
        let response: Registered = frame(stream).await?;
        if response.r#type != "simple-registered-v1"
            || !scope(
                target,
                &response.mode,
                &response.authority_id,
                &response.realm_id,
                &response.save_profile_id,
            )
            || response.connection_id != challenge.connection_id
            || !response.registered
        {
            return Err(invalid());
        }
        if stream.read(&mut [0u8; 1]).await.map_err(|_| rejected())? != 0 {
            return Err(invalid());
        }
        return Ok(None);
    }
    let response: Ready = frame(stream).await?;
    let id = &response.native_account_id;
    if response.r#type != "simple-ready-v1"
        || !scope(
            target,
            &response.mode,
            &response.authority_id,
            &response.realm_id,
            &response.save_profile_id,
        )
        || response.connection_id != challenge.connection_id
        || !response.ready
        || response.username != username
        || id.starts_with('0')
        || id.is_empty()
        || !id.bytes().all(|c| c.is_ascii_digit())
        || !id.parse::<i64>().is_ok_and(|n| n > 0)
    {
        return Err(invalid());
    }
    Ok(Some(Identity {
        authority_id: response.authority_id,
        realm_id: response.realm_id,
        save_profile_id: response.save_profile_id,
        connection_id: response.connection_id,
        native_account_id: response.native_account_id,
        username: response.username,
    }))
}
pub async fn register(target: &Target, username: &str, password: Zeroizing<String>) -> Result<()> {
    validate_credentials(username, &password)?;
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut stream = target.connect().await?;
        authenticate(&mut stream, target, "register", username, &password).await?;
        Ok(())
    })
    .await
    .map_err(|_| rejected())?
}
pub async fn login(
    target: &Target,
    username: &str,
    password: Zeroizing<String>,
) -> Result<Session> {
    validate_credentials(username, &password)?;
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut stream = target.connect().await?;
        let identity = authenticate(&mut stream, target, "login", username, &password)
            .await?
            .ok_or_else(invalid)?;
        Ok(Session { identity, stream })
    })
    .await
    .map_err(|_| rejected())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> Target {
        Target {
            host: "127.0.0.1".into(),
            port: 443,
            server_name: "localhost".into(),
            authority_id: "owner".into(),
            realm_id: "realm".into(),
            save_profile_id: "save".into(),
            ca_certificate: None,
        }
    }
    async fn send<S: AsyncWrite + Unpin>(s: &mut S, value: serde_json::Value) {
        let bytes = serde_json::to_vec(&value).unwrap();
        s.write_u32(bytes.len() as u32).await.unwrap();
        s.write_all(&bytes).await.unwrap();
    }
    async fn peer(
        mut s: tokio::io::DuplexStream,
        operation: &str,
        patch: serde_json::Value,
        native: bool,
    ) {
        s.write_all(MAGIC).await.unwrap();
        send(&mut s,serde_json::json!({"type":"simple-challenge-v1","mode":"simple","authority_id":"owner","realm_id":"realm","save_profile_id":"save","connection_id":"a".repeat(32),"nonce":"b".repeat(64)})).await;
        let size = s.read_u32().await.unwrap();
        let mut bytes = vec![0; size as usize];
        s.read_exact(&mut bytes).await.unwrap();
        let credentials: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(credentials["username"], "Player");
        assert_eq!(credentials["password"], "密 码");
        assert_eq!(credentials["operation"], operation);
        let mut response = serde_json::json!({"type":if operation=="register"{"simple-registered-v1"}else{"simple-ready-v1"},"mode":"simple","authority_id":"owner","realm_id":"realm","save_profile_id":"save","connection_id":"a".repeat(32)});
        if operation == "register" {
            response["registered"] = true.into();
        } else {
            response["ready"] = true.into();
            response["native_account_id"] = "9223372036854775807".into();
            response["username"] = "Player".into();
        }
        for (key, value) in patch.as_object().unwrap() {
            response[key] = value.clone();
        }
        send(&mut s, response).await;
        if native {
            s.write_all(b"native-first-byte").await.unwrap();
        }
    }
    #[tokio::test]
    async fn same_stream_ready_does_not_overread_native_bytes() {
        let (mut client, server) = tokio::io::duplex(16384);
        let task =
            tokio::spawn(async move { peer(server, "login", serde_json::json!({}), true).await });
        let identity = authenticate(&mut client, &target(), "login", "Player", "密 码")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(identity.native_account_id, "9223372036854775807");
        let mut native = [0; 17];
        client.read_exact(&mut native).await.unwrap();
        assert_eq!(&native, b"native-first-byte");
        task.await.unwrap();
    }
    #[tokio::test]
    async fn register_requires_close_and_never_native() {
        for native in [false, true] {
            let (mut client, server) = tokio::io::duplex(16384);
            let task = tokio::spawn(async move {
                peer(server, "register", serde_json::json!({}), native).await
            });
            assert_eq!(
                authenticate(&mut client, &target(), "register", "Player", "密 码")
                    .await
                    .is_ok(),
                !native
            );
            task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn rejects_wrong_scope_username_id_boolean_and_extra_fields() {
        for patch in [
            serde_json::json!({"authority_id":"foreign"}),
            serde_json::json!({"save_profile_id":"foreign"}),
            serde_json::json!({"connection_id":"c".repeat(32)}),
            serde_json::json!({"username":"player"}),
            serde_json::json!({"native_account_id":1}),
            serde_json::json!({"native_account_id":"9223372036854775808"}),
            serde_json::json!({"native_account_id":"01"}),
            serde_json::json!({"ready":1}),
            serde_json::json!({"uid":"10000001"}),
        ] {
            let (mut client, server) = tokio::io::duplex(16384);
            let task = tokio::spawn(async move { peer(server, "login", patch, false).await });
            assert!(
                authenticate(&mut client, &target(), "login", "Player", "密 码")
                    .await
                    .is_err()
            );
            task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn frames_reject_duplicate_unknown_and_oversize() {
        for bytes in [
            br#"{"type":"a","type":"b"}"#.to_vec(),
            br#"{"unknown":true}"#.to_vec(),
            vec![b' '; 4097],
        ] {
            let (mut client, mut server) = tokio::io::duplex(8192);
            server.write_u32(bytes.len() as u32).await.unwrap();
            server.write_all(&bytes).await.unwrap();
            drop(server);
            assert!(frame::<_, Challenge>(&mut client).await.is_err());
        }
    }
    #[test]
    fn exact_credentials_and_scope_are_required() {
        assert!(validate_credentials("Player", "密 码").is_ok());
        assert!(validate_credentials(" Player", "x").is_err());
        assert!(validate_credentials("Player", &"密".repeat(25)).is_err());
        assert!(target().tls().is_ok());
        let mut t = target();
        t.authority_id.clear();
        assert!(t.validate().is_err());
    }
}
