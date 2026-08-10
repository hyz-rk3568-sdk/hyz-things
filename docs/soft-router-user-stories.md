# hyz_things 软路由 User Stories

## 1. 文档状态

本文定义 `hyz_things` RK3568 产品的第一版软路由功能。目标是提供类似 OpenWrt `br-lan` 的 IPv4 NAT 路由器能力，并将硬件能力验证与产品功能实现分开验收。

当前约束和实机事实：

- 板上存在 `eth0`、`eth1` 和 `wlan0`；有线接口仍按产品决定暂缓配置；
- Wi-Fi 芯片为 SDIO RTL8852BS，加载模块为 `8852bs`；
- 产品分支已经启用 `CONFIG_CONCURRENT_MODE=y`，实际创建 `wlan0` STA 和 `p2p0` AP；
- `CONFIG_MCC_MODE=n`，STA 与 AP 当前只保证在同一信道工作；
- 产品 rootfs 已包含 `wpa_supplicant`、`hostapd`、`dnsmasq`、`iw`、`iproute2` 和 legacy iptables；
- 产品内核已启用 IPv4 forwarding、conntrack、NAT 和 MASQUERADE；bridge、TUN、策略路由及 TPROXY/mark/socket/comment/REDIRECT 已进入新固件，并通过板端 `/dev/net/tun`、iptables 扩展和 `ip rule` 检查；
- Wi-Fi-only LAN 已迁移并安装为 `br-lan = p2p0`，LAN 地址、dnsmasq 和防火墙入口均使用 `br-lan`；`eth0`、`eth1` 未加入桥，板端控制面、生命周期、下游关联/DHCP 和普通 NAT 计数通过，客户端应用层 DNS/HTTPS 待确认；
- 8 小时并发稳定性、上游换信道恢复和多客户端压力仍需继续验证。

独立 STA 的扫描、WPA2、DHCP、metric 600、断开重连和重启持久化基线已经通过，详见
[`soft-router-sta-validation.md`](soft-router-sta-validation.md)。同信道 STA+AP 的 WPA2、下游 DHCP
和短时并发冒烟测试已经通过，详见
[`soft-router-sta-ap-validation.md`](soft-router-sta-ap-validation.md)。STA+AP+NAT 的自动启动、持久 DHCP、
受限防火墙、下游上网和 1080p 直播验证也已经通过，详见
[`soft-router-sta-ap-nat-validation.md`](soft-router-sta-ap-nat-validation.md)。Wi-Fi-only `br-lan = p2p0` 的源码迁移和静态检查记录见
[`soft-router-br-lan-validation.md`](soft-router-br-lan-validation.md)。该桥接拓扑的固件安装、板端控制面、客户端关联/DHCP 和普通 NAT 路径已经通过，但仍不代表客户端应用层 DNS/HTTPS、8 小时稳定性、
上游换信道恢复、有线 WAN 或完整 `br-lan = eth1 + p2p0` 已经通过。

代理基础和统一管理面正在收敛：

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
- 统一 Web UI 的状态、LCD/代理模式、节点选择和受控延迟刷新已完成板端功能验证；最新 Web 稳定性 OTA 暴露旧 S81 固定 launch 次数窗口不足，源码退避修正尚未构建安装；
- 管理员认证、强制首次改密、typed AP/STA 设置、两阶段 AP 回滚和 write-only Mihomo HTTPS 订阅更新已经进入统一 Rust 源码，但尚未编译、OTA 或板测；HTTP 管理 LAN 的机密性风险与共享 bootstrap 密码的首次抢占风险由当前产品决策明确接受；
- DNS 接管、自动冷启动复验、2 小时稳定性、节点全部失效/live-hang 自动回退仍未完成，因此代理 Epic 仍不得整体标记完成。

## 2. 产品目标

作为设备管理员，我希望 RK3568 设备能够优先使用有线网络连接上游，在没有可用有线 DHCP 路由时自动使用 Wi-Fi STA，并通过有线 LAN 和 Wi-Fi AP 为下游设备提供稳定的 DHCP、DNS 和互联网访问。

第一版逻辑拓扑：

```text
                        上游 WAN
                 ┌─────────┴─────────┐
                 │                   │
       eth0, DHCP, metric 100   wlan0 STA, metric 600
                 │                   │
                 └─────────┬─────────┘
                           │
                  IPv4 转发、NAT、防火墙
                           │
                  br-lan 192.168.8.1/24
                    ┌──────┴──────┐
                    │             │
                  eth1           p2p0
                              下游 Wi-Fi AP
```

## 3. 已确认的产品决策

