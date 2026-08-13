# Wi-Fi-only 基线收口计划

## 状态

**代码阶段已完成；2026-08-13 已完成首轮板端部分验收，完整矩阵仍待继续。**

创建日期：2026-08-12。代码阶段验证日期：2026-08-13。

本文用于收口当前统一 Rust 固件的 Wi-Fi-only 路由基线：

```text
上游 Wi-Fi
    │
wlan0 STA / DHCP / metric 600
    │
IPv4 forwarding / NAT / firewall
    │
br-lan 192.168.8.1/24
    │
p2p0 AP
    │
下游客户端
```

只有在本文列出的代码、host 测试、固件审计和板端验收全部通过并留下受限证据后，才能在 [`soft-router-user-stories.md`](soft-router-user-stories.md) 中勾选对应任务。

## 1. 目标

本次计划完成以下三项：

- 将 DHCP 地址池从 `192.168.8.100-192.168.8.199` 扩展到 `192.168.8.100-192.168.8.249`，保留合法 lease，并验收边界地址；
- 在强制 forwarding/WAN 故障下重新验收 management-only 可访问性；
- 使用最终统一 `/usr/bin/hyz-router` 固件完成客户端 DNS/HTTPS、上游换信道和多客户端持续流量测试。

## 2. 非目标

本次不实现或验收：

- 将 `eth1` 加入 `br-lan`；
- `eth0` carrier 或 Ethernet DHCP；
- typed uplink model、双 WAN 或自动 fallback；
- PPPoE、VLAN、`ppp0`、MTU/MSS；
- 本地 DNS 过滤中心；
- Tailscale；
- SR-09 的完整双上游板端矩阵、20 次切换和 8 小时稳定性；
- 任意 interface、地址池、命令或故障注入的 LAN Web 配置入口。

本次结果不能标记 **DHCP Router Baseline** 或 **Complete Router Baseline**。

## 3. 代码改动计划

### 3.1 扩展固定 DHCP 地址池

修改统一 Rust adapter 中的固定 dnsmasq 配置：

```text
dhcp-range=192.168.8.100,192.168.8.249,255.255.255.0,10m
```

必须保持以下行为不变：

- dnsmasq 只绑定 `br-lan` 和 `192.168.8.1`；
- DHCP 下发的网关和 DNS 均为 `192.168.8.1`；
- lease 文件继续使用 `/run/hyz-router/dnsmasq.leases`；
- 不在 reconcile、restart 或 runtime config prepare 中主动删除 lease 文件；
- 地址池仍是固定产品配置，不接受浏览器或其他未受信任调用者提供任意范围。

同步更新：

- `apps/rust/router/src/adapters/outbound/management.rs`；
- `apps/rust/router/tests/fixtures/dnsmasq.conf`；
- dnsmasq 配置 golden test；
- 地址池起止边界测试，明确 `.100`、`.249` 合法，`.99`、`.250` 不属于动态池。

### 3.2 补强 management-only host 测试

优先通过 `NetworkDesired`、application planner、ports 和 fake platform 扩展 `apps/rust/router/tests/network_lifecycle.rs`，不得直接测试 Linux adapter 私有实现。

至少覆盖：

- 无 WAN 默认路由时，management-only 不依赖 WAN；
- 从 forwarding 降级时先关闭 IPv4 forwarding，再移除 runtime-owned firewall；
- 降级不删除 `br-lan`、LAN 地址、AP attachment、hostapd 或 dnsmasq；
- forwarding 恢复必须依赖重新观察到的、受 ownership 约束的 WAN route；
- 动作执行成功不能直接产生虚假 readiness；
- unknown 或 foreign bridge/firewall 不得被视为 ready，也不得被错误清理。

## 4. Host 验证

按“静态检查优先”执行，不在普通 host 测试中运行真实 Linux 网络命令或修改 `/run`、`/userdata`、接口、路由和防火墙。

计划执行：

1. `git diff --check`；
2. 项目已有 Rust formatting、check 和 Clippy 静态目标；
3. router native 单元和集成测试；
4. dnsmasq fixture、network lifecycle 和 HTTP readiness 定向测试；
5. 仓库已有的完整 host check（如适用）。

### Host 结果

| 项目 | 状态 | 证据/备注 |
| --- | --- | --- |
| diff/format 静态检查 | 通过 | 2026-08-13：`git diff --check`、`cargo fmt --check` |
| Clippy/check | 通过 | native all-targets Clippy `-D warnings` |
| dnsmasq golden/boundary 测试 | 通过 | `.100`/`.249` 池内，`.99`/`.250` 池外；固定网关、DNS、lease 路径 |
| network lifecycle 测试 | 通过 | management-only、动作顺序、foreign/unknown 拒绝、reprobe、重复 reconcile |
| HTTP readiness 回归 | 通过 | 完整 native 测试中的 HTTP 集成测试通过 |
| 完整 host check | 部分通过 | native 共 170 个测试全部通过；`make check-static` 的 router/source 检查通过后，因当前工作区缺少 SDK DTS 文件而在 SDK 检查阶段中止 |

