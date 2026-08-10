# hyz_things Router User Stories

## 1. 文档目的

本文定义 `hyz_things` RK3568 自用路由产品的功能边界、用户故事、验收标准和实施顺序。

产品定位是：

> **固定 LAN 拓扑、支持多种上游接入方式和 Wi-Fi 热备用的 IPv4 NAT 路由器。**

设备始终为下游客户端提供：

- 有线 LAN；
- Wi-Fi AP；
- DHCP 和 DNS；
- IPv4 forwarding、NAT 和最小安全防火墙；
- 本地管理页面和恢复路径；
- 可选的家庭网络事件、质量指标与断网时间线；
- 可选的本地 DNS 过滤、家庭域名与按客户端策略；
- 可选的 Tailscale 远程管理与固定 LAN subnet access；
- 可选的 Mihomo 显式代理与 TUN 透明代理。

当前 Wi-Fi-only 增量的实现与验证状态：

- Buildroot 源码保留固定的 Mihomo `v1.19.29` Linux ARM64 官方静态二进制包、SHA-256 和许可证哈希；
- 历史固件曾验证 MetaCubeXD 静态文件安装，但 Controller 从未启用；该冗余包现已从源码删除；
- 产品 Web UI 统一为 `apps/rust/router` 内嵌的 Yew bundle，不再安装独立 Dashboard；
- 产品 overlay 已加入 `/userdata/hyz-router/mihomo/config.yaml` 生命周期骨架和无真实凭据模板；
- recovery-free OTA 固件已完成编译和 rootfs 审计，详见
  [`soft-router-proxy-integration.md`](soft-router-proxy-integration.md)；
- recovery-free OTA 已在 RK3568 安装，组件哈希、安全 SKIP、userdata/recovery 保留和基础路由控制面回归通过；
- 板端 `-t`、认证 `MATCH,DIRECT`、仅 LAN 监听、幂等 start/stop 和无残留端口自测通过；
- 持久 `enable`/`disable` marker 已通过第二次 recovery-free OTA 安装和板端验证；
- root-only 订阅已生成本地 provider；本机真实 provider HTTP/HTTPS 和普通 DIRECT 并存通过；
- `p2p0` 下游显式代理基础联网和规则命中通过，但弱信号下只有约 `1.055 Mbps`，视频性能未通过；
- 仅 `br-lan` 入站的 TUN 模式、持久 `explicit`/`tun`/`disabled` 控制、策略路由、排除规则和核心退出清理已通过 recovery-free OTA、重启持久性、手机 TCP/UDP、视频、router restart 与核心 `SIGKILL` 普通 NAT 回退验证；
- 统一 Web UI 的状态、LCD/代理模式、节点选择和受控延迟刷新已完成板端功能验证；S81 已改为总 deadline 内封顶退避，并通过最终 recovery-free OTA 的冷启动和 restart 验证；
- 管理员认证、强制首次改密、typed AP/STA 设置、两阶段 AP 回滚和 write-only Mihomo HTTPS 订阅更新已经进入统一 Rust ELF；错误 STA 自动恢复、AP 未确认超时回滚及无秘密摘要已通过板测，成功切换真实 STA、实际改密和凭据型订阅刷新仍待操作者输入本地凭据；
- DNS 接管、8 小时路由+代理稳定性、节点全部失效/live-hang 自动回退仍未完成，因此代理 Epic 仍不得整体标记完成。

变化的是上游接入方式，不是 LAN 拓扑。完整基础产品必须支持：

1. `eth0` DHCP；
2. `wlan0` STA DHCP；
3. `eth0` 或受控 `eth0.<VLAN ID>` 承载的 PPPoE，三层出口为 `ppp0`；
4. 有线主线路与 Wi-Fi STA 同时在线，通过 route metric 实现有线优先、Wi-Fi 热备用。

本产品不以通用 OpenWrt 后台、透明 Bridge Mode、插件系统、容器平台或任意 Linux 网络配置器为目标。后续功能通过修改、测试和发布本仓库代码增加，不维护运行时插件 ABI 或第三方插件兼容性。

详细实现架构见 [`router.md`](router.md)。

## 2. 固定产品拓扑

### 2.1 下游 LAN

最终固定 LAN 拓扑为：

```text
                 br-lan 192.168.8.1/24
                    ┌──────┴──────┐
                    │             │
                  eth1          p2p0
                           下游 Wi-Fi AP
```

固定要求：

- `br-lan = eth1 + p2p0`；
- `eth0` 和 `wlan0` 不加入 `br-lan`；
- DHCP、DNS 和管理 HTTP 只服务 LAN；
- WAN 变化不得重建 `br-lan`、改变 LAN 地址或清除有效 LAN lease；
- AP 失败不得破坏 `eth1`，`eth1` 失败不得停止 AP；
- 上游不可用时，设备进入严格确认的 management-only 状态，继续提供本地管理网络。

这里的 `br-lan` 是设备内部的 **LAN bridge**，不表示产品提供透明 Bridge Mode。

### 2.2 上游接入

```text
                                  上游 WAN
                     ┌────────────────┴────────────────┐
                     │                                 │
              有线主线路，二选一                  Wi-Fi 备用线路
          ┌──────────┴──────────┐                      │
          │                     │                      │
      eth0 DHCP       eth0 / eth0.<VLAN> PPPoE     wlan0 STA DHCP
      egress=eth0             egress=ppp0           metric 600
      metric 100              metric 100
          │                     │                      │
          └──────────┬──────────┘                      │
                     └────────────────┬─────────────────┘
                                      │
                            IPv4 NAT、防火墙
                                      │
                         br-lan = eth1 + p2p0
```

上游规则：

- `eth0 DHCP` 与 PPPoE 在同一物理接口上互斥；
- 配置有效 Wi-Fi 凭据时，`wlan0` 可与当前有线主线路共存；
- 有线默认路由 metric 为 `100`；
- Wi-Fi 默认路由 metric 为 `600`；
- 两条默认路由同时存在时，新连接走有线；
- 有线有效默认路由消失后，新连接自动使用 Wi-Fi；
- 有线恢复后，新连接重新使用有线；
- WAN 切换期间已有 TCP、UDP 和 NAT 会话允许中断；
- 第一阶段不通过 ping、HTTP 或 DNS 判断公网健康，只根据受控 link、session、DHCP 和默认路由状态切换。

