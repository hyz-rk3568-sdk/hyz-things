# Mihomo core、LAN TUN 与本机系统代理

## 状态

**2026-08-21：本文描述当前源码的代理与 Tailscale 边界。**

本次迁移把原来位于 Tailscale 语义下的代理开关改为 `hyz-things` 的“本机系统代理”。当前产品只保留两个彼此独立的 Mihomo 能力：

- **LAN TUN**：接管固定 LAN 下游的透明代理流量；
- **本机系统代理**：提供固定 `127.0.0.1:7890` 的本机 HTTP/HTTPS 显式代理入口。

本机系统代理不是本机全部流量的透明接管，也不是 Tailscale 中继、DERP 或 peer relay 的控制开关。Tailscale 生命周期、RouterOnly/LAN subnet access、peer 状态以及 Direct/Peer relay/DERP 观测保持独立。

共享 Mihomo core 只在至少一个能力需要时运行。两个能力都关闭时，core、mixed port、TUN 和 watcher 才可以按 ownership 规则停止；订阅、provider 和节点选择等持久配置继续保留。

## 1. 固定边界

### 1.1 Mihomo core

Mihomo runtime 由 `hyz-router` 生成和管理，包含：

- exact `/usr/bin/mihomo` 进程；
- root-only runtime config；
- 固定 loopback mixed port `127.0.0.1:7890`；
- 可选 `hyz-mihomo` TUN；
- 与 TUN 相关的 owned firewall、policy route 和 fail-open watcher。

core 的运行条件是：

```text
mihomo_required = lan_tun_enabled || local_system_proxy_enabled
```

浏览器不能连接 Mihomo Controller，也不能提交 raw YAML、proxy URL、端口、命令或环境变量。

### 1.2 LAN TUN

LAN TUN 只处理固定 `br-lan` 下游的 IPv4 TCP/UDP：

```text
br-lan 192.168.8.0/24
  → owned Mihomo PREROUTING interception
  → mark 0x1000000/0x1000000
  → table 110
  → hyz-mihomo
```

它不安装本机 `OUTPUT` 入口，不接管 router、Mihomo 或其他本机进程，不改变 Tailscale-to-LAN 的固定防火墙边界。关闭或 core 故障时，已确认的普通 NAT 继续作为 LAN 安全基线。

### 1.3 本机系统代理

本机系统代理只提供以下固定入口：

```text
HTTP  proxy: http://127.0.0.1:7890
HTTPS proxy: http://127.0.0.1:7890
```

本机 HTTP/HTTPS client 需要显式配置或选择这个 endpoint。该能力：

- 不安装本机 `OUTPUT` interception；
- 不接管所有本机 TCP/UDP/ICMP/DNS 流量；
- 不修改全局任意环境变量；
- 不启动、停止、重启或修改 `tailscaled`；
- 不改变 Tailscale Direct、Peer relay 或 DERP 连接观测。

## 2. 合法组合

| LAN TUN | 本机系统代理 | Mihomo core | LAN 公网流量 | `127.0.0.1:7890` |
| --- | --- | --- | --- | --- |
| 关 | 关 | 停止 | 普通 NAT | 不提供 |
| 开 | 关 | 运行 | Mihomo TUN | 提供，但不作为 LAN 入口 |
| 关 | 开 | 运行 | 普通 NAT | 提供 |
| 开 | 开 | 运行 | Mihomo TUN | 提供 |

两个 capability 的 readiness 分开计算。core 进程存在不等于 LAN TUN ready，也不等于本机系统代理 ready。

## 3. Tailscale 独立语义

Tailscale 继续由独立的 `TailscaleApplication` 和 `LinuxTailscalePlatform` 管理：

- `Disabled`、`RouterOnly`、`LanSubnetAccess` mode 不变；
- peer online/active、Direct、Peer relay、DERP(region) 和 unknown 只属于 Tailscale 状态；
- Tailscale ready 要求 exact owned process、socket、interface、固定 preferences、认证和 **Direct environment**；
- 本机系统代理的 API、CLI 和 Web switch 不调用 Tailscale application。

### 3.1 旧运行时 identity 迁移

旧版本可能在 `/run/hyz-tailscale` 的 identity 中记录 `MihomoExplicit`。新代码只把它作为 legacy observed value：

1. 删除旧 Tailscale LAN path；
2. 删除旧 Tailscale owned router firewall；
3. 停止旧 owned `tailscaled`；
4. 以 `Direct` environment 重新启动；
5. 等待 backend、恢复固定 preferences 和 router firewall；
6. 按 desired mode 恢复 LAN subnet route/firewall；
7. 最后提交 desired mode。

新代码不再启动 `MihomoExplicit` environment。adapter 可以读取并分类旧环境，但任何新的 `StartBackend` action 只接受 `Direct`。

## 4. Domain 与持久化

公开代理 desired 使用正交字段：

```rust
pub struct ProxyDesired {
    pub lan_tun_enabled: bool,
    pub local_system_proxy_enabled: bool,
    pub direct_macs: BTreeSet<LanDeviceMac>,
}
```

持久 wire value 为：

```rust
pub struct ProxyFeaturesV1 {
    pub version: u8,
    pub lan_tun_enabled: bool,
    pub local_system_proxy_enabled: bool,
}
```

持久 JSON 使用 `deny_unknown_fields`、有界读取、私有目录、临时文件、fsync 和原子发布。为了兼容旧持久化文件，`local_system_proxy_enabled` 接受旧字段名 `tailscale_explicit_proxy_enabled`；重新序列化只写新字段名。