| 项目 | 第一版决策 |
| --- | --- |
| 有线 WAN | `eth0` |
| 有线 LAN | `eth1` |
| Wi-Fi 上游 | `wlan0` STA |
| Wi-Fi 下游 | RTL8852BS 虚拟 AP 接口，实机名称 `p2p0` |
| LAN 桥 | 当前 `br-lan = p2p0` 的固件、控制面、下游 DHCP 和普通 NAT 路径已通过，待应用层确认；完整目标为 `br-lan = eth1 + p2p0` |
| LAN 地址 | `192.168.8.1/24` |
| DHCP 地址池 | 当前无网线验证为 `192.168.8.100` 至 `192.168.8.199`；完整第一版目标为 `192.168.8.100` 至 `192.168.8.249` |
| 有线默认路由 | metric `100` |
| Wi-Fi 默认路由 | metric `600` |
| 上游协议 | IPv4 DHCP |
| 下游路由 | IPv4 NAT，不做上游 STA 透明二层桥接 |
| 有线优先条件 | `eth0` 有 carrier 且 DHCP 已安装有效默认路由 |
| Wi-Fi 回退条件 | 网线断开，或有线 DHCP 尚未取得有效路由 |
| 公网健康检查 | 第一版不实现 |
| AP 安全 | WPA2-PSK 起步；密码不得提交到 Git |
| IPv6 | 第一版不实现 |
| 可选代理核心 | 第二阶段采用 Mihomo；不在板端安装 Clash Verge Rev 或 OpenClash |
| 代理管理界面 | 统一使用 `hyz-router` 内嵌 Yew 页面；不安装 Mihomo 第三方 Dashboard |
| 代理上线顺序 | 先验证显式代理，再实现仅接管 LAN 入站的透明代理 |
| 代理失败策略 | 必须可切回普通 IPv4 NAT，不能因代理核心失败导致下游永久断网 |
| 光猫接入阶段 | 先支持光猫路由模式下的 `eth0` DHCP WAN，再支持光猫桥接后的 PPPoE 直拨 |
| PPPoE 承载 | 默认直接在 `eth0` 拨号，并支持按运营商配置可选的 `eth0.<VLAN ID>` |
| 直拨目标 | 消除设备可控的光猫 NAT + RK3568 NAT 双重 NAT，不承诺绕过运营商 CGNAT |
| NAT 类型目标 | 避免主动采用加剧对称型 NAT 的端口随机化策略，并通过实测记录 UDP 映射行为；不承诺固定 NAT 类型 |
| PPPoE 凭据 | 账号、密码和 VLAN 参数只保存于 `/userdata`，不得进入 Git、固件模板或普通日志 |

有线 carrier 存在且 DHCP 成功后，即使该网络无法访问公网，第一版仍保持使用有线 WAN。公网探测和基于探测结果的故障切换属于后续独立需求。

普通 802.11 STA 无法为多个下游 MAC 提供透明桥接，除非驱动与上游 AP 都支持 4-address/WDS。因此 `wlan0` 不加入 `br-lan`，而是作为三层 WAN 通过 NAT 为下游转发。

## 4. Epic SR：双上游软路由

### SR-00：验证 RTL8852BS STA+AP 并发能力

**User Story**

作为产品开发者，我希望先验证 RTL8852BS 能否稳定地同时运行 STA 和 AP，以便在实现完整软路由前确认单无线芯片方案可行。

**验收标准**

1. 驱动使用产品分支启用 `CONFIG_CONCURRENT_MODE=y`，不直接修改或提交 vendor baseline `sdk-main`。
2. 系统能够创建一个 STA 接口和一个 AP 接口，并明确记录实际接口名。
3. STA 能通过 WPA2 连接上游 AP 并取得 DHCP 地址。
4. 下游客户端能连接本机 AP 并稳定交换数据。
5. STA 与 AP 同时工作时至少连续运行 8 小时，无驱动崩溃、SDIO reset、接口消失或不可恢复断连。
6. 记录同信道限制、支持频段、吞吐量和上游换信道时的 AP 行为。
7. 验证失败时停止单芯片并发集成，并在以下降级方案中选择一种：
   - 有线 WAN + RTL8852BS AP；
   - Wi-Fi STA WAN + `eth1` LAN，不提供并发 AP；
   - 增加第二张 USB Wi-Fi 网卡。

**说明**

当前 `CONFIG_MCC_MODE=n`，所以第一版只要求 STA 与 AP 共享同一信道，不要求 2.4 GHz 与 5 GHz 跨信道并发。

### SR-01：提供软路由系统能力

**User Story**

作为固件开发者，我希望产品 rootfs 和内核包含受控的网络组件，以便后续服务能够配置 STA、AP、桥、DHCP、DNS、NAT 和防火墙。

**验收标准**

1. Buildroot 产品配置至少提供 `iw`、`iproute2`、`wpa_supplicant`、`hostapd` 和 `dnsmasq`。
2. 选定并只使用一种主防火墙管理方案；第一版优先选择与 Rockchip 5.10 内核兼容性更好的 iptables，避免同时维护 nftables 和 iptables 规则。
3. 内核产品配置包含 bridge、IPv4 forwarding、conntrack、NAT、MASQUERADE，以及透明代理和规则所有权后续需要的 TUN、IPv4 multiple tables、mark、socket、comment、TPROXY 和 REDIRECT 符号。
4. 所有 Kconfig 修改位于 owning repository 的产品分支，不修改 `sdk-main`。
5. `make check` 能断言关键包和内核配置存在。
6. 固件中不包含测试 SSID、Wi-Fi 密码或私有网络凭据。

### SR-02：持久化网络配置