### 2.3 三种接入方式

| 接入方式 | 物理入口 | 三层出口 | 地址/会话 | 默认 metric | 角色 |
| --- | --- | --- | --- | ---: | --- |
| Ethernet DHCP | `eth0` | `eth0` | DHCP | `100` | 有线主线路 |
| Wi-Fi STA | `wlan0` | `wlan0` | WPA + DHCP | `600` | 单独上网或热备用 |
| Ethernet PPPoE | `eth0` 或 `eth0.<VLAN>` | `ppp0` | PPPoE | `100` | 有线主线路/运营商直拨 |

上游来自已有路由器时，设备仍为下游客户端执行一层 NAT，可能形成双重 NAT。上游由桥接光猫提供 PPPoE 时，设备终结 PPPoE 并成为本地唯一主路由/NAT；这不保证绕过运营商 CGNAT。

普通三地址 Wi-Fi STA 无法透明承载多个下游 MAC，除非驱动和上游 AP 都支持 4-address/WDS。因此 `wlan0` 始终是三层 WAN，不加入 `br-lan`。

## 3. 已确认的产品决策

| 项目 | 决策 |
| --- | --- |
| 有线 WAN carrier | `eth0` |
| 有线 LAN | `eth1` |
| Wi-Fi STA | `wlan0` |
| Wi-Fi AP | RTL8852BS 虚拟接口 `p2p0` |
| LAN bridge | `br-lan = eth1 + p2p0` |
| LAN 地址 | `192.168.8.1/24` |
| DHCP 地址池 | `192.168.8.100-192.168.8.249` |
| 有线优先级 | DHCP `eth0` 或 PPPoE `ppp0`，metric `100` |
| Wi-Fi 优先级 | `wlan0`，metric `600` |
| 有线选择 | Ethernet DHCP 或 Ethernet PPPoE，二者互斥 |
| Wi-Fi 共存 | 有线正常时允许保持 STA、地址和备用默认路由 |
| WAN 判定 | 依据受控有效默认路由，不仅依据 carrier |
| 公网健康检查 | 基础产品不实现 |
| 下游路由 | IPv4 NAT，不做上游 STA 透明二层桥接 |
| PPPoE VLAN | 支持无 VLAN 或受校验的单个 802.1Q VLAN ID |
| PPPoE 凭据 | 只保存于 root-only `/userdata`，不得进入 Git、镜像模板、argv 或普通日志 |
| 代理核心 | Mihomo，普通 NAT 始终是可恢复基线 |
| 网络黑匣子 | 结构化事件、有界指标、断网时间线和受限诊断快照 |
| 本地 DNS 中心 | 固定成熟 DNS 引擎，由 `hyz-router` 管理 typed 配置、生命周期和 active resolver |
| 远程访问 | 可选 Tailscale；遵循“尽可能 direct，但 relay 永远可用”，先 RouterOnly，再固定 `192.168.8.0/24` subnet access，不提供 exit node |
| 管理 UI | `hyz-router` 内嵌 Yew 页面，不安装第三方 Dashboard |
| 扩展方式 | 修改并发布本仓库代码，不提供插件或容器扩展平台 |

## 4. 当前实现状态

当前实现是完整产品的 Wi-Fi-only 增量：

```text
WAN = wlan0 STA DHCP，metric 600
LAN = br-lan = p2p0
```

已建立的基础包括：

- RTL8852BS `wlan0` STA 与 `p2p0` AP 并发；
- `br-lan`、LAN 地址、dnsmasq、普通 NAT 和受限防火墙；
- 固定管理 HTTP、root-only control socket 和统一 Rust composition root；
- 保守的 ownership、readiness、rollback、management-only、shutdown 和 fail-open；
- Mihomo `explicit`、`tun`、`disabled`，以及核心退出时回退普通 NAT；
- recovery-free OTA、固定 staging、RKFW/SHA-256 和 BCB 验证；
- 内嵌 Yew 状态与受限本地控制页面。

当前尚未完成：

- `eth1` 加入 `br-lan`；
- DHCP 地址池从当前 `.100-.199` 扩展到 `.100-.249`，并验证现有 lease 兼容；
- `eth0` DHCP；
- 有线和 Wi-Fi 默认路由共存与自动切换；
- 每个 uplink 独立的 DHCP、route 和 DNS ownership；
- PPPoE、可选 VLAN、`ppp0` 防火墙和 MTU/MSS；
- 家庭网络黑匣子的结构化事件、指标、断网时间线和诊断快照；
- 本地 DNS 过滤、家庭域名和按客户端策略；
- Tailscale RouterOnly 与固定 LAN subnet access；
- 完整基础产品及上述可选能力的稳定性和端到端测试矩阵。

历史和板端验证记录：

- [`soft-router-sta-validation.md`](soft-router-sta-validation.md)
- [`soft-router-sta-ap-validation.md`](soft-router-sta-ap-validation.md)
- [`soft-router-sta-ap-nat-validation.md`](soft-router-sta-ap-nat-validation.md)
- [`soft-router-br-lan-validation.md`](soft-router-br-lan-validation.md)
- [`soft-router-proxy-integration.md`](soft-router-proxy-integration.md)
- [`soft-router-tun.md`](soft-router-tun.md)

当前增量通过不代表 Ethernet DHCP、双 uplink、完整 `br-lan` 或 PPPoE 已验收。

## 5. Epic SR：固定 LAN 与 DHCP 双上游路由

### SR-00：验证 STA 与 AP 并发

**User Story**

作为产品开发者，我希望 RTL8852BS 能稳定同时运行 STA 和 AP，以便通过 Wi-Fi 连接上游时仍能向下游提供 AP。

**验收标准**

1. 系统创建 `wlan0` STA 和 `p2p0` AP。
2. STA 能关联上游、取得 DHCP 地址并维持 metric `600` 默认路由。
3. AP 客户端能关联并通过 `br-lan` 交换数据。
4. 记录同信道限制、支持频段、吞吐和上游换信道行为。
5. 连续运行至少 8 小时，无驱动崩溃、SDIO reset、接口消失或不可恢复断连。
6. 单芯片并发失败时必须采用明确降级拓扑，不得继续假设 Wi-Fi STA+AP 可用。

### SR-01：提供可复现的路由系统能力

