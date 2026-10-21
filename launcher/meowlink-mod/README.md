# MeowLink 通用 Mod 0.1.1（候选）

作者 zkonikishi，AGPL-3.0-only。通用模式 Rust 接入从 MeowLauncher Beta 通用实现抽取，加入独立进程和配置网页；不含私有平台接入。

Windows / Rust / MSVC：`cargo test --locked`，`cargo build --locked`；Node：`node --test tests/ui.test.cjs`。HTTP 测试用 `MEOWLINK_EXE` 指向构建后的 EXE，执行 `python tests/http_smoke.py`；managed 模式再设置 `MEOWLINK_MANAGED_TEST=1`。Go 集成 fixture 仅供接入完整服务端模块后使用，不是本仓库可独立运行的 Go 测试。

运行 EXE 可用 `--root <独立数据目录>`，嵌入宿主使用 `--launcher-mod --root <数据目录>`。此模式只输出协议 readiness 并接受 stdin shutdown，不打开浏览器。设置中填写目标服务器 IP/DNS、TLS 域名及 CA、authority/realm/save profile、频道描述文件和游戏目录。目标服务端必须显式开放通用模式，不能把现有私有平台端口当作通用入口。

原生启动依赖独立的 MeowClientHost.exe（本仓库不分发）。固定期望 SHA256 为 dd6ac277206e5f29e24e0d5e1f332842b540d74949b73d1af3a4e7226982b1f8；支持客户端 SHA256 为 b8235efe5dbf140ea4c535a026e137c4b34cfaecd7c97c78ed5997c377327c95。必须实现 CLIENTHOST_READY_V1 严格 PID/路径/隔离就绪协议；不匹配时拒绝启动，不提供绕过开关。需另行取得有许可的实现；没有该宿主仍可构建与验证配置/认证，不能声称可进服。

宿主协议见 ../generic-host/docs/launcher-mod-v1.json。包清单需包含实际构建文件的大小与 SHA256；不包含账号密码、私有 CA 或生产目标。程序更新与数据目录分离。实机、跨电脑、实际 WFP 行为与原生界面视觉尚未验收。
