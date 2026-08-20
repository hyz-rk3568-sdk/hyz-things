# RTL8852BS Wi-Fi STA 基线验证

## 状态

**独立 STA 基线已通过，生产集成尚未完成。**

验证日期：2026-08-08。

本验证只覆盖 RTL8852BS 作为单一 Wi-Fi STA 上游，不启用 AP、`br-lan`、NAT、自动 WAN 选择或 `CONFIG_CONCURRENT_MODE`。目的在于先确认驱动、认证、DHCP、路由和持久化配置链路可用。

## 测试固件

- recovery-free OTA SHA-256：`79f682dcad3d64756475f4bdb2be987fcd50fc50d9768b641a433340e18dd4e1`
- boot SHA-256：`ea63b61324572b82d987299bb11bdb78d806a255fdf8e0d9c3fd058df8c71ad7`
- rootfs SHA-256：`fb96dff3dca286560e3e8cf3eeda7f74a3acf588d821504efa9131d438a3890b`
- recovery 保持原基线：`9778353401cf31b6bdca54fd1e5dd59a7a0983d9b32488eb89e3f974f036e363`
- 本地审计目录：`output/sta-validation-audit/`，属于生成物，不提交 Git。

这些哈希用于本次本地审计；重新构建后镜像时间戳和哈希可能变化。

## 固件改动范围

Buildroot 产品配置增加：

```text
BR2_PACKAGE_IPROUTE2=y
BR2_PACKAGE_IW=y
BR2_PACKAGE_WPA_SUPPLICANT=y
BR2_PACKAGE_WPA_SUPPLICANT_NL80211=y
BR2_PACKAGE_WPA_SUPPLICANT_WEXT=y
BR2_PACKAGE_WPA_SUPPLICANT_CLI=y
BR2_PACKAGE_WPA_SUPPLICANT_PASSPHRASE=y
```

明确未启用：

```text
BR2_PACKAGE_HOSTAPD
BR2_PACKAGE_WPA_SUPPLICANT_AP_SUPPORT
```

RTL8852BS 驱动仍保持：

```text
CONFIG_CONCURRENT_MODE=n
CONFIG_MCC_MODE=n
```

例行 OTA 不包含 recovery 和 userdata。打包后的 rootfs 已独立确认包含：

- `/usr/sbin/ip`
- `/usr/sbin/iw`
- `/usr/sbin/wpa_supplicant`
- `/usr/sbin/wpa_cli`
- `/usr/sbin/wpa_passphrase`

## 实机结果

| 项目 | 结果 |
| --- | --- |
| RTL8852BS 模块加载 | 通过，模块 `8852bs` |
| nl80211 识别 | 通过，`iw dev` 显示 managed `wlan0` |
| 支持模式 | 驱动报告 IBSS、managed、AP、P2P client 和 P2P GO |
| 扫描 2.4 GHz/5 GHz | 通过 |
| WPA2-PSK 认证 | 通过 |
| DHCPv4 | 通过 |
| 默认路由 metric 600 | 通过 |
| 网关连通 | 通过 |
| DNS 和 HTTPS | 通过 |
| 强制断开后重新关联 | 通过 |
| 重新获取 DHCP | 通过 |
| 普通重启后 `/userdata` 配置保留 | 通过 |
| 重启后使用持久配置重新连接 | 通过，当前为手动启动 |
| recovery-free OTA BCB 清除 | 通过 |
| recovery 未变化 | 通过 |
| Flutter/Weston 回归 | 未发现，Flutter 运行且 `POST_BUF_EMPTY=0` |

测试网络的真实 SSID、密码、BSSID、租约地址和 DNS 地址不写入版本库。

持久配置位于：

```text
/userdata/hyz-router/wpa_supplicant.conf
```

文件权限为 `0600`，只保存派生 PSK，不保留 `wpa_passphrase` 生成的明文密码注释。

## 当前未实现

- Wi-Fi STA 开机自动启动；
- DHCP 续租的长期守护进程；
- carrier/关联事件触发的地址和路由清理；
- `eth0` metric 100 与 `wlan0` metric 600 的自动优先级管理；
- DNS 随 WAN 切换更新；
- STA+AP 并发；
- 8 小时稳定性和吞吐压力测试。

当前设备上的 STA 连接由诊断命令手动启动，普通重启后不会自动联网。生产集成必须通过产品服务实现，不应依赖手工 ADB 命令。

## 已知警告

### 监管数据库缺失

内核日志包含：

```text
cfg80211: failed to load regulatory.db
```

`iw reg get` 显示 global 和 RTL8852BS self-managed PHY 均为 `country 00: DFS-UNSET`。虽然本次 2.4 GHz STA 连接成功，但在生产 AP、5 GHz 和 DFS 测试前必须补齐并验证监管数据库/驱动国家码处理，确保实际监管域符合部署地区要求。

### nl80211 非致命提示

认证过程中日志出现：

```text
nl80211: kernel reports: Authentication algorithm number required
```

之后仍正常完成 WPA2 关联并保持连接。本提示在当前测试中不是阻塞错误，但应在驱动并发和长期稳定性测试中继续观察。

## 结论和下一步

独立 STA 技术路线成立，可以进入生产服务设计。建议顺序：

1. 补齐 `regulatory.db` 并验证国家码；
2. 实现独立 STA 启动服务、DHCP 生命周期和 metric 600；
3. 验证 `eth0` metric 100 的有线优先切换；
4. 完成长时间 STA 稳定性测试；
5. 再启用 RTL8852BS `CONFIG_CONCURRENT_MODE` 验证 STA+AP。

本文只说明独立 STA 基线通过，不代表 SR-00 STA+AP 并发已经通过。