**User Story**

作为固件开发者，我希望 rootfs 和内核包含受控的 STA、AP、bridge、DHCP、DNS、NAT、防火墙、TUN 和策略路由能力，以便从干净工作区构建同样的产品。

**验收标准**

1. Buildroot 提供 `iproute2`、legacy iptables、`wpa_supplicant`、`hostapd`、`dnsmasq`、`iw` 和所需 DHCP client。
2. 内核提供 bridge、IPv4 forwarding、conntrack、NAT、MASQUERADE、TUN、multiple tables、mark、socket、comment、TPROXY 和 REDIRECT。
3. 只维护一种主防火墙路径，不混用 nftables 和 iptables 作为产品状态源。
4. 静态检查能够断言关键软件和内核符号存在。
5. rootfs 不包含真实网络凭据。

### SR-02：安全持久化网络配置

**User Story**

作为设备管理员，我希望 WAN、STA、AP 和 PPPoE 配置在重启与 OTA 后保留，并能安全恢复默认值。

**验收标准**

1. 可变配置位于 `/userdata/hyz-router/`。
2. rootfs 只提供无秘密模板。
3. 凭据文件必须是 root-owned private regular file。
4. 支持保存 STA SSID、安全类型、密码和监管域。
5. 支持保存 AP SSID、密码、国家码和可选信道。
6. 支持选择 Ethernet DHCP 或 Ethernet PPPoE。
7. 支持配置 PPPoE 账号、密码、服务名、可选 AC 名和 VLAN；这些字段通过 root-only 配置文件交给固定 adapter，不在 argv、状态或普通日志显示。
8. 新配置先校验后提交；失败时保留上一个可用配置和 management-only 入口。
9. 恢复默认值不得删除无关 `/userdata` 内容。

### SR-03：建立固定 LAN bridge

**User Story**

作为下游用户，我希望通过 `eth1` 或 Wi-Fi AP 接入时获得同一局域网服务。

**验收标准**

1. 创建 runtime-owned `br-lan` 并配置 `192.168.8.1/24`。
2. `eth1` 和 `p2p0` 都加入 `br-lan`。
3. `eth0` 和 `wlan0` 永不加入 `br-lan`。
4. dnsmasq 只在 `br-lan` 提供 `192.168.8.100-192.168.8.249` 地址池；从当前 `.100-.199` 扩展时保留合法现有 lease，并验收 `.200` 与 `.249` 边界地址。
5. DHCP 下发网关和 DNS `192.168.8.1`。
6. AP 失败不影响 `eth1`，`eth1` 失败不停止 AP。
7. WAN 切换不改变 LAN 地址、不重启 LAN DHCP、不清除有效 lease。
8. unknown、foreign 或意外 bridge member 不得被当作 owned/ready。
9. shutdown 只移除 runtime-owned member、地址、服务和 bridge。

### SR-04：连接 Wi-Fi STA WAN

**User Story**

作为设备管理员，我希望设备连接已有 Wi-Fi，并在有线 carrier/session/lease 或受控默认路由未就绪时通过该 Wi-Fi 为下游提供网络。

**验收标准**

1. `wpa_supplicant` 使用 root-only 配置管理 `wlan0`。
2. STA 关联后启动独立 DHCP client，并安装地址、DNS 和 metric `600` 默认路由。
3. 有线正常时 `wlan0` 保持关联、地址和备用路由，不因有线恢复而停止。
4. STA 断线后自动重连，不影响 `br-lan`、AP、LAN DHCP 或有线 WAN。
5. Wi-Fi DHCP ownership 与有线 DHCP/PPPoE ownership 独立。
6. 旧 DHCP callback 不得覆盖当前 generation 或其他 uplink 状态。
7. 日志不得输出明文 PSK。

### SR-05：通过 Ethernet DHCP 连接有线 WAN

**User Story**

作为设备管理员，我希望 `eth0` 接入已有路由器、光猫路由口或其他 DHCP 上游后成为首选 WAN。

**验收标准**

1. Ethernet DHCP 模式下，`eth0` carrier up 后启动独立 DHCP client。
2. DHCP 成功后安装地址、网关、DNS 和 metric `100` 默认路由。
3. carrier down、lease 失效或进程退出后删除该 generation 拥有的地址、路由和 DNS。
4. carrier up 但 DHCP 未取得有效默认路由时继续使用 Wi-Fi。
5. 重复插拔不会产生重复 route、地址、进程或 ownership record。
6. Ethernet DHCP 与 PPPoE 在 `eth0` 上互斥。
7. `eth0` 永不加入 `br-lan`。

### SR-06：实现有线优先和 Wi-Fi 热备用

**User Story**

作为设备管理员，我希望有线与 Wi-Fi 能同时保持连接，正常情况下优先使用有线；当有线 carrier/session/lease 或 runtime-owned 默认路由失效时，自动使用 Wi-Fi。

**验收标准**

1. 有线 DHCP/PPPoE 默认路由 metric 为 `100`。
2. Wi-Fi 默认路由 metric 为 `600`。
3. 两条有效默认路由同时存在时，新连接经有线出口。
4. active uplink 依据 runtime-owned 有效默认路由判断，不仅依据 carrier。
5. 有线默认路由消失后，新连接自动使用 `wlan0`。
6. 有线恢复后，新连接自动恢复使用 `eth0` 或 `ppp0`。
7. 切换不重建 `br-lan`、AP、LAN DHCP 或管理 HTTP。
8. 旧 NAT 会话允许中断，第一阶段只保证新连接走正确出口。
9. 每个 uplink 的 resolver entries 必须绑定其 DHCP/PPP generation 和 ownership，不能由另一 uplink 的 renew/deconfig 删除。
10. dnsmasq 只使用当前优先 uplink 的有效 resolver set；切换时原子替换并清除失效上游，不能混用两个 uplink 的 DNS。
11. 即使两个 uplink 下发相同 RFC1918 DNS 地址，状态和清理仍必须按 uplink identity 区分，并可观察当前 active resolver set。
12. 没有任何有效上游 DNS 时应报告 degraded，而不是保留已失效 resolver。
13. 不通过公网探测改变选择，除非以后另立 Story。
14. 状态输出记录切换原因、旧 uplink、新 uplink、active resolver set 和时间，不包含凭据。