**User Story**

作为设备管理员，我希望网络配置在重启和 OTA 后保持不变，并且可以安全恢复默认值。

**验收标准**

1. 可变配置和凭据保存于 `/userdata/hyz-router/`，不放入只读 rootfs。
2. rootfs 仅提供不含密码的默认模板。
3. 配置文件权限限制为 root 可读写。
4. 支持配置上游 STA SSID、安全类型和密码。
5. 支持配置下游 AP SSID、密码、国家码和可选信道。
6. 支持恢复默认配置，但恢复操作不得自动删除其他 `/userdata` 内容。
7. 配置校验失败时不启动不安全的开放 AP，并输出明确日志。

### SR-03：连接有线 WAN

**User Story**

作为设备管理员，我希望设备在 `eth0` 接入有线网络时自动取得上游地址，并将它设为首选 WAN。

**验收标准**

1. `eth0` carrier up 后启动 DHCP 客户端。
2. DHCP 成功后安装地址、网关、DNS 和默认路由 metric `100`。
3. carrier down 后及时停止或释放 DHCP，并删除失效地址和默认路由。
4. DHCP 失败时不保留不可用的有线默认路由。
5. 重复插拔网线不会产生重复地址、重复默认路由或僵尸 DHCP 进程。
6. `eth0` 永远不加入 `br-lan`。

### SR-04：连接 Wi-Fi STA WAN

**User Story**

作为设备管理员，我希望设备使用保存的 Wi-Fi 凭据连接上游 AP，以便在没有可用有线路由时继续联网。

**验收标准**

1. `wpa_supplicant` 使用 `/userdata` 中的配置管理 `wlan0`。
2. STA 关联成功后启动 DHCP，并安装默认路由 metric `600`。
3. 有线 WAN 正常时允许 STA 保持关联，但新流量优先使用 metric `100` 的有线路由。
4. STA 断线后自动重连，不影响 `br-lan` 地址和下游 DHCP 服务。
5. 日志不得打印明文 PSK。
6. 国家码和监管域由配置明确提供，不依赖未定义默认值。

### SR-05：提供下游 Wi-Fi AP

**User Story**

作为下游用户，我希望通过受密码保护的 Wi-Fi AP连接 RK3568 设备，并获得与有线 LAN 相同的局域网服务。

**验收标准**

1. `hostapd` 管理专用 AP 接口 `p2p0`，接口加入 `br-lan`。
2. AP 默认使用 WPA2-PSK，不允许空密码启动生产 AP。
3. STA+AP 单芯片并发时，AP 信道遵循 SR-00 验证出的同信道规则。
4. 上游 STA 换信道导致 AP 重启或短暂断开时，系统能够自动恢复 AP。
5. AP 配置或驱动失败不影响 `eth1` 有线 LAN。
6. AP 客户端之间是否隔离由显式配置控制，默认行为写入文档。

### SR-06：建立 `br-lan`、DHCP 和 DNS

**User Story**

作为下游用户，我希望无论通过 `eth1` 还是 AP 接入，都能获得同一网段地址和 DNS 服务。

**验收标准**

1. 启动时创建 `br-lan`，并配置 `192.168.8.1/24`。
2. `eth1` 和 `p2p0` 加入 `br-lan`；`eth0` 和 `wlan0` 不加入。
3. `dnsmasq` 只在 `br-lan` 上提供 DHCP，默认地址池为 `192.168.8.100-192.168.8.249`。
4. DHCP 下发网关和 DNS 地址 `192.168.8.1`。
5. WAN 切换不改变 `br-lan` 地址，不清除有效 LAN 租约。
6. DNS 转发使用当前系统上游 DNS，并在 WAN DHCP 信息改变时刷新。
7. DHCP/DNS 不监听 WAN 接口。

**当前增量状态**

Wi-Fi-only 第一阶段已安装：服务创建自有 `br-lan`，只加入 `p2p0`，并把 `192.168.8.1/24`、dnsmasq 和防火墙 LAN 入口迁移到桥。服务通过记录 bridge ifindex 和 firewall UUID token 限制清理范围，并用 `/run` 生命周期锁串行化修改操作；发现无所有权标记的同名资源、ifindex/token 不匹配或意外桥成员时拒绝删除。`eth0`、`eth1` 保持不入桥。OTA 保留性、板端拓扑、防火墙、start/restart/stop/start、下游关联/DHCP、LAN 双向可达和普通 NAT 计数已经通过；客户端应用层 DNS/HTTPS 尚待用户确认，SR-06 仍未完成。

### SR-07：提供 IPv4 NAT 和最小安全防火墙

**User Story**

作为设备管理员，我希望下游设备能够访问当前 WAN，同时阻止 WAN 主动访问 LAN，以便设备具备基本路由器安全边界。

**验收标准**

1. 启用 IPv4 forwarding。
2. 对从 `br-lan` 发往 `eth0` 或 `wlan0` 的流量执行 MASQUERADE。
3. 允许 LAN 到 WAN 的新连接和对应返回流量。
4. 默认拒绝 WAN 到 `br-lan` 的新连接。
5. DHCP、DNS 和管理服务只开放在预期接口。
6. 防火墙规则可重复加载，不生成重复规则。
7. 服务停止或配置失败时采用 fail-closed WAN 入站策略。
8. 规则和路由状态可通过诊断命令导出。

