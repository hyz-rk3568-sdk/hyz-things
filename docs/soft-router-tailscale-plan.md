# Tailscale 固定 LAN 远程访问实现计划

## 状态

**代码、主机检查、recovery-free OTA 集成、实际 Tailnet 登录、认证冷启动、Web 等价启用、Tailnet 路由批准、子网路径和路由器管理页面远端访问已完成；Grants、远端 LAN SSH 服务访问和长时间稳定性仍待执行。**

2026-08-13 在 RK3568 目标板通过 USB ADB 安装并验证最新 recovery-free OTA：`output/upgrade.fw` SHA-256 为 `32c34b7e056d4103460296d7b9b34fded57a68579ee8f93615ee214af304594e`，大小 `435118666` bytes；包成员仅为 bootloader、U-Boot、misc、boot、rootfs 和 oem，不含 recovery 或 userdata。打包 `/usr/bin/hyz-router` SHA-256 `0d6cb1a0c8c298aa4ea923d9a19c0202820ea0d5bb41bb9ba4bd640535f9a20e` 与构建输入、Buildroot 安装副本和板端完全一致；且不存在独立 `S82tailscaled`。该版 Web 会依据严格 ready 状态显示“远程 LAN 访问已启用”和绿色本机就绪提示，不再展示设备无法可靠判断的 Tailnet 路由批准字段或黄色外部确认提示。

真实 Tailscale `1.102.2` 板测确认：未认证状态使用 `TailscaleIPs: null`、`AdvertiseRoutes: null`，`ControlURL` 在初始状态可能为空；控制面注册还可能短时返回 HTTP 502 或不完整 JSON。登录链接实现先复用安全 `NeedsLogin` 状态中的现有官方 `AuthURL`，仅当 URL 为空时执行固定 typed 登录命令，并对 URL 发布进行最长 60 秒有界等待；非官方 URL 或其他状态立即拒绝。连续两次 typed 登录请求、请求前后的 native status 均返回同一地址摘要，节点密钥生成计数保持 `1 -> 1`，证明获取链接不会刷新 node key 或使当前链接失效；验证输出未记录 URL token。

登录后的 Web“启用”曾因目标板 `iptables -S` 会重排网络/interface 参数、补充 `-m tcp`/`-m udp`，并将 conntrack 状态规范化为 `NEW,RELATED,ESTABLISHED` 和 `RELATED,ESTABLISHED`，导致 exact ownership 将产品自身规则误判为 foreign 并返回 HTTP 409。最终规则生成器和目标输出 golden test 使用完整目标板规范形式；认证冷启动只在 backend 稳定为 `NeedsLogin` 或 `Running` 后继续。最终 OTA 重启后持久认证保留，`desired_mode`/`effective_mode` 均为 `LanSubnetAccess`，backend 为 `Running`，Tailscale IPv4 为 `100.89.103.59`，固定 `192.168.8.0/24` route 已发布，INPUT/FORWARD/NAT 规则和远程 HTTP listener 均严格 ready。再次执行 Web 等价 typed 启用请求返回 `Ok`，不再返回 409。

开发机的真实 Tailnet 路径访问 `http://100.89.103.59:8080` 和 MagicDNS `http://hyz-things.tail54db37.ts.net:8080` 均返回 HTTP 200、完整安全响应头和管理页；TCP 22 不可达，保持无 SSH/Tailscale SSH 的产品边界。Tailnet 管理端批准 `192.168.8.0/24` 后，开发机到 `192.168.8.1` 的路由切换到 Tailscale 路径，访问 `http://192.168.8.1:8080` 返回 HTTP 200，证明子网路由已可用。路由器本机请求自身 Tailscale IPv4 会命中来源防伪边界而超时，不作为远端可达性判据。设备端仍不尝试推断管理后台批准状态，Web 只展示本机可验证状态。

主机 `make check` 已通过 142 个 library tests、6 个 composition/source-boundary tests、3 个 HTTP tests、26 个 network lifecycle tests、12 个 OTA tests、19 个 Tailscale lifecycle tests 和严格 Clippy。登录 URL token 未写入文档、Git 或普通验证输出；临时控制客户端已从板端删除。

