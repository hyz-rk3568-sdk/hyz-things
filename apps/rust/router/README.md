# hyz-router

`hyz-router` 是 RK3568 产品上**无头**（headless）的路由控制核心。它以单一 Linux ELF 提供管理 LAN、WAN DHCP、IPv4 转发/NAT、Mihomo 代理、Tailscale 生命周期和 recovery-free OTA，**不包含** HTTP 服务、Web UI、管理员认证或摄像头信令——这些属于独立的 `hyz-things` 门户进程。

- 板端程序：`/usr/bin/hyz-router`（init：`/etc/init.d/S81hyz-router`）
- 管理网络：`br-lan` + `p2p0`，`192.168.8.1/24`
- WAN：`wlan0`，DHCP 默认路由 metric `600`
- root-only 控制 socket：`/run/hyz-router/control.sock`
- 就绪标记：`/run/hyz-router/ready`（仅在全管理面严格 reconcile 后写入）
- 详细设计、架构图和板端验证记录：[docs/router.md](../../../docs/router.md)

## 进程边界

`hyz-things`（门户）与 `hyz-camera`（媒体）都是独立进程，各自拥有 composition root：

- `hyz-things` 通过版本化 JSON frame 连接 `/run/hyz-router/control.sock` 执行状态、面板、Tailscale 等操作；它承载管理员认证、CSRF、会话与嵌入式 Yew 页面，并等待 `/run/hyz-router/ready` 后才绑定 LAN HTTP。
- `hyz-camera` 通过 `/run/hyz-camera/control.sock` 接受受限状态、会话和旋转请求；它不执行网络或防火墙命令。
- 共享 wire 契约位于 `apps/rust/contract`（`hyz-contract`）。客户端要求协议版本精确匹配，服务端接受自身与前一版本，滚动推送时按 [`deploy-app.sh`](../../../apps/rust/things/tools/deploy-app.sh) 的兼容性矩阵把关。

## 能力

- 分两阶段收敛管理网络和转发网络；转发失败时优先保留可访问的管理 LAN。
- 支持普通 NAT，以及 Mihomo `explicit`、`tun`、`disabled` 三种模式。
- 通过 root-only Unix socket 统一承接 CLI、udhcpc hook、OTA 和门户操作。
- 校验 RKFW magic 与 SHA-256，并通过固定 staging 路径提交 BCB 或调用 `updateEngine`。
- 仅清理自身拥有的 bridge、iptables、进程和运行时文件；未知或外部资源不会被当作可安全接管的状态。

## 代码结构

```text
src/domain/                 纯状态、值对象、desired/observed 模型和不变量
src/application/            路由、代理、面板、状态、OTA、关闭等用例与 ports
src/adapters/inbound/       CLI、Unix control、udhcpc hook、OTA CLI
src/adapters/outbound/      Linux 网络、进程、存储、Mihomo、面板、Tailscale 和固件实现
src/main.rs                 唯一 production composition root
tests/                      网络生命周期、Tailscale 和 OTA 集成测试
tools/                      开发用 boot override 工具
```

依赖方向保持为 `adapters -> application -> domain`。只有 `src/main.rs` 可以构造生产 outbound adapter；CLI、udhcpc hook 和门户客户端都不能直接执行 Linux 命令。完整依赖图和启动时序见[架构文档](../../../docs/router.md#架构图)。

## 构建与检查

最低 Rust 版本为 `1.85`。native 实现只支持 Linux。

```sh
cargo test --locked --features native
cargo clippy --locked --all-targets --features native -- -D warnings
```

生产 AArch64 构建、Buildroot 安装和固件集成应从仓库根目录使用：

```sh
make router-app
```

## 运行与 CLI

`daemon` 会修改网络、启动管理进程并访问设备节点，只能在具备完整产品运行时的目标板上以 root 启动：

```sh
hyz-router daemon
```

其他公开命令都是 `/run/hyz-router/control.sock` 的客户端：

```text
hyz-router status [--json]
hyz-router router enable|disable
hyz-router proxy lan-tun enable|disable
hyz-router proxy tailscale enable|disable
hyz-router wifi status|scan
hyz-router wifi ap apply|confirm|cancel
hyz-router subscription get [--json]
hyz-router subscription refresh
hyz-router ota verify <upgrade.fw> <sha256>
hyz-router ota download <http(s)-url> <sha256>
hyz-router ota install <upgrade.fw> <sha256> [--reboot]
hyz-router ota install-recovery <upgrade.fw> <sha256> [--reboot]
hyz-router ota apply <http(s)-url> <sha256> [--reboot]
```

control socket 位于 root-only `0700` 目录且自身模式为 `0600`，并以 Linux peer credentials 再次要求 UID 0。不要手工删除 daemon/network/OTA lock 来“恢复”运行；代码会保守拒绝 stale 或身份不明的所有权记录，应先确认对应进程和资源状态。

## 板端配置与依赖

主要持久配置：

| 路径 | 用途 |
| --- | --- |
| `/userdata/hyz-router/network-config-v1.json` | 已提交的 STA/AP 配置 |
| `/userdata/hyz-router/mihomo/config.yaml` | 旧版 Mihomo 配置源及迁移输入 |
| `/userdata/hyz-router/mihomo/subscription/` | write-only 订阅来源、状态和配置代次 |
| `/userdata/hyz-router/mihomo/mode` | 已提交的代理模式 |
| `/userdata/hyz-router/ota/upgrade.fw` | 唯一允许安装的固定 OTA staging 文件 |
| `/userdata/hyz-router/tailscale/` | Tailscale node state（root-only） |

管理员 Argon2id 凭据 `/userdata/hyz-router/admin/credential.json` 与门户运行时由 `hyz-things` 读写，router 不接触。

native adapter 使用固定路径调用 `ip`、legacy `iptables`、`wpa_cli`、`hostapd_cli`、`mihomo`、`updateEngine` 和 `reboot`，并依赖产品镜像提供 `wpa_supplicant`、`udhcpc`、`hostapd`、`dnsmasq` 及对应内核网络能力。它不是可在普通开发机上直接运行的通用路由器守护进程。

OTA 的 SHA-256 只提供完整性校验，不提供发布者认证、anti-rollback 或 A/B rollback。迁移期间不得让旧 `hyz-ota` 与本实现并发运行。