### SR-08：实现有线优先的 WAN 选择

**User Story**

作为设备管理员，我希望设备始终优先使用有线 WAN，并仅在没有可用有线 DHCP 路由时使用 Wi-Fi，以获得确定且可解释的上游选择行为。

**验收标准**

1. `eth0` 默认路由 metric 固定为 `100`。
2. `wlan0` 默认路由 metric 固定为 `600`。
3. 两条默认路由同时存在时，新建连接经 `eth0`。
4. 拔掉网线后删除或停用有线路由，并自动使用 `wlan0`。
5. 网线重新插入且 DHCP 成功后自动恢复 metric `100` 的路由，新建连接切回 `eth0`。
6. 网线存在但 DHCP 未成功时继续使用 Wi-Fi。
7. WAN 切换同步更新有效 DNS 状态，且不重建 `br-lan`。
8. WAN 切换期间已有 NAT 会话可以中断；第一版只保证新建连接使用正确 WAN。
9. 第一版不通过 ping、HTTP 或 DNS 探测判断公网可用性。
10. 每次切换记录原因、旧 WAN、新 WAN 和时间，但不记录凭据。

### SR-09：提供可靠启动、停止和恢复

**User Story**

作为维护人员，我希望软路由服务具有确定的启动顺序和恢复路径，以便网络配置错误不会使设备无法维护。

**验收标准**

1. 启动顺序至少满足：内核接口就绪 → 配置校验 → `br-lan` → DHCP/DNS → STA/AP → WAN 路由 → 防火墙。
2. 服务支持 `start`、`stop`、`restart` 和 `status`。
3. 重复执行 `start` 和 `restart` 是幂等的。
4. 配置错误时保留 ADB 和本地控制台维护能力。
5. 一个 WAN 失败不停止 `br-lan`、DHCP 或另一个 WAN。
6. 服务日志包含接口、carrier、DHCP、关联、路由和防火墙状态。
7. 不依赖 Flutter 或 Weston 才能运行路由功能。
8. OTA 后首次启动沿用 `/userdata` 配置，并能够迁移旧配置版本。

### SR-10：端到端验收软路由

**User Story**

作为产品负责人，我希望通过可重复的网络测试矩阵验收软路由，以便确认功能不是只在单一启动场景下可用。

**验收标准**

至少覆盖以下场景：

1. 只有有线 WAN：`eth1` 客户端和 AP 客户端均可访问互联网。
2. 只有 Wi-Fi STA WAN：两个下游入口均可访问互联网。
3. 有线和 Wi-Fi 同时在线：路由选择 `eth0`。
4. 在线拔掉网线：新连接在规定时间内切到 Wi-Fi。
5. 在线插回网线：DHCP 成功后新连接切回有线。
6. 网线有 carrier 但 DHCP 失败：继续使用 Wi-Fi。
7. STA 断线重连：有线 WAN 和 LAN 服务不受影响。
8. AP 重启：`eth1` LAN 和 WAN 状态不受影响。
9. 连续重启设备至少 20 次，不出现重复进程、重复路由或重复防火墙规则。
10. 运行压力流量时无内核崩溃、驱动 reset、不可恢复接口丢失或 `POST_BUF_EMPTY` 等无关显示回归。

每个场景记录接口地址、路由表、DNS、conntrack/NAT 状态、下游 DHCP 租约和关键日志。

## 5. Epic PW：光猫桥接与 PPPoE 直拨 WAN

该 Epic 是有线 DHCP WAN 和完整 `br-lan` 基线通过后的后续扩展。目标是让光猫工作在桥接模式，
由 RK3568 终结 PPPoE 会话并承担唯一一层本地 IPv4 NAT，从而消除可由本产品控制的双重 NAT。
运营商 CGNAT、上游端口封锁和公网地址分配策略不受本设备控制，因此不得承诺“直拨后一定获得公网 IPv4”
或“一定不是对称型 NAT”。

### PW-00：识别光猫和运营商接入模式

**User Story**

作为部署人员，我希望明确区分光猫路由、光猫桥接和运营商 CGNAT，以便选择正确的 WAN 配置并解释 NAT 层级。

**验收标准**

1. 光猫路由模式继续使用 `eth0` DHCP WAN，并明确提示该拓扑可能形成双重 NAT。
2. 光猫桥接模式允许选择 PPPoE，并明确由 RK3568 负责拨号、默认路由、防火墙和 NAT。
3. 支持“无 VLAN”和“指定 802.1Q VLAN ID”两种 PPPoE 承载方式，不把地区或运营商 VLAN 写死在固件中。
4. 状态输出能够区分 `eth0`、可选 VLAN 接口和 `ppp0` 的链路、会话及默认路由状态。
5. 检测 PPPoE 获得的地址是否属于 RFC1918、`100.64.0.0/10` 或其他非公网范围，并提示可能存在上游 NAT；判断结果不泄漏账号。
6. IPTV、语音业务、多业务 VLAN 和光猫管理通道不随互联网 PPPoE 自动配置，作为独立需求处理。