### SR-07：提供多出口 NAT 和最小安全防火墙

**User Story**

作为设备管理员，我希望 LAN 客户端能够通过当前有效 WAN 访问网络，同时阻止 WAN 主动访问 LAN。

**验收标准**

1. 启用 IPv4 forwarding，并在首次修改前保存原值。
2. 支持受控出口 `eth0`、`wlan0` 和 `ppp0`。
3. `br-lan → 当前 WAN` 的新连接和返回流量允许通过。
4. `当前 WAN → br-lan` 的主动新连接默认拒绝。
5. 对实际 L3 出口执行 MASQUERADE；PPPoE 的出口是 `ppp0`，不是底层 `eth0`。
6. WAN INPUT 对发往设备本机的主动新连接默认拒绝，只允许精确所需的 established/related 和 DHCP/PPPoE 控制流量。
7. DHCP、DNS、HTTP 和其他管理服务不监听 WAN；即使其他进程误监听通配地址，WAN INPUT 规则也必须阻止访问。
8. 分别从 `eth0`、`wlan0` 和 `ppp0` 验证 FORWARD 与 INPUT 边界。
9. firewall chain、hook、顺序和 ownership token 可精确观察。
10. 两个 DHCP uplink 共存时不需要为每次 route 切换重建整个 firewall。
11. 防火墙失败采用 WAN INPUT/FORWARD fail-closed，同时保留已确认的 management-only 网络。
12. shutdown 只删除 runtime-owned 规则并恢复启动前 forwarding 状态。

### SR-08：提供可靠生命周期与状态

**User Story**

作为维护人员，我希望服务启动、停止、重启、故障降级和 OTA 后恢复具有确定行为。

**验收标准**

1. control socket 在可能触发 DHCP callback 的初始化前开始服务。
2. 先建立并严格复核 management LAN，再提交 forwarding/NAT。
3. 任一 WAN 失败不停止 `br-lan`、LAN DHCP 或另一个 WAN。
4. forwarding 或代理恢复失败时降级到严格确认的 management-only。
5. 无法确认安全状态时不得绑定管理 HTTP readiness。
6. 重复启动、停止和重启不累积进程、route、rule、address 或 lock。
7. shutdown 先移除代理 interception，再移除普通 forwarding，最后清理管理网络。
8. OTA 后沿用 `/userdata` 配置，并支持明确的配置版本迁移。
9. 状态按 uplink 分别显示 link、session、address、default route、DNS 和 ownership。

### SR-09：验收 DHCP 双上游路由基线

**User Story**

作为产品负责人，我希望通过可重复矩阵确认 Ethernet DHCP、Wi-Fi STA、固定 LAN 和自动切换可用。

**验收标准**

至少覆盖：

1. Wi-Fi-only：`eth1` 和 AP 客户端均可联网。
2. Ethernet DHCP-only：两个下游入口均可联网。
3. `eth0` 和 `wlan0` 同时在线：新连接走 `eth0`。
4. 在线拔线：有线路由移除后新连接走 Wi-Fi。
5. 在线插线：DHCP 成功后新连接恢复走有线。
6. carrier up 但 DHCP 失败：继续使用 Wi-Fi。
7. STA 断线重连：有线和 LAN 不受影响。
8. AP 重启：`eth1` 和 WAN 不受影响。
9. `eth1` 插拔：AP 和 WAN 不受影响。
10. 连续至少 20 次启动/切换，不出现重复进程、route、rule 或 address。
11. 运行至少 8 小时，记录吞吐、丢包、RSSI、CPU、内存、conntrack 和驱动错误。
12. 每个场景记录地址、路由、DNS、NAT、DHCP lease 和关键日志。

## 6. Epic PW：PPPoE 主路由

PPPoE 属于完整基础产品需求，但在 DHCP 双上游基线稳定后实施。

### PW-00：提供可复现的 PPPoE 与 VLAN 能力

**User Story**

作为固件开发者，我希望镜像内置 PPPoE 和可选 VLAN 能力，以便设备连接桥接光猫并独立拨号。

**验收标准**

1. Buildroot 提供 `pppd`、PPPoE plugin 和所需工具。
2. 内核启用 PPP、PPPoE、802.1Q VLAN、conntrack、NAT 和 firewall 所需符号。
3. 静态检查断言用户态组件和关键内核能力存在。
4. rootfs 只提供无账号、无密码、无运营商 VLAN 的模板。
5. 固件记录 PPP 实现、版本和许可证。

### PW-01：安全保存并选择 PPPoE 配置

**User Story**

作为设备管理员，我希望安全保存 PPPoE 账号、密码和可选 VLAN，并在 Ethernet DHCP 与 PPPoE 之间明确选择。

**验收标准**

1. 账号、密码、服务名、可选 AC 名和 VLAN 位于 root-only `/userdata`。
2. 账号、密码、服务名和可选 AC 名均视为敏感配置，不出现在 argv、状态、普通日志、Git、构建输出或 rootfs 模板。
3. VLAN ID 仅允许 `1..=4094`；创建固定 `eth0.<VID>` 前必须拒绝 foreign/冲突接口，并通过 ownership 约束创建、复用和清理。
4. Ethernet DHCP 与 PPPoE 模式互斥。
5. 清除 PPPoE 配置不删除其他 `/userdata`，并可退回 Ethernet DHCP。
6. 配置校验失败保留上一个可用配置和 management-only。

### PW-02：管理 PPPoE 会话和重拨

**User Story**

作为下游用户，我希望设备在光猫 carrier 可用时建立 PPPoE，并在会话断开后自动恢复。

**验收标准**

1. 在 `eth0` 或 runtime-owned `eth0.<VLAN>` 发起 PPPoE。
2. 启动前确认固定 `ppp0` 不存在或由当前 runtime 精确拥有；foreign/unknown `ppp0` 必须 fail closed。
3. `pppd` 必须请求固定 unit 0；不得静默改用 `ppp1` 并继续提交 readiness、防火墙或默认路由。
4. 成功后严格确认 `ppp0`、`pppd` identity、地址和默认路由均属于当前 session。
5. `ppp0` 默认路由 metric 为 `100`。
6. 认证失败、发现超时和 session 断开采用封顶指数退避。
7. carrier down 时停止失效 session，只删除 runtime-owned `ppp0`、VLAN、route 和 DNS。
8. 重拨导致地址变化时刷新 NAT、防火墙、DNS、状态和可选代理依赖。
9. PPPoE 失败不破坏 `br-lan`、LAN DHCP、Wi-Fi fallback、ADB 或本地控制台。
10. 连续至少 20 次重拨不残留 `pppd`、VLAN、route、rule 或僵尸 session。

