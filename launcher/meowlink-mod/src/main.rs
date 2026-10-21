use meowlink_companion::{web, Config};
use std::{path::PathBuf, sync::Arc};
use tokio::{net::TcpListener, sync::watch};

fn browser(url: &str) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "shell32")]
        extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show: i32,
            ) -> isize;
        }
        let wide = |s: &str| {
            std::ffi::OsStr::new(s)
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>()
        };
        let operation = wide("open");
        let target = wide(url);
        return unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        } > 32;
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        false
    }
}
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{}: {}", error.code, error.message);
        std::process::exit(1);
    }
}
async fn run() -> meowlink_companion::Result<()> {
    let mut root = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .ok_or_else(|| meowlink_companion::error("path", "无法定位 Mod 目录。"))?;
    let mut open = true;
    let mut check = false;
    let mut managed = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--root" {
            root = PathBuf::from(
                args.next()
                    .ok_or_else(|| meowlink_companion::error("arguments", "--root 需要目录。"))?,
            );
        } else if arg == "--no-open" {
            open = false;
        } else if arg == "--check-config" {
            check = true;
        } else if arg == "--launcher-mod" {
            managed = true;
            open = false;
        } else {
            return Err(meowlink_companion::error(
                "arguments",
                "支持 --root <绝对目录>、--no-open、--check-config。密码不能通过命令行传入。",
            ));
        }
    }
    if !root.is_absolute() {
        return Err(meowlink_companion::error(
            "path",
            "Mod 根目录必须为绝对路径。",
        ));
    }
    std::fs::create_dir_all(&root)
        .map_err(|_| meowlink_companion::error("path", "无法创建 Mod 配置目录。"))?;
    if check {
        let config = Config::read(&root.join("config.json"))?;
        let mapping = config.mapping()?;
        println!("{{\"configurationValid\":true,\"channelCount\":{},\"networkTested\":false,\"gameStarted\":false}}",mapping.games.len());
        return Ok(());
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| meowlink_companion::error("http", "无法启动本机配置页面。"))?;
    let port = listener
        .local_addr()
        .map_err(|_| meowlink_companion::error("http", "无法获取本机配置端口。"))?
        .port();
    let (shutdown, receiver) = watch::channel(false);
    let state = Arc::new(web::State::new(
        root,
        web::random_token()?,
        port,
        shutdown.clone(),
        managed,
    ));
    let url = format!("{}/", state.origin);
    if managed {
        println!(
            "LAUNCHER_MOD_READY_V1 {}",
            serde_json::json!({"schema":"launcher-mod-ready-v1","pid":std::process::id(),"url":url})
        );
        let stop = shutdown.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines() {
                match line {
                    Ok(line) if line.trim() == "shutdown" => break,
                    Err(_) => break,
                    _ => {}
                }
            }
            let _ = stop.send(true);
        });
    } else {
        println!("MeowLink Companion {} | zkonikishi",env!("CARGO_PKG_VERSION"));
        println!("独立运行；关闭网页不会停止程序，请使用页面退出或 Ctrl+C。");
        println!("{url}");
    }
    if open && !browser(&url) {
        eprintln!("请手动打开上面的本机地址。");
    }
    let signal = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = shutdown.send(true);
        }
    });
    let result = web::serve(listener, state, receiver).await;
    signal.abort();
    result
}
