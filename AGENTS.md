# hyz-things Development Guide

## User stories

@./docs/soft-router-user-stories.md

## Project architecture

@./docs/architecture.md

## Process boundaries

The product runs three independent root daemons, each with its own
composition root in its own Cargo package:

- `hyz-router`（`apps/rust/router`）：无头路由控制核心。拥有管理 LAN、WAN
  DHCP、IPv4 转发/NAT、Mihomo 代理、Tailscale 生命周期、OTA 与 shutdown。
  没有 HTTP、Web、管理员认证或摄像头逻辑。只在 root-only
  `/run/hyz-router/control.sock` 上服务版本化 JSON frame，并在严格
  management-only reconcile 后才写入 `/run/hyz-router/ready`。
- `hyz-things`（`apps/rust/things`）：门户与管理进程。拥有 LAN/Tailscale
  HTTP listener、管理员认证、会话/CSRF、嵌入式 Yew SPA、camera 客户端与
  Tailscale listener 管理。等待 router ready 标记后才绑定 HTTP，通过
  `hyz-contract` client 驱动 router，通过 `/run/hyz-camera/control.sock`
  驱动 camera。
- `hyz-camera`（`apps/rust/camera`）：媒体进程。V4L2 + GStreamer + MPP
  编码、WebRTC 会话与固定 UDP 端口池。不执行网络或防火墙命令。

共享 wire 契约位于 `apps/rust/contract`（`hyz-contract`）：纯 serde 值对象
与版本化 frame，不依赖 Axum/Yew/Linux。协议版本只在破坏性变更时提升；
服务端接受当前与前一版本（`[current, current - 1]`），客户端要求精确匹配。
所有控制 socket 都位于 root-only 私有目录并复核 peer UID 0。

### Implementation guardrails

- `apps/rust/contract/src`：只定义 serde wire 值与校验/脱敏不变量，不依赖
  Axum、Yew、Linux 路径、进程或具体 adapter；`client` feature 是唯一可选的
  Unix socket 传输。
- `apps/rust/router`：
  - `src/domain` 拥有纯 desired/observed 状态、动作、值对象与不变量。
  - `src/application` 拥有 router、proxy、DHCP、panel、status、OTA、
    fail-open 与 shutdown 用例，只依赖 domain 与 application-level ports。
  - `src/adapters/inbound` 是 driving-adapter 层。CLI、Unix control、
    udhcpc hook 与 OTA CLI 必须跨类型化 inbound 边界调用 application
    用例，不得内嵌业务规则或具体 Linux 执行细节。router 不提供 HTTP。
  - `src/adapters/outbound` 实现 application ports，不依赖 HTTP DTO、
    Yew 组件或未校验的调用方命令。
  - `src/main.rs` 是唯一 production composition root。构造
    `LinuxRouterPlatform`、`LinuxMihomoFailOpenPlatform`、`FirmwareAdapter`
    等具体生产 adapter 只允许出现在这里。
- `apps/rust/things`：
  - `src/application` 拥有 admin、camera client、status 聚合与
    `PortalControlHandler` 端口；`src/adapters/inbound/http` 通过该端口
    调用用例，不直接构造 outbound adapter。
  - `src/adapters/outbound` 实现 `PortalControlHandler`（router UDS
    client）、camera UDS client、admin 凭据存储、Tailscale exact
    listener 与热推送 registry 读取（固定路径
    `/userdata/hyz-things/apps/registry.json`，经 `InstalledAppsPort`
    暴露给 `/api/v1/apps`），不依赖 HTTP DTO 或 Yew。
  - `src/web` 是浏览器侧 driving adapter，只通过受限 HTTP API 通信，
    不得获得对 native inbound/outbound adapter 的 Rust 依赖（`web`
    feature 只编译 `domain` 与 SPA）。
  - `src/main.rs` 是唯一 production composition root。等待
    `/run/hyz-router/ready` 后才绑定 HTTP。
- `apps/rust/camera`：`src/main.rs` 是唯一 production composition root；
  media pipeline 与 WebRTC 由固定 adapter 驱动，不执行 `iptables`、`ip`
  或 shell 命令。

- 保持外部执行固定且类型化：使用固定可执行路径与 typed argv；绝不引入
  `sh -c`，绝不把浏览器提供的命令、路径、URL、timeout、provider 名或
  原始 Mihomo JSON 交给平台执行。
- 保持保守的 ownership 与 readiness 语义：unknown、foreign、stale 或
  部分观测的资源不得视为 owned 或 ready；清理只删除 runtime-owned 状态。
- 保持管理面安全边界：router 或 proxy 失败可降级到严格确认的
  management-only 状态，但在网络安全性未确认前不得暴露 HTTP readiness。
- 不通过 LAN API 暴露 router enable/disable、OTA、任意配置或 Mihomo
  controller。现有 Web mutations 必须保持 fixed、typed、same-origin、
  大小受限且 CSRF 保护。
- 摄像头观看对匿名开放：`camera/session/{create,close}` 接受管理员
  session 或 15 分钟短时 viewer 令牌（内存驻留、数量有界、仅同源领取，
  令牌只能创建/关闭自己名下的会话）；`camera/status` 匿名只读，
  `camera/{profile,rotation}` 保持管理员专属。
- 热推送（`apps/rust/things/tools/deploy-app.sh`）：推送 camera/things
  只停止并重启对应应用的 init 服务，绝不重启 router；任何停止动作之前
  必须通过注册表记录的协议版本做兼容性检查。

## GitHub workflow

