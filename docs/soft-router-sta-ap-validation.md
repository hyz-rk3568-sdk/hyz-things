# RTL8852BS STA+AP 并发验证

## 状态

**同信道 STA+AP 基本并发验证已通过；长期稳定性和 NAT 尚未验证。**

验证日期：2026-08-08。

本验证不接入 `eth0` 或 `eth1`。RTL8852BS 同时承担：

- `wlan0`：上游 WPA2 STA；
- `p2p0`：下游 WPA2 AP；
- 两个接口共享 2.4 GHz 信道 6（2437 MHz）。

## 测试固件

- recovery-free OTA SHA-256：`dad3b0217f7030994f781293ea4423a735b9abbe43f541c6de717ba8ac39ac93`
- boot SHA-256：`9f9c0c8839e38771b55592bda6ee60a2d8dcea31fe8e747ef5119823c92a2a7e`
- rootfs SHA-256：`4b7d2ab5bb20ab7defec31beacf35befc695cd15b4a6943ec377e974f7437a3b`
- `8852bs.ko` SHA-256：`98856b92d7794c90d990069835b3ee3b7da6119069f0ff84c39050e32e36c0ca`
- recovery 保持原基线：`9778353401cf31b6bdca54fd1e5dd59a7a0983d9b32488eb89e3f974f036e363`
- 本地审计目录：`output/sta-ap-validation-audit/`，属于生成物，不提交 Git。

打包模块与 Buildroot target 中的模块逐字节一致，模块编译命令包含 `-DCONFIG_CONCURRENT_MODE`。例行 OTA 不包含 recovery 和 userdata，更新后 BCB 待处理字段已清除。

## 源码改动范围

RTL8852BS vendor Makefile 只修改一行：

```diff
-CONFIG_CONCURRENT_MODE = n
+CONFIG_CONCURRENT_MODE = y
```

Buildroot 在独立 STA 工具基础上增加：

```text
BR2_PACKAGE_HOSTAPD=y
BR2_PACKAGE_HOSTAPD_DRIVER_NL80211=y
BR2_PACKAGE_DNSMASQ=y
BR2_PACKAGE_DNSMASQ_DHCP=y
BR2_PACKAGE_WIRELESS_REGDB=y
```

本轮未启用 iptables、nftables、bridge 或 NAT 内核配置，因此不验收下游互联网访问。

## 实机拓扑

```text
上游 WPA2 AP
      │
wlan0 managed STA
      │
 RTL8852BS 单射频
      │
p2p0 WPA2 AP
      │
下游测试客户端
```

运行时状态：

```text
wlan0: managed, channel 6, upstream DHCP, default route metric 600
p2p0:  AP,      channel 6, 192.168.8.1/24
```

驱动启用 concurrent mode 后自动创建第二接口，实际名称是 `p2p0`，不是最初预期的 `ap0` 或 `wlan1`。生产服务不得硬编码未经确认的接口名；应由配置或驱动探测确定。

## 验证结果

| 项目 | 结果 |
| --- | --- |
| concurrent 模块加载 | 通过 |
| 自动创建第二接口 | 通过，接口为 `p2p0` |
| `wlan0` managed STA | 通过 |
| `p2p0` 切换为 AP | 通过 |
| STA 与 AP 同信道 | 通过，均为信道 6/2437 MHz |
| hostapd WPA2 AP | 通过，状态 `ENABLED` |
| 下游客户端发现 SSID | 通过 |
| 下游 WPA2 关联 | 通过 |
| dnsmasq DHCP | 通过，客户端取得 `192.168.8.100` |
| 下游邻居可达 | 通过 |
| AP 运行期间 STA 保持关联 | 通过 |
| AP 运行期间 STA metric 600 | 通过 |
| AP 运行期间上游 HTTPS | 通过 |
| 2 分钟并发冒烟测试 | 通过，24 个 5 秒周期均成功 |
| 驱动 crash/reset/timeout/fatal | 未发现 |
| 显示回归 | 未发现，`POST_BUF_EMPTY=0` |
| 8 小时并发稳定性 | 未执行 |
| 下游 NAT 上网 | 未实现、未测试 |

2 分钟测试每个周期同时检查：

1. `wlan0` 的 wpa_supplicant 状态为 `COMPLETED`；
2. `p2p0` 的 hostapd 状态为 `ENABLED`；
3. 至少一个下游客户端保持关联；
4. 上游网关可达。

结束时再次验证上游 HTTPS，并检查驱动日志无 crash、reset、timeout 或 fatal。

真实上游 SSID、上游密码、临时 AP 密码、客户端 MAC/BSSID 等信息不写入版本库。

## 已知限制和发现

### 单信道限制

当前驱动仍保持：

```text
CONFIG_MCC_MODE=n
```

所以只确认 STA 与 AP 共享同一信道。没有验证跨信道或跨频段并发。生产实现中 AP 应跟随 STA 信道；STA 漫游或换信道时，AP 可能需要重启并导致下游短暂断开。

### regulatory.db 加载时序仍未解决

虽然 rootfs 已包含：

```text
/lib/firmware/regulatory.db
/lib/firmware/regulatory.db.p7s
```

cfg80211 在 rootfs 挂载前请求文件，内核仍记录：

```text
Direct firmware load for regulatory.db failed with error -2
cfg80211: failed to load regulatory.db
```

因此仅把文件加入 rootfs 不足以解决监管域问题。生产 AP 前需要选择并验证以下方案之一：

- 将 regulatory database 作为 built-in firmware；
- 调整 cfg80211/驱动为 rootfs 就绪后加载；
- 提供可靠的延迟重新加载机制。

### 驱动客户端信号统计异常

`iw station dump` 对已关联下游客户端报告过 `signal: 0 dBm`，该数值不可信。关联、DHCP 和流量状态正常，但生产诊断不应直接信任当前驱动的 AP station signal 字段。

### 测试进程编排

首次诊断曾因旧 hostapd 实例未完全退出而导致第二实例报 `Could not set channel for kernel driver`。彻底停止旧实例并只启动一个 hostapd 后，AP 正常进入 `COUNTRY_UPDATE -> ENABLED`。生产服务必须：

- 使用 PID 文件或 `pidof` 管理进程；
- 等待旧实例退出后再启动；
- 避免通过会匹配自身命令行的 `ps | grep` 杀进程；
- 等待 hostapd 从 `COUNTRY_UPDATE` 进入 `ENABLED`。

## 当前未包含

- 开机自动启动 STA+AP；
- `br-lan`；
- IPv4 forwarding、conntrack、NAT 和防火墙；
- 下游 DNS 转发和互联网访问；
- 有线 WAN；
- STA 换信道时 AP 自动跟随；
- DHCP 租约持久化；
- 8 小时稳定性和吞吐压力测试。

## 结论和下一步

RTL8852BS 单芯片同信道 STA+AP 路线基本可行，可以继续产品化。建议下一步：

1. 修复 regulatory.db 加载时序；
2. 实现幂等的 STA、hostapd 和 dnsmasq 服务编排；
3. 创建 `br-lan`，先只加入 AP，之后再加入 `eth1`；
4. 补齐内核 conntrack/NAT/MASQUERADE 与防火墙；
5. 验证下游经 STA NAT 上网；
6. 完成 STA 换信道恢复和 8 小时稳定性测试；
7. 最后再加入有线 WAN metric 100 的优先切换。

本结果说明基本并发和客户端接入通过，不等同于整个软路由 Epic 完成。
