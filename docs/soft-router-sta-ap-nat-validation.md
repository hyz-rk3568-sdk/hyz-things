# RTL8852BS STA+AP+NAT 验证记录

## 状态

> 本文中的安装路径和命令属于已验证的历史 shell 固件。当前 active source 已切换为单一 `/usr/bin/hyz-router`、最小 `S81hyz-router` 和同 ELF DHCP hook；统一运行时的构建、OTA 与功能板测见 [`router-panel.md`](router-panel.md)，本文只保留旧 STA/AP/NAT parity 数据。

**基础 STA+AP+IPv4 NAT 上网已通过；吞吐受弱上游信号和单射频中继限制，8 小时稳定性尚未执行。**

验证日期：2026-08-09。

> 本文以下实机数据对应当时已安装的 direct-L3 `p2p0` 固件。后续 Wi-Fi-only `br-lan = p2p0` 固件已经编译、安装并通过板端控制面回归，但测试时没有下游客户端，不能用本文旧 direct-L3 客户端数据代替新 bridge 路径验收。迁移结果见 [`soft-router-br-lan-validation.md`](soft-router-br-lan-validation.md)。

本轮不接入或配置 `eth0`、`eth1`，只验证：

- `wlan0`：上游 WPA2 STA，DHCP 默认路由 metric `600`；
- `p2p0`：下游 WPA2 AP，地址 `192.168.8.1/24`；
- dnsmasq：DHCP 地址池 `192.168.8.100-192.168.8.199` 和 LAN DNS 转发；
- `p2p0 → wlan0`：IPv4 forwarding、conntrack、受限 FORWARD chain 和 MASQUERADE。

## 测试固件

- recovery-free OTA SHA-256：`143e507b3da66b3f9b6df7cba61bae8d1680e34a2458d76429ce4d967a27a2af`
- 打包 boot SHA-256：`4ef766adb948492a3502f7695dac6b6393a578914b4e22848c9250947ce4e927`
- 打包 rootfs SHA-256：`fe9310404f37bcf5b28b72b3fbd0e2066168e606b9bac0b84dbc9e77e0376771`
- `8852bs.ko` SHA-256：`3fc65cdfc97b0050f99dff21ccc2914e8f4a03a1f86c0165d4c770599d5241ed`
- 打包并安装的 `hyz-router` SHA-256：`7656892f39ab58bd5052f466e8e88e3f705d9748896b08d2e1db892043f621a0`
- recovery 保持原基线：`9778353401cf31b6bdca54fd1e5dd59a7a0983d9b32488eb89e3f974f036e363`
- 本地构建日志：`output/sta-ap-nat-hardened-build.log`，属于生成物，不提交 Git。
- 本地审计目录：`output/sta-ap-nat-hardened-audit/`，属于生成物，不提交 Git。

OTA 包不包含 recovery 或 userdata。更新后 `/userdata/hyz-router/` 中 STA/AP 配置保留，安装到设备的
`hyz-router` 与打包源文件逐字节一致。

## 固件改动

### 内核

`rockchip_linux_defconfig` 增加基础 IPv4 NAT 能力：

```text
CONFIG_NF_CONNTRACK=y
CONFIG_NETFILTER_XT_MATCH_CONNTRACK=y
CONFIG_IP_NF_FILTER=y
CONFIG_IP_NF_NAT=y
CONFIG_IP_NF_TARGET_MASQUERADE=y
```

生成的内核配置同时解析出 `CONFIG_NF_NAT=y` 和 MASQUERADE target 依赖。

### Buildroot

产品配置增加 legacy `iptables`，保留上一轮的 hostapd、dnsmasq、wpa_supplicant、iw 和 iproute2。
产品 overlay 安装：

```text
/usr/sbin/hyz-router
/etc/init.d/S81hyz-router
/etc/hyz-router/dnsmasq.conf
/usr/share/udhcpc/default.script.d/50-hyz-wlan-metric
```

Buildroot 通用 `S80dnsmasq` 在产品 post-build 阶段移除，避免它抢先绑定端口；dnsmasq 生命周期由
`S81hyz-router` 单独管理。

### 生命周期和防火墙

服务只在 `/userdata/hyz-router/wpa_supplicant.conf` 和
`/userdata/hyz-router/hostapd-sta-ap.conf` 可读时启动。主要行为：

