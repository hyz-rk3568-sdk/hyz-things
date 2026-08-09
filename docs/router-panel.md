# 统一 Rust Router 应用

## 目标与当前状态

路由控制面正在收敛为 `apps/rust/router` **一个 Cargo 包、一个 native composition root、一个最终板端 ELF**：

```text
apps/rust/router -> /usr/bin/hyz-router
```

它采用包内六边形架构，而不是为每个 adapter 建 crate：

```text
src/domain/                 纯状态、值对象与不变量
src/application/            用例与 outbound ports
src/adapters/inbound/       Axum、CLI，后续 root-only local control
src/adapters/outbound/      network、proxy、process、storage、firmware、system
src/web/                    同一包内的 Yew wasm build target
src/main.rs                 唯一 production composition root
```

`router-web` 只在宿主机构建阶段产生 WASM/静态资源并嵌入 `hyz-router`，不会作为第二个板端程序安装。旧的 `apps/router-panel/{shared,server,adapter-linux,frontend}` 多 crate 方案已被否决并从源码删除；状态契约、HTTP 行为和 UI 已迁入本包。独立 MetaCubeXD 静态包也已删除，产品只保留这一套 Web UI。

**源码、rootfs 与 recovery-free OTA 已完成统一 cutover，并已在 RK3568 完成功能验证。** 最新安装的 Web 稳定性 OTA 在冷启动时因旧 S81 固定五次 launch 上限提前停止，显式启动后路由、TUN 和 Web 均恢复正常；源码已改为封顶指数退避，但该 init-only 修正尚未重新构建或安装。因此当前不能把最新源码标记为自动冷启动验收通过。`make apps` 只构建统一 `hyz-router`，`make overlay` 只安装 `/usr/bin/hyz-router` 和产品元数据；Buildroot board overlay 只保留最小 `S81hyz-router` 及 Mihomo 无凭据示例。旧 shell 路由/Mihomo wrapper、S82、独立 DHCP hook、独立 OTA 和 hello demos 已从最终 rootfs 删除。

## Composition root

`src/main.rs` 是唯一装配点，构造：

- `LinuxRouterPlatform`：typed `ip`/legacy `iptables`、管理网络进程身份、网络和 Mihomo 状态；
- `LinuxMihomoFailOpenPlatform`：同一 ELF 的隐藏 watcher 角色，仅负责 PID/start 绑定的 TUN fail-open；
- `FirmwareAdapter`：OTA 下载、RKFW/SHA-256、BCB、`updateEngine`、reboot；
- `ReadStatus`：router/proxy/system 的部分成功聚合；
- Axum server、root-only local control 与嵌入 Yew assets。

候选命令为：

```text
hyz-router daemon
hyz-router status [--json]
hyz-router router enable|disable
hyz-router proxy explicit|tun|disable
hyz-router ota verify|download|install|install-recovery|apply ...
```

`daemon` 是正常控制路径中唯一构造完整生产 adapter 的角色。普通 CLI 和同一 ELF 的 udhcpc hook 都是 `/run/hyz-router/control.sock` 客户端；socket 位于 root-only `0700` 目录，文件模式 `0600`，并用 Linux peer credentials 再次要求 UID 0。Mihomo watcher 是唯一的最小特权例外：它由同一 composition root 装配，只能按已记录的 core PID/start/exe/argv 身份执行 fail-open，不提供公开 CLI、HTTP 或 control operation。

OTA、router enable/disable 和任意配置内容仍不通过 LAN API 暴露。Web 只新增五个固定、类型化的本地操作：LCD 背光、产品代理模式、已验证组内节点选择、固定目标当前路径延迟测试和 provider 全节点延迟刷新；它们在同一 daemon 内复用 application/control handler，不接受命令、路径、URL、timeout、provider 名或原始 Mihomo JSON。

## 六边形依赖规则

