use crate::{package, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

#[cfg(windows)]
struct Job(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for Job {}
#[cfg(windows)]
impl Job {
    fn finish(&self) -> Result<()> {
        use windows::Win32::System::JobObjects::*;
        unsafe {
            TerminateJobObject(self.0, 0).map_err(|_| "无法终止自有 Mod Job。".to_string())?;
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                QueryInformationJobObject(
                    self.0,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut _,
                    std::mem::size_of_val(&info) as u32,
                    None,
                )
                .map_err(|_| "无法核验自有 Mod Job 清理。".to_string())?;
                if info.ActiveProcesses == 0 {
                    return Ok(());
                }
                if Instant::now() >= until {
                    return Err("Mod 子进程清理未确认，拒绝复用。".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(windows)]
fn assign_job(child: &Child) -> Result<Job> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{Foundation::HANDLE, System::JobObjects::*};
    unsafe {
        let job =
            Job(CreateJobObjectW(None, None)
                .map_err(|_| "无法创建 Mod 进程隔离 Job。".to_string())?);
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of_val(&info) as u32,
        )
        .map_err(|_| "无法配置 Mod 进程清理。".to_string())?;
        AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle()))
            .map_err(|_| "Mod 进程未加入受控 Job，拒绝打开配置页。".to_string())?;
        Ok(job)
    }
}
#[cfg(not(windows))]
struct Job;
#[cfg(not(windows))]
impl Job {
    fn finish(&self) -> Result<()> {
        Err("仅支持 Windows。".into())
    }
}
#[cfg(not(windows))]
fn assign_job(_child: &Child) -> Result<Job> {
    Err("通用 Mod 原生进程宿主目前仅支持 Windows。".into())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    schema: String,
    pid: u32,
    url: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub capabilities: Vec<String>,
    pub trust: String,
    pub running: bool,
    pub settings_url: Option<String>,
    pub previous_version: Option<String>,
    pub problem: Option<String>,
}
pub struct Running {
    child: Child,
    endpoint: String,
    _pin: File,
    _job: Job,
}
impl Running {
    fn active(&mut self) -> Result<bool> {
        let active = self
            .child
            .try_wait()
            .map(|v| v.is_none())
            .map_err(|_| "无法核验 Mod 进程状态。".to_string())?;
        if !active {
            self._job.finish()?;
        }
        Ok(active)
    }
    fn stop(&mut self) -> Result<()> {
        if self.active()? {
            if let Some(stdin) = &mut self.child.stdin {
                let _ = stdin.write_all(b"shutdown\n");
                let _ = stdin.flush();
            }
            let until = Instant::now() + Duration::from_secs(50);
            while self.active()? && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        self._job.finish()?;
        self.child
            .wait()
            .map_err(|_| "Mod 进程退出未确认。".to_string())?;
        Ok(())
    }
}
#[derive(Default)]
pub struct Manager {
    running: HashMap<PathBuf, Running>,
}
pub static MANAGER: LazyLock<Mutex<Manager>> = LazyLock::new(|| Mutex::new(Manager::default()));
pub fn lock() -> Result<std::sync::MutexGuard<'static, Manager>> {
    MANAGER
        .lock()
        .map_err(|_| "Mod 宿主状态异常，拒绝继续。".into())
}
fn local_url(url: &str) -> bool {
    url.strip_prefix("http://127.0.0.1:")
        .and_then(|s| s.strip_suffix('/'))
        .and_then(|s| s.parse::<u16>().ok())
        .is_some_and(|p| p != 0)
}
#[cfg(windows)]
fn owns_listener(pid: u32, port: u16) -> Result<bool> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, TCP_TABLE_OWNER_PID_LISTENER,
    };
    let mut size = 0u32;
    unsafe {
        GetExtendedTcpTable(None, &mut size, false, 2, TCP_TABLE_OWNER_PID_LISTENER, 0);
    }
    if size < 4 || size > 4 * 1024 * 1024 {
        return Err("Mod 监听端口清单无效。".into());
    }
    let mut bytes = vec![0u8; size as usize];
    if unsafe {
        GetExtendedTcpTable(
            Some(bytes.as_mut_ptr() as *mut _),
            &mut size,
            false,
            2,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    } != 0
    {
        return Err("无法核验 Mod 监听端口归属。".into());
    }
    let count = u32::from_ne_bytes(bytes[..4].try_into().unwrap()) as usize;
    if count > (bytes.len() - 4) / 24 {
        return Err("Mod 监听端口清单截断。".into());
    }
    for row in bytes[4..].chunks_exact(24).take(count) {
        let word = |i: usize| u32::from_ne_bytes(row[i * 4..i * 4 + 4].try_into().unwrap());
        if word(0) == 2
            && word(1) == u32::from_ne_bytes([127, 0, 0, 1])
            && u16::from_be(word(2) as u16) == port
            && word(5) == pid
        {
            return Ok(true);
        }
    }
    Ok(false)
}
#[cfg(not(windows))]
fn owns_listener(_pid: u32, _port: u16) -> Result<bool> {
    Err("仅支持 Windows。".into())
}
fn pin(path: &Path, expected: &str) -> Result<File> {
    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        opts.share_mode(1);
    }
    let mut file = opts
        .open(path)
        .map_err(|_| "无法打开 Mod 入口。".to_string())?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取 Mod 入口。".to_string())?;
    if package::digest(&bytes) != expected.to_ascii_lowercase() {
        return Err("Mod 入口在校验后被修改，拒绝启动。".into());
    }
    Ok(file)
}
fn spawn_ready(mut command: Command, pin: File, log: &Path) -> Result<Running> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(
            File::create(log).map_err(|_| "无法创建 Mod 日志。".to_string())?,
        ));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "无法启动 Mod 配置进程。".to_string())?;
    let job = match assign_job(&child) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Mod 未提供就绪管道。".to_string())?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let pid = child.id();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut total = 0usize;
        loop {
            let mut line = Vec::new();
            let mut part = std::io::Read::by_ref(&mut reader).take(4097);
            if part.read_until(b'\n', &mut line).unwrap_or(0) == 0 {
                break;
            }
            total += line.len();
            if line.len() > 4096 || total > 16384 {
                break;
            }
            let Ok(text) = std::str::from_utf8(&line) else {
                break;
            };
            if let Some(json) = text.trim().strip_prefix("LAUNCHER_MOD_READY_V1 ") {
                let ready = serde_json::from_str::<Ready>(json).ok();
                let _ = tx.send(ready);
                let _ = std::io::copy(&mut reader, &mut std::io::sink());
                return;
            }
        }
        let _ = tx.send(None);
    });
    let ready = rx.recv_timeout(Duration::from_secs(10)).ok().flatten();
    let Some(ready) =
        ready.filter(|r| r.schema == "launcher-mod-ready-v1" && r.pid == pid && local_url(&r.url))
    else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Mod 就绪响应无效或超时，未打开配置页。".into());
    };
    let port = ready
        .url
        .strip_prefix("http://127.0.0.1:")
        .and_then(|s| s.strip_suffix('/'))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| "Mod 端口无效。".to_string())?;
    if !owns_listener(pid, port)? {
        let _ = child.kill();
        let _ = child.wait();
        return Err("配置页监听端口不属于本次 Mod 进程，拒绝载入。".into());
    }
    if child
        .try_wait()
        .map_err(|_| "无法核验 Mod 存活状态。".to_string())?
        .is_some()
    {
        return Err("Mod 就绪后已退出。".into());
    }
    Ok(Running {
        child,
        endpoint: ready.url,
        _pin: pin,
        _job: job,
    })
}
impl Manager {
    fn key(root: &Path, id: &str) -> Result<PathBuf> {
        package::current(root, id)
    }
    fn refresh(&mut self) -> Result<()> {
        let mut stopped = Vec::new();
        for (key, value) in self.running.iter_mut() {
            if !value.active()? {
                stopped.push(key.clone())
            }
        }
        for key in stopped {
            self.running.remove(&key);
        }
        Ok(())
    }
    pub fn any_active(&mut self) -> Result<bool> {
        self.refresh()?;
        Ok(!self.running.is_empty())
    }
    pub fn idle(&mut self, root: &Path, id: &str) -> Result<()> {
        self.refresh()?;
        if self.running.contains_key(&Self::key(root, id)?) {
            return Err("Mod 配置或游戏实例仍在运行，请先停止后再更新、回滚或卸载。".into());
        }
        Ok(())
    }
    pub fn list(&mut self, root: &Path) -> Result<Vec<View>> {
        self.refresh()?;
        let packages = root.join("packages");
        if !packages.exists() {
            return Ok(Vec::new());
        }
        let mut rows = Vec::new();
        for entry in fs::read_dir(&packages)
            .map_err(|_| "无法读取 Mod 列表。".to_string())?
            .take(128)
        {
            let entry = entry.map_err(|_| "读取 Mod 条目失败。".to_string())?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !package::valid_id(&id) {
                continue;
            }
            package::recover(root, &id)?;
            let path = Self::key(root, &id)?;
            if !path.exists() {
                continue;
            }
            match package::approved_metadata(&path) {
                Ok(p) => {
                    let running = self.running.get(&path);
                    let previous = package::approved_metadata(&entry.path().join("previous"))
                        .ok()
                        .map(|p| p.manifest.version);
                    rows.push(View {
                        id,
                        name: p.manifest.name,
                        version: p.manifest.version,
                        author: p.manifest.author,
                        capabilities: p.manifest.capabilities,
                        trust: p.trust,
                        running: running.is_some(),
                        settings_url: running.map(|r| r.endpoint.clone()),
                        previous_version: previous,
                        problem: None,
                    });
                }
                Err(e) => rows.push(View {
                    id: id.clone(),
                    name: id,
                    version: String::new(),
                    author: String::new(),
                    capabilities: Vec::new(),
                    trust: "invalid".into(),
                    running: false,
                    settings_url: None,
                    previous_version: None,
                    problem: Some(e),
                }),
            }
        }
        Ok(rows)
    }
    pub fn open(&mut self, root: &Path, id: &str) -> Result<String> {
        self.refresh()?;
        let key = Self::key(root, id)?;
        if let Some(running) = self.running.get(&key) {
            return Ok(running.endpoint.clone());
        }
        if self.any_active()? {
            return Err("请先停止当前启动器 Mod 实例，避免多个 Mod 争用游戏桥接。".into());
        }
        let p = package::approved(&key)?;
        if p.manifest.id != id {
            return Err("包 ID 与安装目录不一致。".into());
        }
        let entry = key.join(package::relative(&p.manifest.entrypoint)?);
        let expected = p
            .manifest
            .files
            .iter()
            .find(|f| f.path == p.manifest.entrypoint)
            .ok_or_else(|| "入口未登记。".to_string())?;
        let guard = pin(&entry, &expected.sha256)?;
        let data = root.join("data").join(id);
        fs::create_dir_all(&data).map_err(|_| "无法创建 Mod 数据目录。".to_string())?;
        let mut command = Command::new(&entry);
        command.env_clear();
        for name in [
            "SystemRoot",
            "WINDIR",
            "TEMP",
            "TMP",
            "PATH",
            "LOCALAPPDATA",
            "APPDATA",
            "USERPROFILE",
            "USERNAME",
            "OS",
            "NUMBER_OF_PROCESSORS",
            "PROCESSOR_ARCHITECTURE",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramData",
            "COMSPEC",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .args(["--launcher-mod", "--root"])
            .arg(&data)
            .current_dir(&key)
            .env_remove("DFO_ACCOUNT");
        let running = spawn_ready(command, guard, &data.join("launcher-mod.stderr.log"))?;
        let url = running.endpoint.clone();
        self.running.insert(key, running);
        Ok(url)
    }
    pub fn stop(&mut self, root: &Path, id: &str) -> Result<()> {
        let key = Self::key(root, id)?;
        if let Some(running) = self.running.get_mut(&key) {
            running.stop()?;
        }
        self.running.remove(&key);
        Ok(())
    }
    pub fn stop_all(&mut self) -> Result<()> {
        let keys = self.running.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            if let Some(r) = self.running.get_mut(&key) {
                r.stop()?;
            }
            self.running.remove(&key);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn actual_listener_inventory_is_bound_to_the_owned_pid() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(owns_listener(std::process::id(), port).unwrap());
        assert!(!owns_listener(std::process::id() + 1, port).unwrap());
    }
    #[cfg(windows)]
    #[test]
    fn actual_owned_job_ready_pipe_and_graceful_shutdown_without_game() {
        let shell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let log = std::env::temp_dir().join(format!(
            "launcher-mod-process-test-{}.log",
            std::process::id()
        ));
        let script = r#"$testListener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Parse('127.0.0.1'),0);$testListener.Start();[Console]::WriteLine(('LAUNCHER_MOD_READY_V1 {{"schema":"launcher-mod-ready-v1","pid":{0},"url":"http://127.0.0.1:{1}/"}}' -f $PID,$testListener.LocalEndpoint.Port));$controlLine=[Console]::ReadLine();$testListener.Stop()"#;
        let mut command = Command::new(&shell);
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env_remove("PSModulePath");
        let mut running = spawn_ready(command, File::open(&shell).unwrap(), &log).unwrap();
        assert!(running.active().unwrap());
        assert!(local_url(&running.endpoint));
        running.stop().unwrap();
        assert!(!running.active().unwrap());
        drop(running);
        let _ = fs::remove_file(log);
    }
    #[test]
    fn ready_url_cannot_select_remote_or_credential_urls() {
        for url in [
            "https://127.0.0.1:9/",
            "http://example.com/",
            "http://127.0.0.1:0/",
            "http://127.0.0.1:80/?token=secret",
            "http://127.0.0.1:80@evil/",
        ] {
            assert!(!local_url(url))
        }
        assert!(local_url("http://127.0.0.1:50000/"));
    }
}
