//! Local, headless integration check. Own temporary package/data only; no game.
use launcher_mod_runtime::{package, process::Manager};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn http(
    url: &str,
    path: &str,
    body: Option<&Value>,
    token: Option<&str>,
    frame: bool,
) -> (u16, String) {
    let port = url
        .strip_prefix("http://127.0.0.1:")
        .unwrap()
        .trim_end_matches('/')
        .parse::<u16>()
        .unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let data = body
        .map(|v| serde_json::to_vec(v).unwrap())
        .unwrap_or_default();
    let mut headers=format!("{} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",if body.is_some(){"POST"}else{"GET"},data.len());
    if let Some(token) = token {
        headers.push_str(&format!("X-MeowLink-Token: {token}\r\n"));
    }
    if frame {
        headers.push_str("Origin: http://tauri.localhost\r\nSec-Fetch-Site: cross-site\r\nSec-Fetch-Dest: iframe\r\n");
    }
    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes()).unwrap();
    stream.write_all(&data).unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let status = text.split(' ').nth(1).unwrap().parse().unwrap();
    (status, text)
}
fn boot(url: &str) -> String {
    let (status, text) = http(url, "/", None, None, true);
    assert_eq!(status, 200);
    assert!(text.contains("frame-ancestors 'self' http://tauri.localhost tauri://localhost"));
    let token = text
        .split("name=\"meowlink-access\" content=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(token.len(), 64);
    token
}
fn api(url: &str, path: &str, body: Option<&Value>, token: &str) -> (u16, Value) {
    let (status, text) = http(url, path, body, Some(token), false);
    (
        status,
        serde_json::from_str(text.split("\r\n\r\n").nth(1).unwrap()).unwrap(),
    )
}
fn copy_version(source: &Path, to: &Path, new_version: &str) {
    let p = package::inspect(source).unwrap();
    fs::create_dir_all(to).unwrap();
    let mut manifest = p.manifest;
    for file in &mut manifest.files {
        let destination = to.join(&file.path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        let mut bytes = fs::read(source.join(&file.path)).unwrap();
        if file.path == manifest.entrypoint {
            bytes.extend_from_slice(b"MEOWLINK_INDEPENDENT_UPDATE_FIXTURE_V1");
        }
        fs::write(&destination, &bytes).unwrap();
        file.bytes = bytes.len() as u64;
        file.sha256 = package::digest(&bytes);
    }
    manifest.version = new_version.into();
    fs::write(
        to.join("launcher-mod.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn main() {
    let source = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("package directory required"),
    );
    let root = std::env::temp_dir().join(format!(
        "launcher-mod-e2e-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let _temp = Temp(root.clone());
    let host_binary = std::env::current_exe().unwrap();
    let host_before = package::digest(&fs::read(&host_binary).unwrap());
    let store = root.join("store");
    let first = package::inspect(&source).unwrap();
    let id = first.manifest.id.clone();
    let first_entry = first
        .manifest
        .files
        .iter()
        .find(|f| f.path == first.manifest.entrypoint)
        .unwrap()
        .sha256
        .clone();
    package::install(&store, &source, &first.manifest_sha256).unwrap();
    let mut manager = Manager::default();
    let url = manager.open(&store, &id).unwrap();
    let token = boot(&url);
    let (status, state) = api(&url, "/api/status", None, &token);
    assert_eq!(status, 200);
    assert_eq!(state["embeddedInLauncher"], true);
    assert_eq!(state["running"], false);
    assert_eq!(state["identity"], Value::Null);
    let config = json!({"schemaVersion":1,"target":{"host":"192.0.2.50","port":28000,"serverName":"game.example.com","authorityId":"fixture-owner","realmId":"fixture-realm","saveProfileId":"fixture-save","caCertificate":null},"playerTargetFile":root.join("no-player-target.json"),"hostExecutable":root.join("never-launched-host.exe"),"clientDirectory":root.join("no-game-client")});
    assert_eq!(api(&url, "/api/config", Some(&config), &token).0, 200);
    assert_eq!(
        api(&url, "/api/start", Some(&json!({})), &token).1["code"],
        "login_required"
    );
    assert!(manager.idle(&store, &id).is_err());
    let saved = store.join("data").join(&id).join("config.json");
    let config_before = package::digest(&fs::read(&saved).unwrap());
    manager.stop(&store, &id).unwrap();
    assert!(!manager.any_active().unwrap());
    let next = root.join("update");
    copy_version(&source, &next, "0.1.2");
    let update = package::inspect(&next).unwrap();
    let second_entry = update
        .manifest
        .files
        .iter()
        .find(|f| f.path == update.manifest.entrypoint)
        .unwrap()
        .sha256
        .clone();
    assert_ne!(first_entry, second_entry);
    manager.idle(&store, &id).unwrap();
    package::install(&store, &next, &update.manifest_sha256).unwrap();
    assert_eq!(
        package::approved(&package::current(&store, &id).unwrap())
            .unwrap()
            .manifest
            .version,
        "0.1.2"
    );
    let url = manager.open(&store, &id).unwrap();
    let token = boot(&url);
    let state = api(&url, "/api/status", None, &token).1;
    assert_eq!(state["config"], config);
    assert_eq!(state["identity"], Value::Null);
    assert_eq!(config_before, package::digest(&fs::read(&saved).unwrap()));
    manager.stop(&store, &id).unwrap();
    fs::write(next.join(&update.manifest.entrypoint), b"corrupt-update").unwrap();
    assert!(package::inspect(&next).is_err());
    assert_eq!(
        package::approved(&package::current(&store, &id).unwrap())
            .unwrap()
            .manifest
            .version,
        "0.1.2"
    );
    package::rollback(&store, &id).unwrap();
    assert_eq!(
        package::approved(&package::current(&store, &id).unwrap())
            .unwrap()
            .manifest
            .version,
        first.manifest.version
    );
    let url = manager.open(&store, &id).unwrap();
    let token = boot(&url);
    assert_eq!(api(&url, "/api/status", None, &token).1["config"], config);
    manager.stop(&store, &id).unwrap();
    package::uninstall(&store, &id).unwrap();
    assert!(manager.list(&store).unwrap().is_empty());
    assert!(saved.exists());
    assert_eq!(
        host_before,
        package::digest(&fs::read(host_binary).unwrap())
    );
    println!(
        "{}",
        json!({"genericHostCore":true,"actualMeowLinkManagedProcess":true,"importToEmbeddedPage":true,"ownedPidAndListenerVerified":true,"stopConfirmed":true,"independentBinaryUpdate":true,"updatedExeIdentityChangedViaTestOverlay":true,"hostBinaryUnchanged":true,"configPreservedAcrossUpdateRollbackUninstall":true,"badUpdateRejected":true,"rollbackRestartsOriginalVersion":true,"uninstallPreservesConfig":true,"browserOpened":false,"gameStarted":false,"productionChanged":false})
    );
}