### PW-01：提供可复现的 PPPoE 系统能力

**User Story**

作为固件开发者，我希望产品固件包含由 Buildroot 配置生成的 PPPoE 和 VLAN 能力，以便从干净工作区重建直拨环境。

**验收标准**

1. Buildroot 产品配置提供 `pppd`、Linux PPPoE 支持及所需插件，不依赖目标板临时下载软件。
2. 内核产品配置包含 PPP、PPPoE、802.1Q VLAN、conntrack、NAT 和所选防火墙路径需要的符号。
3. `make check` 或等价静态检查断言用户态工具、插件和关键内核符号存在。
4. rootfs 只提供无账号、无密码、无运营商专用 VLAN 的模板。
5. 所有改动位于对应 owning repository 的产品分支，并可由 manifest 固定版本。
6. 固件记录所用 PPP 实现、许可证和版本，不提交生成物或本地 `.config`。

### PW-02：安全保存 PPPoE 和 VLAN 配置

**User Story**

作为设备管理员，我希望拨号配置在重启和 recovery-free OTA 后保留，同时账号密码不会泄漏。

**验收标准**

1. PPPoE 账号、密码、服务名、可选 AC 名称和 VLAN ID 保存于 `/userdata/hyz-router/` 下的 root-only 配置。
2. 密码不出现在进程命令行、状态输出、普通日志、构建日志、Git 或 rootfs 默认模板中。
3. VLAN ID 仅允许合法范围，并拒绝会与 LAN、管理接口或保留 VLAN 发生冲突的配置。
4. 支持在不删除其他 `/userdata` 内容的前提下清除 PPPoE 凭据并退回有线 DHCP WAN。
5. 配置变更先校验再生效；失败时保留上一个可用配置和本地维护能力。
6. OTA 配置迁移失败时不进行无限高速重拨，并留下不含秘密的诊断信息。

### PW-03：实现可靠的拨号和断线重拨

**User Story**

作为下游用户，我希望 RK3568 能在光猫桥接链路可用时自动拨号，并在会话断开后恢复互联网连接。

**验收标准**

1. `eth0` carrier up 后在选定的物理或 VLAN 接口发起 PPPoE，会话成功后创建 `ppp0` 并安装默认路由。
2. 拨号进程由受控服务管理，支持 `start`、`stop`、`restart`、`status` 和禁用，重复调用保持幂等。
3. 认证失败、发现阶段超时和会话断开采用有上限的指数退避，避免日志和拨号风暴。
4. carrier down 时终止失效会话并清理对应路由；carrier 恢复后自动重新拨号。
5. PPPoE 重连导致地址或接口变化时，自动刷新 DNS、NAT、防火墙和可选代理的 WAN 绑定。
6. 连续执行至少 20 次重连，不残留重复 `pppd`、VLAN 接口、默认路由、防火墙规则或僵尸会话。
7. PPPoE 失败不破坏 `br-lan`、DHCP、DNS、ADB 和本地控制台。

### PW-04：适配 PPPoE 防火墙、MTU 和 DNS

**User Story**

作为设备管理员，我希望 PPPoE WAN 具有与 DHCP WAN 一致的安全边界，并避免 MTU 问题造成部分网络不可用。

**验收标准**

1. 只允许 `br-lan → ppp0` 的新连接和对应返回流量，默认拒绝 `ppp0 → br-lan` 的主动新连接。
2. MASQUERADE 跟随实际 PPP WAN 接口和会话生命周期，重拨后不保留失效规则。
3. 默认按 PPPoE 协商结果使用 MTU；典型未扩展场景验证 `1492`，不得无条件假设所有运营商都相同。
4. 对转发 TCP 配置与路径 MTU 一致的 MSS clamp，并验证 HTTPS、大响应、VPN 和长连接不因 PMTU 黑洞卡住。
5. 使用 PPP 协商得到的 DNS 或显式配置的 DNS 更新转发器，停止会话后清除失效上游 DNS。
6. DHCP、DNS、`hyz-router` 管理 HTTP 和其他管理端口不得监听或开放在 `ppp0`。
7. 防火墙加载失败时采用 WAN 入站 fail-closed，并保持可诊断和可回滚。

### PW-05：验收双重 NAT 消除和 NAT 边界

**User Story**

作为产品负责人，我希望通过可重复测试确认直拨确实减少了本地 NAT 层级，并准确说明剩余的运营商限制。

**验收标准**