### PW-03：实现 PPPoE 与 Wi-Fi fallback

**User Story**

作为设备管理员，我希望 PPPoE 是主线路，同时在 PPPoE 不可用时自动使用 Wi-Fi。

**验收标准**

1. `ppp0` metric `100` 与 `wlan0` metric `600` 可同时存在。
2. PPPoE 正常时新连接走 `ppp0`。
3. PPPoE 默认路由消失后新连接走 `wlan0`。
4. PPPoE 重拨成功后新连接恢复走 `ppp0`。
5. 切换不停止 STA、不重建 LAN、不清除 LAN lease。
6. DNS 跟随当前优先 uplink。
7. 已有连接允许中断，不承诺 session 迁移。

### PW-04：适配 PPPoE firewall、MTU 和 DNS

**User Story**

作为设备管理员，我希望 PPPoE 具有与 DHCP WAN 一致的安全边界，并避免 MTU 问题导致部分网络不可用。

**验收标准**

1. 允许 `br-lan → ppp0` 新连接和返回流量，拒绝 `ppp0 → br-lan` 主动新连接。
2. MASQUERADE 跟随实际 `ppp0` session。
3. 使用协商 MTU；典型场景验证 `1492`，不把该值假设为所有运营商固定值。
4. 为转发 TCP 配置合适的 MSS clamp，并验证 HTTPS、大响应、VPN-like 流量和长连接。
5. PPP DNS 或显式 DNS 安全更新 dnsmasq，session 停止后清除失效 DNS。
6. 管理服务不监听 `ppp0`。
7. 防火墙失败采用 WAN 入站 fail-closed。

### PW-05：验收 PPPoE 主路由

**User Story**

作为产品负责人，我希望确认光猫桥接后由 RK3568 终结 PPPoE、承担本地 NAT，并准确说明运营商限制。

**验收标准**

1. 分别记录“光猫路由 + Ethernet DHCP”和“光猫桥接 + PPPoE”拓扑。
2. 使用受控实验室 PPPoE server 分别验证无 VLAN 和至少一个非零 VLAN，不依赖当前运营商是否要求 VLAN。
3. 验证 PPPoE + Wi-Fi fallback、重拨、DNS、NAT、防火墙和 MTU/MSS。
4. 比较 PPP 地址和外部观测地址，准确标记公网、私网或 CGNAT。
5. 不承诺绕过运营商 CGNAT 或保证固定 NAT 类型。
6. 验证 Web、游戏、WebRTC、UDP 和代表性长连接。
7. 运行至少 8 小时 PPPoE+NAT 稳定性测试。
8. 凭据经过 recovery-free OTA 保留验证且未泄漏。

## 7. Epic PX：可选代理网关

代理功能不阻塞基础路由交付，也不得破坏普通 NAT、uplink fallback 或 management-only。

### PX-00：提供可复现的 Mihomo 与内嵌 Web

**验收标准**

1. Mihomo 固定版本、ARM64 发布物、SHA-256、许可证和源码位置。
2. Yew、Trunk、Cargo lock 和 deterministic bundle 由 `apps/rust/router` 统一构建。
3. 不安装 MetaCubeXD、OpenClash、Clash Verge Rev 或独立 Dashboard。
4. 不提交二进制下载物、订阅、节点、secret 或缓存。

### PX-01：安全持久化代理配置

**验收标准**

1. 真实配置位于 root-only `/userdata/hyz-router/mihomo/`。
2. 启动前校验配置；失败时保持普通 NAT。
3. 订阅 URL、节点凭据和 controller secret 不进入 Git、状态或普通日志。
4. runtime config、controller secret 和 process identity 位于 `/run` 私有目录。

### PX-02：提供显式代理和 TUN 模式

**验收标准**

1. 支持 `explicit`、`tun` 和 `disabled`。
2. 显式代理只监听 LAN 地址。
3. TUN 只接管 `br-lan` 入站，不绑定物理 LAN member。
4. TUN readiness 依赖至少一个已确认 uplink、对应普通 NAT 和严格代理 data plane。
5. Mihomo outbound/direct 流量跟随系统当前优先 uplink。
6. route 切换后新代理连接使用新的 active uplink。

### PX-03：保证代理 fail-open

**验收标准**

1. interception 作为最后提交点。
2. Mihomo core 退出时，受严格身份约束的 watcher 撤销 TUN interception。
3. 普通 NAT 对当前 active uplink 保持可用。
4. unknown 或 foreign process、rule、route 不被当作 owned。
5. shutdown 先撤销代理，再撤销普通 forwarding。

### PX-04：提供受限管理页面

**验收标准**

1. 页面只通过管理 LAN 提供。
2. 状态 API 为固定 GET；写操作为固定 typed POST。
3. 写操作要求小 body、精确同源 Origin、CSRF token 和未知字段拒绝。
4. 不向浏览器暴露 Mihomo controller、原始 JSON、任意命令、路径、URL、timeout 或 provider 名。
5. router enable/disable、Ethernet/PPPoE 配置、PPPoE 凭据、OTA、任意 interface 和任意网络配置不通过 LAN API 暴露；Wi-Fi STA/AP 设置是唯一的受限例外。
6. Wi-Fi STA/AP 设置要求已完成强制改密的管理员 session，只接受受验证的 typed DTO，并保持凭据不回显；未来 Ethernet/PPPoE 配置若开放，也必须沿用同一边界，不接受 interface 或命令字符串。

### PX-05：验收代理生命周期

**验收标准**

1. 显式代理、TUN、disabled、重启、OTA 和 core crash 行为可重复。
2. 覆盖 Ethernet DHCP、Wi-Fi-only、双 uplink、PPPoE 和 WAN 切换适用场景。
3. 代理关闭后不残留 rule、route、mark、process 或端口。
4. 管理 API 和代理端口从 WAN 不可达。
5. 运行至少 8 小时路由+代理稳定性测试。

## 8. 非功能要求

