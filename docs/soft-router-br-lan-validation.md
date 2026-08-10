# Wi-Fi-only `br-lan` 迁移与验证记录

## 状态

> 本文的安装路径和大部分板测数据对应已完成的旧 shell runtime，保留为 bridge/NAT parity 历史基线。当前 active source 已切换为统一 `/usr/bin/hyz-router`、`/run/hyz-network.lock` 和 Rust 内部 ownership records；统一运行时的构建、OTA 与功能板测见 [`router.md`](router.md)，本文不再代表当前验证状态。

**recovery-free OTA 编译、产物审计、安装、板端控制面及下游关联/DHCP/普通 NAT 路径已通过；客户端应用层 DNS/HTTPS 仍待用户确认。**

记录日期：2026-08-09。

当前范围严格限制为：

```text
wlan0 STA WAN, DHCP, metric 600
        │
 IPv4 forwarding / NAT / firewall
        │
br-lan 192.168.8.1/24
        │
      p2p0 AP
```

`eth0`、`eth1` 均不配置，`wlan0` 永远不加入 `br-lan`。完整目标 `br-lan = eth1 + p2p0` 留待有线范围重新开放后实现。

## 源码改动

### LAN 三层入口

`hyz-router` 将稳定 LAN 接口定义为 `br-lan`：

- 启动时创建 bridge，关闭 STP，并将 forward delay 设为 0；
- `192.168.8.1/24` 只配置在 `br-lan`，不再配置在 `p2p0`；
- 从 userdata AP 配置生成 root-only `/run/hyz-router/hostapd.conf`，移除旧 `bridge=` 后显式追加 `bridge=br-lan`；hostapd/nl80211 可在切换 AP 模式时自动加桥，脚本在 AP 达到 `ENABLED` 后再次验证并在必要时补做 attach；
- dnsmasq 只绑定 `br-lan`，DHCP 地址池暂时保持 `192.168.8.100-192.168.8.199`；
- `HYZ_ROUTER_FWD` 的 LAN 入/出口从 `p2p0` 改为 `br-lan`；
- MASQUERADE 仍只匹配 `192.168.8.0/24 → wlan0`；
- `status` 增加 `lan_if`、`lan_addr` 和 `ap_master`。

服务在 `/run/hyz-router/br-lan.ifindex` 保存 bridge ifindex 所有权标记，清理前必须确认当前同名接口的 ifindex 仍匹配。停止或部分启动失败时，只删除带有匹配标记、类型确认为 bridge 且没有意外成员的 `br-lan`。如果已存在无标记的同名接口，服务拒绝接管；如果 bridge 被替换或桥中出现 `p2p0` 之外的成员，服务拒绝删除。防火墙 chain 也使用独立所有权标记，遇到无标记的同名 chain 时拒绝清理。`start`、`stop` 和 `restart` 由 `/run/hyz-router.lock` 串行化，避免并发回滚另一实例刚创建的资源。

迁移清理会移除旧 direct-L3 拓扑遗留在 `p2p0` 上的 `192.168.8.1/24`，但不会配置或清理 `eth0`、`eth1`。

### Linux 5.10 内核前置项

`rockchip_linux_defconfig` 当前包含：

```text
CONFIG_BRIDGE=y
CONFIG_TUN=y
CONFIG_IP_ADVANCED_ROUTER=y
CONFIG_IP_MULTIPLE_TABLES=y
CONFIG_NETFILTER_XT_TARGET_MARK=y
CONFIG_NETFILTER_XT_MATCH_MARK=y
CONFIG_NETFILTER_XT_MATCH_SOCKET=y
CONFIG_NETFILTER_XT_MATCH_COMMENT=y
CONFIG_NETFILTER_XT_TARGET_TPROXY=y
CONFIG_NETFILTER_XT_TARGET_REDIRECT=y
CONFIG_IP_NF_MANGLE=y
```

这些配置为 bridge 和后续 Mihomo TUN/透明代理提供内核基础；comment match 还用于给自有 iptables chain 写入每次启动唯一的所有权 token，防止 stale marker 授权删除后来创建的同名 chain。本次只补前置能力，不安装透明代理策略路由、TPROXY 规则、DNS 接管或 Mihomo TUN 配置。

## 已完成的静态检查

- `hyz-router` 和 `hyz-mihomo` 的 `sh -n`：通过；
- dnsmasq `interface=br-lan` 以及脚本无 `eth0`/`eth1` 引用：通过；
- LAN 防火墙中无 direct-L3 `p2p0` 入/出口残留：通过；
- 使用 Linux 5.10 现有 Kconfig `conf` 将上述十一个符号解析为 built-in `y`：通过；
- 生命周期锁隔离测试（正常 acquire/release、模拟 PID 复用、并发第二调用拒绝）：通过；
- iptables comment-only 所有权规则经 `iptables-translate` 语法解析：通过；
- 两轮只读生命周期审查及修正后的最终复审：无剩余 high/medium 问题；
- Buildroot `check-package`：84 行，0 warning；
- OTA/板端回归后完整 `make check`：Rust OTA 6 个单测、shell、package、manifest、产品与 recovery Buildroot defconfig 检查全部通过；
- product、Buildroot、kernel `git diff --check`：通过；
- 源码和文档中的 credential-bearing URL 扫描：通过。

最初尝试使用隔离 `O=` 执行 `olddefconfig`，但内核源码树存在历史生成状态，构建系统要求先执行 `mrproper`。为保护现有产物，没有清理源码树，而是直接使用已有 `scripts/kconfig/conf`，将结果写入忽略目录 `output/kernel-brlan-config-check/.config`。

## 固件和板端结果

### recovery-free OTA

2026-08-09 执行 `make upgrade` 成功：