本计划优先交付固定 LAN 远程访问：手机或电脑安装 Tailscale 后，通过软路由访问 `192.168.8.0/24` 中未安装 Tailscale 的设备。实现仍先建立 `RouterOnly` 安全基础，再在同一交付周期增加 `LanSubnetAccess`。

计划日期：2026-08-13。

## 目标

首个可用版本直接交付以下闭环：

```text
手机/电脑（安装 Tailscale）
        │ direct / DERP / Peer Relay
        ▼
软路由 tailscale0
        │ 固定 Subnet Router + SNAT
        ▼
192.168.8.0/24 中未安装 Tailscale 的设备
        ├── TCP 22：SSH/tmux
        └── TCP 8088 等：设备自身临时 HTTP 服务
```

确定的产品决策：

- 首版使用官方 Tailscale 控制面，不支持浏览器输入 `login-server`，Headscale 留待后续 typed 扩展。
- 对外首版直接交付 `LanSubnetAccess`；内部仍保留 `RouterOnly` 作为安全基础和降级状态。
- 只允许发布固定 `192.168.8.0/24`，不支持任意 subnet、exit node、Tailscale SSH 或 accept-routes。
- 目标 IP 和端口权限由 Tailnet Grants 管理；产品不增加任意端口或防火墙编辑器。
- 本地 Web 提供完整的启用、一次性登录 URL、认证状态、LAN Access 和注销流程；不接收或保存 reusable auth key。
- Tailscale 与 Mihomo 并列运行。`tailscale0 -> br-lan` 和返回流量必须绕过 Mihomo interception；LAN 普通公网流量仍按 Mihomo 策略处理。

### 模式术语与产品边界

- **RouterOnly**：Tailscale 只到软路由本机，是远程管理的安全基础和故障降级模式；不允许转发到 LAN 或 WAN。
- **LanSubnetAccess**：Tailscale 可以经过软路由访问固定 `192.168.8.0/24`，是本计划最终交付的使用模式；目标设备无需安装 Tailscale，目标和端口授权由 Tailnet Grants 控制。
- **Exit Node**：远程客户端通过软路由访问互联网；本产品第一版明确不提供。

三者不能混用：RouterOnly ready 不代表 LAN subnet 已开放，LanSubnetAccess 也不代表软路由成为 Exit Node。

## 安全与生命周期模型

### 模式

新增纯 domain 类型 `TailscaleMode`：

- `Disabled`：`tailscaled`、`tailscale0`、Tailscale HTTP listener 和所有 runtime-owned Tailscale 规则均不存在；持久 node state 保留。
- `RouterOnly`：已认证节点只能访问路由器固定管理 HTTP/API；禁止向 `br-lan` 和 WAN 转发。
- `LanSubnetAccess`：在 RouterOnly 基础上，只发布 `192.168.8.0/24`，并允许 `tailscale0 -> br-lan`；继续禁止 `tailscale0 -> WAN` 和 `br-lan -> tailscale0` 主动新连接。

持久 desired mode 为 `LanSubnetAccess` 时，如果普通路由 forwarding/NAT 或 Tailscale LAN firewall 未被严格确认，effective mode 只能降级为 `RouterOnly`，不得把部分配置视为 LAN Access ready。下次显式 reconcile 或启动时，在依赖重新 ready 后恢复 LAN Access。

### 固定运行参数

由 outbound adapter 内部生成固定 argv，不接受浏览器或调用方字符串：

- `/usr/bin/tailscaled`
- `--state=/userdata/hyz-router/tailscale/tailscaled.state`
- `--socket=/run/hyz-tailscale/tailscaled.sock`
- `--tun=tailscale0`
- `--port=41641`
- 固定官方控制面默认值

通过固定 `/usr/bin/tailscale --socket=...` 命令设置或验证：

- `accept-dns=false`
- `accept-routes=false`
- `advertise-exit-node=false`
- `exit-node=`
- `ssh=false`
- `netfilter-mode=off`
- `advertise-routes=` 或严格等于 `192.168.8.0/24`
- subnet SNAT 由产品 owned 规则实现，不依赖 Tailscale 自动修改 iptables

