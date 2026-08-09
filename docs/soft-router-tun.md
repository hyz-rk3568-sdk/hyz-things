# Mihomo TUN 透明代理实现

## 状态

> 本文记录的是已经完成板测的旧 shell runtime 和对应命令，保留用于数据面 parity 历史基线。当前源码已由统一 `/usr/bin/hyz-router` 接管 proxy 生命周期，旧 `/usr/sbin/hyz-mihomo` 不再进入 rootfs staging；统一运行时的构建、OTA、功能板测和仍待完成的自动冷启动复验见 [`router-panel.md`](router-panel.md)，本文不再代表当前运行命令。

源码、最终 recovery-free OTA 和板端 TUN 数据面均已完成验证。首轮 OTA 板测发现 watcher 会随启动它的 ADB shell 退出、`status` PID 被辅助函数覆盖；最终源码改为 `nohup setsid` 独立 watcher 并修复变量名，重新打包、安装和重启后通过持久 TUN、崩溃回退与手机 TCP/UDP 实测。

本实现只处理 IPv4，并且只接管来自 `br-lan`（当前成员为 `p2p0`）的下游 TCP/UDP。它不修改 `/userdata/hyz-router/mihomo/config.yaml`，不接管 RK3568 本机发起的流量，也不启用 Mihomo `auto-route`。

## 模式与配置边界

`/usr/sbin/hyz-mihomo` 提供三个持久模式：

```sh
hyz-mihomo explicit  # LAN-only mixed-port，兼容 enable
hyz-mihomo tun       # br-lan 下游透明 TCP/UDP
hyz-mihomo disable   # 停止 Mihomo，保留普通 NAT
hyz-mihomo status
```

模式保存在 root-only `/userdata/hyz-router/mihomo/mode`；`disabled` marker 继续兼容旧固件。无 mode 文件时默认为 `explicit`。模式切换先用仅在当前进程有效的 override 启动并验证新数据面，成功后才原子写入持久 mode；因此普通失败或 HUP/INT/TERM 不会把未通过的 `tun` 留给下次开机。如果原状态为 disabled，失败时 marker 始终保持或恢复且服务停止。

真实 provider、节点、订阅和认证信息仍只存在于 root-only userdata 配置。每次启动都会把源配置复制到 `/run/hyz-mihomo/config.yaml`，删除其中所有顶层 `tun:` 块，再追加唯一的受控块。运行时文件和校验日志模式为 `0600`，源配置不被重写。

TUN 模式追加：

```yaml
tun:
  enable: true
  stack: system
  device: hyz-mihomo
  auto-route: false
  auto-redirect: false
  auto-detect-interface: false
  strict-route: false
  dns-hijack: []
  mtu: 1500
```

Mihomo 自身 `-t` 校验成功后才允许启动核心。

## 数据面

固定资源：

| 项目 | 值 |
| --- | --- |
| TUN 接口 | `hyz-mihomo` |
| mangle chain | `HYZ_MIHOMO_PRE` |
| filter chain | `HYZ_MIHOMO_FWD` |
| fwmark/mask | `0x1000000/0x1000000`（与 `0x01000000` 数值相同，采用 `ip rule show` 的规范输出形式） |
| RPDB priority | `11000` |
| 路由表 | `110` |
| 透明入口 | `PREROUTING -i br-lan -s 192.168.8.0/24` |

启动顺序刻意把入口 jump 作为最后的 commit point：

1. 确认 `br-lan`、`wlan0` 默认路由和普通 router chain 已就绪，并拒绝同名未拥有资源；
2. 启动 Mihomo，等待 `hyz-mihomo` 接口出现；
3. 创建带随机 UUID comment token 的自有 mangle/filter chain；
4. 安装 table `110` 的 TUN default route 和 priority `11000` 的 fwmark rule；
5. 最后安装仅匹配 `br-lan` 源网段的 mangle PREROUTING jump。

只有 TCP 和 UDP 会被打 mark。以下流量先 `RETURN`，继续走现有普通 NAT/本机路径：

- `192.168.8.0/24` LAN 和上游网关；
- IPv4 私网、loopback、link-local、CGNAT、benchmark、文档、组播及保留网段；
- UDP 67–68（DHCP）；
- TCP/UDP 53（第一阶段 DNS 继续由 `br-lan` 上的 dnsmasq 处理并直接出站）。

