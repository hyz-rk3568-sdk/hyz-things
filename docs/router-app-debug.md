# hyz-router 应用级调试部署

## 状态

当前开发部署使用**启动时覆盖**，不再在线停止并重新接管正在运行的 Router、Mihomo、Tailscale 和网络资源。开发版与正式版使用同一份 `hyz-router` ELF 构建配置；差异只存在于板端临时启动配置：

- 正式固件保留 `/usr/bin/hyz-router` 和 `/etc/init.d/S81hyz-router`；
- `router-deploy-dev` 将按 SHA-256 命名的 ELF 放入 `/userdata/hyz-router/dev/`；
- 临时 `/etc/init.d/S80hyz-router-dev` 在 `S81hyz-router` 之前运行，把已校验 ELF 复制到干净的 `/run/hyz-router-dev/`，再 bind mount 到正式路径；
- `router-revert-dev` 只删除临时 `S80` 配置并重启，rootfs 中的正式 ELF 从未被覆盖；
- 开发和回退都在新 boot 中验证运行 ELF、Router、Mihomo、Tailscale、LAN HTTP 和 Tailscale HTTP readiness。

这一区分属于部署配置，不使用 Cargo feature。这样可以保证真实设备上验证的应用逻辑与正式构建一致，避免开发 feature 隐藏生产差异。

该流程已在目标板完成成对验证：部署路径通过主机侧 `adb reboot` 进入新 boot 并激活指定开发 ELF；回退路径删除临时 `S80`、再次进入新 boot，并恢复到预先记录的固件 ELF SHA-256。两次启动均通过严格 Router、Mihomo、Tailscale 和双 HTTP listener 检查。

## 使用方式

`ADB_SERIAL` 在 USB 场景传 USB serial；无 USB 线时用网络 ADB，serial 传
`192.168.8.1:5555` 且 `ADB="adb -P 5038"`（本 WSL 上默认 client 连的是
Windows adb server，路由不到板子），见 [network-adb.md](network-adb.md)。

先完成静态检查，再构建并部署：

```sh
make check-static
make router-deploy-dev \
  ADB=/path/to/adb \
  ADB_SERIAL=USB_SERIAL
```

该目标依赖 `router-app`，会构建内嵌 Web 的 AArch64 `hyz-router`、完成主机与板端 SHA-256 校验、写入临时启动配置并重启。只回退当前开发 ELF 时不重新构建：

```sh
make router-revert-dev \
  ADB=/path/to/adb \
  ADB_SERIAL=USB_SERIAL
```

回退删除 `/etc/init.d/S80hyz-router-dev` 后重启。重启后的 `/usr/bin/hyz-router` 必须与首次部署前记录的固件 SHA-256 一致，否则脚本拒绝报告成功。

## 安全语义

`apps/rust/things/tools/deploy-dev.sh` 保持以下边界：

- 只接受主机提供的固定 ELF 路径和显式 ADB serial，不向应用 HTTP/API 增加命令、路径或认证参数；
- 推送到 root-only 的 `/userdata/hyz-router/dev/`，主机和板端都校验 SHA-256；
- 开发 init 脚本固定使用已校验的版本化 ELF、`/run/hyz-router-dev` 和 `/usr/bin/hyz-router`，不接受板端或浏览器提供的命令、路径与参数；
- 不在线执行 `S81hyz-router stop/start`，不使用 `pkill`、`kill -9`、删除 ownership 文件或强制接管；
- 使用 boot ID 确认设备确实完成了一次新启动，而不是把 reboot 前仍可访问的 ADB 连接误判为成功；
- 启动后核对运行 ELF SHA、顶层 `state: ok`、Mihomo、Tailscale 固定 LAN 模式、本地防火墙和两个 HTTP listener；
- 固件 SHA-256 缺失且已有开发启动覆盖时拒绝猜测底层 ELF；失败状态保留给人工检查，不执行未经验证的在线清理。

旧的在线 stop、卸载、bind mount、start 实现已移除。真实设备曾在该路径上因 foreign/changed route ownership 拒绝 shutdown；部署配置分离不能消除在线资源接管风险，因此不再将旧路径作为可选模式保留。

## init 动作边界

`S81hyz-router` 只支持 `start`、`stop` 和 `status`，不再提供组合式 `restart`。实板再次确认，运行期间的 DHCP-owned route 已被外部或内核状态改变时，保守 `stop` 会拒绝删除无法精确证明归属的网络状态；旧 `restart` 随后不会执行 `start`，会把设备留在管理服务已停止的半状态。

需要重新加载 Router、切换开发 ELF 或安装 OTA 时统一使用主机侧 `adb reboot` 或系统 reboot，并通过 boot ID 和严格 readiness 确认新 boot。`stop` 仍保留给系统关机阶段的有界优雅清理，但不得把一次 `stop` 成功当作在线重新部署前提，也不得在失败后删除 ownership 文件或强制接管网络资源。

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

## startup 身份修复

