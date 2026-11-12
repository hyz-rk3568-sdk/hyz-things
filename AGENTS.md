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
- 摄像头状态、直播会话（create/close）、音量、画面与旋转操作均复用现有
  管理员 Session；写操作同时严格检查同源 Origin 和 CSRF。Camera 页面
  使用与网络页面一致的 AdminAuthGate，不发放匿名 viewer 令牌。
- 热推送（`apps/rust/things/tools/deploy-app.sh`）：推送 camera/things
  只停止并重启对应应用的 init 服务，绝不重启 router；任何停止动作之前
  必须通过注册表记录的协议版本做兼容性检查。

### 训练页面组件化

- 专项训练、题库及资料分析、判断推理、数量关系等页面，在新增或修改
  状态栏、答题区、复盘区、选项、分栏、导航与滚动交互前，先检查
  `apps/rust/things/src/web/pages` 中已有共享组件，优先复用或抽取具有
  相同行为的组件。组件使用明确的 props、事件回调及可访问标签。
- 共享组件只负责公共界面与交互；各题型的答题规则、计时、成绩、权限、
  题型专属数据请求和差异化展示由对应页面或现有应用边界负责。跨页面的
  通用数据请求可封装在受限 HTTP 驱动的共享组件或 hook 中。保留各页面
  已有的移动端布局、草稿纸、滚动位置、失败状态及安全约束。
- 修改共享组件时，批量运行覆盖所有受影响页面的相关 E2E，并在浏览器中
  验证答题、交卷、复盘与相关边界状态；布局调整还须检查桌面和手机尺寸。
  避免为只有表面相似、行为不同的页面强行合并组件。

## GitHub workflow

- 较大的功能开发或重构必须在独立分支完成，并通过 GitHub Pull Request 合入默认分支；不得直接提交到 `main`。
- 小型文档、测试维护、CI 修复或明确的局部 bugfix 可以直接提交；一旦范围扩大到跨模块、架构调整或大范围行为变化，必须切换为 PR。

### 已确认 Plan 的单 PR 交付（强制）

- 用户明确要求「用一个 PR 实现该 Plan」时，沿用现有 Plan PR：计划文档、实现、相关测试、CI 修复、验收都在**同一个 PR** 追踪。不得为了实施阶段、单个失败或测试调整擅自另开 PR，也不得直接推送 `main`；确需拆分时先征得用户同意。
- 按阶段实现并在同一 PR 上提交有意义的功能批次。每个阶段记录已实现范围、尚未完成的验收项与失败原因；不能把“已 push”“前端构建通过”冒充“计划已实现”。
- 必须以最终候选 HEAD 的所有必需 CI 检查通过为合并门槛；确认 PR 可合并后才合入 `main`。禁止绕过分支保护、取消失败项或通过跳过测试制造绿灯。
- 若 CI 暴露与当前 Plan 无关的既有缺陷，先用日志、基线或受影响文件定位并**明确告知用户**：失败的测试/步骤、症状、复现证据、是否阻塞合并及建议修复边界。不要悄悄将无关业务修复混进当前功能 PR；若该缺陷阻塞必需 CI，仍须修复或与用户明确协调，不能忽略。

### 相关 E2E 的最小充分覆盖（强制）

- 对 PR/本地变更，**只运行与本次改动相关的 E2E**，不要默认跑所有 Playwright spec。依据 `base...HEAD` 变更文件、代码依赖与测试映射选择用例；覆盖直接修改的页面、共享组件的全部实际使用方、功能权限和相关回归风险，不能仅按文件名选一条“看似相关”的测试。
- 同一批相关 Playwright spec 在**一次** runner 调用中执行，以复用 verified bundle 和 harness。涉及共享答题组件时，应包括真题、专项、资料分析、逻辑、数量中实际受影响的用例；发现遗漏时扩大受影响集合，而不是直接扩大到整个无关套件。
- PR CI 也应按变更范围选择相关 E2E；除非用户明确要求或发布策略规定，不在 PR 上无差别运行全量 E2E。已有 `main` 分支的发布/全量验证约束不因此放宽。
- CI 中按本次实际选中的可运行 E2E 数量自动决定 Runner：**60 个及以下 1 片，61–120 个 2 片，121–180 个 3 片，超过 180 个 4 片**。每个 Runner 各自启动独立 E2E 后端和 WebKit；每 Runner 维持 `workers: 1`，使用测试级分片平衡大 spec 的执行量。矩阵设置 `fail-fast: false` 以汇集所有分片的失败，并保留稳定的聚合检查供合并门槛使用。
- 不通过删除断言、`skip`、放宽失败判定、忽略错误退出码或屏蔽相关 spec 来换取通过；修复失败后要重新验证被改动行为。