- **安全性**：默认最小授权；秘密不进入 Git、构建日志、argv、状态或普通日志。
- **可恢复性**：WAN、配置、黑匣子存储、DNS、Tailscale、代理或 OTA 问题不能阻塞本地管理与恢复路径。
- **幂等性**：重复 reconcile、DHCP callback、PPPoE 重拨和代理切换不累积状态。
- **所有权**：只修改和清理 runtime-owned 资源；unknown、foreign、stale 不视为 ready。
- **可诊断性**：状态按 LAN、Ethernet、Wi-Fi、PPPoE、firewall、DNS、Tailscale、proxy 和 system 分组，并以结构化事件记录关键状态变化及原因。
- **可复现性**：所有软件、内核配置、Rust/Web 产物和模板由仓库固定输入生成。
- **确定性**：基础 WAN 选择依赖 link/session/route，不依赖外部健康探测服务。
- **测试性**：通过 domain、application ports、control/HTTP seams 和 fake ports 驱动测试。
- **扩展方式**：后续能力通过修改代码和发布固件增加，不建设插件或容器平台。

## 9. 明确不包含

- AP/透明 Bridge Mode；
- 将 `eth0` 加入 `br-lan`；
- 将普通 `wlan0` STA 透明二层桥接到下游；
- 通用插件系统、第三方应用商店、稳定插件 ABI；
- Docker、LXC 或用户可安装运行时；
- NAS、Samba、照片备份、媒体服务和通用家庭服务器能力；
- 任意 interface、bridge、route table 或 firewall 编辑器；
- 多 WAN 负载均衡或按流量策略路由；
- 基于公网探测结果自动切换上联；
- IPv6、DHCPv6 和前缀委派；
- 蜂窝 WAN；
- WireGuard、IPsec、OpenVPN 或任意站点 VPN；Tailscale 仅按本文固定模式集成；
- Tailscale exit node 和任意 subnet 发布；
- Captive Portal；
- QoS/SQM、家长控制和完整流量分析；
- 动态多 VLAN、多 SSID、IPTV、语音和光猫管理通道；
- 自动 UPnP、NAT-PMP 或 PCP；
- 保证公网 IPv4、绕过 CGNAT 或固定 NAT 类型。

这些能力以后如有自用需求，应通过新的 typed domain model、application use case、测试和固件发布实现，而不是开放任意命令或插件入口。

## 10. TODO

### P0：完成并稳定 DHCP 双上游基础路由

- [ ] 将固定 `WAN_INTERFACE=wlan0` 重构为受限 typed uplink model。
- [ ] 保持现有 Wi-Fi-only 行为不变地完成第一步 domain/application 重构。
- [ ] 将 network observed state 改为按 Ethernet、Wi-Fi 和 active uplink 分别观察。
- [ ] 将 DHCP event、generation、地址、route 和 resolver ownership 按 uplink 隔离。
- [ ] 将 `eth1` 加入 `br-lan`，实现 `br-lan = eth1 + p2p0`。
- [ ] 将 DHCP 地址池从 `.100-.199` 扩展到 `.100-.249`，保留合法 lease 并验收边界地址。
- [ ] 验证 AP 失败不影响 `eth1`，`eth1` 插拔不影响 AP。
- [ ] 实现 `eth0` carrier 和 DHCP lifecycle。
- [ ] 安装 `eth0` metric `100` 与 `wlan0` metric `600` 默认路由。
- [ ] 实现有线优先、Wi-Fi 热备用和新连接自动切换。
- [ ] 让 dnsmasq 原子使用当前 active uplink 的 resolver set，并验证同地址 DNS 的 ownership 隔离。
- [ ] 将普通 NAT/FORWARD 扩展到 `eth0` 与 `wlan0` 固定出口。
- [ ] 增加 WAN INPUT 默认拒绝和 DHCP 所需精确例外，并从两个 WAN 验证。
- [ ] 更新 status、CLI、Web 状态和问题报告以显示每个 uplink 与 active resolver set。
- [ ] 用 fake ports 覆盖 exact action order、rollback、stale callback 和 unknown rejection。
- [x] 构建并安装包含 S81 封顶指数退避修正的新固件，并完成自动冷启动验收。
- [ ] 在强制 forwarding/WAN 故障下重新验收 management-only 可访问性。
- [ ] 完成客户端 DNS/HTTPS、上游换信道和多客户端持续流量测试。
- [ ] 完成 SR-09 的 20 次切换、8 小时稳定性和完整板端矩阵。

### P1：在 P0 通过后完成 PPPoE 主路由

- [ ] 确认 Buildroot `pppd`、PPPoE plugin、VLAN 和内核 PPP 配置。
- [ ] 定义受限 Ethernet DHCP/PPPoE 配置和安全迁移格式。
- [ ] 实现 root-only PPPoE 账号、密码、service/AC 和可选 VLAN 配置。
- [ ] 增加固定 executable、typed argv 和 identity-bound `pppd` adapter。
- [ ] 拒绝 foreign/unknown `ppp0` 和 VLAN，强制 unit 0，不静默使用 `ppp1`。
- [ ] 观察 `eth0`/VLAN carrier、`ppp0` session、地址、route 和 resolver ownership。
- [ ] 实现认证失败和断线重拨的封顶指数退避。
- [ ] 将 NAT/FORWARD/WAN INPUT 出口扩展到 `ppp0`。
- [ ] 实现 PPPoE MTU、MSS clamp 和 DNS 更新。
- [ ] 实现 `ppp0` metric `100` 与 `wlan0` metric `600` fallback。
- [ ] 使用受控实验室 PPPoE server 验证无 VLAN 和非零 VLAN。
- [ ] 完成 PW-05 板端矩阵、20 次重拨、8 小时稳定性和 OTA 保留验证。

### P2：实现家庭网络“黑匣子”