- OTA：`output/upgrade.fw`；
- 大小：`417292874` bytes，约 `397.96 MiB`；
- SHA-256：`a282374a5f4b08c7cc30b49a82f40ae79b802e9363a0784c66235278a20eac42`；
- 打包 boot SHA-256：`d663ffb5a856612d793991974c85211e411736b1c689a3d754e06af4b5e758d0`；
- 打包 rootfs SHA-256：`f71bea07e77ffa6cd7153be552d9b088beaf03547927fdf476ec7184218fd037`；
- 打包 `hyz-router` SHA-256：`a99c1cd4fe63967f03715377f8a8b9b90219b9491597f8ef7a0069bade4cb448`；
- 打包 dnsmasq 配置 SHA-256：`bcf8a28996163c4788d3327f5aa1e84337ad8406519f50773347136db72216d8`。

使用 `rkImageMaker` 和 `afptool` 对最终 OTA 再次解包，包内只有 bootloader、U-Boot、misc、boot、rootfs 和 oem，确认不包含 recovery 或 userdata。打包 rootfs 中的 `hyz-router`、dnsmasq 配置和 `hyz-mihomo` 与审查源码逐字节一致。实际构建使用的 kernel `.config` 中上述十一个符号均为 `y`。

构建 recovery 中间产物时仍出现已知的 stale recovery Buildroot `.config` 差异警告，并临时生成了 recovery 镜像；该 recovery 没有进入 recovery-free OTA，也没有安装。后续若要发布 recovery，仍必须先清理 recovery output 并单独重建、审计。

### OTA 安装和保留性

OTA 在设备端重新计算 SHA-256 后，由 `hyz-ota install` 写入 staging BCB，再重启进入 recovery 更新流程。结果：

- 安装后的 `hyz-router` 和 dnsmasq 配置与解包 rootfs 哈希一致；
- `/userdata/hyz-router/` 下所有文件的 OTA 前后哈希集合一致，未读取或输出文件内容；
- Mihomo 的 root-only `disabled` marker 和配置保持存在，服务状态仍为持久禁用；
- recovery SHA-256 保持 `9778353401cf31b6bdca54fd1e5dd59a7a0983d9b32488eb89e3f974f036e363`；
- 安装完成后已删除临时 `/userdata/upgrade.fw`。

### 板端控制面

| 项目 | 结果 |
| --- | --- |
| `br-lan` 创建与 `192.168.8.1/24` | 通过 |
| `p2p0` master 为 `br-lan`，自身无 IPv4 地址 | 通过 |
| `wlan0`、`eth0`、`eth1` 不属于 `br-lan` | 通过 |
| STA `COMPLETED`，默认路由 metric `600` | 通过 |
| AP `ENABLED` | 通过 |
| `/dev/net/tun` 字符设备 | 通过 |
| mark/socket/comment/MARK/TPROXY/REDIRECT iptables 扩展 | 通过 |
| `ip rule` 策略路由用户态路径 | 通过 |
| firewall UUID token、`br-lan ↔ wlan0` FORWARD 和 MASQUERADE | 通过 |
| 板端普通直连 ping、经 LAN dnsmasq DNS、HTTPS | 通过 |
| 重复 `start` 不改变 PID、bridge ifindex 或 firewall token | 通过 |
| `restart` 后完整恢复 | 通过 |
| `stop` 清理 bridge、地址、chain、runtime config、marker、lock 并恢复 `ip_forward=0` | 通过 |
| stop 后再次 start | 通过 |
| Mihomo 保持禁用，普通 NAT 独立工作 | 通过 |
| 下游客户端关联、DHCP 租约和 LAN 双向可达 | 通过，1 个客户端、1 个活动租约 |
| 下游普通 NAT 转发 | 通过，`br-lan → wlan0` FORWARD 和 MASQUERADE 计数持续增加 |
| 下游应用层 DNS/HTTPS | 待用户确认 |
| 透明代理规则和 Mihomo TUN | 本次已安装固件中未实现；后续源码现已实现但尚未编译或板端验证 |

ADB 会早于完整 SysV 启动流程上线。OTA 后第一次检查发生在 STA 仍扫描、路由服务尚未完成时，短暂看到 `br-lan` 缺失；继续等待服务完成后状态稳定通过。后续自动化必须等待 `hyz-router status` 中 `masquerade=enabled`，不能只以 `adb wait-for-device` 判断路由服务已经就绪。

## 待补验证

1. 由用户在下游客户端确认 DNS 和普通 HTTP/HTTPS 应用访问；关联、DHCP、客户端 ping 和 direct NAT 计数已经通过。
2. 重新执行弱信号吞吐基线，确认迁移 bridge 没有引入额外明显损失。
3. 执行失败注入、8 小时稳定性和多客户端压力测试。
4. PX-04 TUN 的源码设计和待授权板端计划已转入 [`soft-router-tun.md`](soft-router-tun.md)；DNS 接管仍是后续阶段。

## 回退条件

出现以下任一情况时，不继续透明代理集成：

- `p2p0` 加桥后 AP 关联、WPA2 握手或 DHCP 失败；
- hostapd/RTL8852BS 在 bridge 生命周期中崩溃、reset 或丢失接口；
- 服务停止后残留自有 bridge、地址、iptables chain 或错误的 `ip_forward` 状态；
- 普通 NAT 无法在 Mihomo 禁用时独立工作；
- OTA 意外修改 recovery、userdata 或秘密文件权限。

回退应恢复上一版已验证的 direct-L3 `p2p0` 固件，不在设备上临时修改未审计脚本充当发布修复。

本文不得记录真实上游 SSID、密码、派生 PSK、BSSID、租约地址、订阅 URL、代理节点或控制密钥。