## 5. 固件构建与审计

固件、SDK、Buildroot、kernel、rootfs 或 OTA 构建只有在用户明确授权后执行。

构建后必须记录但不得泄漏秘密：

- Git revision；
- `upgrade.fw` 大小和 SHA-256；
- 打包 rootfs、`/usr/bin/hyz-router` 和 `/etc/init.d/S81hyz-router` SHA-256；
- rootfs 内 dnsmasq runtime 配置来源；
- OTA 成员列表，确认 recovery-free OTA 不包含 recovery 和 userdata；
- 构建产物与安装后 ELF/config 的一致性。

### 固件证据

| 项目 | 结果 |
| --- | --- |
| Git revision | `8f8db58897bcc704f1c530ace4e204508b258d35` |
| `upgrade.fw` SHA-256 | `0184426d62ba651f815c2c7b49b0795ef8549731178cb1d3777467b41769bb82`（379,544,138 bytes） |
| boot SHA-256 | `03f499a6ff7d052be218ba283b0339618e027ef12327a4aa40ac92ba2baf32fc` |
| rootfs SHA-256 | `f353f13f885099bcb65fb5c941ece99e449dfac535a5a9fcbf411140151ae37c` |
| oem SHA-256 | `d18a778d157476c46a76528f7ee9b7b2f77fb8b07d6910fca4b4b7ce52780039` |
| `/usr/bin/hyz-router` SHA-256 | `85c9c8902a3793093592bef951b1cb73ba68e2a2bcabd8f3d24b77d356d27ed2` |
| `S81hyz-router` SHA-256 | `a3c4741821adb3db8c3f997dbd5c0c45a2db31be529ae43e08829f56e5c165cd` |
| OTA 成员审计 | 通过；实际 payload 为 bootloader、U-Boot、misc、boot、rootfs、oem，不含 recovery、userdata |
| 安装后一致性 | 通过；板端 ELF 与 rootfs 内 ELF 哈希一致，运行时包含 `.100-.249` DHCP 配置 |

## 6. 板端测试拓扑与前置条件

需要：

- 一台 RK3568 目标设备；
- 可用 ADB，建议同时保留串口恢复路径；
- 一台能够重启和切换 2.4 GHz 信道的可控上游 AP；
- 至少两个可控下游客户端；
- 一个 OTA 前已经存在的 `.100-.199` 合法 DHCP lease；
- 不含真实凭据的采样和记录方式。

不得将以下信息写入本文或 Git：

- 真实 SSID、密码、派生 PSK 或 BSSID；
- 客户端真实 MAC、完整租约地址或 hostname；
- 订阅 URL、代理节点、token 或 controller secret；
- 其他设备凭据和可用于识别家庭网络的信息。

## 7. DHCP 地址池与 lease 保留验收

### 7.1 步骤

1. OTA 前记录 dnsmasq lease 文件的脱敏摘要和至少一个合法旧 lease；
2. 安装新固件并等待 `hyz-router` 完整 readiness，不以 `adb wait-for-device` 作为路由就绪条件；
3. 确认 runtime dnsmasq 配置的地址池是 `.100-.249`；
4. 确认 OTA、daemon restart 和 dnsmasq restart 没有无条件清空合法旧 lease；
5. 验收 `.200` 与 `.249` 可作为合法动态 lease 地址；
6. 确认 `.250` 不会从动态地址池分配；
7. 确认客户端获得网关和 DNS `192.168.8.1`；
8. 确认客户端仍可访问本地管理面。

若自然分配无法稳定到达 `.249`，可在隔离测试环境使用预置合法 lease 或受控测试 MAC 验证边界；不得因此增加产品运行时的任意地址池配置能力。

### 7.2 通过标准

- `.100-.199` 范围内的合法旧 lease 不因地址池扩容被主动清空；
- `.100`、`.200`、`.249` 被配置和解析逻辑视为合法池内地址；
- `.99`、`.250` 不被动态分配；
- DHCP、DNS 和管理面行为无回归。

### 7.3 结果

| 场景 | 结果 | 备注 |
| --- | --- | --- |
| 旧 lease 保留 | 通过 | OTA 前后观察到旧范围 lease；随后在 USB ADB 下以不输出 MAC/hostname 的脱敏身份摘要复测 daemon restart，`.134` 旧范围 lease 与 `.220` 扩展范围 lease 的身份摘要前后完全一致 |
| `.100` 下边界 | 待执行 | host 边界测试已通过，板端尚未取得 `.100` lease |
| `.200` 新扩展范围 | 部分通过 | 板端实际取得 `.220`，证明 `.200-.249` 扩展区间可分配；精确 `.200` 尚未取得 |
| `.249` 上边界 | 待执行 | 需预置合法 lease 或受控测试客户端 |
| `.250` 池外拒绝 | 待执行 | 需受控边界分配测试 |
| 网关/DNS 下发 | 部分通过 | runtime 配置确认网关和 DNS 均为 `192.168.8.1`，尚未从两个客户端侧留证 |

