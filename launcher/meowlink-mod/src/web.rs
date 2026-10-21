use crate::{
    error, generic,
    generic_bridges::{self, LoggedIn},
    native, server_settings, Config, Result,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{watch, Mutex, Semaphore},
};
use zeroize::Zeroizing;

const HTML: &str = include_str!("../web/index.html");
const SCRIPT: &str = include_str!("../web/app.js");
pub struct Inner {
    pub config: Option<Config>,
    pub config_error: Option<String>,
    pub account: Option<Arc<LoggedIn>>,
    pub running: Option<native::Running>,
}
pub struct State {
    pub root: PathBuf,
    pub inner: Mutex<Inner>,
    pub token: String,
    pub origin: String,
    pub shutdown: watch::Sender<bool>,
    pub embedded: bool,
}
impl State {
    pub fn new(
        root: PathBuf,
        token: String,
        port: u16,
        shutdown: watch::Sender<bool>,
        embedded: bool,
    ) -> Self {
        let path = root.join("config.json");
        let loaded = if path.exists() {
            Config::read(&path).map(Some)
        } else {
            Ok(None)
        };
        let (config, config_error) = match loaded {
            Ok(value) => (value, None),
            Err(e) => (None, Some(e.message)),
        };
        Self {
            root,
            inner: Mutex::new(Inner {
                config,
                config_error,
                account: None,
                running: None,
            }),
            token,
            origin: format!("http://127.0.0.1:{port}"),
            shutdown,
            embedded,
        }
    }
    pub async fn stop(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        if let Some(running) = &mut inner.running {
            running.stop().await?;
        }
        inner.running.take();
        inner.account.take();
        Ok(())
    }
}
pub struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Zeroizing<Vec<u8>>,
}
fn bad() -> crate::AppError {
    error("request", "请求格式无效。")
}
async fn request(stream: &mut TcpStream) -> Result<Request> {
    let mut raw = Zeroizing::new(Vec::new());
    let end;
    loop {
        let byte = stream.read_u8().await.map_err(|_| bad())?;
        raw.push(byte);
        if raw.ends_with(b"\r\n\r\n") {
            end = raw.len();
            break;
        }
        if raw.len() >= 8192 {
            return Err(bad());
        }
    }
    let header = std::str::from_utf8(&raw[..end]).map_err(|_| bad())?;
    let mut lines = header.split("\r\n");
    let first: Vec<_> = lines.next().ok_or_else(bad)?.split(' ').collect();
    if first.len() != 3
        || first[2] != "HTTP/1.1"
        || !["GET", "POST"].contains(&first[0])
        || !first[1].starts_with('/')
        || first[1].contains(['?', '#'])
    {
        return Err(bad());
    }
    let method = first[0].to_owned();
    let path = first[1].to_owned();
    let mut headers = HashMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (key, value) = line.split_once(':').ok_or_else(bad)?;
        if key.is_empty()
            || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || value.chars().any(|c| c.is_control() && c != '\t')
        {
            return Err(bad());
        }
        if headers
            .insert(key.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err(bad());
        }
    }
    if headers.contains_key("transfer-encoding") || headers.contains_key("expect") {
        return Err(bad());
    }
    let length: usize = headers
        .get("content-length")
        .map(|s| s.parse())
        .transpose()
        .map_err(|_| bad())?
        .unwrap_or(0);
    if length > 16384 || (method == "GET" && length != 0) {
        return Err(bad());
    }
    let mut body = Zeroizing::new(vec![0; length]);
    stream.read_exact(&mut body).await.map_err(|_| bad())?;
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}
fn authorized(req: &Request, state: &State) -> bool {
    if req.headers.get("host").map(String::as_str) != state.origin.strip_prefix("http://") {
        return false;
    }
    let embedded_bootstrap = state.embedded
        && req.method == "GET"
        && req.path == "/"
        && req
            .headers
            .get("sec-fetch-dest")
            .is_some_and(|s| s == "iframe");
    if let Some(origin) = req.headers.get("origin") {
        let launcher_origin = embedded_bootstrap
            && ["http://tauri.localhost", "tauri://localhost"].contains(&origin.as_str());
        if origin != &state.origin && !launcher_origin {
            return false;
        }
    }
    if req
        .headers
        .get("sec-fetch-site")
        .is_some_and(|s| !["same-origin", "none"].contains(&s.as_str()))
        && !embedded_bootstrap
    {
        return false;
    }
    if req.path == "/" || req.path == "/app.js" {
        return req.method == "GET";
    }
    if req.headers.get("x-meowlink-token") != Some(&state.token) {
        return false;
    }
    req.method != "POST"
        || req.headers.get("content-type").map(String::as_str) == Some("application/json")
}
fn value<T: serde::de::DeserializeOwned>(request: &Request) -> Result<T> {
    serde_json::from_slice(&request.body).map_err(|_| bad())
}
struct Credentials {
    username: String,
    password: Zeroizing<String>,
}
// Zeroizing<String> intentionally has no serde feature: deserialize, then immediately
// transfer into a zeroizing buffer without ever putting credentials in state files.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireCredentials {
    username: String,
    password: String,
}
fn credentials(request: &Request) -> Result<Credentials> {
    let raw: WireCredentials = value(request)?;
    Ok(Credentials {
        username: raw.username,
        password: Zeroizing::new(raw.password),
    })
}
fn idle(inner: &Inner) -> Result<()> {
    if inner
        .running
        .as_ref()
        .is_some_and(|r| r.active() || r.quarantined())
    {
        return Err(error(
            "game_running",
            "请先停止当前 MeowLink 实例；清理失败时禁止切换账号或配置。",
        ));
    }
    Ok(())
}
async fn api(request: &Request, state: &State) -> Result<Value> {
    let mut inner = state.inner.lock().await;
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/status") => Ok(
            json!({"config": inner.config, "configurationError": inner.config_error, "identity": inner.account.as_ref().map(|a| &a.identity), "running":inner.running.as_ref().is_some_and(|r|r.active()),"cleanupFailed":inner.running.as_ref().is_some_and(|r|r.quarantined()),"author":"zkonikishi","launcherModified":false,"embeddedInLauncher":state.embedded,"defaults":{"hostExecutable":std::env::current_exe().ok().and_then(|p|p.parent().map(|p|p.join("runtime/MeowClientHost.exe"))),"playerTargetFile":state.root.join("player-target.json")}}),
        ),
        ("POST", "/api/config") => {
            idle(&inner)?;
            let config: Config = value(request)?;
            config.validate()?;
            let bytes = serde_json::to_vec_pretty(&config).map_err(|_| bad())?;
            server_settings::replace(&state.root.join("config.json"), &bytes)?;
            inner.account.take();
            inner.running.take();
            inner.config = Some(config);
            inner.config_error = None;
            Ok(json!({"message":"配置已保存，原账号会话已清除，请重新登录。"}))
        }
        ("POST", "/api/check") => {
            idle(&inner)?;
            let config = inner
                .config
                .as_ref()
                .ok_or_else(|| error("configuration", "请先保存服务器配置。"))?;
            let mapping = config.mapping()?;
            generic_bridges::probe_directory_tls(&mapping.directory.upstream).await?;
            // Opening a TLS socket is a certificate/connectivity check, not an account
            // login or a claim that a native game has entered the server.
            let _ = tokio::time::timeout(Duration::from_secs(10), config.target.connect())
                .await
                .map_err(|_| error("timeout", "服务器连接超时。"))??;
            Ok(
                json!({"message":"频道描述、目录 TLS 和认证端口 TLS 校验通过；尚未登录或启动游戏。","routes":mapping.games.len()}),
            )
        }
        ("POST", "/api/register") => {
            idle(&inner)?;
            let config = inner
                .config
                .as_ref()
                .ok_or_else(|| error("configuration", "请先保存配置。"))?;
            let creds = credentials(request)?;
            generic::register(&config.target, &creds.username, creds.password).await?;
            Ok(json!({"message":"注册成功，请登录。"}))
        }
        ("POST", "/api/login") => {
            idle(&inner)?;
            inner.account.take();
            let config = inner
                .config
                .as_ref()
                .ok_or_else(|| error("configuration", "请先保存配置。"))?;
            let creds = credentials(request)?;
            let account =
                LoggedIn::authenticate(config.target.clone(), creds.username, creds.password)
                    .await?;
            let identity = account.identity.clone();
            inner.account = Some(account);
            Ok(json!({"message":"账号认证成功，尚未启动游戏。","identity":identity}))
        }
        ("POST", "/api/start") => {
            idle(&inner)?;
            let config = inner
                .config
                .as_ref()
                .ok_or_else(|| error("configuration", "请先保存配置。"))?;
            let account = inner
                .account
                .clone()
                .ok_or_else(|| error("login_required", "请先登录。"))?;
            let running = native::start(config, account, &state.root.join("runtime")).await?;
            let result = json!({"message":"已核验原生进程，是否成功进服仍以游戏结果为准。","started":running.started});
            inner.running = Some(running);
            Ok(result)
        }
        ("POST", "/api/stop") | ("POST", "/api/logout") | ("POST", "/api/shutdown") => {
            if let Some(running) = &mut inner.running {
                running.stop().await?;
            }
            inner.running.take();
            inner.account.take();
            if request.path == "/api/shutdown" {
                let _ = state.shutdown.send(true);
            }
            Ok(json!({"message":"已停止 MeowLink 实例并清除账号会话。"}))
        }
        _ => Err(error("route", "接口不存在。")),
    }
}
async fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) {
    let header = format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; frame-ancestors 'self' http://tauri.localhost tauri://localhost; form-action 'none'\r\n\r\n", body.len());
    let _ = stream.write_all(header.as_bytes()).await;
    let _ = stream.write_all(body).await;
    let _ = stream.shutdown().await;
}
async fn connection(mut stream: TcpStream, state: Arc<State>) {
    let request = match tokio::time::timeout(Duration::from_secs(5), request(&mut stream)).await {
        Ok(Ok(request)) => request,
        _ => {
            respond(
                &mut stream,
                "400 Bad Request",
                "application/json",
                br#"{"message":"Invalid request"}"#,
            )
            .await;
            return;
        }
    };
    if !authorized(&request, &state) {
        respond(
            &mut stream,
            "403 Forbidden",
            "application/json",
            br#"{"message":"Forbidden"}"#,
        )
        .await;
        return;
    }
    match request.path.as_str() {
        "/" => {
            let page = HTML.replace("{{ACCESS_TOKEN}}", &state.token);
            respond(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                page.as_bytes(),
            )
            .await
        }
        "/app.js" => {
            respond(
                &mut stream,
                "200 OK",
                "application/javascript; charset=utf-8",
                SCRIPT.as_bytes(),
            )
            .await
        }
        _ => {
            let (status, response) = match api(&request, &state).await {
                Ok(v) => ("200 OK", v),
                Err(e) => (
                    "400 Bad Request",
                    json!({"code":e.code,"message":e.message}),
                ),
            };
            if let Ok(bytes) = serde_json::to_vec(&response) {
                respond(
                    &mut stream,
                    status,
                    "application/json; charset=utf-8",
                    &bytes,
                )
                .await;
            }
        }
    }
}
pub async fn serve(
    listener: TcpListener,
    state: Arc<State>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let slots = Arc::new(Semaphore::new(8));
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tasks.join_next(), if !tasks.is_empty() => {},
            accepted = listener.accept() => {
                let (stream, peer) = accepted.map_err(|_|error("http","本机配置服务监听失败。"))?;
                if !peer.ip().is_loopback() { continue; }
                if let Ok(permit) = slots.clone().try_acquire_owned() {
                    let state = state.clone(); tasks.spawn(async move { let _permit=permit; connection(stream,state).await; });
                }
            }
        }
    }
    // Wait for in-flight actions before cleanup; never cancel a native startup in
    // the middle and then report that its child process was cleaned up.
    while tasks.join_next().await.is_some() {}
    state.stop().await
}
pub fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        extern "system" {
            fn BCryptGenRandom(
                handle: *mut std::ffi::c_void,
                buffer: *mut u8,
                size: u32,
                flags: u32,
            ) -> i32;
        }
        if unsafe { BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), 32, 2) } < 0 {
            return Err(error("random", "无法生成本机访问密钥。"));
        }
    }
    #[cfg(not(windows))]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|_| error("random", "无法生成本机访问密钥。"))?;
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn source_digest() -> String {
    format!("{:x}", Sha256::digest(SCRIPT.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_page_bootstrap_does_not_grant_launcher_origin_api_access() {
        let mut s = state();
        s.embedded = true;
        let mut r = req("GET", "/");
        r.headers
            .insert("sec-fetch-site".into(), "cross-site".into());
        r.headers.insert("sec-fetch-dest".into(), "iframe".into());
        r.headers
            .insert("origin".into(), "http://tauri.localhost".into());
        assert!(authorized(&r, &s));
        r.path = "/api/status".into();
        assert!(!authorized(&r, &s));
        r.path = "/".into();
        r.headers
            .insert("origin".into(), "https://evil.example".into());
        assert!(!authorized(&r, &s));
        s.embedded = false;
        r.headers.remove("origin");
        assert!(!authorized(&r, &s));
    }
    fn state() -> State {
        let (tx, _) = watch::channel(false);
        State::new(
            std::env::temp_dir().join("meowlink-nonexistent-config-unit"),
            "secret".into(),
            12345,
            tx,
            false,
        )
    }
    fn req(method: &str, path: &str) -> Request {
        Request {
            method: method.into(),
            path: path.into(),
            headers: HashMap::from([
                ("host".into(), "127.0.0.1:12345".into()),
                ("x-meowlink-token".into(), "secret".into()),
                ("content-type".into(), "application/json".into()),
            ]),
            body: Zeroizing::new(Vec::new()),
        }
    }
    #[test]
    fn api_requires_secret_and_exact_host() {
        let state = state();
        let mut request = req("GET", "/api/status");
        assert!(authorized(&request, &state));
        request.headers.remove("x-meowlink-token");
        assert!(!authorized(&request, &state));
        request
            .headers
            .insert("x-meowlink-token".into(), "secret".into());
        request
            .headers
            .insert("host".into(), "evil.example:12345".into());
        assert!(!authorized(&request, &state));
    }
    #[test]
    fn cross_origin_and_forms_cannot_change_settings() {
        let state = state();
        let mut request = req("POST", "/api/config");
        request
            .headers
            .insert("origin".into(), "https://evil.example".into());
        assert!(!authorized(&request, &state));
        request.headers.remove("origin");
        request
            .headers
            .insert("content-type".into(), "text/plain".into());
        assert!(!authorized(&request, &state));
    }
    #[test]
    fn tokens_are_unpredictable_size_and_unique() {
        let a = random_token().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, random_token().unwrap());
    }
    #[tokio::test]
    async fn duplicate_framing_and_chunked_requests_are_rejected() {
        for headers in [
            "Content-Length: 0\r\nContent-Length: 1\r\n",
            "Transfer-Encoding: chunked\r\n",
            "Content-Length: 16385\r\n",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let bytes = format!("POST /api/config HTTP/1.1\r\nHost: localhost\r\n{headers}\r\n");
            let send = tokio::spawn(async move {
                let mut s = TcpStream::connect(addr).await.unwrap();
                s.write_all(bytes.as_bytes()).await.unwrap();
            });
            let (mut s, _) = listener.accept().await.unwrap();
            assert!(request(&mut s).await.is_err());
            send.await.unwrap();
        }
    }
}