1. 分别记录“光猫路由 + RK3568 NAT”和“光猫桥接 + RK3568 PPPoE/NAT”两种拓扑的地址、路由和 NAT 层级。
2. 在桥接直拨拓扑中确认光猫不再为互联网路径执行 IPv4 NAT，RK3568 成为本地唯一 NAT 设备。
3. 比较 PPPoE WAN 地址与外部观测地址；不一致或获得私网/CGNAT 地址时明确标记运营商侧 NAT。
4. 使用受控 STUN/UDP 测试记录多个目的端点下的映射和过滤行为，不仅根据产品规则推断 NAT 类型。
5. NAT 规则不得主动启用随机源端口策略来加剧映射不稳定；如内核或 conntrack 行为无法保证端口保持，必须记录限制。
6. 验证游戏、WebRTC、UDP 会话、端口转发等代表性场景；UPnP、NAT-PMP 或 PCP 需另立安全 Story，默认不开放。
7. 测试结论使用“消除可控双重 NAT”和“检测到/未检测到 CGNAT”，不得宣称能够绕过运营商 CGNAT 或保证固定 NAT 类型。
8. 完成至少 8 小时 PPPoE+NAT 稳定性测试，记录重拨、丢包、吞吐、延迟、CPU、内存和 conntrack 状态。

## 6. Epic PX：可选代理网关

该 Epic 是基础软路由完成后的第二阶段扩展，不阻塞 SR-00 至 SR-10 的交付。这里的 Mihomo 是规则代理和
VPN-like TUN/透明代理能力，不等同于 WireGuard、IPsec 或 OpenVPN 远程接入 VPN。

### PX-00：选择适合无头 Buildroot 的代理栈

**User Story**

作为产品维护者，我希望选择适合 ARM64 无头路由器的受维护代理核心和 Web 控制面，以避免把桌面或
OpenWrt 专用应用错误移植到 Buildroot。

**验收标准**

1. 板端代理核心选择 Mihomo，管理界面统一为 `hyz-router` 内嵌 Yew 页面。
2. 不安装 MetaCubeXD、Clash Verge Rev 或其他独立 Dashboard；Clash Verge Rev 只作为桌面客户端参考。
3. OpenClash 不直接移植；它依赖 OpenWrt、LuCI 和对应包管理/防火墙框架。
4. sing-box 保留为配置完全自管时的备选；Xray-core 仅在明确需要其特定协议时重新评估。
5. dae 在当前 Linux 5.10 上不采用；其 LAN/WAN bind 要求 Linux 5.17 或更高版本。
6. 选型记录官方仓库、许可证、ARM64 发布物和内核要求，并明确区分事实与工程建议。

### PX-01：提供可复现的 Mihomo 与统一 Web bundle

**User Story**

作为固件开发者，我希望 Mihomo 与 `hyz-router` 内嵌 Yew 资源都由固定输入生成，以便从干净工作区重建相同代理与管理组件，而不依赖目标板在线下载最新版。

**验收标准**

1. Mihomo 固定到明确 tag 和 Linux ARM64 官方发布物，下载文件使用 reviewed SHA-256 校验。
2. 当前锁定候选版本为 Mihomo `v1.19.29`；正式发布前重新查询官方 Release 并审核是否升级，任何升级都必须更新资产和许可证哈希。
3. Yew、Trunk、Cargo lock 和 deterministic archive 都由 `apps/rust/router` 统一构建，不安装独立静态 Dashboard 包。
4. Mihomo Buildroot 包记录许可证、对应源码位置和 license hash，并能够纳入 `legal-info`。
5. 不提交下载后的二进制、订阅内容、节点信息、API secret 或运行时缓存。
6. 若采用官方预构建 ARM64 二进制，文档明确记录该供应链选择；源码重建作为后续独立任务。
7. Mihomo 与统一 `hyz-router` ELF 在 RK3568 上分别通过架构、依赖、启动和基本自检。

### PX-02：安全持久化代理配置

**User Story**

作为设备管理员，我希望代理配置在重启和 recovery-free OTA 后保留，同时不把订阅和认证信息泄漏到
rootfs、Git 或普通日志。

**验收标准**

1. 真实配置位于 `/userdata/hyz-router/mihomo/`，rootfs 只提供无秘密模板。
2. 配置目录和包含订阅、节点、证书或控制密钥的文件仅允许 root 访问。
3. 支持本地 YAML 和订阅生成的配置，但订阅 URL 不出现在版本库、构建日志或状态输出中。
4. 启动前执行配置语法校验；校验失败时保持普通 NAT 可用并输出不含秘密的错误。
5. GeoIP、GeoSite、rule-provider 和缓存使用固定目录，并定义 OTA、恢复默认值和空间上限策略。
6. 日志默认不打印节点凭据、订阅 URL、完整请求 URL 或控制 API secret。

### PX-03：先验证显式代理

**User Story**

作为产品开发者，我希望先通过显式代理验证 Mihomo 核心、配置和上游节点，以便在修改透明路由前隔离问题。

**验收标准**

1. Mihomo 的 mixed 代理端口只监听 `br-lan` 地址 `192.168.8.1`，不监听 WAN 地址。
2. 下游客户端手工设置 `192.168.8.1:<port>` 后能够访问 HTTP 和 HTTPS。
3. 验证 DNS、TCP、至少一种 UDP 场景、节点切换和代理核心重启。
4. 未设置代理的下游流量继续使用当前普通 NAT，不被隐式接管。
5. 记录代理前后吞吐和延迟；测试结果与当前弱 RSSI、单射频 STA+AP 限制分开解释。
6. 显式代理连续运行至少 2 小时，无进程崩溃、持续内存增长或 STA/AP 回归。