旧的 Mihomo mode 迁移保持保守：

| 旧 mode | `lan_tun_enabled` | `local_system_proxy_enabled` | 处理 |
| --- | ---: | ---: | --- |
| `disabled` | false | false | 保持停用 |
| `explicit` | false | false | 不把旧 LAN 显式模式误认为本机系统代理 |
| `tun` | true | false | 保留 LAN TUN |
| 未知值 | — | — | 拒绝确认旧状态，不猜测 capability；保持代理未就绪并进入 degraded/management-only 语义 |

## 5. Runtime 与生命周期

Mihomo runtime config 固定追加：

```yaml
mixed-port: 7890
allow-lan: false
bind-address: 127.0.0.1
authentication: []
```

TUN block 只在 `lan_tun_enabled` 时追加。关闭一个 capability 时，application 只撤销该 capability 的 resources；只有最后一个 capability 关闭时才停止共享 core 和 watcher。

本机系统代理开关的 control operation 是：

```text
ControlOperation::ProxyLocalSystem { enabled: bool }
POST /api/v1/control/proxy/local-system
hyz-router proxy local-system enable|disable
```

它只更新 `ProxyDesired.local_system_proxy_enabled`，经过 `ProxyApplication` reconcile Mihomo core/mixed port 和相关 runtime resources，不包含任何 Tailscale action。

LAN TUN 继续使用：

```text
ControlOperation::ProxyLanTun { enabled: bool }
POST /api/v1/control/proxy/lan-tun
hyz-router proxy lan-tun enable|disable
```

## 6. 状态与 Web

`ProxyStatus` 现在包含：

```rust
pub struct ProxyStatus {
    pub configured: bool,
    pub mihomo: MihomoCoreStatus,
    pub lan_tun: LanTunStatus,
    pub local_system_proxy: LocalSystemProxyStatus,
}
```

本机系统代理状态只使用：

```rust
pub enum LocalSystemProxyEffective {
    Ready,
    Disabled,
    NotConfirmed,
}
```

Web 复用原代理设置区域和开关位置，但文案改为：

```text
本机系统代理
为本机 HTTP/HTTPS 显式代理；使用 Mihomo 127.0.0.1:7890，不接管所有本机流量。
```

状态规则：

- desired=true 且 Mihomo core/mixed port 已严格确认：`已启用`；
- desired=true 但依赖未确认或已降级：`已降级 · 未确认`；
- desired=false 且 effective 为 Disabled：`已关闭`；
- desired unknown：`未知 · 未确认`。

Tailscale 区域只显示 Tailscale mode 与 Direct、Peer relay、DERP(region) 或连接未知，不再显示代理环境或代理开关状态。

## 7. Control protocol 兼容性

本次 wire contract 协议版本为 `12`。服务端按现有滚动规则接受当前版本和前一版本；客户端要求精确匹配。协议中删除旧的 `ProxyTailscale` operation，新增 `ProxyLocalSystem` operation。

旧的 `/api/v1/control/proxy/tailscale` 不在 POST allowlist 中。HTTP mutation 继续要求管理员 session、same-origin、CSRF、固定 JSON body、body size limit 和 `deny_unknown_fields`。

## 8. 测试与验收

主机测试必须通过 domain/application ports、control handler 或 HTTP seam 验证，不执行真实 host 网络命令。重点覆盖：

- 四种 LAN TUN/本机系统代理组合及 shared core 引用关系；
- 本机系统代理切换不产生 Tailscale action；
- 旧 `tailscale_explicit_proxy_enabled` 持久字段可读、序列化后使用新字段；
- 旧 `MihomoExplicit` Tailscale identity 在 ready 前按 Direct 顺序恢复；
- 新 Tailscale backend action 拒绝 `MihomoExplicit`；
- local-system HTTP route、POST allowlist、管理员/Origin/CSRF/body limit 和 unknown field；
- ProxyStatus 的 ready、disabled、not-confirmed、unknown 展示；
- desktop/mobile viewport 下本机系统代理 switch 的 busy、error 和 shared-state 行为。

目标板验收还需确认：

1. `127.0.0.1:7890` 只在本机系统代理或 LAN TUN 需要 core 时监听；
2. LAN TUN 的 PREROUTING 入口仍只匹配 `br-lan`，没有本机 `OUTPUT` 入口；
3. 本机系统代理开关不会重启 `tailscaled`，Tailscale mode、peer 状态和 Direct/relay/DERP 观测不变；
4. core `SIGKILL` 后 LAN TUN 按现有 fail-open 回到普通 NAT，本机系统代理进入未确认或关闭状态；
5. reboot、WAN/DHCP 恢复、Tailscale 登录/注销、RouterOnly/LAN subnet access 和 OTA 不产生 foreign/stale resource 误清理。

真实浏览器工具不可用时，使用仓库的 loopback Axum harness 和 Playwright E2E 作为最近替代；不能把单张截图或单次静态渲染当作端到端验证。

## 9. 不在范围内

- 本机全部 `OUTPUT` 透明代理；
- 自动修改任意桌面/进程的代理环境；
- 通过本机系统代理开关控制 `tailscaled`；
- Tailscale DERP、Peer relay 或 WireGuard direct 的强制代理；
- 任意 Mihomo Controller、原始配置、provider URL、命令或节点凭据透传。