- 较大的功能开发或重构必须在独立分支完成，并通过 GitHub Pull Request 合入默认分支；不得直接提交到 `main`。
- 小型文档、测试维护、CI 修复或明确的局部 bugfix 可以直接提交；一旦范围扩大到跨模块、架构调整或大范围行为变化，必须切换为 PR。

### CI batching and push discipline

- 不要为了每个小修复、格式调整、测试定位器修改或单个 Red/Green 循环分别 push 并触发 GitHub Actions。
- TDD 的“一个行为一个 Red-Green-Refactor 循环”约束的是实现与验证粒度，不等于“一次循环一个 commit / 一次 push / 一轮 CI”。
- 在同一任务和同一分支内，多个彼此独立、不会互相掩盖失败原因的修改应先在工作树中累计成一个有意义的 checkpoint，再统一 commit/push。
- 优先按功能批次触发 CI。例如一个功能可以同时包含 contract / DTO、application / adapter 实现、Web/API、unit / E2E，以及同一轮已经明确发现的相关回归修复。
- 对纯格式、测试 locator、文案、计数基线等机械性修复，除非它本身阻塞后续分析，否则不要单独 push；与下一批相关代码一起提交。
- 一轮 CI 失败后，先完整检查该轮所有失败 job 和日志，尽量一次收集并修复所有互不冲突的问题，再触发下一轮 CI；不要只修第一个错误就立即 push。
- 仅在以下情况优先单独触发 CI：需要 CI 环境才能确认根因；修改涉及高风险共享契约、workflow 或安全边界，适合先建立独立检查点；当前批次过大，继续累积会降低可审查性或让失败归因变得困难；用户明确要求立即验证某个提交。
- 合入 PR 前必须在最终候选 HEAD 上完整通过所有相关 CI；中间减少 CI 触发次数不得降低最终验证覆盖率。

## TDD

测试即文档。

### Red-Green-Refactor（强制流程）

所有行为或代码变更必须按以下顺序循环，不能只做到测试变绿：

1. **Red**：先写一个只描述单一外部行为的失败测试；运行最小范围测试，确认失败原因正是缺少该行为。
2. **Green**：用最小实现让测试通过，不夹带无关功能或重构。
3. **Refactor**：测试变绿后必须整理代码和测试，消除重复、收敛抽象、修正依赖方向或改善命名；每次整理后重新运行测试。没有完成 Refactor，就不能视为任务完成。

一次只推进一个行为；完成 Refactor 后再补边界、错误、回滚和并发场景，并继续从 Red 开始下一轮。这里的“一次只推进一个行为”指开发与测试设计粒度；多个已完成的 Red-Green-Refactor 循环可以在不影响失败归因和可审查性的前提下，批量组成一次 commit/push 和一次远端 CI 验证。

### Test guardrails

- 测试通过 domain 函数、application 用例、ports、`ControlHandler`（router
  side）或 `PortalControlHandler`（things side）、HTTP/control seams 驱动
  行为，而不是直接依赖具体 `apps/rust/*/src/adapters/outbound` 实现。
- 当测试需要自定义平台行为时，先增加或扩展稳定的 application-level port
  或 server-facing 注入 seam，不要把测试耦合到
  `LinuxRouterPlatform` 等具体实现的内部。
- 用 fake ports 记录 desired/observed reconcile、精确 action 顺序、回滚、
  ownership、fail-open、shutdown、OTA verify-before-commit 与
  unknown-state 拒绝。
- HTTP 测试必须继续记录安全边界：精确路由与方法、typed JSON、body 上限、
  origin/CSRF 检查、秘密排除、安全头与 SPA/API fallback 行为。
- 部署工具测试（`apps/rust/things/tools/test-deploy-app.sh`）用 fake adb
  断言：camera/things 推送绝不调用 router init 脚本、协议不匹配时在停止
  任何服务前拒绝、回滚按注册表记录版本把关。
- 执行真实 Linux 命令或修改网络、块设备、进程状态、`/run` 或
  `/userdata` 的集成测试属于目标设备测试，不得作为普通 host 测试运行。
- 保持测试确定性：不依赖真实 WAN 访问、真实 provider 数据、墙钟时序、
  设备凭据、生成固件或忽略的审计产物。
- 先运行静态检查再构建。除非用户明确要求，不启动 Rust、前端、SDK、
  Buildroot、kernel、rootfs 或 firmware 构建。

## 构建，验证以及测试入口

@./Makefile

## 开发部署与板端验证速查

- 非 router 应用通过 `apps/rust/things/tools/deploy-app.sh` 热部署：使用 `deploy <app> <ELF>` 和 `revert <app>`；工具会先做协议兼容性和 SHA-256 校验，只重启目标应用。
- router 也可以在授权设备上做不经 OTA 的临时热替换验证：执行 `make router-app`。USB ADB 场景先上传、校验并备份候选/旧 ELF，再执行 `/etc/init.d/S81hyz-router stop`、原子替换和 `start`；网络 ADB 场景不得先 stop，必须在旧进程仍运行时上传、校验、备份并原子替换，然后执行 `adb reboot`，等待选定的网络 ADB 恢复并验收。两条路径都必须核对主机/设备 SHA-256、wire protocol 和回滚文件；它们属于开发调试用的受控热替换，不是正式发布，router 正式发布仍必须走 OTA。
- 详细流程见 [`docs/router-app-debug.md`](docs/router-app-debug.md)，ADB 连接参数见 [`docs/network-adb.md`](docs/network-adb.md)。