### PX-04：提供可回退的透明代理

**User Story**

作为下游用户，我希望设备按规则透明代理 LAN 流量，而无需逐台配置客户端，同时在代理不可用时仍可恢复普通 NAT。

**验收标准**

1. 实现前验证并启用所选路径需要的内核能力，包括 `CONFIG_TUN`、策略路由和必要的 TPROXY/mark/socket 符号。
2. 透明代理只接管来自 `br-lan` 的目标流量；当前唯一桥成员为 `p2p0`，以后加入 `eth1` 时无需把规则重新绑定到物理 LAN 接口。不得接管 ADB、DHCP、局域网管理和上游维护流量。
3. 明确排除 LAN 子网、上游网关、代理服务器地址、本机地址和保留地址，防止代理环路。
4. TCP、UDP 和 DNS 行为分别验收；第一轮只承诺 IPv4。
5. 规则安装幂等，使用独立 chain、mark 和 policy-routing table，不破坏 `HYZ_ROUTER_FWD`、`HYZ_ROUTER_NAT` 或未来 WAN 选择。
6. 支持 `explicit`、`tun` 和 `disabled` 三种明确模式，并能在不重启设备的情况下切换。
7. Mihomo 停止、配置损坏或节点全部失效时，能够撤销代理规则并按产品策略回退普通 NAT。
8. 状态命令输出当前模式、监听地址、策略路由和规则健康度，但不输出节点或订阅秘密。

### PX-05：提供统一的受限管理页面

**User Story**

作为设备管理员，我希望从 LAN 浏览器查看路由和代理状态，同时不把 Mihomo Controller 或 root mutation 暴露给未认证客户端。

**验收标准**

1. 页面由 `hyz-router` 内嵌 Yew bundle 并通过固定 `192.168.8.1:8080` 提供，不安装独立 Dashboard。
2. 状态 API 保持 GET-only；仅允许固定的 LCD、代理模式、组内节点选择和受控延迟测试 typed POST，未知 `/api/*` 不进入 SPA fallback。
3. HTTP 无 CORS，并保持严格 CSP；每个控制请求要求精确同源 Origin、小尺寸 typed JSON、自定义 CSRF header 和 daemon 随机 token，未知字段和路径拒绝。
4. Mihomo external controller 只监听 `127.0.0.1` 并使用随机 secret；浏览器不能直接访问 controller，页面/API 不返回订阅 URL、节点连接参数、密码、原始配置/history 或 secret。
5. 页面显示路由、WAN、LAN/AP、Mihomo/TUN、WAN 流量和经过净化的代理运行元数据，并适配手机浏览器。
6. OTA、router enable/disable、任意 URL/timeout/provider 输入和原始配置 mutation 仍仅通过 root-only Unix control socket；页面失效不得影响代理核心、fail-open 或普通 NAT。
7. 管理 LAN 客户端属于当前控制信任边界；若未来允许不可信客户端接入，必须增加独立认证和 HTTPS。

### PX-06：验收代理生命周期和故障恢复

**User Story**

作为产品负责人，我希望代理功能在重启、OTA、节点失败和网络重连时行为可预测，以便它不会降低基础路由可靠性。

**验收标准**

1. 服务支持 `start`、`stop`、`restart`、`status` 和禁用，重复调用不累积进程、规则、route 或 mark。
2. recovery-free OTA 保留 `/userdata` 配置；版本迁移失败时回退并保留可诊断信息。
3. 覆盖正常代理、全部节点失败、DNS 失败、STA 重连、AP 客户端重连、Mihomo 崩溃和配置损坏。
4. 代理开启和关闭时都能通过普通 NAT 基线测试；关闭代理后无残留透明代理规则。
5. 执行至少 8 小时 STA+AP+NAT+代理稳定性测试，记录 RSSI、吞吐、延迟、内存、CPU、连接数和驱动错误。
6. 管理 API 和代理监听端口通过 WAN 侧不可达测试。
7. 不把代理 Epic 标记完成，除非显式代理、透明代理、统一管理页面、安全边界和回退路径全部通过。

## 7. 非功能要求

- **安全性**：所有默认入站策略最小授权；凭据不进入 Git、构建日志或普通运行日志。
- **可恢复性**：配置错误不能阻塞 ADB、本地控制台或恢复镜像；代理失败不能破坏普通 NAT 基线。
- **幂等性**：服务、DHCP hook、NAT 和透明代理规则可被重复调用，不累积状态。
- **可诊断性**：提供单一状态命令汇总 link、地址、路由、STA、AP、DHCP、防火墙和可选代理状态。
- **可复现性**：所有软件包、内核符号、脚本和默认模板由 SDK 源码生成，不依赖手工修改目标板 rootfs。
- **供应链**：第三方代理核心及 Yew/Cargo/Trunk Web 依赖固定版本、哈希、许可证及源码位置，不在启动时下载 `latest`。
- **仓库边界**：Buildroot、RTL8852BS 驱动、内核网络配置和产品服务分别提交到 owning repository；使用 manifest 锁定跨仓库版本。

## 8. 第一版不包含