- [ ] 在 domain/application 定义结构化网络事件、事件来源、严重级别、切换原因和 correlation ID，不由 Web 根据状态差异猜测原因。
- [ ] 为事件写入、指标采样、保留和查询定义 application-level ports，路由核心不依赖具体数据库或 Web DTO。
- [ ] 记录启动、management-only、DHCP、PPPoE、地址、route、resolver generation、防火墙、NAT、代理和 active uplink 状态变化。
- [ ] 记录每次 uplink 切换的旧线路、新线路、准确原因、action 顺序、普通 NAT 恢复点和中断时长。
- [ ] 采样每条 uplink 的延迟、丢包、DNS 响应时间、吞吐量及系统温度；观测探测结果不得参与基础自动切换决策。
- [ ] 采用有界事件与指标存储，定义分钟、小时和按天聚合以及自动清理策略，避免持续写入耗尽 `/userdata`。
- [ ] 故障时保存受限诊断快照，包括 desired/observed state、ownership generation、route、resolver 和受管理进程身份摘要。
- [ ] 默认不长期保存完整 DNS 查询、URL、数据包内容、PPPoE/Tailscale secret 或其他凭据。
- [ ] 提供只读、固定时间范围、大小受限的状态、时间线、指标和诊断 API，并在内嵌 Web 展示当前状态与断网历史。
- [ ] 用 fake ports 验证事件顺序、重复 reconcile 去重、时钟回拨、存储失败、保留清理和辅助观测故障不影响路由 readiness。

### P3：实现本地 DNS 中心

- [ ] 选择并固定成熟的 ARM64 DNS 过滤引擎、版本、SHA-256、许可证和可复现构建输入，不在 Rust 中重写 DNS 协议实现。
- [ ] 保留 dnsmasq 对 DHCP lease 的唯一所有权，并定义 DNS 引擎、LAN 主机名和私有反向解析之间的固定端口拓扑。
- [ ] 让 LAN 客户端直接使用 `192.168.8.1:53`，支持缓存、固定规则集、allowlist/blocklist 和 `home.arpa` 家庭域名。
- [ ] 支持受限的按客户端 DNS 策略，不把 MAC OUI 推断结果直接当作安全身份。
- [ ] 将 active uplink resolver generation 原子提交给 DNS 引擎；无有效上游时明确进入 DNS degraded，不继续使用 stale resolver。
- [ ] 支持固定枚举的上游模式，包括活动 DHCP/PPP resolver 和预置 DoH/DoT provider，不接受浏览器提供任意 URL 或配置文本。
- [ ] 禁用或不暴露 DNS 引擎原生管理页面，只通过 typed application use case 和受限同源 Web API 修改配置。
- [ ] 定义查询日志的默认关闭、脱敏、大小和保留策略；秘密、完整长期历史和原始配置不得进入普通状态或诊断包。
- [ ] DNS 引擎崩溃、配置损坏或更新失败时不得影响 DHCP、LAN 管理、uplink reconciliation 或 management-only。
- [ ] 验证 Ethernet DHCP、Wi-Fi fallback、PPPoE、上联切换、OTA、规则更新和 8 小时稳定性。

### P4：集成 Tailscale RouterOnly

- [ ] 固定 Tailscale ARM64 版本、校验值、许可证、Buildroot/rootfs 输入以及 TUN 和所需内核能力。
- [ ] 定义 `Disabled` 与 `RouterOnly` typed mode，通过固定 executable、typed argv 和受身份约束的 `tailscaled` adapter 管理生命周期。
- [ ] 将 node state 保存到 root-only `/userdata/hyz-router/tailscale/`，不得进入 Git、argv、普通日志、HTTP 状态或诊断包。
- [ ] 使用一次性浏览器登录 URL 完成 tailnet 认证；不在管理页面保存或回显 reusable auth key。
- [ ] RouterOnly 只允许 `tailscale0` 访问固定管理 HTTP/API 和明确启用的路由器服务，不允许转发到 `br-lan` 或 WAN。
- [ ] 路由器自身保持 `accept-dns=false`，Tailscale 不得覆盖 active uplink resolver ownership 或本地 DNS 决策。
- [ ] 为 `tailscale0` 安装独立、最小、runtime-owned 防火墙规则，不把该接口等同于可信 LAN。
- [ ] Tailscale 登录、控制面、DERP 或进程失败不得影响 LAN、DHCP、DNS、普通 NAT、uplink fallback、management-only 或 router readiness。
- [ ] Tailscale 连接策略遵循“尽可能 direct，但 relay 永远可用”：direct 只作为性能优化，不作为远程访问 readiness 条件；存在可达 DERP 或 Peer Relay 时，direct 失败必须自动回退 relay。
- [ ] 为公网 IPv6/IPv4 direct 使用固定、typed UDP 监听端口和精确 WAN INPUT 规则；未满足 direct 条件时不得因此判定 Tailscale 不可用。
- [ ] 状态只显示 enabled、authenticated、`direct`/`peer-relay`/`DERP` 连接类型和错误类别，不返回 node key、auth key、完整登录 URL 历史或 peer secret。
- [ ] 验证首次登录、注销、重启、失去公网、上联切换、OTA 保留、恢复出厂清理和 8 小时稳定性。

### P5：增加 Tailscale LAN subnet access

- [ ] 在 RouterOnly 稳定后增加 `LanSubnetAccess` typed mode，只允许发布固定 `192.168.8.0/24`，不接受任意 subnet。
- [ ] 明确记录 tailnet 侧 route approval 和 ACL/grants 前置条件，设备不得假设发布即代表已授权。
- [ ] 仅在该模式严格 ready 时允许 `tailscale0 -> br-lan` 转发，并安装固定 FORWARD/conntrack 规则。
- [ ] 默认拒绝 `br-lan -> tailscale0` 主动新连接，并继续拒绝 `tailscale0 -> WAN`；第一版不提供 exit node。
- [ ] 将 Tailscale 远程客户端访问本地 DNS 中心和 `home.arpa` 作为独立受控选项，不改变路由器自身 DNS。
- [ ] mode disable、注销、进程退出和 shutdown 后不得残留 subnet route、forwarding rule 或监听端口。
- [ ] 验证远程管理、LAN 设备访问、DNS、ACL 拒绝、WAN 切换、控制面离线和故障回退。
- [ ] 证明 Tailscale subnet 功能失败不会改变 Complete Router Baseline 或 RouterOnly 的安全边界。

### P6：让代理适配多 uplink

- [ ] 将 proxy readiness 从固定 `wlan0` 改为已确认 active uplink。
- [ ] 验证 Ethernet DHCP、Wi-Fi fallback 和 PPPoE 下的 explicit/TUN。
- [ ] 验证 WAN 切换后 Mihomo 新连接跟随主路由表。
- [ ] 验证 core crash 后普通 NAT 回到当前 active uplink。
- [ ] 完成 DNS 接管、节点全部失效和 live-hang 回退策略。
- [ ] 完成路由+代理 8 小时稳定性测试。