选择 `netfilter-mode=off` 是为了保持本项目“单一 iptables 产品状态源、精确 ownership、固定规则体”的约束。产品必须自行实现 Tailscale 所需 INPUT/FORWARD/NAT 规则，同时继续依赖 Tailscale 内部 packet filter 执行 Tailnet Grants。

### 持久与临时状态

```text
/userdata/hyz-router/tailscale/
├── tailscaled.state       # root-only node/auth state
└── mode                   # Disabled/RouterOnly/LanSubnetAccess

/run/hyz-tailscale/
├── tailscaled.sock
├── tailscaled.pid         # PID/start/exe/exact argv identity
├── tailscaled.log         # bounded/private
└── firewall.owner         # runtime ownership token
```

- `/userdata` 目录为 `0700`、文件为 `0600`，使用原子写入并 fsync。
- disable、restart 和普通 shutdown 不删除 node state。
- logout 使用 typed operation 调用固定命令并验证未认证状态；factory reset 才删除产品拥有的 Tailscale 目录。
- node key、auth key、peer secret、完整历史登录 URL 不得进入 Git、argv、普通日志、status、HTTP GET 或诊断包。
- 一次性登录 URL 只允许出现在管理员发起的 mutation response 中；只验证并返回一个有界的官方 HTTPS URL，不做持久化。

## 分阶段实施

### 1. 更新需求契约与测试清单

修改：

- `docs/soft-router-user-stories.md`
- `docs/router.md`
- 本文持续记录部署、Tailnet route approval、Grants 示例和板测矩阵

明确补充：

- LAN 设备无需安装 Tailscale。
- 允许访问 LAN 设备自己的 SSH 和 HTTP 等服务，但授权由 Tailnet Grants 控制。
- 不提供任意端口编辑器。
- Tailscale-to-LAN 流量不得进入 Mihomo TUN。
- 同一交付周期完成 RouterOnly 基础和 LanSubnetAccess，但测试与 commit 仍保持两个安全阶段。
- route approval 和 Grants 是外部前置条件；本地 ready 不等于 tailnet 已批准路由。

### 2. 增加 domain 与 application ports

新增：

- `apps/rust/router/src/domain/tailscale.rs`
- `apps/rust/router/src/application/tailscale.rs`
- `apps/rust/router/tests/tailscale_lifecycle.rs`

修改：

- `apps/rust/router/src/domain/mod.rs`
- `apps/rust/router/src/application/mod.rs`
- `apps/rust/router/src/application/ports.rs`
- `apps/rust/router/src/application/reconcile.rs`

Domain 模型至少包含：

- `TailscaleMode`
- `TailscaleBackendState`：`Stopped`、`NeedsLogin`、`Running`、`Unknown`
- `TailscaleConnectionKind`：`Direct`、`PeerRelay`、`Derp(region code)`、`Unknown`，仅用于状态，不参与 readiness
- `TailscaleDesired`
- `TailscaleObserved`
- `TailscaleAction`
- 固定常量：`tailscale0`、`192.168.8.0/24`、`100.64.0.0/10`、UDP 41641 和 owned chain 名

新增独立 application-level ports：

- `TailscalePlatformPort`：lock、start/stop、fixed CLI operations、firewall actions、login/logout
- `TailscaleProbePort`：process/socket/backend/auth/IP/prefs/route/firewall/listener observation

不要把 Tailscale action 塞入现有 `RouterPlatformPort::apply_network/apply_proxy`，避免继续扩张一个过粗的端口。

先写 planner/use-case 测试，覆盖：