与 shutdown 侧同源的 `/proc` 竞态也存在于冷启动：`Command::spawn` 返回后，子进程要等 `execve` 完成才呈现固定身份。旧 `identify_spawned_service` 一旦看到进程可见就先取一次 exe/argv 快照，第一次严格匹配失败就立刻以 `Conflict` 报错并杀掉重来——RTL8852BS 冷启动下，这会在 fork/exec 尚未稳定时误判“新进程没有固定身份”，表现为每轮多 launch（板上实测 5 次手动前台运行中 4 次命中该错误）。它并不是 foreign process，而是“身份还没来得及落定”的瞬态。

修复保持 fail-closed 语义，只在窗口内重试瞬态不匹配：

- `identify_spawned_service_with_probe` 是注入式重试核心：`probe` 返回 `NotReady`（进程尚不可见）/`Mismatch`（可见但身份未落定）/`Ready(identity)`；
- 生产路径在每个 `POLL_INTERVAL`（100ms）重新快照并重新匹配，直到 `PROCESS_WAIT`（5s）deadline；
- deadline 前从未匹配：曾观察到 `Mismatch` 则报 `Conflict("… did not have the fixed identity")`，全程 `NotReady` 则报 `ProbeFailed("… disappeared during startup")`——均保持原有错误语义，仍由 `start_service` 杀掉并回滚；
- 单测注入脚本化 probe，不触碰真实 `/proc`，也不依赖墙钟：瞬态 `Mismatch → Mismatch → Ready` 必须返回落定身份；持续 `Mismatch` 必须 fail closed；全程 `NotReady` 必须 fail closed。

这使冷启动的“身份竞态导致额外 relaunch”被消除（板上复验：多次 launch 中不再出现 “did not have the fixed identity”）。在此基础上新增**单射频信道就绪门**：编程 hostapd 前，只有 committed STA 已确认在共享信道上（`wpa_cli` COMPLETED + 频率匹配）且 RTL8852BS 驱动当前支持信道表（`/proc/net/rtl8852bs/{iface}/cur_spt_op_class_ch`，即 hostapd “current mode channel list” 的数据源）包含该信道时才起 AP；无上游时在窗口内降级为记录信道启动（LAN-only 保持可用），窗口耗尽仍无信道则 fail-closed。TDD：用板上实测的“扫描期/稳定期”信道表做解析单测、注入式就绪门语义单测（瞬态重试/fail-closed/无上游降级），并断言就绪门位于信道选择之后、`start_hostapd_on_channel` 之前且对回退信道同样生效。

板上复验（多次独立冷启动）：`Hardware does not support configured channel (161)` 类启动失败已消除，launch 次数从 4-5 稳定到 **3（到就绪 112-152 秒）**。剩余 launch 由射频物理沉降时间主导：驱动信号（当前信道表、STA COMPLETED）在关联后即报“就绪”，但严格 VHT80-ENABLED 往往要再等数十秒（跨 boot 实测 70-210 秒不等，无可读信号提前预测），且存在一轮“已到 dnsmasq+attach 仍退出”的偶发后续失败（原因待抓）。尝试把 `AP_READY_WAIT` 提到 90 秒反而更慢（到就绪 ~255 秒），已回退 30 秒。这不改变“AP 仍从 committed STA 的（历史共享）信道开始、不做 AP-first、不跳过单射频确认窗口”的既定流程。

配套修复：startup 的 device-policy recovery 对生命周期锁（`/run/hyz-network.lock`）的获取改为有界重试（镜像 DHCP worker 的 3 分钟上限、20ms 间隔语义），因为管理启动期间 udhcpc 拿到租约后，DHCP dispatcher 会在 reconcile 释放锁后短暂持有它；recovery 若即时失败会让整个 daemon 无法 boot。该重试保持 fail-closed：窗口内始终被占用仍报 `Busy`。

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

因此已确认的冷启动放大机制是 hostapd 失败后 daemon 全量清理和 init 退避重试；Tailscale、Mihomo 与 DHCP server 不是该长延迟的主因。产品接受总计 300 秒 deadline 内的 clean daemon retry，并以本轮约 92 秒作为当前 RTL8852BS 冷启动基线；不再以“必须 launch attempt 1”作为通过条件。为恢复 2026-08-14 凌晨版本的稳定等待窗口，生产流程使用 45 秒 STA channel 窗口（仅在无有效 last-good 记录时的慢路径）、30 秒 AP readiness、单次 hostapd clean retry以及 S81 封顶指数退避；last-good 快启动（SHA-1 指纹匹配 + 7 天新鲜度）在稳定部署下把该窗口收窄为 15 秒确认窗口（拿到 STA 信道用实际信道、拿不到回退记录信道），且实测证实 fast path 必须保留该短窗口——完全跳过会导致单射频扫描期编程 hostapd 与驱动竞争、冷启动反复 relaunch。WAN route、forwarding、Tailscale 和 Mihomo 仍保留在管理 HTTP 之后，不恢复旧的同步阻塞路径。HT20 只作为诊断基线，VHT80 exact readiness 才是正式 5 GHz 配置；模块重载只用于诊断，不进入普通生产启动路径。
