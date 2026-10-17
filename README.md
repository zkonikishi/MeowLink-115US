# MeowLink-115US

MeowLink 是面向 **115US**（DNF 86 级版本 115US 服务端）的多人联机插件：把组队、同城镇可见、普通世界共享与副本共享这些「多人域」能力做成可接入服务端与客户端的一层。

本仓库是 **赞助版 MeowLink（product id `meowlink`）的 115US 域层源码快照**。定制版 MeowLink+（product id `meowlink+`）的源码不在本仓库、也不公开。

本仓库**只包含本项目自有代码**，不包含 115US 上游服务端源码、不包含游戏本体/游戏资源（PVF 等）、不包含客户端二进制，也不包含预编译宿主程序。

## 内容

| 目录 | 内容 |
| --- | --- |
| `domain/` | Go 域层（module `dfolan`，Go 1.26）：组队、普通世界、副本共享与协议体的离线解码 |
| `client/native-party-gate/` | 客户端原生组队门控的 C++ 补丁：只替换「成员本地性」判断，保留原生成功路径 |
| `client/legion-party-hud/` | 军团真实队员 HUD 客户端 Mod（C，本地 ModStart 宿主加载） |
| `docs/` | 架构与接入说明、域文件导入清单 |

### domain/

- `internal/party` — 组队域：Hub、注册表、租约（lease）、招募目录、组队设置、通知、观察者、团本营地/重入/保留/投影、月色进度、受控角色、公开资料。
- `internal/ordinary` — 普通世界共享域（2..4 人）：以 `dungeon.WithRoomCommit` 约束事务与锁顺序。
- `internal/dungeon` — 副本共享域：共享戳（stamp）与房间提交守卫。
- `internal/partywire` — B823 协议逻辑体的**离线解码**：只解码已单独证明的逻辑体，不做传输填充推断、不做成功投影、不做授权判定。

## 构建与测试

```bash
cd domain
go vet ./...
go test ./...
```

要求 Go 1.26。域层只依赖标准库与自身内部包，可独立编译测试（2026-10-17 本地 `go vet` 与 `go test ./...` 全部通过）。

## 状态与边界

- 域层是可独立测试的源码快照；**实机联机验收（原生协议接线、实机组队/进图、生产准入）尚未完成**，本仓库不作任何可用性承诺。
- 域层不拥有游戏状态：角色、会话、好友、资产交易的权威仍在服务端/平台；本仓库不包含账号体系、支付或部署配置。
- 客户端补丁只修改运行时进程内存中的显示/门控逻辑，不修改磁盘上的游戏文件。
- 接入方式见 [`docs/integration.md`](docs/integration.md)；架构与边界见 [`docs/architecture.md`](docs/architecture.md)。

## 许可

**非商业使用**：仅供免费技术交流与个人学习研究，禁止任何形式的商业化使用。详见 [`NOTICE.txt`](NOTICE.txt)。