- Disabled → RouterOnly：先启动和认证，再安装最小 INPUT，最后提交 mode。
- RouterOnly → LAN Access：先确认普通 router forwarding ready，再设置固定 route、安装 subnet firewall/NAT，最后提交 mode。
- LAN Access → RouterOnly：先撤销 forwarding/NAT 和 route advertisement，保留节点与远程路由器管理。
- 任意状态 → Disabled：先关闭 LAN path，再移除 listener/rules，最后停止 exact process。
- 未认证时只返回 NeedsLogin 和瞬时 URL，不安装 LAN forwarding。
- router forwarding unknown、foreign 或 not-ready 时只允许 RouterOnly，不允许部分 LAN ready。
- 任何 unknown、foreign、stale process/rule/socket 均拒绝接管或清理。
- action 中途失败执行逆序补偿；补偿不能删除 foreign 状态。
- direct 失败但 DERP 可用不构成 readiness failure。

### 3. 实现固定 Tailscale 进程与状态适配器

新增：

- `apps/rust/router/src/adapters/outbound/tailscale.rs`

修改：

- `apps/rust/router/src/adapters/outbound/mod.rs`
- `apps/rust/router/src/adapters/outbound/paths.rs`
- `apps/rust/router/src/adapters/outbound/process.rs`，只抽取可复用的 exact process identity 支持，不弱化 Mihomo 校验
- `apps/rust/router/src/adapters/outbound/storage.rs`
- `apps/rust/router/src/adapters/outbound/system.rs`

实现要求：

- fixed executable、fixed argv、`env_clear`、固定 PATH/LC_ALL、无 `sh -c`。
- exact PID/start/exe/argv identity；停止前重新验证。
- bounded command output、bounded deadline，非 UTF-8 或超大输出拒绝。
- 对 `tailscale status --json`、IP 和 prefs 建立窄模型解析；选定版本的真实脱敏 fixture 纳入测试。新增字段可忽略，但关键字段缺失、歧义或格式变化必须产生 Unknown，不能猜 ready。
- 登录命令只接受固定官方控制面，提取单个官方 HTTPS 登录 URL；零个、多个、非官方或超长 URL 均失败。
- `tailscaled` 崩溃不得触碰普通 router/Mihomo 状态；其规则在 observed 中变为不 ready，并由下一次 reconcile 保守清理。

首轮实现开始时重新确认并固定当时的 stable ARM64 版本、asset、SHA-256 和 license hash；不要在 Buildroot 源中使用浮动 `stable/latest` URL。

### 4. 增加独立 owned firewall，并统一 FORWARD hook 顺序

新增固定 chains，名称最终在 domain 中统一：

```text
HYZ_TS_INPUT
HYZ_TS_FWD
HYZ_TS_NAT
```

RouterOnly 规则：

- `tailscale0` 到路由器固定管理 HTTP/API 允许。
- `tailscale0 -> br-lan` 拒绝。
- `tailscale0 -> WAN` 拒绝。
- 非 `tailscale0` 接口伪造 `100.64.0.0/10` 源地址拒绝。
- active WAN 上固定 UDP 41641 只作为 direct 优化；没有该能力仍可经 relay ready。

LanSubnetAccess 增量规则：

- 允许 `tailscale0 -> br-lan`、目标严格为 `192.168.8.0/24` 的 NEW/ESTABLISHED/RELATED。
- 允许 `br-lan -> tailscale0` 的 ESTABLISHED/RELATED 回包。
- 拒绝 `br-lan -> tailscale0` 主动 NEW。
- 拒绝 `tailscale0 -> WAN`。
- POSTROUTING 对 `100.64.0.0/10 -> 192.168.8.0/24`、出口 `br-lan` 做产品 owned MASQUERADE，使 LAN 设备无需回程路由。
- 本地规则不按 22/8088 限制；Tailnet Grants 执行用户、目标和端口授权。

必须重构当前 FORWARD hook 顺序检查，形成唯一顺序：

```text
1. Mihomo hook（存在时）
2. Tailscale hook（存在时）
3. ordinary router hook（存在时）
```

现有 ordinary router chain 会丢弃 `br-lan` 发往非 WAN 的流量；若 Tailscale 回包规则排在它后面，LAN → 手机的 established 回包会被提前丢弃。

修改并扩展：

- `apps/rust/router/src/adapters/outbound/network.rs`
- `apps/rust/router/src/adapters/outbound/proxy.rs`
- `apps/rust/router/src/adapters/outbound/system.rs`
- `apps/rust/router/tests/network_lifecycle.rs`

