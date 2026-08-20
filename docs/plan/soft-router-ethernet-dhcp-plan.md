# Ethernet DHCP 双上游与下游无扰动引入计划

## 1. 结论与产品边界

当前源码**不能直接在运行中插入 `eth0` 并自动切换到有线**。当前生产实现仍是 Wi-Fi-only：

```text
WAN = wlan0 STA DHCP，metric 600
LAN = br-lan = p2p0
```

具体限制包括：

- domain 固定 `WAN_INTERFACE=wlan0`、`LAN_MEMBER=p2p0`；
- management readiness 只观察 AP member，没有 `eth1`；
- 只有一套 `udhcpc`、DHCP generation、lease ownership 和 resolver record；
- 默认路由 readiness 只识别 `wlan0` metric `600`；
- NAT/FORWARD、Mihomo direct gateway probe、status 和流量统计均固定使用 `wlan0`；
- 没有 `eth0` carrier watcher、Ethernet DHCP client 或 active uplink 状态机。

因此，直接增加第二个 `udhcpc` 或仅添加 metric `100` 路由会破坏现有单 uplink ownership 假设，可能错误覆盖 Wi-Fi 地址、路由、DNS、进程记录或 readiness。

本计划所称的**下游无扰动**固定定义为：

1. 插入或拔出 WAN 网线不重建 `br-lan`，不改变 `192.168.8.1/24`，不清除合法 LAN lease；
2. 不因 WAN 切换重启 dnsmasq、hostapd、`hyz-things` 或本地管理 HTTP；
3. `eth1` 客户端保持物理链路，AP 客户端保持 association；
4. LAN 内通信以及到 `192.168.8.1` 的管理访问持续可用；
5. `eth0` 严格 ready 之前，新连接继续使用已确认的 Wi-Fi；ready 后新连接自动使用有线；
6. `wlan0` 在有线成为主线路后继续保留关联、地址和 metric `600` 备用路由。

第一阶段**不承诺已有跨 WAN 会话迁移**。不同出口通常具有不同源地址和 NAT 映射，在线插线后，既有 TCP、UDP、WebRTC、WebSocket、游戏或下载连接允许中断；本计划只保证 LAN/管理面连续和新连接选路正确。若以后要求旧 flow 留在 Wi-Fi、新 flow 使用 Ethernet，应另立 policy routing/conntrack Story。

## 2. 目标拓扑

```text
                         上游 WAN
              ┌──────────────┴──────────────┐
              │                             │
       eth0 DHCP · metric 100       wlan0 STA · metric 600
              │                             │
              └──────── active uplink ──────┘
                             │
                  IPv4 NAT / firewall
                             │
                  br-lan 192.168.8.1/24
                    ┌────────┴────────┐
                    │                 │
                  eth1              p2p0
                                下游 Wi-Fi AP
```

固定不变量：

- `eth0`、`wlan0` 永不加入 `br-lan`；
- `eth1`、`p2p0` 是固定 LAN members；
- Ethernet DHCP 与未来 PPPoE 在 `eth0` 上互斥；
- active uplink 依据 runtime-owned 地址、有效默认路由和 resolver generation 判断，不仅依据 carrier；
- 基础切换不依赖 ping、HTTP、DNS 或其他公网健康探测。

## 3. 风险分析

### 3.1 单一 DHCP ownership 会互相清理

当前 DHCP callback 只携带 generation，没有 uplink identity；运行时也只有单一 active generation、lease ownership 和 resolver 文件。第二个 DHCP client 若复用该模型，一个 uplink 的 renew/deconfig 可能覆盖或删除另一个 uplink 的地址、默认路由和 DNS。

### 3.2 readiness 只认识 Wi-Fi

当前 `NetworkObserved` 只有 `wan_default_route_present`，planner 的错误和严格检查明确要求 `wlan0` metric `600`。如果直接安装 Ethernet 默认路由，forwarding readiness、后台恢复和状态仍可能把系统判为 Wi-Fi 未就绪或错误 ready。

### 3.3 firewall 与代理固定出口