规则只挂 PREROUTING，不修改 OUTPUT，因此板端进程和 Mihomo 节点连接不进入 TUN，不会形成代理递归。ICMP 也保持普通 NAT/direct。

`HYZ_MIHOMO_FWD` 必须位于 `HYZ_ROUTER_FWD` 前面，否则 router chain 的 LAN drop 会先拒绝 TUN 流量。`hyz-router` 已增加所有权校验和规则位置计算：路由服务重启时，如果发现有效的 Mihomo-owned hook，会把自己的 hook 插在其后；未发现有效 hook 时仍保持普通 NAT chain 在首位。Router 的 FORWARD/POSTROUTING hook 也携带同一个随机 firewall token；升级时只迁移旧固件留下的单个无 comment hook，遇到多个歧义引用会拒绝删除。

两个服务共同修改 FORWARD，因此现在共用 `/run/hyz-network.lock`；规则检查、位置计算、插入和清理不会并发执行。锁使用 PID 和进程启动时间验证。为避免不安全的 stale takeover 竞态，异常 `SIGKILL` 留下的 stale/损坏锁不会自动删除，需要重启（`/run` 自动清空）或由 root 明确检查后清理。

## 失败与清理

- 任一 TUN 准备步骤失败时，入口尚未提交或由失败路径立即删除，然后停止核心，普通 NAT 保持可用。
- 正常 stop 首先删除 mangle 入口，再删除 RPDB rule、table route 和 FORWARD hook，最后删除自有 chain 并停止核心。
- 每组 chain 都用 `/run/hyz-mihomo/tun.owned` 中的随机 token 验证；token 缺失或不匹配时拒绝删除同名资源。
- watcher 通过 `nohup setsid` 脱离调用它的 SysV/ADB shell，每 2 秒检查由 PID 和 `/proc/<pid>/stat` 启动时间共同约束的 Mihomo 实例。进程退出后，非持久 TUN 接口及其 kernel route 自动消失；watcher 会在启动父进程暂时持锁时持续重试，取得共享生命周期锁后清理剩余入口、rule、hook 和 marker，使策略路由空表继续落回 main table/普通 NAT。
- 核心和 watcher 都分别保存 PID 与启动时间，清理前同时验证，避免 stale PID 复用后误杀另一个 Mihomo/脚本实例。
- 生命周期命令收到 HUP/INT/TERM 时先执行幂等清理再释放锁；无法捕获的 `SIGKILL` 仍可能留下需要重启或人工检查的 stale lock。
- `status` 只有在确认没有 interception/owned marker 时才报告 `fallback=ordinary-nat`；数据面不完整但仍有资源时报告 `fallback=not-confirmed`，避免把潜在黑洞误报为已回退。

自动回退当前只覆盖核心进程退出。**Mihomo 进程仍存活但线程卡死、所有代理节点同时不可用或上游链路质量差时，不会自动切换普通 NAT**；不能把这一版描述为完整健康探测回退。

## 第一阶段限制

- 仅 IPv4；
- DNS direct，无 DNS hijack/fake-IP 防泄漏；客户端硬编码公共 TCP/UDP 53 也保持 direct；
- 不处理 UDP fragment/CONNMARK；
- 不自动判断节点全部失效或核心 live-hang；
- 已完成最终 OTA 板端 TCP/UDP、模式切换、核心 kill、router restart、重启持久性和手机应用回归。

## 首轮 OTA 与板端结果

首轮 recovery-free OTA：

| 项目 | 结果 |
| --- | --- |
| `output/upgrade.fw` | `417292874` bytes，SHA-256 `153ef6b88dff70fb2ad43e0a82e8d2640700edfb644b26b9ae82e1debf7172ac` |
| 打包 `boot.img` | SHA-256 `d6e639984e44bdf125861fcc5ec23f8afbf9b9e434da2e8b2910bbf3f5505b97` |
| 打包 `rootfs.img` | SHA-256 `77cb00ab583518b243facb16d385f2e08d8759912cb6b76b292bb20f6a236579` |
| 首轮 `hyz-mihomo` | SHA-256 `2bf4bcbba3820ecf1c191cc4703cdd99750b0dba89ef2c100b97963414be8998` |
| OTA 分区 | bootloader、U-Boot、misc、boot、rootfs、oem；无 recovery/userdata |