测试组合包括无 hook、router only、proxy+router、tailscale+router、proxy+tailscale+router，以及每种 install/remove/restart 顺序；所有 hook 必须 exact、unique、owned，foreign reference 一律冲突。

Mihomo 共存要求：

- Mihomo mangle 只拦截 `-i br-lan`。
- 返回手机的 `100.64.0.0/10` 由 private/CGNAT RETURN 规则绕过 mark。
- `tailscale0 -> br-lan` 不进入 Mihomo mangle。
- TUN core crash 与 Tailscale crash 互不改变对方 ownership。

### 5. 接入启动、降级、shutdown 与远程 HTTP listener

修改：

- `apps/rust/router/src/main.rs`
- `apps/rust/router/src/application/shutdown.rs`
- 必要时新增 `apps/rust/router/src/application/tailscale_listener.rs`，或者由 composition root 维护 listener lifecycle

启动顺序：

1. 建立现有 management LAN。
2. 建立 ordinary router forwarding/NAT；失败时可进入严格确认的 management-only。
3. 恢复 Mihomo。
4. 执行 Tailscale reconcile；失败只将 Tailscale 标为 degraded，不得把已确认的 LAN/NAT/management 降级。
5. 已认证 RouterOnly/LAN 模式下，为路由器 Tailscale IPv4 启动第二个 exact HTTP listener；继续保留 `192.168.8.1` LAN listener。

HTTP listener 不允许直接改成不受接口约束的 `0.0.0.0`。优先绑定 observed 中严格验证过的单个 Tailscale IPv4；地址变化时停止旧 listener 后再绑定新地址。RouterOnly listener 只暴露现有固定 HTTP/API，仍由管理员 session、same-origin、CSRF 和 typed DTO 保护。

Shutdown 顺序：

1. 撤销 Tailscale LAN forwarding/NAT、route advertisement、remote HTTP listener 和 Tailscale owned hooks。
2. 停止 exact `tailscaled`，保留 node state 和 mode。
3. 执行现有 proxy-first cleanup。
4. 执行 ordinary network cleanup。

在现有 router enable/disable、WAN 恢复和相关 reconcile 成功点重新运行 Tailscale reconcile：desired 为 LAN Access 但 ordinary forwarding 不 ready 时 effective RouterOnly；恢复后才能重装 LAN path。

### 6. 增加 status、Unix control、HTTP API 与 Web UI

修改：

- `apps/rust/router/src/domain/status.rs`
- `apps/rust/router/src/application/status.rs`
- `apps/rust/router/src/adapters/outbound/status.rs`
- `apps/rust/router/src/adapters/inbound/control.rs`，提升 protocol version
- `apps/rust/router/src/adapters/inbound/http/mod.rs`
- `apps/rust/router/src/web/main.rs`
- `apps/rust/router/src/web/ui.rs` 和 CSS，如需要

`StatusSnapshot` 增加 `tailscale: Component<TailscaleStatus>`，只包含：

- desired/effective mode
- backend state
- authenticated
- Tailscale IPv4，可选且不含 key
- route advertised 和 local firewall ready
- route approval state；如果本地版本无法可靠观测，则显示 `unknown/external approval required`
- connection type 和 DERP region code，仅作为观测信息
- typed error category

敏感 API：

- `GET /api/v1/tailscale`：管理员登录后读取配置和状态，不返回 login URL 历史。
- `POST /api/v1/control/tailscale/mode`：typed `{ "mode": "disabled" | "router_only" | "lan_subnet_access" }`。
- `POST /api/v1/control/tailscale/login`：空 body，启动或复用 NeedsLogin 流程；mutation response 可包含当前一次性 URL。
- `POST /api/v1/control/tailscale/logout`：空 body，显式注销并降到 Disabled。

所有 mutation 必须满足：

- 精确同源 Origin
- CSRF
- 已修改默认密码的管理员 session
- 小 body
- `deny_unknown_fields`
- 未知 method/path 继续返回 404/405
- 不接受 URL、auth key、subnet、端口、argv、timeout、tag 或 raw JSON

