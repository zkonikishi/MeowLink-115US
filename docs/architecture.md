# 架构与边界

MeowLink 的 115US 版本把「多人域」拆成三块：服务端域层（`domain/`）、客户端原生接线（`client/native-party-gate/`）、客户端显示 Mod（`client/legion-party-hud/`）。本仓库是源码快照，**不包含** 115US 上游服务端代码、数据库 schema、协议编码器或游戏资源。

## domain/internal/party —— 组队域

- **Lease**：服务端拥有的「角色选择世代」凭据（不是客户端 token）。它把旧 socket / 旧选择发起的回调栅栏掉：WireID 被复用时，旧 Lease 依然失效。
- **Hub**：跨连接持有队伍成员关系。Hub 不做任何 IO，从不向外部暴露可变的内部 Party 指针；Join 只能由一条仍然有效、精确匹配的邀请授权，网络通知由调用方负责发送。
- **Directory / Registry / Settings / Notices / Watch**：招募目录、队伍注册表、组队设置、通知与观察者。
- **raid_\***：团本营地（camp）、重入（reentry）、保留位（retained）、投影（projection）、回家恢复（home recovery）。
- **moon_progress / controlled_actors / public_profile**：月色进度、受控角色、公开资料。

## domain/internal/ordinary —— 普通世界共享域

把既有 party Hub 接到「普通共享 run」owner 上：可信宿主提供已认证的选中角色绑定；本包**不认证 socket、不编码客户端包**。典型流程是 Bind → Invite/Respond → 观察队伍变化 → 为该队伍创建/更新共享房间。

## domain/internal/dungeon —— 副本共享域

- `SharedStamp{RunID, Room}` 标识一次共享 run / 房间；`SharedPhase` 为 Loading / Running / Aborted；`SharedSnapshot` 携带首领与成员 Lease、4 个座位、来源、地图、准入计数与原因。
- `WithRoomCommit` 是提交守卫：锁顺序为 **host 排序后的 auth account Lease + 完整选中集合 → run.mu → 原始 Store SQL 事务**；回调内只允许有界 SQL / 纯值，禁止回入 run、Hub、网络或观察者。
- 幂等键是 run + room/map；不包含 attempt nonce 或 confirm/refund 协议。

## domain/internal/partywire —— 协议体离线解码

只解码已经单独证明过的 B823 逻辑体（N7/N8 mode 0，profile 由 SHA256 固定）：不做传输填充推断、不做原生成功投影、不做授权判定。它用于离线复核，不是线上编解码器。

## 客户端

### client/native-party-gate（C++）

在原生「选择门控」里**只替换成员本地性（nearby）块**，其余选择条件保持原生成功路径。gate 只在已验证的游戏线程上替换；旧的小快照保留到进程退出，避免钩子与释放竞态。头文件另外暴露只读的 override 供隔离 fixture 使用，以及本地 peer 完成、门名单过滤、转场回调等辅助入口。

### client/legion-party-hud（C）

只隐藏特定军团副本里的**纯空位**：不改变名册、容量、入场限制、血量或奖励；真实角色（包括已死亡队员）照常绘制。补丁只替换原生绘制调用点，保留原生栈、返回地址与非易失寄存器；启动时校验五处指令，窗口管理器已存在则拒绝迟加载。使用本地 ModStart 宿主加载，状态写入 `.115us-mods/legion-party-hud-<PID>.status.json`。

## 非目标（边界）

- 不做 socket 认证、不编码客户端包、不实现账号体系或支付——这些属于宿主 / 平台 / 上游服务端。
- 不拥有游戏状态：角色、好友、会话、资产交易的权威仍在服务端与平台。
- 不修改磁盘上的游戏文件（客户端补丁只改运行时内存）。
- 实机联机（原生协议接线、实机组队/进图、生产准入）**尚未完成验收**，本仓库不作可用性承诺。
