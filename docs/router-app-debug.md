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

2026-08-13 的冷启动证据仍表明：

- kernel uptime 约 18 秒时 `wlan0` ready；
- `hyz-router` 在 management readiness 前可能退出并由 init 按 1、2、4、8 秒退避重试；
- 历史最慢样本中，kernel uptime 约 165 秒时 `p2p0` 才加入 `br-lan`；
- management network ready 后，Mihomo 约 1 秒启动，Tailscale 从启动到 `Running` 约 5 秒。

因此长冷启动主要来自 RTL8852BS concurrent-mode 的 `p2p0`/AP 初始化与 daemon 重试，不是 Tailscale。应用级部署在网络接口已经稳定时，实测 stop、切换、start 和严格验证约 38–45 秒，避免了完整 OTA 和冷启动等待。

冷启动优化仍应作为独立工作：让 daemon 在驱动接口暂未出现时保持安全、可观察的有界等待或管理降级，而不是重复退出并重启整个初始化事务。