Web 交互：

1. 选择“启用远程 LAN 访问”。
2. 若 NeedsLogin，显示一次性官方登录链接，新窗口使用 `noopener noreferrer`。
3. 用户完成登录后点击“已完成登录，继续启用”，再次执行幂等 LAN mode reconcile。
4. 显示“等待管理员批准 `192.168.8.0/24` 路由并配置 Grants”的明确步骤。
5. 显示 effective RouterOnly/LAN Access、direct/peer-relay/DERP 和错误类别。
6. 提供 disable 与显式 logout，清楚区分“停用但保留认证”和“注销并移除认证”。

### 7. Buildroot/SDK 集成

当前产品工作区可能没有 materialized `sdk/`。执行此阶段前先按 SDK 开发规则检查 `repo status`、各 owning repository 分支和 pinned manifest；不得直接修改 `sdk-main`。

在 `sdk/buildroot` owning repository 中：

- 新增 `package/tailscale/{Config.in,tailscale.mk,tailscale.hash}`。
- 固定官方 Linux ARM64 tarball 中的 `tailscale` 与 `tailscaled`。
- 固定 version、asset SHA-256、license 和 license hash。
- 安装 `/usr/bin/tailscale`、`/usr/bin/tailscaled`，root `0755`。
- `hyz_things.config` 增加 `BR2_PACKAGE_TAILSCALE=y`。
- 不安装 state、auth key、配置模板或独立 Tailscale Web client。
- 不增加独立 `S82tailscaled`；唯一生命周期 authority 仍是 `/usr/bin/hyz-router` 和 `S81hyz-router`。

检查 `sdk/kernel` owning repository 的源 defconfig 是否已覆盖 TUN、IPv4 forwarding、conntrack、iptables filter/NAT/comment 所需能力；只在实际缺失时增加最小符号。

扩展顶层 `Makefile check-static`：

- Buildroot `check-package`
- exact Tailscale version assertion
- package enable assertion
- required kernel symbol assertion
- 固定 executable/state/socket/port 检查
- 禁止独立 init wrapper、`sh -c`、任意 login-server/auth key/subnet/exit-node Web 输入
- OTA 继续排除 userdata

跨仓库完成后更新 development/release pinned manifest；应用、Buildroot、kernel（如需要）分别保持 repository-local cohesive commit。

## 建议的提交顺序

1. **`docs: define fixed Tailscale LAN access contract`**
   固定模式、授权边界、外部 approval/Grants 和验收矩阵。
2. **`feat(router): add typed Tailscale domain and application lifecycle`**
   纯 domain、ports、planner、fake tests，不含 Linux 命令。
3. **`feat(router): add owned Tailscale process and firewall adapters`**
   exact process、status parser、firewall、hook ordering 和 shutdown。
4. **`feat(router): expose constrained Tailscale control and Web flow`**
   control protocol、HTTP、status、Yew、HTTP 安全与 E2E。
5. **`feat(buildroot): package pinned Tailscale ARM64 binaries`**
   Buildroot package/config/hash 和静态审计。
6. **`test(router): document target LAN access validation`**
   板测记录、OTA state 保留和故障注入结果；只有全部通过才更新 DoD 状态。

如果实际提交跨 repository，保持上述逻辑顺序，但 Buildroot 和 product app commit 分属各 owning repository，最后由 pinned manifest 集成。

## 验证计划

### Host 静态与自动测试

这些测试不得执行真实 Linux 网络命令：

- domain/application fake tests：exact action order、rollback、unknown/foreign rejection、fallback。
- parser fixture tests：selected Tailscale version 的脱敏 JSON/CLI 输出。
- process identity tests：PID reuse、argv 变化、stale socket、duplicate process。
- firewall pure tests：exact chain body、hook 顺序、partial install rollback、foreign rule refusal。
- HTTP tests：精确路由和方法、管理员、Origin、CSRF、body limit、unknown field、secret/login URL exclusion。
- Yew/Playwright：登录 URL 流程、重新确认启用、approval 提示、disable/logout、错误状态和无障碍。
- `cargo fmt`、native tests、strict Clippy、WASM/frontend checks 和 `git diff --check`。
- Buildroot/package/kernel/manifest 静态检查。