普通 NAT/FORWARD 和相关 exact-rule probe 固定匹配 `wlan0`。Mihomo 的 direct gateway probe 也从 `wlan0` 读取默认网关。仅依赖 Linux metric 切路由不足以保证 exact firewall ownership、代理 readiness 和 fail-open 仍成立。

### 3.4 LAN 还没有有线入口

当前 bridge attachment 只有 `p2p0`。如果在未拆分 member 生命周期前直接把 `eth1` 塞入同一动作，AP 或 Ethernet LAN 的单点失败可能触发整套 management rollback，反而影响现有下游 Wi-Fi。

### 3.5 DNS 切换可能产生 stale resolver

两条 DHCP uplink 可能下发相同 RFC1918 DNS 地址。resolver 必须按 uplink 与 generation 记录 ownership，不能仅按文本地址判断归属；否则 renew/deconfig 会误删当前 active resolver，或把失效 DNS 保留给 dnsmasq。

## 4. 分阶段实施

### 阶段 A：先完成固定 LAN `eth1 + p2p0`

目标是在不改变 Wi-Fi WAN 的前提下建立两个互相独立的下游入口。

1. 将单一 `LAN_MEMBER` 拆成固定 `ETHERNET_LAN_INTERFACE=eth1` 与 `AP_INTERFACE=p2p0`。
2. `NetworkObserved` 分别记录两个 member 的 presence、master、link/association 和 ownership。
3. planner 增加独立、可补偿的 attach/detach action；单个 member 修复不得重建 bridge。
4. management services 与 LAN 地址继续只绑定 `br-lan`。
5. 验证：
   - `eth1` 插拔不重启 AP、dnsmasq 或门户；
   - AP 失败不移除 `eth1`；
   - LAN 地址和合法 lease 保持；
   - 任一入口可访问管理面，两个入口同时存在时可互访。

### 阶段 B：引入 typed uplink model，先保持 Wi-Fi-only 等价

在 `apps/rust/router/src/domain` 定义受限固定模型，例如：

```text
UplinkId = Ethernet | Wifi
UplinkL3Interface = eth0 | wlan0
UplinkMetric = 100 | 600
UplinkObserved = link/session/address/default-route/resolver/ownership
ActiveUplink = Ethernet | Wifi | None
```

要求：

- active 选择规则是纯 domain 规则；
- Ethernet 有效默认路由优先，否则 Wi-Fi，否则无 active uplink；
- unknown、foreign、stale、partial 不得提升为 ready；
- 第一提交只映射现有 Wi-Fi，现有 action order、rollback、management-only、Mihomo、Tailscale 和 status 行为必须等价。

该阶段必须先重构再加 Ethernet，避免在固定 `wlan0` 常量周围继续堆条件分支。

### 阶段 C：DHCP contract 与 ownership 按 uplink 隔离

1. DHCP event 增加固定 uplink identity；浏览器和 CLI 不能提交任意 interface 字符串。
2. 若 wire 发生破坏性变化，提升 router control 协议版本，并维持服务端接受当前与前一版本、客户端精确匹配的兼容规则。
3. `DhcpPlatformPort` 按 uplink 查询 generation 和提交 event。
4. 每个 uplink 使用独立固定路径保存：
   - DHCP client identity；
   - active generation；
   - lease ownership；
   - address/route ownership；
   - resolver generation。
5. stale callback 只与同 uplink 当前 generation 比较；任何 callback 都不能清理另一 uplink。
6. lease 事务按地址、默认路由、resolver ownership record 提交，失败精确逆序回滚。
7. deconfig 只删除该 generation 的 runtime-owned 状态。

### 阶段 D：实现 `eth0` carrier 与 DHCP lifecycle

1. application-level port 只观察固定 `eth0` carrier，不接受调用方提供路径或接口。
2. daemon 内由有界事件循环或轮询触发 Ethernet reconcile；hotplug 脚本只允许传递类型化事件，不承载业务规则。
3. carrier up：
   - 确认接口身份；
   - 拉起 `eth0`；
   - 启动精确 PID/start/exe/argv 约束的 Ethernet `udhcpc`；
   - DHCP 尚未形成有效默认路由时 active uplink 保持 Wi-Fi。
