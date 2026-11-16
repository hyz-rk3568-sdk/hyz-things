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
- CI 中按本次实际选中的可运行 E2E 数量自动决定 Runner：**60 个及以下 1 片，超过 60 个 2 片（最多 2 片）**。每个 Runner 各自启动独立 E2E 后端和 WebKit；每 Runner 维持 `workers: 1`，使用测试级分片平衡大 spec 的执行量。矩阵设置 `fail-fast: false` 以汇集所有分片的失败，并保留稳定的聚合检查供合并门槛使用。
- 不通过删除断言、`skip`、放宽失败判定、忽略错误退出码或屏蔽相关 spec 来换取通过；修复失败后要重新验证被改动行为。

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

#### 本地并行分片（优先使用，节约测试时间）

- 同一份 verified frontend bundle 可以同时服务多个 E2E 后端。每个 `hyz-things-e2e` 进程拥有独立的内存后端与 `temp_dir()/hyz-things-e2e-flashcards-<pid>/` 目录，因此不同端口上的 harness 互不共享状态。
- 为每个并行分片启动一对独立端口，并复用同一 bundle。需要真实申论样卷 fixture 的 spec（`shenlun.spec.ts` 中的真实题库/上传用例）必须在启动时显式带上 `HYZ_THINGS_E2E_SHENLUN_UPLOAD_FIXTURE=1`；`playwright.config.ts` 只为自己拉起的 server 设置该变量，`HYZ_THINGS_E2E_REUSE_SERVER=1` 不会补上：
  ```sh
  cd apps/rust/things
  HYZ_THINGS_E2E_WEB_PORT=3192 HYZ_THINGS_E2E_CONTROL_PORT=3193 \
  HYZ_THINGS_E2E_SHENLUN_UPLOAD_FIXTURE=1 \
  HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE=1 bash tools/start-e2e-server.sh
  ```
- 分片运行时显式附着到对应 harness，并启用测试级分片；`workers` 仍保持 `1`：
  ```sh
  HYZ_THINGS_E2E_WEB_PORT=3192 HYZ_THINGS_E2E_CONTROL_PORT=3193 \
  HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE=1 HYZ_THINGS_E2E_REUSE_SERVER=1 \
  HYZ_THINGS_E2E_TEST_SHARD=1 \
  ./node_modules/.bin/playwright test <spec...> --shard=2/2 --reporter=line
  ```
- **不要让两个 Playwright 进程同时附着到同一个 harness**：`resetHarness` 与内存状态会互相覆盖。同一 harness 可以串行复用；并行分片必须各自启动后端。本地分片数量不受 CI 的 2 片上限限制，可按 spec、`--grep` 或 `--shard=i/n` 继续细分；`--list` 用于核对实际选中数量。
- `resetHarness`（`PUT /reset`）只重置内存门户状态、摄像头会话与上传观测，**不清空题库数据**（收藏、作答、AI 记录、练习会话）。同一 harness 内多个 spec 或分片共享题库 SQLite；会改动收藏等题库状态的用例必须回到观测到的初始状态，需要干净题库时显式调用 `PUT /reset-question-bank`。
- 改动 `src/web` 或 `e2e` 下的文件后，bundle 缓存校验会判定 `frontend inputs changed`；启动新 harness 前先重新执行 `apps/rust/things/tools/build-frontend-bundle.sh`，已在运行的 harness 继续使用旧 bundle。
- 这是 CI 的等价做法：CI 每个 matrix shard 启动独立后端、共享已验证 bundle，并用 `HYZ_THINGS_E2E_TEST_SHARD=1` 平衡大 spec；本地可用同样结构更快定位失败。
- 启动多个 harnees 的目的是为了让测试总时间最短，需要均匀拆分

## 构建，验证以及测试入口

@./Makefile

## 开发部署与板端验证速查

- 非 router 应用通过 `apps/rust/things/tools/deploy-app.sh` 热部署：使用 `deploy <app> <ELF>` 和 `revert <app>`；工具会先做协议兼容性和 SHA-256 校验，只重启目标应用。
- router 也可以在授权设备上做不经 OTA 的临时热替换验证：执行 `make router-app`。USB ADB 场景先上传、校验并备份候选/旧 ELF，再执行 `/etc/init.d/S81hyz-router stop`、原子替换和 `start`；网络 ADB 场景不得先 stop，必须在旧进程仍运行时上传、校验、备份并原子替换，然后执行 `adb reboot`，等待选定的网络 ADB 恢复并验收。两条路径都必须核对主机/设备 SHA-256、wire protocol 和回滚文件；它们属于开发调试用的受控热替换，不是正式发布，router 正式发布仍必须走 OTA。
- 详细流程见 [`docs/router-app-debug.md`](docs/router-app-debug.md)，ADB 连接参数见 [`docs/network-adb.md`](docs/network-adb.md)。