### 单轮 CI 尽可能暴露全部问题（强制）

- **推送前集中检查**：优先运行 `git diff --check`、格式化检查、静态脚本/配置检查、E2E 套件清单及计数校验，以及受影响的最小测试集；先消灭显而易见的格式、locator、清单与构建脚本问题，再触发远端 CI。
- **一次 CI 批次汇集失败**：保持可并行 job 同时执行；格式、静态、单元测试、Clippy、相关 E2E 在安全可行的情况下采用独立 job 或带 `!cancelled()` 的 failure-collecting 步骤，不因一个可独立排查的检查失败就提前跳过其他检查。各步骤必须保留非零退出码与可访问日志。
- **一次读完失败结果**：完整检查所有失败 job、测试名、断言、日志和产物；能相互独立修复的错误应成批修完再 push。特别是同一轮同时出现格式、静态、native 测试和 Playwright 失败时，不得只修一个就反复启动整个流水线。
- **卡住也必须能诊断**：为可能无限等待的异步测试加入带具体操作名的有限超时；CI 设置合理的 job/测试超时并保留当前测试名、stdout/stderr 与失败产物。原生测试长时间无输出时定位正在等待的具体测试和 channel/锁/网络边界；不能仅靠增加总超时，不能直接跳过卡住的测试。
- 每次 CI 结果必须区分：当前 PR 回归、既有基线问题、测试自身过时/不稳定、环境依赖；无足够证据就标记“待定位”，不可声称一定是历史问题。**最终合入前**仍须在实际最终 HEAD 上通过全部必需检查。

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

### E2E 构建产物与服务复用（本地测试优先使用）

- 相关 Playwright spec 优先放在同一条 `playwright test <spec...>` 命令中运行；一次 invocation 只构建 bundle、启动 harness 一次，所有 spec 共用该服务。不要为同一批 spec 逐个启动 runner。
- 需要跨多轮调试复用时，先构建一次 verified frontend bundle：
  ```sh
  PATH="$(dirname "$(command -v node)"):$PATH" bash apps/rust/things/tools/build-frontend-bundle.sh
  ```
- 在一个终端启动固定端口的 harness，并校验 bundle 缓存：
  ```sh
  cd apps/rust/things
  HYZ_THINGS_E2E_WEB_PORT=3190 \
  HYZ_THINGS_E2E_CONTROL_PORT=3191 \
  HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE=1 \
  bash tools/start-e2e-server.sh
  ```
- 在另一个终端用相同端口批量运行相关 spec：
  ```sh
  cd apps/rust/things
  HYZ_THINGS_E2E_WEB_PORT=3190 \
  HYZ_THINGS_E2E_CONTROL_PORT=3191 \
  HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE=1 \
  HYZ_THINGS_E2E_REUSE_SERVER=1 \
  ./node_modules/.bin/playwright test e2e/logic-judgment.spec.ts e2e/comprehensive.spec.ts --reporter=line
  ```
- `HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE=1` 会校验当前输入、toolchain 与 archive hash；缓存不匹配时先重建一次。`HYZ_THINGS_E2E_REUSE_SERVER=1` 仅供本地显式附着到已启动的 harness。CI 不设置这两个变量，保持独立构建与服务。

## 构建，验证以及测试入口

@./Makefile

## 开发部署与板端验证速查

- 非 router 应用通过 `apps/rust/things/tools/deploy-app.sh` 热部署：使用 `deploy <app> <ELF>` 和 `revert <app>`；工具会先做协议兼容性和 SHA-256 校验，只重启目标应用。
- router 也可以在授权设备上做不经 OTA 的临时热替换验证：执行 `make router-app`。USB ADB 场景先上传、校验并备份候选/旧 ELF，再执行 `/etc/init.d/S81hyz-router stop`、原子替换和 `start`；网络 ADB 场景不得先 stop，必须在旧进程仍运行时上传、校验、备份并原子替换，然后执行 `adb reboot`，等待选定的网络 ADB 恢复并验收。两条路径都必须核对主机/设备 SHA-256、wire protocol 和回滚文件；它们属于开发调试用的受控热替换，不是正式发布，router 正式发布仍必须走 OTA。
- 详细流程见 [`docs/router-app-debug.md`](docs/router-app-debug.md)，ADB 连接参数见 [`docs/network-adb.md`](docs/network-adb.md)。