1. `domain` 不知道 Axum、Yew、Linux 路径、进程或命令。
2. `application` 只依赖 domain 和自身定义的 ports。
3. HTTP/CLI 只能调用 application use cases，不能直接构造或调用 outbound adapter。
4. outbound adapter 实现 application ports，不依赖 HTTP DTO 或 Yew。
5. 只有 `main.rs` 可以构造生产 adapter。
6. adapter 不接受来自 HTTP 的命令字符串；外部程序只能通过固定 executable 和 typed argv 调用，禁止 `sh -c`。

当前核心能力边界保持粗粒度：`RouterPlatformPort`、`FirmwarePlatformPort`、`SystemProbePort` 和 `ClockPort`，避免重新演变成大量微型 crate/traits。

## Web 状态与受限本地控制

HTTP 固定绑定 `192.168.8.1:8080`，不会回退到 `0.0.0.0`：

- `GET /api/v1/health`
- `GET /api/v1/status`
- `GET /api/v1/panel`
- `POST /api/v1/control/display`
- `POST /api/v1/control/proxy/{mode,selection,delay,delays}`

没有 CORS。未知 `/api/*` 返回 JSON 404，不进入 SPA fallback；除上述五个精确控制路径外，API 非 GET 方法一律拒绝。每个 POST 都要求小尺寸 typed JSON、精确管理 origin、自定义 CSRF header 和 daemon 启动时生成的随机 token；因此普通跨站表单、foreign origin 和未知字段不能触发操作。token 用于同源 CSRF 防护，不是管理员认证：管理 AP/LAN 上能直接读取面板的客户端属于当前信任边界，也能操作背光和代理。若管理 LAN 将来包含不可信客户端，必须先增加独立认证和 HTTPS，不能把 Mihomo controller secret 当作浏览器凭据。

响应继续带 CSP、frame deny、nosniff、referrer、permissions、COOP/CORP 等安全头。Trunk 生成的 inline module bootstrap 会在 deterministic bundle 阶段被严格提取成同源 `/router-bootstrap.js`，因此不需要 nonce 或 `'unsafe-inline'`。Yew 启动需要浏览器编译同源 WASM，所以 `script-src` 精确允许 `'self' 'wasm-unsafe-eval'`；后者只开放 WebAssembly 编译，不开放普通 JavaScript `eval`。

Yew 页面显示系统、WAN、LAN/AP、转发/NAT、Mihomo/TUN、`wlan0` WAN 累计流量、LCD 背光、实际使用的代理组、当前节点、逐项延迟/超时和从节点名称保守推断的国家/地区。Mihomo 内置但在当前 rule 模式不承载流量的 `GLOBAL` 组被过滤；没有数据的状态卡、控制卡和代理区域直接隐藏，不显示“不可用”占位。页面首次进入或浏览器完整刷新时只触发一次受限 provider 全量测速，约两秒的状态轮询不会测速；手动按钮可再次刷新，五秒内重复请求返回缓存成功结果而不是 409。provider history 只合并经过名称、数量和字段白名单校验的 `delay`/`alive`，浏览器不能指定 provider、测试 URL 或 timeout。代理节点、组名和地区属于 LAN-visible operational metadata；API 不返回 server/port、订阅 URL、密码、UUID、controller secret、原始 history 或 Mihomo JSON。写操作期间控件禁用，状态失败时保留最近成功快照。

LCD 的 DTS `default-brightness-level = <0>` 让 U-Boot/Linux 冷启动默认保持零 PWM，但 panel/DSI 仍注册，因此 Web 可以点亮。黑屏操作把 brightness 设为 0 并 powerdown；面板连接的是共享 always-on `vcc5v0_sys`，软件不能让 LCD 连接器 5V 物理归零。

状态 probe 直接调用固定的 `/usr/sbin/wpa_cli`、`/usr/bin/hostapd_cli`、`/usr/sbin/ip`、`/usr/sbin/iptables` 并读取固定 procfs/sysfs，不调用 `/usr/sbin/hyz-router` 或 `/usr/sbin/hyz-mihomo` wrapper。

## 路由与 Mihomo direct adapter

已建立的领域模型把管理平面和转发平面分开：

- management LAN：`br-lan`、`p2p0`、DHCP/DNS；
- WAN：`wlan0`、DHCP、metric `600`；
- forwarding：IPv4 forwarding 和普通 NAT；
- proxy：`explicit`、`tun`、`disabled`。