- 校验 PID 对应的进程名和命令行，不使用 `ps | grep`；
- `udhcpc` 常驻运行，支持续租；DHCP hook 持续将 `wlan0` 默认路由设置为 metric `600`；
- 重复 `start` 识别健康实例，不重复启动 hostapd、dnsmasq 或 DHCP 客户端；
- 部分启动失败时回滚本轮进程、AP 地址、防火墙和 `ip_forward`；
- 使用 `HYZ_ROUTER_FWD` 和 `HYZ_ROUTER_NAT` 独立 chain；
- 只允许源地址为 `192.168.8.0/24` 的 `p2p0 → wlan0` 新建/已建立流量；
- 只允许对应的 `wlan0 → p2p0` 已建立/相关返回流量；
- 拒绝其他 WAN 到 AP 和未经授权的 AP 转发；
- MASQUERADE 只匹配 `192.168.8.0/24 → wlan0`；
- stop 时删除自有 chain，并恢复启动前的 `ip_forward` 值。

## 实机结果

### 自动启动和普通上网

recovery-free OTA 后自动启动通过：

```text
sta_state=COMPLETED
sta_route=default via <upstream-gateway> metric 600
ap_state=ENABLED
ip_forward=1
masquerade=enabled
```

STA 和 AP 均位于 2437 MHz / 信道 6。下游 iPhone 关联成功，DHCP 获得 `192.168.8.100`，能够通过
NAT 使用 DNS、HTTPS 和互联网直播。设备本身的上游网关、DNS 和 HTTPS 同时正常。未出现新的
RTL8852BS crash、reset、timeout 或 fatal 日志。

强制向常驻 `udhcpc` 发送续租信号后，进程 PID 保持、STA 地址有效、默认路由仍为 metric `600`，
STA、AP 和 NAT 状态未中断。

### 吞吐和直播

当前测试时 Wi-Fi 天线尚未到位，上游 RSSI 在 `-74` 至 `-78 dBm`，结果只能作为弱信号基线。

| 场景 | 下行/接收 | 网关延迟 | 丢包/接口 drop | 结果 |
| --- | ---: | ---: | ---: | --- |
| 普通直播，30 秒 | `p2p0` 约 `1.45 Mbps` | 平均 `49 ms`，最高 `394 ms` | ping 0%，接口新增 drop 0 | 通过 |
| 1080p 直播，45 秒 | `p2p0` 约 `2.76 Mbps` | 平均 `215 ms`，最高 `1.15 s` | 约 0.67%，接口新增 drop 0 | 可播放，满载延迟偏高 |
| 临时关闭 AP，STA 直连上游下载 10 MB | 约 `3.78 Mbps` | 满载平均 `11 ms`，最高 `78 ms` | 0% | 上游基线 |

1080p 中继吞吐约为当时 STA 直连吞吐的 73%，吞吐损失约 27%。测试期间 CPU 仍有约 72% idle，
没有证据表明 conntrack、iptables 或 RK3568 CPU 是瓶颈。主要限制是同一 RTL8852BS 在同一信道先接收、
再发送每个下行帧，以及弱 RSSI 下的 airtime 排队。满载时延迟明显高于直连上游。

## 已通过与未通过

| 项目 | 状态 |
| --- | --- |
| recovery-free OTA 保留 userdata | 通过 |
| 开机自动恢复 STA+AP+NAT | 通过 |
| STA 默认路由 metric 600 | 通过 |
| AP WPA2 和下游 DHCP | 通过 |
| LAN DNS 转发 | 通过 |
| 下游 HTTPS/互联网 | 通过 |
| 受限 FORWARD/MASQUERADE chain | 通过 |
| 服务幂等启动和 PID 身份校验 | 通过 |
| DHCP 常驻与强制续租 | 通过 |
| 1080p 单客户端直播 | 通过，弱信号下满载延迟偏高 |
| 8 小时 STA+AP+NAT 稳定性 | 未执行 |
| 多客户端压力和吞吐 | 未执行 |
| 上游漫游/换信道后的 AP 自动跟随 | 未实现 |
| STA 不可用时后台持续重试 | 未实现 |
| Wi-Fi-only `br-lan = p2p0` | OTA、板端控制面和生命周期通过，待下游客户端补测 |
| 完整 `br-lan = eth1 + p2p0` 与有线 WAN/LAN | 按用户决定暂缓 |
| regulatory.db 启动时加载 | 仍失败，早于 rootfs 挂载 |

## 后续

1. 天线到位后先把上游 RSSI 改善到至少约 `-60 dBm`，重新做 STA 直连与 STA+AP A/B。
2. 补上游 STA 掉线重试和同信道 AP 恢复状态机。
3. 执行 8 小时 STA+AP+NAT 稳定性和多客户端压力测试。
4. 为 Wi-Fi-only `br-lan = p2p0` 补下游客户端 DHCP/DNS/HTTPS/NAT，再实现仅接管 `br-lan` 入站的透明代理。
5. 未经用户重新开放范围，不配置 `eth0`、`eth1`，也不把 `eth1` 加入桥。

真实上游 SSID、密码、派生 PSK、BSSID、租约地址、订阅 URL、代理节点和控制密钥不得写入本文或版本库。
