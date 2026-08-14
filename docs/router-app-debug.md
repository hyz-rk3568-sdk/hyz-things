# hyz-router 应用级调试部署

## 状态

**已实现并完成目标板验证。** 2026-08-13 在完整 Router + Mihomo + Tailscale `lan_subnet_access` 状态下，USB ADB 应用级部署和显式回退均无需重启开发板。

最终 recovery-free OTA：

- `upgrade.fw` SHA-256：`18c8d8159ede656eff8b093466db61e3a0682535f10fa9410095195427daa614`
- `/usr/bin/hyz-router` SHA-256：`911c2bb0e29e4f78e9eabfe17147221cc652c62fa494f76098fd413430f6074e`
- OTA payload：bootloader、U-Boot、misc、boot、rootfs、oem；不含 recovery、userdata
- 打包 rootfs 中的 ELF 与本次 AArch64 构建 ELF 逐字节一致

最终验收结果：

1. `router-deploy-dev` 推送约 7.1 MB ELF，主机与板端 SHA-256 一致；
2. 正式 `S81hyz-router stop` 完成 Tailscale、Mihomo 和 network runtime 清理；
3. ELF 从 `/userdata/hyz-router/dev/` 复制到 `/run/hyz-router-dev/`，再 bind mount 到 `/usr/bin/hyz-router`；
4. 正式 init 启动成功，Router、Mihomo、Tailscale、LAN HTTP 和 Tailscale HTTP 全部严格就绪；
5. `router-revert-dev` 正常停机、卸载 bind mount、恢复固件 ELF并重新启动，全程未重启开发板；
6. 最终板端无开发 bind mount、无 shutdown 失败日志，Tailscale 仍为已认证的 `lan_subnet_access`。

## 使用方式

先完成静态检查，再构建并部署：

```sh
make check-static
make router-deploy-dev \
  ADB=/path/to/adb \
  ADB_SERIAL=USB_SERIAL
```

该目标依赖 `router-app`，会构建内嵌 Web 的 AArch64 `hyz-router`，然后执行安全部署。只回退当前开发 ELF时不重新构建：

```sh
make router-revert-dev \
  ADB=/path/to/adb \
  ADB_SERIAL=USB_SERIAL
```

开发覆盖位于 `/run`，整机重启也会自动恢复 rootfs 中的正式 ELF。显式回退更适合在本次调试结束时验证正常 shutdown，并让板卡保持已确认的正式固件状态。

## 安全语义

`apps/rust/router/tools/deploy-dev.sh` 保持以下边界：

- 只接受主机提供的固定 ELF 路径和显式 ADB serial，不向应用 HTTP/API 增加命令、路径或认证参数；
- 推送到 root-only 的 `/userdata/hyz-router/dev/`，主机和板端都校验 SHA-256；
- 通过正式 init 脚本 stop/start，不使用 `pkill`、`kill -9`、删除 daemon lock 或强制接管；
- 用板端状态文件取得远端 shell 的真实退出状态，不信任 Windows ADB 对 stdin script 的宿主进程退出码；
- stop 后要求 runtime 进程及关键 ownership 记录全部消失，残留状态会阻止部署；
- 启动后核对运行 ELF SHA、顶层 `state: ok`、Mihomo、Tailscale 固定 LAN 模式、本地防火墙和两个 HTTP listener；
- 部署失败时只尝试已经验证的正常 stop、卸载和正式 init 恢复；unknown、foreign 或 partial ownership 会被保留供调查。

shutdown 失败会写入 root-only、有界的 `/run/hyz-router/shutdown.log`。下一次完整初始化成功后该日志会被清除。

## shutdown 修复

目标板复现确认，失败来自进程退出瞬间的 `/proc` 竞态，而不是需要强制清理的 foreign runtime：

- `tailscaled` 收到 TERM 后可能短暂成为 zombie；PID 和 start-time 仍属于原进程，但 `/proc/PID/exe`、`cmdline` 已不可用于完整 live identity 匹配；
- `hostapd`、`dnsmasq` 等管理进程也可能在退出后暴露空的 `/proc/PID/cmdline`；
- 旧逻辑将这些正常退出状态误报为身份替换或 argv framing 错误，从而按安全策略保留 ownership 并拒绝重启。

当前实现使用 PID、start-time 和 `/proc/PID/stat` 状态区分：