`router disable` 的模型只移除 forwarding/NAT/TUN，保留管理 LAN 和页面。

Rust candidate 当前覆盖：

- `/run/hyz-network.lock` 的保守目录锁；
- bridge ifindex、iptables exact rule/hook 和 comment-token ownership；
- 全局 `ip_forward` 在首次变更前保存，daemon shutdown 最后恢复启动前值并删除 ownership marker；
- WPA、udhcpc、hostapd、dnsmasq 的固定 argv 与 PID/start/exe/argv 身份；
- DHCP lease generation 的地址、route metric `600` 和 resolver 所有权记录；
- 冷启动两阶段 reconcile：先启动离线可用的管理 LAN/AP/DNS，再仅为 forwarding 执行有界 DHCP route 等待和所有权复核，最后提交 firewall/forwarding；
- `SIGTERM` 先停止 watcher/撤销 TUN，再撤销普通转发并按精确身份停止管理进程，最后删除自有 bridge；
- Mihomo source 约束、受控 `tun:` 替换、persistent data/runtime state 分离；source 不允许 controller/UI/secret 键，runtime 每次生成双 UUID secret 和 `127.0.0.1:9090` controller；
- controller 启动前经过 authenticated `/version` readiness；浏览器只经过固定 typed facade，不能直连 controller；公开组/节点数据经过长度、成员关系、字段白名单和响应上限校验；
- TUN exact chain、policy route/rule、`rp_filter`、interception commit-last；
- 同一 ELF detached watcher、严格 core/watcher 身份和 ordinary-NAT fail-open；
- proxy mode 持久化最后提交；
- strict observed readiness，unknown/foreign 绝不当作 ready。

Host 与板端 parity 已完成：统一包唯一的 `Cargo.lock`、99 项 native 测试、native/WASM 严格 Clippy、Trunk release bundle、连续两次一致的 deterministic tar、嵌入真实前端的 AArch64 ELF、Buildroot rootfs、kernel 和 recovery-free OTA 均已通过。统一运行时的 Web LCD/代理控制、节点切换与恢复、延迟、真实 Mihomo core 崩溃 fail-open、普通 NAT 和 TUN 恢复已完成板测。早期候选固件曾通过 S81 clean relaunch 冷启动；最新 Web 稳定性固件则暴露固定 launch 次数窗口不足，自动冷启动需待退避版 S81 进入后续固件后重新验收。

仍保留以下边界：

1. `updateEngine` 的 recovery-free BCB、staging、重启和最终分区结果已板测；任意 URL 下载路径的端到端网络 deadline 与目标 block-device provenance 仍需单独故障注入；
2. Mihomo 受控键仍采用与已验证 shell 相同的保守文本策略，尚未升级为结构化 YAML 验证；
3. 已看到一个 AP 客户端和非零 TUN/NAT 计数，但最终固件上的手机 TCP/UDP/视频主观验收仍需用户完成；
4. cold boot 后 backlight `brightness=0`、`actual_brightness=0` 已确认，尚无摄像或人工观察记录可独立证明 U-Boot 到 Linux 全程没有瞬时闪光。

## OTA 合并

现有 `hyz-ota` 能力已迁入同包的 domain/application/firmware adapter，最终 CLI 形式为 `hyz-router ota ...`。保留：

- RKFW magic 和 SHA-256；
- normal/recovery OTA 分离；
- 16 KiB BCB offset、1088-byte exact readback；
- pending command 拒绝；
- `updateEngine` 后验 BCB 验证；
- staging 成功后才可 reboot。

固件输入现在通过 `O_NOFOLLOW` 打开并持有 trusted file descriptor，复制到固定的 root-only `/userdata/hyz-router/ota/upgrade.fw` staging 后再验证。SHA-256、RKFW prefix、fsync 和 staging 前复核都基于同一 device/inode/size 身份；下载临时文件保持打开，验证成功后才 atomic rename 并 fsync parent。OTA mutation 使用独立目录锁；迁移切换时必须原子移除旧 `hyz-ota`，两套 updater 不允许并存执行。SHA-256 仍只是完整性检查，不是发布者认证；签名、anti-rollback、A/B/rollback 仍在生产安全边界之外。

