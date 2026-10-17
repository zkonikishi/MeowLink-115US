# 接入说明（115US 服务端）

本仓库是源码快照，不是可直接运行的发行物。下面说明它按什么假设接入 115US 服务端与客户端。

## 1. 前置

- 一份 115US 服务端 Go 源码树（本仓库不包含，也不分发）。
- Go 1.26。
- 客户端侧若要使用两个补丁，需要对应的客户端补丁宿主（见第 3 节），本仓库不提供宿主与游戏文件。

## 2. 服务端域层

1. 将 `domain/internal/{party,ordinary,dungeon,partywire}` 放入服务端模块的 `internal/` 下，并把源码里的 `meowlink/internal/` 导入前缀改写为该服务端模块的 module 路径（本仓库的 module 名 `meowlink` 只用于独立编译测试）。
2. 接线点（按服务端实际代码调整）：
   - **角色选定 / 切频道**：`party.Hub.Bind` / `Unbind` / `IsCurrent`；旧 Lease 必须失效并触发清理（`lifecycle_candidate_test.go` 覆盖了切频道拒绝旧回复的语义）。
   - **组队协议**：`Invite` / `Respond` 等；Join 只由有效且精确匹配的邀请授权。
   - **进图**：`ordinary.Service` 或 `dungeon` 的 shared run 创建；`SharedSnapshot` 交给服务端编码成原生包广播。
   - **返回城镇 / 断线重连**：`raid_home_recovery.go`、`raid_reentry.go`、`raid_retained.go` 提供对应保留与恢复语义。
3. 锁顺序与事务：`dungeon.WithRoomCommit` 要求 **host 排序后的 auth account Lease + 完整选中集合 → run.mu → 原始 Store SQL 事务**；SQL 回调必须是短的有界操作，禁止回入 Hub / 网络 / 观察者。
4. 幂等：以 run + room/map 为键。不要引入未经验证的 nonce 或 confirm/refund 协议。

双模式（自定义模式 / 通用模式）的差异只在账号与认证接入层：域层本身不认证 socket、不编码客户端包，也不依赖 Meowopia 平台身份。

## 3. 客户端

- **client/native-party-gate**：在原生选择门控入口安装「成员本地性」块（33 字节补丁 + 钩子，按已证明的 prologue 复制并登记 unwind 信息）。只在游戏线程替换 gate；任何校验失败都必须 fail closed，保留原生行为。安装入口是 `install_native_party_nearby_gate` / `arm_native_party_nearby_gate`。
- **client/legion-party-hud**：由本地 ModStart 宿主加载（`build-mod.py` / `build-plugin.cmd` 产出 Mod 包），状态写 `.115us-mods/legion-party-hud-<PID>.status.json`。只改显示层。

## 4. 验证

```bash
cd domain
go vet ./...
go test ./...
```

实机验收清单（本仓库未宣称已完成）：两人组队进图、队员死亡、断线重连、返回等待室、切频道后旧 Lease 拒答、普通世界 2..4 人共享、同城镇成员可见。

## 5. 来源与可复现性

- 域层提取自 115US 服务端工作树（快照 `meowlink-preview-r1`，2026-10-16；对应服务端提交 `6302409c42f7c7c14ffbc2007c7085c946abf8ad`），上游为 `gitgud.io/fuckworld/115`（HEAD `329525600f8014b3726d3319114455e2e56bc316`）。
- 发布副本除 Go module / 导入路径改写外，保持源文件内容；文件路径、字节数与 SHA256 见 `docs/evidence/domain-manifest.json`，可逐项对照校验：

```bash
# Git Bash / WSL
sha256sum domain/internal/party/hub.go
```

- `domain/internal/partywire/` 与其余域文件同源，一并列入该清单。

## 6. 已知限制

- 不含 SQL schema / database 层；`ordinary` 域要求宿主提供 Store 事务。
- 不含原生协议编解码器、不认证 socket。
- 不含上游服务端源码与游戏资源，不能单独运行成完整服务端。
- 客户端偏移、宿主接口与游戏版本绑定，升级后必须重新验证。
