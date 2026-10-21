use crate::{error, generic::Target, Result};
use std::{io::Write, path::Path};
pub fn updated_target(
    current: &Target,
    host: String,
    port: u16,
    server_name: String,
) -> Result<Target> {
    let mut result = current.clone();
    result.host = host.trim().to_owned();
    result.port = port;
    result.server_name = server_name.trim().to_owned();
    result.validate()?;
    if let Ok(ip) = result.host.parse::<std::net::IpAddr>() {
        if ip.is_unspecified() || ip.is_multicast() {
            return Err(error(
                "server_config",
                "请填写实际服务器 IP 或域名，不能填写监听通配地址。",
            ));
        }
    }
    Ok(result)
}
pub fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() > 16384 {
        return Err(error("server_config", "服务器配置过大。"));
    }
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| error("server_config", "系统时间无效。"))?
        .as_nanos();
    let staging = path.with_extension(format!("json.new-{}-{suffix}", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)
        .map_err(|_| error("server_config", "无法创建配置暂存文件。"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| error("server_config", "保存配置失败，原配置未改。"))?;
    drop(file);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        }
        let a: Vec<u16> = staging.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 1 | 8) } == 0 {
            return Err(error("server_config", "配置替换失败，原配置保留。"));
        }
    }
    #[cfg(not(windows))]
    std::fs::rename(staging, path).map_err(|_| error("server_config", "配置替换失败。"))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> Target {
        Target {
            host: "127.0.0.1".into(),
            port: 27004,
            server_name: "game.example.com".into(),
            authority_id: "owner".into(),
            realm_id: "realm".into(),
            save_profile_id: "profile".into(),
            ca_certificate: None,
        }
    }
    #[test]
    fn local_lan_and_public_dns_keep_tls_and_storage_scope() {
        for host in ["127.0.0.1", "192.168.1.20", "game.example.com"] {
            let t =
                updated_target(&target(), host.into(), 27004, "game.example.com".into()).unwrap();
            assert_eq!(t.host, host);
            assert_eq!(t.server_name, "game.example.com");
            assert_eq!(t.authority_id, "owner");
            assert_eq!(t.realm_id, "realm");
            assert_eq!(t.save_profile_id, "profile");
        }
    }
    #[test]
    fn wildcard_and_invalid_destination_are_rejected() {
        for host in ["0.0.0.0", "::", "224.0.0.1", "bad/host", ""] {
            assert!(
                updated_target(&target(), host.into(), 27004, "game.example.com".into()).is_err()
            );
        }
        assert!(updated_target(
            &target(),
            "192.168.1.20".into(),
            0,
            "game.example.com".into()
        )
        .is_err());
    }
    #[test]
    fn atomic_replace_preserves_complete_configuration() {
        let path = std::env::temp_dir().join(format!(
            "meowlink-server-save-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"old").unwrap();
        replace(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        std::fs::remove_file(path).unwrap();
    }
}