## TDD 与构建闸门

已放置的测试层包括：

- status Serde/秘密排除；
- partial degraded aggregation 与并发状态读取合并；
- real random-port Axum/security/fallback；
- deterministic bundle 的 CSP-compatible external bootstrap；
- Yew formatting/state 与单一 WAN 流量口径；
- network/proxy action order、rollback、unknown non-readiness；
- OTA BCB、fake-port call order、verify-before-commit/stage/reboot。

仓库本地固定版本 Trunk 的安装与显式 candidate 构建入口：

```sh
cargo install trunk --locked --version 0.21.14 --root .tools/trunk
make router-app
```

该入口执行 Trunk/Cargo release WASM build、deterministic tar、资源嵌入和 AArch64 native build。由于 Trunk 0.21.14 默认的 Binaryen 版本不能接受当前 rustc 产生的标准 WASM 特性，前端显式禁用可选的 `wasm-opt` 二次处理；Cargo release 的 LTO/strip 保留。最终 tar 连续构建 SHA-256 一致，AArch64 ELF 已确认嵌入真实资源并逐字节进入 rootfs。

## 2026-08-10 Web 稳定性 OTA 与板端结果

| 产物 | SHA-256 |
| --- | --- |
| `output/upgrade.fw`（378,495,562 bytes） | `e1f771422ec08de32c4f23263d62f6905e5d712ec12849a6cac5350f1c6ea655` |
| 打包 `boot.img` | `1142514684745fb933da025cd39aa1d0a0a81d3e62c46d1d774320da0954454f` |
| 打包 `rootfs.img` | `17993e58abe1728a0e69c85b24cc12c76d6971c36f5a4b025522f33fceb02033` |
| `/usr/bin/hyz-router` | `27655c741ad48563cce79784f2fdbabd9225f3f860b4077ff7c1230436982460` |
| `/etc/init.d/S81hyz-router` | `678a058a94b888be368996a99197f569f1aad802b11d399783985c5134219671` |
| deterministic frontend tar | `d8f5dd31c5ea9ad280e9210fe1a328983430b0edd3f9578246942d30396bba94` |

OTA 解包成员只有 bootloader、U-Boot、misc、boot、rootfs 和 oem，不含 recovery/userdata。安装后 ELF 与打包 ELF 哈希一致，稳定 userdata 配置和 recovery 保持不变；最终 rootfs 没有 bind mount，只安装一个产品 ELF，已审计的旧产品路径均不存在。

本轮 Web 修复使用按 bundle 内容生成的版本化 `/router-bootstrap.js?v=<hash>`，bootstrap 和 SPA HTML 返回 `no-store`，缺失的旧 `.js`、`.wasm`、`.css` 等静态资源返回 404 而不是 SPA HTML。板端验证确认版本化 bootstrap、严格 CSP、正确 WASM MIME/magic、provider 全量测速入口、节点选择持久化和 Web API 均正常。真实 provider、节点名称、数量、延迟分布、余额和控制 token 属于本地运行数据，不写入本文或 Git。

最新 OTA 冷启动时，Wi-Fi 完整就绪晚于旧 S81 的固定五次 launch 窗口；脚本在约 143 秒停止，而约 168 秒后的显式 `start` 立即成功。恢复后 router/TUN 状态、Web、版本化 bootstrap、静态资源 404 和选择持久化均持续正常，因此 ELF 与 Web 修复本身有效，但自动冷启动未通过。源码现已删除固定 launch 次数上限，并在 clean exit 后采用 `1、2、4、8、16、30…秒`的封顶指数退避，同时保留 readiness 总边界和 stale ownership 拒绝策略。该 init-only 修正没有构建、没有 OTA、没有板端安装，必须在后续固件中单独完成冷启动验收。

本轮审计资料位于忽略的 `output/audit/router-web-stability-20260810/`，其中可能包含设备运行元数据，仅供本地验证，不提交或发布。