4. lease ready：安装地址、metric `100` 默认路由和 per-uplink resolver，但只在完整严格复核后提交 active uplink。
5. carrier down：停止/回收 Ethernet generation，只清理 Ethernet 自有资源。
6. DHCP 超时、无 gateway、无 DNS、重复 ACK、stale callback 或快速插拔均不得破坏 Wi-Fi 和 management LAN。

### 阶段 E：双出口 NAT、WAN INPUT 与 active resolver

1. 普通 router firewall 一次性包含固定 `eth0` 与 `wlan0` 出口：
   - `br-lan → eth0/wlan0` 的 NEW/ESTABLISHED/RELATED；
   - 两个 WAN 返回方向只允许 ESTABLISHED/RELATED；
   - 两个 WAN 主动访问 LAN 默认拒绝；
   - 两个 WAN 分别执行 MASQUERADE。
2. 两个 WAN 的 INPUT 默认拒绝，只开放 DHCP/PPPoE 将来需要的精确控制流量。
3. route 切换不得重建整套 firewall chain；exact rules、hook order 和 ownership token 继续可观察。
4. resolver 按 uplink generation 保存；仅 active uplink 的有效 resolver set 原子提交给 dnsmasq。
5. 有线 active 提交门槛至少包括：
   - runtime-owned DHCP address；
   - metric `100` 有效默认路由；
   - 双出口 NAT/FORWARD 已严格确认；
   - 对应 resolver generation 已确认。
6. 提交失败时保留上一条已确认路径；若没有任何安全路径，则降级到 management-only，而不是暴露半完成 forwarding。

### 阶段 F：代理、Tailscale、OTA 与状态兼容

1. Mihomo direct/TUN 不再读取固定 `wlan0` gateway，改为依赖已确认 active uplink。
2. 切换不重启 Mihomo 或 Tailscale；失败时保留普通 NAT 或各自已有的安全降级语义。
3. contract status 按 Ethernet、Wi-Fi 和 active uplink 分别显示 link、session、address、route、DNS 与 ownership。
4. `hyz-things` 第一阶段只增加只读展示，不开放 Ethernet enable/disable、接口名、任意路由或任意网络配置 API。
5. contract 版本变化时同步更新热推送协议矩阵测试，camera/things 推送仍不得重启 router。

## 5. TDD 计划

### 5.1 Domain 与 application host 测试

先写测试，再实现 adapter。至少覆盖：

- Wi-Fi-only 重构前后 exact action order 等价；
- Ethernet carrier 不能单独成为 active uplink；
- Ethernet DHCP ready 前，新 flow 继续使用 Wi-Fi；
- Ethernet 严格 ready 后，新 flow 使用 Ethernet，Wi-Fi 地址和 route 保留；
- Ethernet deconfig/down 只清理 Ethernet，新 flow 回到 Wi-Fi；
- DHCP 失败、无 gateway、无 DNS、重复 ACK 和 stale callback 不影响 Wi-Fi；
- 两个 uplink 下发相同 DNS 地址时 ownership 仍隔离；
- resolver 原子切换失败时恢复上一 generation；
- unknown/foreign/partial Ethernet 不 ready、不清理；
- firewall 一次覆盖两个固定 WAN，route 切换不重建 chain；
- `eth1`/AP 独立 attach、detach、rollback；
- WAN 切换不产生停止 dnsmasq、hostapd、bridge 或门户的 action；
- Mihomo/Tailscale 在切换和故障路径保持既有 fail-open/fail-closed 边界；
- shutdown 只删除 runtime-owned 双 uplink 状态。

测试通过 domain、application ports、`ControlHandler` 和 fake ports 驱动，不直接依赖 `LinuxRouterPlatform` 私有实现，也不在 host 测试执行真实网络命令。

### 5.2 静态检查

按仓库规则先运行静态检查，再运行 Rust host 测试；不在普通 host 测试中修改 `/run`、`/userdata`、真实 route、iptables 或进程状态。除非明确进入板端阶段，不启动 SDK、Buildroot、kernel、rootfs 或 firmware 构建。

### 5.3 板端验收矩阵

