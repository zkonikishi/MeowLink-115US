# 客户端维护与开发控制：仅源码审查归档

作者 zkonikishi。本目录保留权利，不授予新增开源许可证；不适用 launcher/ 的 AGPL。不是已发布 MeowLink 功能，不直接接入游戏或生产。

- `ime-context/`：上下文/bootstrap 候选。历史 MSVC、fixture、隐藏 HWND/IMM 检查通过；缺少可信游戏 HWND/门禁接线及现代 IME 验收。公开注册消息可被外部触发，宿主授权请求硬化未完成，不能直接启用。
- `wording-transaction/`：参数化本地安装/回滚候选，只完成 Python AST 检查。需要部署方自己的兼容性文件、注册表快照、原件备份；没有资源文件。guard 使用 assert，python -O 会删除检查，不能作为生产安全安装器。仍保留 wording17 的历史范围，不是通用 modkit。
- `server-devcontrol/`：依赖另行提供的 meowlink/devtest API v1。历史 18 个控制测试与 2 个宿主 overlay 测试通过；本目录没有完整模块或宿主管理会话/RBAC适配，默认禁用，不支持独立构建。

逐文件 SHA、历史验证和未完项见 source-export.json。缺失的实机/网络证据不能判通过。C0000374 根因未解决；相关私有 dump、会话日志、游戏资源、地址表和二进制均未上传。