- IPv6 路由、DHCPv6 或前缀委派；
- 第一版基础软路由不包含 PPPoE；光猫桥接直拨由后续 PW Epic 实现；
- 蜂窝 WAN，以及 WireGuard、IPsec、OpenVPN 等远程接入或站点到站点 VPN；
- 上游公网健康探测；
- 多 WAN 负载均衡或按流量策略路由；
- 透明 Wi-Fi STA 二层桥接；
- Captive Portal；
- 除统一 `hyz-router` 状态与受限控制页面之外的通用软路由 Web 后台或 Flutter 网络配置 UI；
- QoS/SQM、完整流量统计、家长控制；
- 动态 VLAN 或多 SSID；
- 对跨频段 STA+AP 并发的保证。

Mihomo 与统一 `hyz-router` 管理面已作为第二阶段 PX Epic 纳入，光猫桥接和 PPPoE 直拨已作为后续 PW Epic 纳入；
两者都不改变第一版基础软路由边界。Mihomo 不应被描述成远程接入 VPN，PPPoE 直拨也不应被描述成
能够绕过运营商 CGNAT 或保证特定 NAT 类型。

## 9. 推荐实施顺序

当前按“不接网线，先完成 Wi-Fi STA+AP”的产品决定执行：

1. `SR-00`：完成短时 RTL8852BS 同信道 STA+AP 技术验证；8 小时稳定性仍待执行。
2. `SR-01`、`SR-04`、`SR-05`、`SR-07`：完成 Wi-Fi STA、AP、DHCP/DNS、NAT 和最小防火墙基线。
3. `SR-09`：继续补上游断线重试、上游换信道后的 AP 恢复和状态诊断。
4. `PX-00` 至 `PX-03`：固定 Mihomo 与统一 Web bundle 输入并验证显式代理。
5. `PX-04` 至 `PX-06`：在显式代理通过后实现透明代理、统一受限页面和故障回退。
6. 执行 8 小时 STA+AP+NAT 基线与 STA+AP+NAT+代理稳定性测试，两者结果分别记录。
7. 用户重新开放有线范围后实现 `SR-03`、`SR-06` 和 `SR-08`，先通过光猫路由模式下的 `eth0` DHCP WAN、完整 `br-lan` 和 WAN 切换验证。
8. 执行 `SR-10` 基础双 WAN/双 LAN 实机矩阵，不把尚未实现的 PPPoE 结果计入基础 Epic。
9. 在有线基线稳定后实现 `PW-00` 至 `PW-04`：光猫桥接、可选 VLAN、PPPoE 生命周期、防火墙和 MTU/DNS。
10. 最后执行 `PW-05` 的拓扑对比、CGNAT/NAT 行为和 8 小时 PPPoE 稳定性验收。

在用户重新开放有线范围前，不得配置 `eth0`、`eth1`、VLAN 或 PPPoE。不得在 `SR-00` 失败后继续假设
单 RTL8852BS 可以提供 STA+AP；必须选择并记录降级拓扑。代理功能和 PPPoE 功能都不得掩盖普通 NAT 基线问题。

## 10. Definition of Done

基础软路由 Epic 只有在以下条件全部满足后才能标记完成：

1. 所有纳入第一版的 SR Stories 均达到验收标准；
2. 通过 `make check` 和相关仓库静态检查；
3. 从干净克隆可构建相同配置的固件；
4. recovery-free OTA 成功安装并保留 `/userdata` 网络配置；
5. SR-10 实机矩阵通过并留存结果；
6. 产品仓库提交已进入 `main`，变更的 SDK 组件提交已进入各自的 `hyz-things/main`，开发 manifest 指向正确分支；
7. 发布前生成固定提交的 release manifest；
8. 不包含真实 Wi-Fi 密码、订阅 URL、节点凭据、API secret、私钥或其他秘密。

可选 PPPoE 直拨 PW Epic 还必须满足：

1. 光猫路由模式和桥接模式的边界、切换与回退均有文档和实机结果；
2. 无 VLAN 和配置 VLAN 的 PPPoE 路径按实际运营商条件完成适用项验证；
3. 拨号、重拨、MTU/MSS、DNS、NAT、防火墙及 WAN 入站隔离通过；
4. 直拨前后 NAT 层级、WAN 地址与外部观测地址已对比，并准确标记公网、私网或 CGNAT 情况；
5. PPPoE+NAT 8 小时稳定性和 20 次重连测试通过，无残留进程、接口、路由或规则；
6. PPPoE 凭据经过 recovery-free OTA 保留验证，且未进入版本库、固件模板、进程命令行或普通日志。

可选代理 PX Epic 还必须满足：

1. Mihomo 与统一 Yew Web bundle 的版本、锁文件、许可证及源码位置已锁定；
2. 显式代理、透明代理、DNS、统一管理页面、WAN 隔离和普通 NAT 回退均通过；
3. 代理启停不残留进程、iptables 规则、policy route、mark 或监听端口；
4. STA+AP+NAT+代理 8 小时稳定性通过并留存资源和网络指标；
5. 代理配置经过 recovery-free OTA 保留验证且没有秘密进入版本库。