## 8. Management-only 故障验收

至少执行两类相互独立的故障。

### 8.1 WAN 故障

可选择一种或多种可重复方式：

- 关闭上游 AP；
- 对 STA 执行受控 deauthentication；
- 使 DHCP lease/default route 消失；
- 上游恢复后重新取得 metric `600` 默认路由。

### 8.2 Forwarding 提交故障

在受控测试固件或稳定的测试注入 seam 中使 firewall/forwarding 提交失败。不得临时修改已安装生产脚本来伪造结果，也不得使用 `sh -c` 或任意调用者提供的命令。

### 8.3 每个场景的通过标准

故障期间必须确认：

- `br-lan` 和 `192.168.8.1/24` 保持存在；
- `p2p0` AP、LAN DHCP 和 DNS 保持可用；
- 下游客户端仍能访问 `192.168.8.1:8080`；
- HTTP 只在严格确认正常或 management-only 安全状态后提供 readiness；
- `ip_forward=0`；
- 没有部分提交或错误残留的 runtime-owned NAT/FORWARD；
- unknown/foreign 状态没有被当作 ready；
- WAN 恢复后可以重新建立普通 forwarding；
- 重复故障与恢复不累积进程、route、rule、address、ownership record 或 lock。

### 8.4 结果

| 场景 | 管理面 | DHCP/DNS | forwarding 安全状态 | 自动恢复 | 结果 |
| --- | --- | --- | --- | --- | --- |
| typed `router disable` / `enable` | HTTP 200 | hostapd、dnsmasq 各保持单实例 | disable 后 `ip_forward=0` 且 runtime-owned router firewall hook 为 0 | enable 后 metric `600` route、NAT/FORWARD 恢复；随后显式恢复原 TUN 模式 | 通过 |
| WAN route 消失 | 待执行 | 待执行 | 待执行 | 待执行 | 待执行 |
| 上游 AP 关闭/恢复 | 待执行 | 待执行 | 待执行 | 待执行 | 待执行 |
| forwarding 提交失败 | 待执行 | 待执行 | 待执行 | 待执行 | 待执行 |
| 重复故障/恢复 | 待执行 | 待执行 | 待执行 | 待执行 | 待执行 |

### 8.5 首轮板测的 network ADB 误判与 USB ADB 复测

2026-08-13 首轮使用网络 ADB 执行 `/etc/init.d/S81hyz-router restart` 时，命令在 `Stopping hyz-router:` 后失去上游连接。后续确认这是观察通道选择不当：网络 ADB 依赖 `wlan0`，而 daemon stop 会主动清理 STA，因此网络 transport 断开不能证明 stop 卡死。

改用固定 USB ADB serial 后连续执行两次 SysV restart，均得到 `Stopping hyz-router: OK` 和 `Starting hyz-router: OK`，单次耗时约 21 秒且返回码为 0。整机 uptime 连续增长，daemon PID 正常更换；每次恢复后 STA、metric `600` 默认路由、AP、LAN attachment、IPv4 forwarding、普通 NAT、Mihomo TUN、HTTP 和 router/proxy/system readiness 均正常。关键子进程数量未累积，init action lock 无持有者，runtime ownership 无异常残留。

当前 BusyBox 1.36.0 源码明确说明 `start-stop-daemon` 接受但忽略 `-R <param>`，因此脚本中的 `-R TERM/1810/KILL/5` 在本固件上不是 1810 秒等待策略。首轮关于“超长 TERM 等待窗口”的判断已由源码和 USB 板测推翻，不需要据此修改 init 脚本。

本次先在 AP 无客户端时执行两次 restart，空 lease 文件摘要保持一致。随后接入两个客户端并得到两条合法 lease：一条位于旧范围 `.134`，一条位于扩展范围 `.220`。使用只包含客户端身份与租约地址、且不输出原始 MAC/hostname 的不可逆摘要比较，restart 前后摘要完全一致，两条 lease 均被保留。该次 restart 输出 stop/start OK、返回码为 0，恢复后仍有两条 lease；状态采样时已有一个客户端重新关联，另一个 lease 仍合法保留。

## 9. DNS/HTTPS、换信道和多客户端持续流量

### 9.1 基础双客户端测试

普通 NAT 模式下，两个下游客户端分别确认：

- DHCP lease；
- DNS 查询；
- HTTP/HTTPS；
- TCP；
- 轻量 UDP；
- 本地管理页面。