遵循仓库规则：先运行静态检查；Rust、frontend、SDK、Buildroot、kernel、rootfs 和 firmware 构建必须在用户明确授权后才启动。

### 板端功能验收

构建和 OTA 需要另行明确授权。验收步骤：

1. 本地 LAN Web 启用 `LanSubnetAccess`，取得一次性登录 URL 并完成认证。
2. 重启后 node state 保留，`accept-dns=false`，路由器 resolver ownership 未改变。
3. Tailnet 管理端批准且只批准 `192.168.8.0/24`。
4. 配置 Grants：仅操作者身份可访问测试设备 `tcp:22` 和 `tcp:8088`；另选一个未授权端口证明拒绝。
5. 手机关闭 Wi-Fi、使用 5G：
   - SSH 到 `192.168.8.x:22`，进入 tmux。
   - 浏览器访问设备启动的 `python3 -m http.server 8088` 图片目录。
   - LAN 设备无需安装 Tailscale 或增加 `100.64.0.0/10` 回程路由。
6. 分别在 Mihomo `disabled`、`explicit`、`tun` 下重复；Tailscale 流量不计入代理 interception，LAN 普通公网代理行为不变。
7. 观察 direct；阻断或破坏 direct UDP 后确认自动回退 DERP/Peer Relay，SSH/tmux 仍可用。
8. 未批准 route、无 Grants、错误目标网段、LAN 主动连接 tailnet、`tailscale0` 访问 WAN 均失败。
9. kill exact `tailscaled`：普通 LAN、DHCP、DNS、NAT、Mihomo、本地管理继续 ready；不把残留或 foreign 状态视为 ready。
10. WAN 断开/恢复、上联切换、router management-only：LAN Access 降为 RouterOnly 或 unavailable，不暴露未确认 forwarding；恢复后显式 reconcile 成功。
11. disable：运行规则、接口、listener 无残留，认证保留；重新启用无需登录。
12. logout：认证移除，重新启用必须生成新登录 URL。
13. SysV restart/shutdown：Tailscale 先清理，随后 proxy/network；无 owned route/rule/socket/PID 残留。
14. recovery-free OTA：包不含 userdata，前后 Tailscale state 内容哈希集合保持一致，升级后仍认证且可访问。
15. 最后进行 8 小时稳定性，记录连接类型变化、断线恢复和无秘密的状态事件。

## 完成标准

只有以下条件同时满足才可将本次能力标记完成：

- 手机或电脑经 Tailscale 访问固定 LAN 中未安装 Tailscale 的设备。
- SSH 22、非标准 HTTP 端口和未授权端口的正反例通过。
- route 严格固定为 `192.168.8.0/24`；无 exit node、任意 subnet、auth key 或 login-server 输入。
- Tailnet Grants 与 route approval 边界已记录并实测。
- RouterOnly 和 LanSubnetAccess 的独立 readiness、rollback 和 cleanup 通过。
- Tailscale 规则与 Mihomo/ordinary router hook 顺序 exact、unique、owned。
- Tailscale 故障不改变 Complete Router Baseline、Mihomo fail-open 或本地 management-only 安全边界。
- DNS ownership 不被 Tailscale 覆盖。
- reboot、disable、logout、shutdown、OTA 和 8 小时稳定性通过。
- Buildroot 输入、应用提交、SDK 组件提交和 pinned manifest 可追溯。

## 明确不在本次范围

- Headscale 或任意自定义 control server。
- 自建 DERP/Peer Relay 管理。
- Exit Node、站点 VPN、动态 subnet/VLAN 发布。
- Tailscale SSH server、Taildrop、Serve/Funnel、Tailscale Web client。
- 产品内 ACL/Grants 编辑器、Tailscale API token 或自动修改 tailnet policy。
- LAN 设备反向主动访问 tailnet。
- IPv6 subnet access。
- 将 Tailscale 底层连接强制通过 Mihomo 香港代理。