安装前后 `/userdata/hyz-router` 的 10 个文件哈希集合完全一致，未读取或输出文件内容。产品 rootfs 仍为 `/dev/mmcblk0p6`，recovery 分区哈希保持 `6b37d53edb22fc4fe55a86ad354b7783754d851bf0d35bc4efcc35ee45facd7f`。

板端验证：

1. `explicit → tun → disabled → explicit → tun` 全部通过；disabled 下普通 start 返回状态 3；
2. TUN 设备为 `hyz-mihomo`，RPDB 为 priority `11000` + mark `0x1000000/0x1000000` + table `110`；
3. PREROUTING 入口只匹配 `-i br-lan -s 192.168.8.0/24`，未安装 OUTPUT 入口；板端 direct HTTPS 经 `wlan0` 成功且 TUN mark 计数保持 0；
4. `hyz-router restart` 前后 Mihomo 核心和 watcher 实例不变，`HYZ_MIHOMO_FWD` 仍位于 `HYZ_ROUTER_FWD` 前；
5. 对 Mihomo 核心执行 `SIGKILL` 后，watcher 清除 TUN、入口、RPDB、route、chain 和 marker；`masquerade=enabled` 保持，status 确认 `fallback=ordinary-nat`，随后按持久 tun mode 重启成功；
6. 一台手机关联并取得一个 DHCP lease，无手工 HTTP 代理。30 秒样本中 TCP mark `3121` 包、UDP mark `111` 包、`br-lan → TUN` TCP `3126` 包、UDP `111` 包、TUN 返回 `7424` 包，真实 `using PROXY` 日志计数增加 4；
7. 用户确认手机网页和视频均正常，TUN 应用层功能验收通过；本样本未观察到传统 TCP/UDP 53 请求（手机可能使用缓存或加密 DNS），因此 DNS direct 只由规则和 dnsmasq 架构确认，不宣称本次有动态端口 53 命中。

首轮板测同时发现 watcher 未脱离调用 shell，shell 退出后 watcher 消失；辅助函数还会覆盖 status 的核心 PID。热替换修复后，watcher 在 ADB shell 退出后保持存活、PID 正确显示，并重新通过 router restart、核心 `SIGKILL` 回退、模式切换和手机流量测试。

## 最终修复 OTA

包含 watcher/PID 修复的最终 recovery-free OTA 已重新编译、解包审计并安装：

| 项目 | SHA-256 |
| --- | --- |
| `output/upgrade.fw`（`417292874` bytes） | `4a05e0bed0aa2f91fa5012dc560bdfdca75c650e0cd83910225ededfe3c19ec6` |
| 打包 `boot.img` | `04f51b35d75ffc8feea3626493afbc2a13f0cc194e6cf96777b35f1b4651dfc0` |
| 打包 `rootfs.img` | `61a8fad5bb2d84f351256b1f053a118cade6e20d2542f4b195f4bcc45f414e4a` |
| 最终 `hyz-mihomo` | `a94d3065acb176dda64bbc70db493529ea22cb0ef182c3d42a6d41243536133c` |
| 最终 `hyz-router` | `2cff49a83673bb13f3a1cdfeaa55f7326693f4bfa0733a389b0b041442b77143` |

最终包的分区仍只有 bootloader、U-Boot、misc、boot、rootfs 和 oem；不含 recovery/userdata。第二轮安装前后 `/userdata/hyz-router` 的 11 个文件（包含测试后持久化的 `mode=tun` 文件）哈希集合一致；未读取或输出其内容。recovery 分区哈希仍为 `6b37d53edb22fc4fe55a86ad354b7783754d851bf0d35bc4efcc35ee45facd7f`。

最终重启后无需人工切换即恢复 `mode=tun`，status 正确显示核心 PID；detached watcher 存活，RPDB/table、TUN 设备和 chain 顺序正确。手机自动重连后已观察到 TCP mark `1059` 包、UDP mark `3455` 包和 TUN 返回 `23039` 包。随后再次对最终安装的 Mihomo 核心执行 `SIGKILL`，普通 NAT 回退和持久 TUN 重启均通过。

完整 `make check` 通过。构建过程中 recovery Buildroot 仍出现已知 stale `.config` 差异并生成中间 recovery；该 recovery 未进入 OTA、未安装，也不得作为发布 recovery 使用。
