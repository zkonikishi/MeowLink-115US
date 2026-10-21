use crate::{
    client_host, error,
    generic_bridges::{probe_directory_tls, DirectoryTransport, LoggedIn, RunningBridges},
    generic_target::parse_remote_artifact,
    Config, Result,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::Path,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::{AsyncReadExt, BufReader},
    sync::watch,
    task::JoinHandle,
};
const HOST_SHA: &str = "dd6ac277206e5f29e24e0d5e1f332842b540d74949b73d1af3a4e7226982b1f8";
const CLIENT_SHA: &str = "b8235efe5dbf140ea4c535a026e137c4b34cfaecd7c97c78ed5997c377327c95";
fn invalid() -> crate::AppError {
    error(
        "native_integrity",
        "客户端宿主或原生就绪核验失败，未报告进服。",
    )
}
fn pin(path: &Path, expected: &str) -> Result<File> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(invalid());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(path).map_err(|_| invalid())?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 65536];
    loop {
        let n = file.read(&mut bytes).map_err(|_| invalid())?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    if format!("{:x}", hash.finalize()) != expected {
        return Err(invalid());
    }
    Ok(file)
}
fn file_time() -> Result<u64> {
    Ok((SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid())?
        .as_nanos()
        / 100) as u64
        + 116444736000000000)
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub instance_id: String,
    pub host_pid: u32,
    pub client_pid: u32,
    pub stage: &'static str,
}
pub struct Running {
    pub started: Started,
    cancel: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
    failed: Arc<AtomicBool>,
}
impl Running {
    pub fn active(&self) -> bool {
        self.task.as_ref().is_some_and(|t| !t.is_finished())
    }
    pub fn quarantined(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }
    pub async fn stop(&mut self) -> Result<()> {
        let _ = self.cancel.send(true);
        if let Some(mut task) = self.task.take() {
            match tokio::time::timeout(Duration::from_secs(12), &mut task).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    self.failed.store(true, Ordering::SeqCst);
                }
                Err(_) => {
                    self.task = Some(task);
                    self.failed.store(true, Ordering::SeqCst);
                    return Err(error("cleanup_failed", "实例清理未确认，禁止复用端口。"));
                }
            }
        }
        if self.quarantined() {
            return Err(error(
                "cleanup_failed",
                "客户端宿主清理失败，请检查实例日志。",
            ));
        }
        Ok(())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}
pub async fn start(config: &Config, account: Arc<LoggedIn>, data: &Path) -> Result<Running> {
    #[cfg(windows)]
    {
        #[link(name = "shell32")]
        extern "system" {
            fn IsUserAnAdmin() -> i32;
        }
        if unsafe { IsUserAnAdmin() } == 0 {
            return Err(error("native_admin", "严格客户端隔离需要管理员权限。"));
        }
    }
    #[cfg(not(windows))]
    {
        return Err(error("native_platform", "仅支持 Windows 原生客户端。"));
    }
    let host = config
        .host_executable
        .canonicalize()
        .map_err(|_| invalid())?;
    let directory = config
        .client_directory
        .canonicalize()
        .map_err(|_| invalid())?;
    let launcher = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|_| invalid())?;
    client_host::require_bridge_outside_client_root(&launcher, &directory)?;
    let client = directory.join("DFO.exe");
    let guards = vec![pin(&host, HOST_SHA)?, pin(&client, CLIENT_SHA)?];
    let meta = std::fs::symlink_metadata(&config.player_target_file).map_err(|_| invalid())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 65536 {
        return Err(invalid());
    }
    let raw = std::fs::read(&config.player_target_file).map_err(|_| invalid())?;
    let mapping = parse_remote_artifact(&raw, &config.target)?;
    account.refresh().await?;
    probe_directory_tls(&mapping.directory.upstream).await?;
    let bridges = RunningBridges::start_transport(
        account,
        mapping.games,
        7002,
        Some(DirectoryTransport::Tls(mapping.directory)),
    )
    .await?;
    let instance_id = format!(
        "meowlink-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| invalid())?
            .as_nanos()
    );
    let logs = data.join(&instance_id);
    std::fs::create_dir_all(&logs).map_err(|_| invalid())?;
    let plan = client_host::strict_plan(
        &host,
        &directory,
        &logs.join("client.log"),
        &logs.join("breakpoints.txt"),
        7002,
    )?;
    let stderr = File::create(logs.join("host.stderr.log")).map_err(|_| invalid())?;
    let mut command = std::process::Command::new(&plan.executable);
    command
        .args(&plan.arguments)
        .current_dir(&plan.working_directory)
        .env_remove("DFO_ACCOUNT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let before = file_time()?;
    let mut child = command.spawn().map_err(|_| invalid())?;
    let pid = child.id().ok_or_else(invalid)?;
    let mut reader = BufReader::new(child.stdout.take().ok_or_else(invalid)?);
    let ready = tokio::time::timeout(Duration::from_secs(35), async {
        loop {
            let mut line = Vec::new();
            loop {
                let b = reader.read_u8().await.map_err(|_| invalid())?;
                if b == b'\n' {
                    break;
                }
                if line.len() >= 16384 {
                    return Err(invalid());
                }
                line.push(b);
            }
            let line = String::from_utf8(line).map_err(|_| invalid())?;
            if line.contains("CLIENTHOST_READY_V1") {
                return client_host::parse_ready(&line, pid, &client);
            }
        }
    })
    .await;
    let announced = match ready {
        Ok(Ok(v)) => v,
        _ => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            bridges.close().await;
            return Err(invalid());
        }
    };
    #[cfg(windows)]
    {
        let verified = match client_host::observe_processes(pid, announced.client_pid).await {
            Ok(rows) => client_host::verify_observations(
                &announced,
                &host,
                &client,
                before,
                file_time()?,
                &rows,
            ),
            Err(e) => Err(e),
        };
        if let Err(e) = verified {
            let _ = child.kill().await;
            let _ = child.wait().await;
            bridges.close().await;
            return Err(e);
        }
    }
    let started = Started {
        instance_id,
        host_pid: pid,
        client_pid: announced.client_pid,
        stage: "verified-native-process",
    };

    let (cancel, mut receiver) = watch::channel(false);
    let failed = Arc::new(AtomicBool::new(false));
    let failure = failed.clone();
    let task = tokio::spawn(async move {
        let _guards = guards;
        // Drain all later stdout so diagnostic output cannot stall the native host.
        let drain = tokio::spawn(async move {
            loop {
                let mut b = [0u8; 4096];
                if reader.read(&mut b).await.unwrap_or(0) == 0 {
                    break;
                }
            }
        });
        let (result, bridge_failed) = tokio::select! {
            value=child.wait()=>(value,false),
            _=receiver.changed()=>{let _=child.kill().await;(child.wait().await,false)},
            _=async {while bridges.is_running(){tokio::time::sleep(Duration::from_millis(250)).await;}}=>{
                let _=child.kill().await;(child.wait().await,true)
            }
        };
        let bad = bridge_failed
            || result
                .as_ref()
                .map(|s| s.code() == Some(13))
                .unwrap_or(true);
        failure.store(bad, Ordering::SeqCst);
        bridges.close().await;
        drain.abort();
    });
    Ok(Running {
        started,
        cancel,
        task: Some(task),
        failed,
    })
}