| 场景 | 预期 active uplink | 下游连续性 | 关键拒绝条件 |
| --- | --- | --- | --- |
| Wi-Fi-only | `wlan0` | `eth1` 与 AP 均可联网/管理 | 不依赖 `eth0` |
| Ethernet-only | `eth0` | 两个 LAN 入口均可联网/管理 | Wi-Fi 缺失不破坏 LAN |
| 双在线 | `eth0` | Wi-Fi 保持备用 | 不停止 STA |
| 在线插线，DHCP 未完成 | `wlan0` | LAN/管理持续 | 不因 carrier 提前切换 |
| 在线插线，DHCP 完成 | `eth0` | LAN/管理持续，新 flow 转有线 | 不清 lease、不重启服务 |
| carrier up，DHCP 失败 | `wlan0` | LAN/管理持续 | 不提交半完成 Ethernet |
| 在线拔线 | `wlan0` | LAN/管理持续，新 flow 回 Wi-Fi | 只清 Ethernet ownership |
| STA 断线重连 | `eth0` 或无 active | 有线和 LAN 不受影响 | 不清 Ethernet 状态 |
| AP 重启 | 当前 WAN 不变 | `eth1` 持续 | 不重启 WAN |
| `eth1` 插拔 | 当前 WAN 不变 | AP 持续 | 不重启 AP/WAN |

在线插线全过程分别从 `eth1` 和 AP 客户端持续确认：

- 客户端地址、网关和 DNS 地址不变；
- 无 DHCPNAK、强制 renew 或 lease 删除；
- 到 `192.168.8.1` 的管理请求持续；
- LAN 内双向通信持续；
- dnsmasq、hostapd、bridge 和门户 identity/generation 不变；
- Ethernet ready 前持续新建 DNS/HTTPS 请求仍走 Wi-Fi；
- ready 后新建 DNS、TCP 和 UDP flow 走 Ethernet；
- `wlan0` 地址和 metric `600` route 仍存在。

完成至少 20 次快速插拔/切换，检查无重复 DHCP client、地址、route、rule、resolver generation、ownership record 或僵尸进程；再运行至少 8 小时，记录吞吐、丢包、RSSI、CPU、内存、conntrack 和驱动错误。

既有跨 WAN 长连接的中断单独记录，但按第一阶段边界不判失败。LAN 控制面中断、在已有可用 Wi-Fi 时新连接中断、lease 被清除或下游服务重启均判失败。

## 6. 回滚与发布边界

- 每个阶段保持 Wi-Fi-only 可运行，不能把所有变化压进一个不可回退提交。
- 建议提交顺序：
  1. 文档与测试骨架；
  2. `eth1` LAN member 生命周期；
  3. typed uplink 的 Wi-Fi-only 等价重构；
  4. per-uplink DHCP contract/ownership；
  5. `eth0` carrier + DHCP；
  6. 双出口 firewall/NAT + resolver；
  7. status/CLI/Web 只读展示；
  8. 板端验收记录。
- 任一阶段不能严格确认安全状态时，保留 management-only 与上一条已确认 uplink；不得通过临时 shell、`sh -c`、任意接口编辑器或放宽 ownership/readiness 绕过问题。
- 板端失败时回退到上一版已验证 recovery-free OTA，不在设备上保留未审计手工网络修改。

## 7. 完成条件

只有同时满足以下条件，才可在用户故事中勾选 Ethernet DHCP 与双上游任务：

1. `br-lan = eth1 + p2p0` 已完成独立故障隔离；
2. typed uplink 和 per-uplink DHCP/route/resolver ownership 已通过 host 测试；
3. `eth0` carrier/DHCP、metric `100` 与 `wlan0` metric `600` 共存；
4. NAT/FORWARD/WAN INPUT 覆盖两个固定 DHCP WAN；
5. 在线插线期间 LAN 地址、lease、AP、`eth1`、dnsmasq 和管理入口不变；
6. Ethernet ready 前新连接继续走 Wi-Fi，ready 后新连接走 Ethernet；
7. 20 次切换与 8 小时板端矩阵通过；
8. 状态和诊断能区分每条 uplink 与 active resolver set；
9. 既有会话允许中断的产品边界已在验收报告中明确记录。