- 同一 PID/start-time 的 `Z`/`X` 状态属于已退出身份，可以继续精确清理已记录节点；
- 活着但 executable/argv 不匹配，或 PID start-time 已变化，仍属于 replaced/foreign，必须拒绝发送信号或清理；
- zombie 的空 cmdline 只表示“不再是 live match”，不再被解释为损坏输入。

相关单元测试覆盖正常 live-to-exited、zombie、replacement、bounded timeout 和空 cmdline；完整 `make check` 已通过。

## 适用边界

应用级部署适合：

- Rust 应用逻辑；
- HTTP/API；
- 内嵌 Yew、CSS、JavaScript/WASM；
- 不改变 rootfs 依赖的用户态修复。

以下变化必须继续使用 recovery-free OTA：

- Buildroot 包、动态库或其他 rootfs 文件；
- init 脚本；
- kernel、设备树、bootloader、分区或 recovery；
- 正式打包、升级保留性和冷启动验收。

应用级部署只用于开发调试；正式验收和交付仍使用经过审计的 recovery-free OTA。

## 当前冷启动结论

2026-08-14 在正式 rootfs 上停止 `hyz-router` 后，以不输出 SSID、PSK 或其他凭据的固定 shell 命令完成了 RTL8852BS 分层验证：

- 默认 channel 6 AP 和管理 LAN 可以在约 6 秒内建立，但 AP 运行时 STA 无法关联到 5 GHz 上游；
- 仅停止 hostapd 或将 `p2p0` down 仍不能恢复扫描，`wpa_supplicant` 持续报告 `CTRL-EVENT-SCAN-FAILED ret=-16`，直接 nl80211 scan 也返回 `Device or resource busy`；
- 该启动还出现 `cfg80211_netdev_notifier_call` 内核警告，说明反复失败的 AP 切换会把 8852BS 留在忙状态；
- 通过固定 `/usr/lib/modules/8852bs.ko` 真正卸载并重载模块后，接口约 2 秒恢复，纯 STA 约 6 秒关联到 5805 MHz；
- 原 channel 161 HT40/VHT80 hostapd profile 在这次未主动执行 `iw reg set <AP country>` 的诊断脚本中立即报 `Hardware does not support configured channel`；
- 同一 STA channel 161 改用 HT20、`ieee80211ac=0` 后，AP 约 6 秒进入 `ENABLED`，STA 保持 `COMPLETED`；
- 在该稳定 STA/AP 组合下，临时 no-address DHCP hook 收到 lease，证明此前 DHCP 无结果是无线驱动忙状态的后果，不是上游 DHCP server 不响应；
- 源码历史提交 `a88cde9` 已实现 VHT80 并在每次启动 hostapd 前主动设置 AP country，因此上述不完整 shell 复现不能推翻既有 VHT80 性能结果，也不能把 HT20 直接提升为生产配置；
- 随后的正式 recovery-free OTA 已确认板端 `/usr/bin/hyz-router` 与打包 ELF 逐字节一致、BCB 在安装后清除；第 4 次 daemon launch 于 kernel uptime 约 92 秒达到管理 HTTP readiness，STA 为 5805 MHz/`COMPLETED`，AP 精确读回 channel 161、`secondary_channel=-1`、802.11ac、VHT width 1 与 center 155，且未出现新的 cfg80211/8852 error；
- 该样本中 `udhcpc` 仍在运行但尚无 metric-600 默认路由，下游 AP station 数为 0，因此它完成的是启动与 VHT80 radio 验收，不是 DHCP、转发或历史 100+ Mbps 吞吐验收。

因此已确认的冷启动放大机制是 hostapd 失败后 daemon 全量清理和 init 退避重试；Tailscale、Mihomo 与 DHCP server 不是该长延迟的主因。产品接受总计 300 秒 deadline 内的 clean daemon retry，并以本轮约 92 秒作为当前 RTL8852BS 冷启动基线；不再以“必须 launch attempt 1”作为通过条件。为恢复 2026-08-14 凌晨版本的稳定等待窗口，生产流程使用 45 秒 STA channel 窗口、30 秒 AP readiness、单次 hostapd clean retry以及 S81 封顶指数退避；WAN route、forwarding、Tailscale 和 Mihomo 仍保留在管理 HTTP 之后，不恢复旧的同步阻塞路径。HT20 只作为诊断基线，VHT80 exact readiness 才是正式 5 GHz 配置；模块重载只用于诊断，不进入普通生产启动路径。
