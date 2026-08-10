# hyz-router

`hyz-router` 是 RK3568 产品上的统一路由控制服务。它以单个 Linux ELF 提供管理 LAN、WAN DHCP、IPv4 转发/NAT、Mihomo 代理、状态面板和 recovery-free OTA；Yew/WASM 前端在构建时作为静态资源嵌入同一个 ELF，不会安装第二个 Web 服务程序。

- 板端程序：`/usr/bin/hyz-router`
- 管理地址：`http://192.168.8.1:8080`
- 管理 LAN：`br-lan` + `p2p0`，`192.168.8.1/24`
- WAN：`wlan0`，DHCP 默认路由 metric `600`
- 详细设计、架构图和板端验证记录：[docs/router.md](../../../docs/router.md)

## 能力

- 分两阶段收敛管理网络和转发网络；转发失败时优先保留可访问的管理 LAN。
- 支持普通 NAT，以及 Mihomo `explicit`、`tun`、`disabled` 三种模式。
- 通过 root-only Unix socket 统一承接 CLI、udhcpc hook 和 OTA 操作。
- 提供只绑定管理 LAN 的 Axum API 和 Yew 状态/控制面板，并以管理员 session 保护 AP、STA 与订阅设置。
- 校验 RKFW magic 与 SHA-256，并通过固定 staging 路径提交 BCB 或调用 `updateEngine`。
- 仅清理自身拥有的 bridge、iptables、进程和运行时文件；未知或外部资源不会被当作可安全接管的状态。

## 代码结构

```text
src/domain/                 纯状态、值对象、desired/observed 模型和不变量
src/application/            路由、代理、面板、状态、OTA、关闭等用例与 ports
src/adapters/inbound/       CLI、Unix control、udhcpc hook、Axum HTTP
src/adapters/outbound/      Linux 网络、进程、存储、Mihomo、面板和固件实现
src/web/                    Yew/WASM 前端
src/main.rs                 唯一 production composition root
frontend/                   Trunk HTML/CSS 入口
tests/                      HTTP、网络生命周期和 OTA 集成测试
tools/                      可复现前端 bundle 工具
```

依赖方向保持为 `adapters -> application -> domain`。只有 `src/main.rs` 可以构造生产 outbound adapter；浏览器、CLI 和 DHCP hook 都不能直接执行 Linux 命令。完整依赖图和启动时序见[架构文档](../../../docs/router.md#架构图)。

## 构建与检查

最低 Rust 版本为 `1.85`。native 实现只支持 Linux。

在本目录执行默认 native 检查：

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

直接执行 `cargo build --locked --release` 时，如果没有前端 bundle，`build.rs` 会嵌入安全占位页。需要真实管理页面时，先生成 deterministic bundle，再构建 native ELF：

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked --version 0.21.14 --root ../../../.tools/trunk
PATH="../../../.tools/trunk/bin:$PATH" ./tools/build-frontend-bundle.sh
cargo build --locked --release
```

前端产物默认写入仓库根目录的 `target/frontend-bundle/router-frontend.tar`。也可以通过 `ROUTER_FRONTEND_ARCHIVE=/absolute/path/router-frontend.tar` 指定已有归档。生产 AArch64 构建、Buildroot 安装和固件集成应从仓库根目录使用：

```sh
make router-app
```

不要提交 `target/`、前端 bundle、交叉编译输出或设备运行数据。

## 运行与 CLI

`daemon` 会修改网络、启动管理进程并访问设备节点，只能在具备完整产品运行时的目标板上以 root 启动：

```sh
hyz-router daemon
```

默认 HTTP 端口为 `8080`，可在 daemon 启动前通过 `HYZ_ROUTER_HTTP_PORT` 修改；监听 IP 始终固定为 `192.168.8.1`，不会回退到 `0.0.0.0`。

其他公开命令都是 `/run/hyz-router/control.sock` 的客户端：

```text
hyz-router status [--json]
hyz-router router enable|disable
hyz-router proxy explicit|tun|disable
hyz-router ota verify <upgrade.fw> <sha256>
hyz-router ota download <http(s)-url> <sha256>
hyz-router ota install <upgrade.fw> <sha256> [--reboot]
hyz-router ota install-recovery <upgrade.fw> <sha256> [--reboot]
hyz-router ota apply <http(s)-url> <sha256> [--reboot]
```

control socket 位于 root-only `0700` 目录且自身模式为 `0600`。不要手工删除 daemon/network/OTA lock 来“恢复”运行；代码会保守拒绝 stale 或身份不明的所有权记录，应先确认对应进程和资源状态。

## 板端配置与依赖

主要持久配置：

| 路径 | 用途 |
| --- | --- |
| `/userdata/hyz-router/network-config-v1.json` | 已提交的 STA/AP 配置 |
| `/userdata/hyz-router/admin/credential.json` | 管理员 Argon2id 凭据 |
| `/userdata/hyz-router/mihomo/config.yaml` | 旧版 Mihomo 配置源及迁移输入 |
| `/userdata/hyz-router/mihomo/subscription/` | write-only 订阅来源、状态和配置代次 |
| `/userdata/hyz-router/mihomo/mode` | 已提交的代理模式 |
| `/userdata/hyz-router/ota/upgrade.fw` | 唯一允许安装的固定 OTA staging 文件 |

native adapter 使用固定路径调用 `ip`、legacy `iptables`、`wpa_cli`、`hostapd_cli`、`mihomo`、`updateEngine` 和 `reboot`，并依赖产品镜像提供 `wpa_supplicant`、`udhcpc`、`hostapd`、`dnsmasq` 及对应内核网络能力。它不是可在普通开发机上直接运行的通用路由器守护进程。

## HTTP 边界

公开读取接口为：

```text
GET /api/v1/health
GET /api/v1/status
GET /api/v1/panel
```

匿名 Web 只调用固定、类型化的背光和代理控制接口。AP/STA 与 write-only 订阅来源使用独立的固定 typed API，并要求管理员 session；默认 `admin` bootstrap 密码只能用于首次登录，完成强制改密后才能修改设置。凭据和订阅 URL 不通过读取 API 回显。

所有写请求都要求同源 `Origin`、JSON、启动期 CSRF token 和受限 DTO；CSRF token 不是认证。router enable/disable、Ethernet/PPPoE、OTA、任意命令、路径、interface、原始 Mihomo controller 和任意网络配置均不暴露到 LAN API。

产品明确继续使用 HTTP，因此 session cookie 不能设置 `Secure`，管理 LAN 上的流量嗅探者仍可能获取密码、Wi-Fi 凭据、订阅 URL 或 session。首次上线应立即改密；若要消除此风险，必须增加 HTTPS，不能把 CSRF 或 WPA2 当作传输加密。

OTA 的 SHA-256 只提供完整性校验，不提供发布者认证、anti-rollback 或 A/B rollback。迁移期间不得让旧 `hyz-ota` 与本实现并发运行。