## 11. 推荐实施顺序

1. 保持当前 Wi-Fi-only 行为，先引入 typed uplink 和按 uplink ownership。
2. 完成固定 LAN：`br-lan = eth1 + p2p0`。
3. 实现 Ethernet DHCP 和 metric `100`。
4. 实现 Ethernet/Wi-Fi 共存、Wi-Fi metric `600` 和自动 fallback。
5. 完成 DHCP 双上游 NAT、DNS、状态和 SR-09 测试。
6. 完成当前稳定性与 management-only 故障降级遗留项。
7. 实现 PPPoE 系统能力、安全配置和 `ppp0` lifecycle。
8. 实现 PPPoE NAT、MTU/MSS、DNS 和 Wi-Fi fallback。
9. 完成 PW-05 和完整基础产品验收。
10. 在基础路由事件产生点加入结构化事件，完成网络黑匣子的有界存储、断网时间线、指标和诊断快照。
11. 集成固定 DNS 过滤引擎，完成家庭域名、按客户端策略、active resolver 原子切换和隐私边界。
12. 集成 Tailscale `RouterOnly`，只开放受限远程管理，不允许 LAN/WAN forwarding。
13. 在 RouterOnly 稳定后增加固定 `192.168.8.0/24` 的 `LanSubnetAccess`，继续禁止 exit node。
14. 最后补齐 Mihomo 对 Ethernet、Wi-Fi fallback、PPPoE、本地 DNS 中心和 Tailscale 共存场景的组合测试。

每一步都先更新 domain desired/observed model 和 fake-port 测试，再扩展 application planner，最后实现 Linux adapter。不得以 shell wrapper、任意字符串命令或浏览器直接配置 Linux 资源绕过架构。

## 12. Definition of Done

### 12.1 DHCP 双上游里程碑

以下全部完成后，可标记 **DHCP Router Baseline**：

1. `br-lan = eth1 + p2p0`；
2. Ethernet DHCP 和 Wi-Fi STA DHCP 均可独立上网；
3. 两者可共存，metric `100 < 600`；
4. 有线 carrier/session/lease 或 runtime-owned 默认路由失效和恢复时，新连接按 metric 自动切换；不把公网黑洞纳入基础切换承诺；
5. per-uplink resolver ownership、active resolver set、NAT、FORWARD、WAN INPUT 和 status 跟随 active uplink；
6. management-only、ownership、rollback 和 shutdown 通过；
7. SR-09 实机矩阵和稳定性适用项通过；
8. recovery-free OTA 保留网络配置；
9. 无真实凭据进入仓库、固件模板或普通日志。

### 12.2 完整基础路由产品

以下全部完成后，才能标记 **Complete Router Baseline**：

1. DHCP Router Baseline 已完成；
2. 使用受控实验室 PPPoE server 验证无 VLAN 和至少一个非零 VLAN 路径；
3. Ethernet DHCP 与 PPPoE 模式互斥且可安全切换；
4. PPPoE 与 Wi-Fi fallback 共存；
5. PPPoE 重拨、MTU/MSS、DNS、NAT 和防火墙通过；
6. PW-05 实机矩阵、20 次重拨和 8 小时稳定性通过；
7. PPPoE 凭据经过 OTA 保留验证且没有泄漏；
8. 从干净工作区可重建相同 SDK、Rust、Web 和固件输入；
9. 产品提交、SDK owning repositories 和 pinned manifest 均可追溯。

### 12.3 家庭网络黑匣子

黑匣子能力只有在以下全部满足后才能标记完成：

1. uplink、DHCP、PPPoE、route、resolver、NAT 和 readiness 变化产生结构化事件及准确原因；
2. 能从时间线确认一次故障的开始、切换、恢复和中断时长；
3. 指标、事件和诊断快照采用有界保留，存储失败不影响路由核心；
4. 观测探测不参与基础自动切换决策；
5. 默认不长期保存完整 DNS 查询、URL、数据包或秘密；
6. 时间线和只读诊断 API 的大小、时间范围和权限边界通过测试。

### 12.4 本地 DNS 中心

DNS 中心只有在以下全部满足后才能标记完成：

1. dnsmasq DHCP ownership 与固定 DNS 引擎的职责和端口无冲突；
2. LAN 客户端可使用过滤、`home.arpa` 和受限按客户端策略；
3. DNS 上游原子跟随 active resolver generation，stale resolver 不被继续使用；
4. 原生管理面、任意 URL、任意配置文本和长期完整查询历史未暴露；
5. DNS 引擎崩溃、配置损坏和更新失败不影响 LAN 管理或 Complete Router Baseline；
6. 双 DHCP uplink、PPPoE、fallback、OTA 和稳定性测试通过。

### 12.5 Tailscale 远程访问

Tailscale 分两级验收：

1. **RouterOnly**：远程客户端只能访问固定路由器管理服务，不能转发到 LAN 或 WAN；
2. **LanSubnetAccess**：只发布固定 `192.168.8.0/24`，并要求 tailnet route approval 与 ACL/grants；
3. node state 和认证秘密保持 root-only，不进入 argv、日志、HTTP 状态或诊断包；
4. Tailscale 不覆盖路由器自身 DNS，不改变 active uplink resolver ownership；
5. disable、注销、崩溃和 shutdown 不残留 route、rule、forwarding 或监听状态；
6. 登录、控制面或 DERP 故障不影响 Complete Router Baseline、本地 DNS 或 management-only；
7. direct 只作为性能优化，不作为 readiness 条件；存在可达 DERP 或 Peer Relay 时，direct 失败能自动回退 relay；
8. 第一版不提供 exit node 或任意 subnet 发布。

### 12.6 可选代理能力

代理能力只有在以下全部满足后才能标记完成：

1. explicit、TUN、disabled 和普通 NAT 回退通过；
2. Ethernet DHCP、Wi-Fi-only、双 uplink 和 PPPoE 适用组合通过；
3. core crash、配置损坏、WAN 切换和 shutdown 不残留 interception；
4. 管理 API、controller 和秘密边界通过；
5. 路由+代理稳定性测试通过；
6. 代理功能失败不影响 Complete Router Baseline。
