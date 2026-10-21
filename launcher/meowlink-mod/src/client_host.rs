//! Launcher-side strict client-host ABI adapter. No host/DFO is started here.
//! File provenance/signature installation and actual WFP remain independent
//! gates; parsing a readiness event alone is never proof of either.
use crate::{error, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub struct StrictPlan {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
}
fn invalid() -> crate::AppError {
    error(
        "client_host_ready",
        "客户端宿主尚未提供可核验的隔离就绪状态，未报告游戏启动。",
    )
}
pub fn strict_plan(
    host: &Path,
    client_directory: &Path,
    log: &Path,
    breakpoints: &Path,
    alias_port: u16,
) -> Result<StrictPlan> {
    if !host.is_absolute()
        || !client_directory.is_absolute()
        || !log.is_absolute()
        || !breakpoints.is_absolute()
        || alias_port == 0
    {
        return Err(invalid());
    }
    let path = |value: &Path| {
        value
            .to_str()
            .filter(|v| !v.contains('\0'))
            .map(str::to_owned)
            .ok_or_else(invalid)
    };
    let payload = format!(
        "3?127.0.0.4?{alias_port}?probe?{}?0?0?30?0?0?0",
        "0".repeat(32)
    );
    Ok(StrictPlan {
        executable: host.to_path_buf(),
        working_directory: client_directory.to_path_buf(),
        arguments: vec![
            "--host-client-strict".into(),
            path(client_directory)?,
            path(log)?,
            "55".into(),
            "interactive-ui".into(),
            path(breakpoints)?,
            payload,
        ],
    })
}
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ready {
    pub event: String,
    pub version: u32,
    pub host_pid: u32,
    pub client_pid: u32,
    pub executable: String,
    pub isolated: bool,
    pub network_self_test: bool,
    pub job_assigned: bool,
    pub resumed: bool,
}
fn normalize_path(value: &str) -> String {
    value
        .strip_prefix(r"\\?\")
        .unwrap_or(value)
        .replace('/', "\\")
}
fn same_path(a: &str, b: &Path) -> bool {
    b.to_str()
        .is_some_and(|b| normalize_path(a).eq_ignore_ascii_case(&normalize_path(b)))
}
/// Call with canonical paths. WFP scans every client-root EXE/AES; keeping the
/// Rust remote proxy outside that root avoids filtering the proxy itself.
pub fn require_bridge_outside_client_root(launcher: &Path, client_root: &Path) -> Result<()> {
    if !launcher.is_absolute() || !client_root.is_absolute() {
        return Err(invalid());
    }
    let file = normalize_path(launcher.to_str().ok_or_else(invalid)?).to_lowercase();
    let directory = normalize_path(client_root.to_str().ok_or_else(invalid)?)
        .trim_end_matches('\\')
        .to_lowercase();
    if file == directory || file.starts_with(&(directory + "\\")) {
        return Err(error(
            "bridge_location",
            "请将启动器放到游戏目录之外的独立目录，避免客户端网络隔离限制远端桥接。",
        ));
    }
    Ok(())
}
pub fn parse_ready(line: &str, expected_host_pid: u32, expected_client: &Path) -> Result<Ready> {
    if line.len() > 16384 || !expected_client.is_absolute() {
        return Err(invalid());
    }
    let (tick, json) = line
        .trim_end_matches(['\r', '\n'])
        .split_once(" CLIENTHOST_READY_V1 ")
        .ok_or_else(invalid)?;
    if tick.is_empty() || !tick.bytes().all(|b| b.is_ascii_digit()) || tick.parse::<u64>().is_err()
    {
        return Err(invalid());
    }
    let ready: Ready = serde_json::from_str(json).map_err(|_| invalid())?;
    if ready.event != "clienthost-ready-v1"
        || ready.version != 1
        || expected_host_pid == 0
        || ready.host_pid != expected_host_pid
        || ready.client_pid == 0
        || ready.client_pid == ready.host_pid
        || !ready.isolated
        || !ready.network_self_test
        || !ready.job_assigned
        || !ready.resumed
        || !same_path(&ready.executable, expected_client)
    {
        return Err(invalid());
    }
    Ok(ready)
}
/// Values obtained freshly from the OS, not copied from the ready JSON.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessObservation {
    pub pid: u32,
    pub parent_pid: u32,
    pub image: String,
    pub created_file_time_utc: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListenerObservation {
    pub address: String,
    pub port: u16,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalOwnerObservation {
    pub process: ProcessObservation,
    pub listeners: Vec<ListenerObservation>,
}
pub fn verify_observations(
    ready: &Ready,
    host_executable: &Path,
    client_executable: &Path,
    not_before_file_time: u64,
    now_file_time: u64,
    observations: &[ProcessObservation],
) -> Result<()> {
    if observations.len() != 2 || not_before_file_time > now_file_time {
        return Err(invalid());
    }
    let host = observations
        .iter()
        .find(|p| p.pid == ready.host_pid)
        .ok_or_else(invalid)?;
    let client = observations
        .iter()
        .find(|p| p.pid == ready.client_pid)
        .ok_or_else(invalid)?;
    if !same_path(&host.image, host_executable)
        || !same_path(&client.image, client_executable)
        || client.parent_pid != host.pid
    {
        return Err(invalid());
    }
    let mut times = Vec::new();
    for process in [host, client] {
        let created = process
            .created_file_time_utc
            .parse::<u64>()
            .map_err(|_| invalid())?;
        if created < not_before_file_time || created > now_file_time {
            return Err(invalid());
        }
        times.push(created);
    }
    if times[1] < times[0] {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(windows)]
pub async fn observe_processes(host_pid: u32, client_pid: u32) -> Result<Vec<ProcessObservation>> {
    use std::os::windows::process::CommandExt;
    if host_pid == 0 || client_pid == 0 || host_pid == client_pid {
        return Err(invalid());
    }
    let system = std::env::var_os("SystemRoot").ok_or_else(invalid)?;
    let powershell = PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let script = r#"$ErrorActionPreference='Stop';[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false);$rows=@();foreach($id in @([uint32]$env:MEOW_HOST_PID,[uint32]$env:MEOW_CLIENT_PID)){$p=Get-CimInstance Win32_Process -Filter ('ProcessId = '+$id);if(-not $p){throw 'missing process'};$rows+=@{pid=[uint32]$p.ProcessId;parentPid=[uint32]$p.ParentProcessId;image=$p.ExecutablePath;createdFileTimeUtc=$p.CreationDate.ToUniversalTime().ToFileTimeUtc().ToString()}};ConvertTo-Json -InputObject $rows -Compress"#;
    let mut command = std::process::Command::new(powershell);
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x08000000)
        .env("MEOW_HOST_PID", host_pid.to_string())
        .env("MEOW_CLIENT_PID", client_pid.to_string());
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().eq_ignore_ascii_case("PSModulePath") {
            command.env_remove(key);
        }
    }
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
        .await
        .map_err(|_| invalid())?
        .map_err(|_| invalid())?;
    if !output.status.success() || output.stdout.len() > 16384 {
        return Err(invalid());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| invalid())
}

#[cfg(windows)]
pub async fn observe_local_owner(pid: u32) -> Result<LocalOwnerObservation> {
    use std::os::windows::process::CommandExt;
    if pid == 0 {
        return Err(invalid());
    }
    let powershell = PathBuf::from(std::env::var_os("SystemRoot").ok_or_else(invalid)?)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let script = r#"$ErrorActionPreference='Stop';[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false);$id=[uint32]$env:MEOW_HOST_PID;$p=Get-CimInstance Win32_Process -Filter ('ProcessId = '+$id);if(-not $p){throw 'missing process'};$ports=@(Get-NetTCPConnection -State Listen -OwningProcess $id|ForEach-Object {@{address=$_.LocalAddress;port=[uint16]$_.LocalPort}});@{process=@{pid=[uint32]$p.ProcessId;parentPid=[uint32]$p.ParentProcessId;image=$p.ExecutablePath;createdFileTimeUtc=$p.CreationDate.ToUniversalTime().ToFileTimeUtc().ToString()};listeners=$ports}|ConvertTo-Json -Depth 4 -Compress"#;
    let mut command = std::process::Command::new(powershell);
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x08000000)
        .env("MEOW_HOST_PID", pid.to_string());
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().eq_ignore_ascii_case("PSModulePath") {
            command.env_remove(key);
        }
    }
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
        .await
        .map_err(|_| invalid())?
        .map_err(|_| invalid())?;
    if !output.status.success() || output.stdout.len() > 65536 {
        return Err(invalid());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event() -> serde_json::Value {
        serde_json::json!({"event":"clienthost-ready-v1","version":1,"hostPid":101,"clientPid":102,"executable":"D:/Game/DFO.exe","isolated":true,"networkSelfTest":true,"jobAssigned":true,"resumed":true})
    }
    fn line(value: serde_json::Value) -> String {
        format!("123 CLIENTHOST_READY_V1 {value}\r\n")
    }
    #[test]
    fn strict_plan_has_no_credentials_or_legacy_fallback() {
        let plan = strict_plan(
            Path::new("D:/host.exe"),
            Path::new("D:/Game"),
            Path::new("D:/logs/client.log"),
            Path::new("D:/logs/breakpoints.txt"),
            7002,
        )
        .unwrap();
        assert_eq!(plan.arguments[0], "--host-client-strict");
        assert_eq!(plan.arguments[4], "interactive-ui");
        assert!(plan.arguments[6].contains("127.0.0.4?7002?probe?"));
        assert!(!plan.arguments.iter().any(|a| a == "--host-client"));
        assert!(strict_plan(
            Path::new("host.exe"),
            Path::new("D:/Game"),
            Path::new("D:/log"),
            Path::new("D:/b"),
            7002
        )
        .is_err());
    }
    #[test]
    fn launcher_must_remain_outside_filtered_client_root() {
        assert!(require_bridge_outside_client_root(
            Path::new("D:/Launcher/MeowLauncher.exe"),
            Path::new("D:/Game")
        )
        .is_ok());
        assert!(require_bridge_outside_client_root(
            Path::new("D:/Games/MeowLauncher.exe"),
            Path::new("D:/Game")
        )
        .is_ok());
        assert!(require_bridge_outside_client_root(
            Path::new("d:/GAME/MeowLauncher.exe"),
            Path::new("D:/Game")
        )
        .is_err());
        assert!(require_bridge_outside_client_root(
            Path::new("D:/Game/tools/MeowLauncher.exe"),
            Path::new("D:/Game")
        )
        .is_err());
    }
    #[test]
    fn ready_requires_every_gate_and_exact_process_and_executable() {
        assert!(parse_ready(&line(event()), 101, Path::new("D:/Game/DFO.exe")).is_ok());
        for (key, value) in [
            ("isolated", serde_json::json!(false)),
            ("networkSelfTest", serde_json::json!(false)),
            ("jobAssigned", serde_json::json!(false)),
            ("resumed", serde_json::json!(false)),
            ("hostPid", serde_json::json!(999)),
            ("clientPid", serde_json::json!(101)),
            ("version", serde_json::json!(2)),
            ("executable", serde_json::json!("D:/Other/DFO.exe")),
            ("extra", serde_json::json!(true)),
        ] {
            let mut e = event();
            e[key] = value;
            assert!(
                parse_ready(&line(e), 101, Path::new("D:/Game/DFO.exe")).is_err(),
                "{key}"
            );
        }
        assert!(parse_ready("CLIENTHOST_READY_V1 {}", 101, Path::new("D:/Game/DFO.exe")).is_err());
        assert!(parse_ready(
            "12 CLIENTHOST_READY_V1 {\"event\":\"a\",\"event\":\"b\"}",
            101,
            Path::new("D:/Game/DFO.exe")
        )
        .is_err());
    }
    #[test]
    fn os_proof_rejects_parent_image_and_reused_pid() {
        let ready = parse_ready(&line(event()), 101, Path::new("D:/Game/DFO.exe")).unwrap();
        let make = || {
            vec![
                ProcessObservation {
                    pid: 101,
                    parent_pid: 1,
                    image: "D:/host.exe".into(),
                    created_file_time_utc: "101".into(),
                },
                ProcessObservation {
                    pid: 102,
                    parent_pid: 101,
                    image: "D:/Game/DFO.exe".into(),
                    created_file_time_utc: "102".into(),
                },
            ]
        };
        let check = |rows: &[ProcessObservation]| {
            verify_observations(
                &ready,
                Path::new("D:/host.exe"),
                Path::new("D:/Game/DFO.exe"),
                100,
                200,
                rows,
            )
        };
        assert!(check(&make()).is_ok());
        let mut rows = make();
        rows[1].parent_pid = 999;
        assert!(check(&rows).is_err());
        let mut rows = make();
        rows[1].image = "D:/Other/DFO.exe".into();
        assert!(check(&rows).is_err());
        let mut rows = make();
        rows[0].created_file_time_utc = "99".into();
        assert!(check(&rows).is_err());
        let mut rows = make();
        rows[1].created_file_time_utc = "999".into();
        assert!(check(&rows).is_err());
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn actual_os_observation_handles_polluted_powershell_parent_without_starting_game() {
        use std::os::windows::process::CommandExt;
        let shell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut command = std::process::Command::new(&shell);
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[Threading.Thread]::Sleep(20000)",
            ])
            .creation_flags(0x08000000);
        let mut command = tokio::process::Command::from(command);
        command.kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let pid = child.id().unwrap();
        let previous = std::env::var_os("PSModulePath");
        std::env::set_var(
            "PSModulePath",
            "C:/dev-toolchain/tmp/nonexistent-pwsh7-modules",
        );
        let observed = observe_processes(std::process::id(), pid).await;
        match previous {
            Some(value) => std::env::set_var("PSModulePath", value),
            None => std::env::remove_var("PSModulePath"),
        };
        let _ = child.kill().await;
        let _ = child.wait().await;
        let rows = observed.unwrap();
        let actual = rows.iter().find(|p| p.pid == pid).unwrap();
        assert_eq!(actual.parent_pid, std::process::id());
        assert!(same_path(&actual.image, &shell));
        assert!(actual.created_file_time_utc.parse::<u64>().unwrap() > 0);
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn actual_local_listener_inventory_belongs_to_current_test_process() {
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = socket.local_addr().unwrap().port();
        let observation = observe_local_owner(std::process::id()).await.unwrap();
        assert_eq!(observation.process.pid, std::process::id());
        assert!(observation
            .listeners
            .iter()
            .any(|p| p.address == "127.0.0.1" && p.port == port));
    }
}
