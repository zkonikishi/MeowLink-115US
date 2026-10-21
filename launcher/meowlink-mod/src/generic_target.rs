//! Parser for the host's LOCAL readiness artifact, not an installer descriptor
//! or a trust root. Caller must separately verify the live owner lifecycle and
//! listener ownership before using raw-loopback directory forwarding.
use crate::{
    error,
    generic::Target,
    generic_bridges::{GameRoute, LoopbackDirectory, TlsDirectory},
    Result,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Artifact {
    schema: String,
    mode: String,
    authority_id: String,
    realm_id: String,
    save_profile_id: String,
    game_transport: String,
    server_name: String,
    certificate_sha256: String,
    routes: Vec<Route>,
    directory: Directory,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Route {
    channel_id: u32,
    listen_address: String,
    advertise_address: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Directory {
    address: String,
    transport: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RemoteArtifact {
    schema: String,
    mode: String,
    authority_id: String,
    realm_id: String,
    save_profile_id: String,
    game_transport: String,
    server_name: String,
    certificate_sha256: String,
    routes: Vec<Route>,
    directory: RemoteDirectory,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteDirectory {
    address: String,
    port: u16,
    transport: String,
}

/// Validated mapping data only. In particular, this does NOT mean the stopped
/// host from an old readiness file is running, or that its certificate hash is
/// a trusted CA. The actual TLS adapter still uses independent Target trust.
pub struct LocalMapping {
    pub games: Vec<GameRoute>,
    pub directory: LoopbackDirectory,
    pub artifact_sha256: String,
    pub diagnostic_certificate_sha256: String,
}
pub struct RemoteMapping {
    pub games: Vec<GameRoute>,
    pub directory: TlsDirectory,
    pub artifact_sha256: String,
    pub diagnostic_certificate_sha256: String,
}
fn invalid() -> crate::AppError {
    error(
        "simple_target",
        "本机频道映射与可信目标不一致，未连接或启动游戏。",
    )
}
pub fn parse_local_artifact(raw: &[u8], trusted: &Target) -> Result<LocalMapping> {
    trusted.validate()?;
    if raw.len() > 65536 {
        return Err(invalid());
    }
    let value: Artifact = serde_json::from_slice(raw).map_err(|_| invalid())?;
    let host: IpAddr = trusted.host.parse().map_err(|_| invalid())?;
    if !host.is_loopback()
        || value.schema != "meow115-local-host-target-v1"
        || value.mode != "simple"
        || value.game_transport != "tls1.3-simple-v1"
        || value.authority_id != trusted.authority_id
        || value.realm_id != trusted.realm_id
        || value.save_profile_id != trusted.save_profile_id
        || value.server_name != trusted.server_name
        || value.routes.len() != 51
        || value.certificate_sha256.len() != 64
        || !value
            .certificate_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid());
    }
    let mut ids = HashSet::new();
    let mut ports = HashSet::new();
    let mut games = Vec::new();
    for route in value.routes {
        let listen: SocketAddr = route.listen_address.parse().map_err(|_| invalid())?;
        let advertise: SocketAddr = route.advertise_address.parse().map_err(|_| invalid())?;
        if listen.ip() != host
            || listen.port() == 0
            || advertise.ip() != IpAddr::V4(Ipv4Addr::new(127, 0, 0, 4))
            || advertise.port() != listen.port()
            || !ids.insert(route.channel_id)
            || !ports.insert(listen.port())
        {
            return Err(invalid());
        }
        let mut upstream = trusted.clone();
        upstream.port = listen.port();
        games.push(GameRoute {
            channel_id: route.channel_id,
            local_port: advertise.port(),
            upstream,
        });
    }
    if games[0].upstream.port != trusted.port {
        return Err(invalid());
    }
    let directory: SocketAddr = value.directory.address.parse().map_err(|_| invalid())?;
    if value.directory.transport != "native-loopback-tcp-v1"
        || directory.ip() != host
        || !directory.ip().is_loopback()
        || directory.port() == 0
        || ports.contains(&directory.port())
        || directory.port() == 7002
        || ports.contains(&7002)
    {
        return Err(invalid());
    }
    Ok(LocalMapping {
        games,
        directory: LoopbackDirectory {
            local_port: directory.port(),
            upstream: directory,
        },
        artifact_sha256: format!("{:x}", Sha256::digest(raw)),
        diagnostic_certificate_sha256: value.certificate_sha256,
    })
}

/// v2 remote contract candidate: the host owns this schema. No remote PID is
/// interpreted as a local process; no CA or endpoint hostname is learned from
/// the artifact. Network host/trust always comes from independent Target.
pub fn parse_remote_artifact(raw: &[u8], trusted: &Target) -> Result<RemoteMapping> {
    trusted.validate()?;
    if raw.len() > 65536 {
        return Err(invalid());
    }
    let value: RemoteArtifact = serde_json::from_slice(raw).map_err(|_| invalid())?;
    if value.schema != "meow115-host-target-v2"
        || value.mode != "simple"
        || value.game_transport != "tls1.3-simple-v1"
        || value.authority_id != trusted.authority_id
        || value.realm_id != trusted.realm_id
        || value.save_profile_id != trusted.save_profile_id
        || value.server_name != trusted.server_name
        || value.routes.len() != 51
        || value.certificate_sha256.len() != 64
        || !value
            .certificate_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid());
    }
    let mut games = Vec::new();
    let mut ids = HashSet::new();
    let mut ports = HashSet::new();
    for route in value.routes {
        let listen: SocketAddr = route.listen_address.parse().map_err(|_| invalid())?;
        let advertise: SocketAddr = route.advertise_address.parse().map_err(|_| invalid())?;
        if listen.port() == 0
            || advertise.ip() != IpAddr::V4(Ipv4Addr::new(127, 0, 0, 4))
            || advertise.port() != listen.port()
            || !ids.insert(route.channel_id)
            || !ports.insert(listen.port())
        {
            return Err(invalid());
        }
        let mut upstream = trusted.clone();
        upstream.port = listen.port();
        games.push(GameRoute {
            channel_id: route.channel_id,
            local_port: advertise.port(),
            upstream,
        });
    }
    let directory: SocketAddr = value.directory.address.parse().map_err(|_| invalid())?;
    if games[0].upstream.port != trusted.port
        || value.directory.transport != "native-tls1.3-v1"
        || value.directory.port == 0
        || directory.port() != value.directory.port
        || value.directory.port == 7002
        || ports.contains(&value.directory.port)
        || ports.contains(&7002)
    {
        return Err(invalid());
    }
    let mut upstream = trusted.clone();
    upstream.port = value.directory.port;
    Ok(RemoteMapping {
        games,
        directory: TlsDirectory {
            local_port: value.directory.port,
            upstream,
        },
        artifact_sha256: format!("{:x}", Sha256::digest(raw)),
        diagnostic_certificate_sha256: value.certificate_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const STOPPED: &[u8] = include_bytes!("../fixtures/generic-local-target-stopped.json");
    fn trusted() -> Target {
        Target {
            host: "127.0.0.1".into(),
            port: 61869,
            server_name: "127.0.0.1".into(),
            authority_id: "universal-smoke".into(),
            realm_id: "fixture".into(),
            save_profile_id: "native".into(),
            ca_certificate: None,
        }
    }
    #[test]
    fn actual_stopped_host_snapshot_preserves_source_order_without_opening_ports() {
        let result = parse_local_artifact(STOPPED, &trusted()).unwrap();
        assert_eq!(result.games.len(), 51);
        assert_eq!(result.games[0].channel_id, 1);
        assert_eq!(result.games[1].channel_id, 6);
        assert_eq!(result.games[1].local_port, 61870);
        assert_eq!(
            result.directory.upstream,
            "127.0.0.1:61921".parse().unwrap()
        );
        assert_eq!(
            result.diagnostic_certificate_sha256,
            "24a01cc54443c8440d69b40b73c94b62c1c4394f0b091e6f8d0c6a8bd10d3c58"
        );
        assert_eq!(result.artifact_sha256.len(), 64);
    }
    #[test]
    fn rejects_wrong_scope_transport_name_and_untrusted_ca_fields() {
        let source: serde_json::Value = serde_json::from_slice(STOPPED).unwrap();
        for (key, value) in [
            ("mode", "account"),
            ("authorityId", "foreign"),
            ("serverName", "foreign.invalid"),
            ("gameTransport", "plaintext"),
            ("certificateSha256", "bad"),
            (
                "caCertificate",
                "certificate-from-artifact-must-not-be-trusted",
            ),
        ] {
            let mut changed = source.clone();
            changed[key] = value.into();
            assert!(
                parse_local_artifact(&serde_json::to_vec(&changed).unwrap(), &trusted()).is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn rejects_duplicate_id_port_or_nonmatching_listener_advertisement() {
        let source: serde_json::Value = serde_json::from_slice(STOPPED).unwrap();
        for (key, value) in [
            ("channelId", serde_json::json!(1)),
            ("listenAddress", serde_json::json!("127.0.0.1:61869")),
            ("advertiseAddress", serde_json::json!("127.0.0.1:61870")),
            ("advertiseAddress", serde_json::json!("127.0.0.4:1")),
        ] {
            let mut changed = source.clone();
            changed["routes"][1][key] = value;
            assert!(
                parse_local_artifact(&serde_json::to_vec(&changed).unwrap(), &trusted()).is_err()
            );
        }
    }
    #[test]
    fn rejects_public_raw_directory_and_partial_or_reordered_mappings() {
        let source: serde_json::Value = serde_json::from_slice(STOPPED).unwrap();
        for (key, value) in [
            ("transport", "rawTLS"),
            ("address", "203.0.113.1:61921"),
            ("address", "127.0.0.1:61869"),
            ("address", "127.0.0.1:7002"),
        ] {
            let mut changed = source.clone();
            changed["directory"][key] = value.into();
            assert!(
                parse_local_artifact(&serde_json::to_vec(&changed).unwrap(), &trusted()).is_err()
            );
        }
        let mut changed = source.clone();
        changed["routes"].as_array_mut().unwrap().pop();
        assert!(parse_local_artifact(&serde_json::to_vec(&changed).unwrap(), &trusted()).is_err());
        let mut changed = source;
        changed["routes"].as_array_mut().unwrap().swap(0, 1);
        assert!(parse_local_artifact(&serde_json::to_vec(&changed).unwrap(), &trusted()).is_err());
        assert!(parse_local_artifact(&vec![b' '; 65537], &trusted()).is_err());
    }
    fn remote_fixture() -> serde_json::Value {
        let mut value: serde_json::Value = serde_json::from_slice(STOPPED).unwrap();
        value["schema"] = "meow115-host-target-v2".into();
        value["directory"]["transport"] = "native-tls1.3-v1".into();
        value["directory"]["address"] = "0.0.0.0:61921".into();
        value["directory"]["port"] = 61921.into();
        for route in value["routes"].as_array_mut().unwrap() {
            let port = route["listenAddress"]
                .as_str()
                .unwrap()
                .rsplit(':')
                .next()
                .unwrap()
                .to_owned();
            route["listenAddress"] = format!("0.0.0.0:{port}").into();
        }
        value
    }
    #[test]
    fn remote_uses_independent_host_trust_never_server_bind_address_or_pid() {
        let mut trust = trusted();
        trust.host = "host.example.invalid".into();
        let map =
            parse_remote_artifact(&serde_json::to_vec(&remote_fixture()).unwrap(), &trust).unwrap();
        assert_eq!(map.games[0].upstream.host, "host.example.invalid");
        assert_eq!(map.directory.upstream.host, "host.example.invalid");
        assert_eq!(map.directory.upstream.server_name, trust.server_name);
        assert_eq!(map.directory.upstream.ca_certificate, trust.ca_certificate);
        assert_eq!(map.directory.upstream.port, 61921);
        assert!(
            parse_local_artifact(&serde_json::to_vec(&remote_fixture()).unwrap(), &trust).is_err()
        );
    }
    #[test]
    fn remote_rejects_raw_or_unknown_directory_transport_and_port_conflicts() {
        for transport in ["native-loopback-tcp-v1", "rawTLS", "tls", "unknown"] {
            let mut value = remote_fixture();
            value["directory"]["transport"] = transport.into();
            assert!(
                parse_remote_artifact(&serde_json::to_vec(&value).unwrap(), &trusted()).is_err()
            );
        }
        for port in [0, 7002, 61869, 61922] {
            let mut value = remote_fixture();
            value["directory"]["port"] = port.into();
            assert!(
                parse_remote_artifact(&serde_json::to_vec(&value).unwrap(), &trusted()).is_err()
            );
        }
        let mut value = remote_fixture();
        value["caCertificate"] = "untrusted-ca".into();
        assert!(parse_remote_artifact(&serde_json::to_vec(&value).unwrap(), &trusted()).is_err());
    }
}