建议负载：

- 客户端 A 持续 HTTPS 下载或视频；
- 客户端 B 持续 DNS、HTTPS 和轻量 UDP；
- 测试期间持续访问或轮询管理页面。

### 9.2 上游断开与恢复

持续流量期间关闭并恢复上游 AP，确认：

- LAN/AP 和本地管理面保持；
- WAN 不可用期间明确降级；
- STA 自动重新关联；
- DHCP 地址和 metric `600` 默认路由恢复；
- NAT/FORWARD 恢复；
- 无需重启板卡或手工修复 runtime 状态。

### 9.3 上游换信道

将可控上游 AP 切换到当前 renderer 支持的另一个 2.4 GHz 信道，确认：

- `wlan0` 能重新关联；
- 单射频 `p2p0` AP 能跟随并恢复 ready；
- 下游客户端可重新使用 DNS/HTTPS；
- DHCP lease 文件和 LAN 地址没有因 WAN 换信道被清空或重建；
- 记录中断和恢复时长。

本次不把 5 GHz 候选纳入通过范围；当前配置边界仍应对不受支持的并发信道 fail closed。

### 9.4 持续时间与指标

本次持续流量测试建议至少运行 60 分钟。8 小时 soak 继续属于后续 SR-09/稳定性任务，不能因本次 60 分钟测试而提前勾选。

记录：

- DNS/HTTPS 成功情况；
- 中断与恢复时长；
- 丢包和吞吐摘要；
- `wlan0` RSSI；
- route、NAT/FORWARD 和 conntrack 摘要；
- CPU、内存；
- RTL8852BS、SDIO、hostapd、wpa_supplicant 和 kernel 错误摘要。

### 9.5 结果

| 场景 | 客户端 A | 客户端 B | 管理面 | 恢复时长 | 结果 |
| --- | --- | --- | --- | --- | --- |
| 普通 NAT DNS/HTTPS/TCP/UDP | 待执行 | 待执行 | 待执行 | 不适用 | 待执行 |
| 上游 AP 关闭/恢复 | 待执行 | 待执行 | 待执行 | 待记录 | 待执行 |
| 2.4 GHz 上游换信道 | 待执行 | 待执行 | 待执行 | 待记录 | 待执行 |
| 至少 60 分钟持续流量 | 待执行 | 待执行 | 待执行 | 不适用 | 待执行 |

## 10. 失败和回退条件

出现以下任一情况，不得勾选对应任务：

- 地址池扩容清空或破坏合法旧 lease；
- `.249` 无法作为合法边界，或 `.250` 被错误动态分配；
- WAN/forwarding 故障导致 `br-lan`、AP、DHCP、DNS 或本地管理面不可恢复；
- 无法确认安全状态时仍暴露 HTTP readiness；
- 故障后残留部分 NAT/FORWARD、重复进程、route、rule、address 或 lock；
- 清理 foreign/unknown 网络资源；
- WAN 向设备本机暴露管理服务；
- 换信道后必须重启板卡或手工修改网络状态才能恢复；
- 多客户端持续流量导致驱动崩溃、SDIO reset、接口消失或不可恢复断连。

必要时回退到上一版已验证的 recovery-free OTA；不得在设备上临时修改未审计脚本作为发布修复。

## 11. 文档收口

全部验收通过后：

1. 将本文状态改为“已完成”，填写验证日期、固件哈希、结果和已知限制；
2. 更新 [`router.md`](router.md) 的当前统一固件验证状态；
3. 更新 [`soft-router-br-lan-validation.md`](soft-router-br-lan-validation.md) 顶部说明，指向本文的最终统一固件结果，同时保留其历史基线性质；
4. 在 [`soft-router-user-stories.md`](soft-router-user-stories.md) 中勾选：

```markdown
- [x] 将 DHCP 地址池从 `.100-.199` 扩展到 `.100-.249`，保留合法 lease 并验收边界地址。
- [x] 在强制 forwarding/WAN 故障下重新验收 management-only 可访问性。
- [x] 完成客户端 DNS/HTTPS、上游换信道和多客户端持续流量测试。
```

以下任务继续保持未完成：

```markdown
- [ ] 完成 SR-09 的 20 次切换、8 小时稳定性和完整板端矩阵。
```

## 12. PR 完成条件

同一个 PR 可包含代码、测试、本文计划和最终板测记录，但分为两个阶段：

1. **代码阶段**：完成 DHCP 池修改、host 测试和计划文档，三个用户故事 checkbox 保持 `[ ]`；
2. **板测阶段**：经明确授权构建和安装固件，执行本文矩阵，补充证据后再勾选三个任务。

如果暂时没有目标设备、两个客户端或可控上游 AP，PR 可以保持 draft 或仅完成代码阶段，但不能将尚未执行的验证写成通过。
