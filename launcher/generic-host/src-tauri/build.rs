//! 打包期注入 Windows 应用清单。
//!
//! 分级刻意跟着构建配置走：发布件要 `highestAvailable`（网关的 WFP 回环隔离需要管理员，
//! 拿不到时编排 CLI 自己降级），开发件必须 `asInvoker` —— 一旦 dev 构建提权，
//! `tauri dev` 的 stdout 管道与非提权的前端进程会被 UAC 切断，热重载直接哑掉。
//! 清单是整体替换的，所以 Common-Controls v6 依赖（dialog 插件要用）必须自己带上。

fn main() {
    // tauri-build 只盯 tauri.conf.json，换图标不会让脚本重跑：编好的 resource.lib 会被原样
    // 链进新 exe，图标静默留在旧版。必须自己声明依赖。
    println!("cargo:rerun-if-changed=icons/icon.ico");

    let mut attributes = tauri_build::Attributes::new();

    #[cfg(windows)]
    {
        let level = if std::env::var("PROFILE").as_deref() == Ok("release") {
            "highestAvailable"
        } else {
            "asInvoker"
        };
        attributes = attributes.windows_attributes(
            tauri_build::WindowsAttributes::new().app_manifest(&format!(
                r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v2">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="{level}" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#,
            )),
        );
    }

    tauri_build::try_build(attributes).expect("Tauri 构建脚本失败");
}
