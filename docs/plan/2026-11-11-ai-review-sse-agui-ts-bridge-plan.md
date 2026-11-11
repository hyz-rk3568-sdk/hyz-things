# AI 解析 SSE 流式输出、Agent 执行可视化与 TypeScript Bridge 实施计划

日期：2026-11-11。状态：**计划待评审；仅文档，不包含功能实现**。目标分支：`main`。

## 0. 决策摘要

**推荐方案：**继续使用 Rust Axum + Yew/WASM；PC 侧 Codex App Server 仍通过 WebSocket/JSON-RPC 接入；RK3568 侧补充受控的 Agent 事件映射 + SSE；**将 Vite + TypeScript Bridge 纳入标准浏览器构建链**，在真正需要时导入 npm 库。

- **不**替换 Yew，不引入 React/Vue，不让 Node.js/Bun 常驻 RK3568，不重写 Codex Agent。
- **不**直接把 Codex 原始 JSON-RPC、内部推理文本、原始工具参数或执行日志透传给浏览器。
- **参考 AG-UI 协议、按实际需求实现可互操作的子集**，先完成内部有版本号的事件契约及端到端行为；只有确需 TS SDK 时才增加 `@ag-ui/core`，不以“全量兼容 AG-UI”作为未经验证的承诺。
- **将 Vite/TS Bridge 标准化为前端基础设施，可作为阶段 0 先行实施**；但 SSE、消息队列、后端任务状态不强制依赖 TS SDK。第三方 npm 库通过产品收益、包体积、浏览器兼容和维护成本逐项准入。
- 保留现有“用户主动发起、后台继续执行、手动停止、按题追问、持久 Codex Thread、历史消息可读”的行为。

## 1. 已核实的仓库现状（制定本计划时）

### hyz-things（`hyz-rk3568-sdk/hyz-things`，`main`）

| 实际文件 | 已有行为 / 待解决问题 |
| --- | --- |
| `apps/rust/things/src/adapters/outbound/codex_app_server.rs` | 自行实现 Codex App Server WS/JSON-RPC；`run_turn` 逐帧读取事件，但主要用 `item/completed` 的 `agentMessage` 和 `turn/completed` 收口，未对上层报告结构化增量。`turn_review` 强制 `outputSchema` 将最终 Markdown 包在 JSON `answer` 字段中。 |
| `apps/rust/things/src/adapters/outbound/question_bank_ai.rs` | 异步 `tokio::spawn`；`tokio::sync::watch` 用于停止/完成；`active` 以 `(question_id, stem_fingerprint)` 定位（**不要沿用 11-04 旧方案里 session_id 的描述**）；完成后写 SQLite 的 `question_ai_messages`；并发上限 `MAX_ACTIVE=2`。 |
| `apps/rust/things/src/application/question_bank/practice.rs` | 存在 `AiReviewBackend` 和 `AiReviewTurn` 端口，可以演进为接受事件汇报回调或有界发送器，并保留测试替身。 |
| `apps/rust/things/src/adapters/inbound/http/question_bank.rs` | 已有按题 `/api/v1/question-bank/practice/questions/{id}/ai` 的 GET/POST/DELETE 和追问、停止等受保护接口；适合加同资源下的 `/ai/events`。 |
| `apps/rust/things/src/web/pages/question_bank_practice.rs` | Yew 组件每 1500 ms 拉取 `running` 状态；显示“正在生成”等粗粒度状态，没有工具、阶段和增量正文。`AiReply` 已有 Markdown、复制源码和 SVG 绘图渲染。 |
| `apps/rust/things/frontend/index.html`、`apps/rust/things/tools/build-frontend-bundle.sh` | Trunk 构建 Rust/WASM、npm 构建 CSS；前端静态资源打包到 Rust 服务。CSP 禁止随意内联脚本、外部 CDN 等；`frontend-bundle-cache.py` 有输入指纹与产物一致性约束。 |
| `apps/rust/things/src/web/ai_diagram.js` | 已存在 Rust/WASM 调 JS 的局部能力，但尚无 Device Hub 那样的一套 Vite+TS npm bundle 工程。 |
| `docs/plan/2026-11-04-question-bank-ai-review-codex.md` 与 `docs/plan/2026-11-09-shenlun-zhenti-schema-import-plan.md` | 早期方案描述“只读沙盒”，但后来已按产品决定改为 `danger-full-access`；实施时**以当前源码为准**，将这个权限事实列为新增可观测性的安全门槛，避免文档造成误判。 |

### Device Hub（`hyz123a/device-hub`，`master`；参考实现，不作为本 PR 的改动对象）

- `frontend/bundle/index.ts` + `frontend/bundle/vite.config.js` + `frontend/bundle/package.json`：将 `@xterm/xterm`、`@xterm/addon-fit` 打包成浏览器 ES Module，并封装为 JS 桥接函数。
- `frontend/src/components/xterm_terminal.rs`：通过 `wasm-bindgen` 调用 bridge；Rust/Yew 管组件生命周期、JS 库管复杂视图实现。
- `frontend/index.html`、`frontend/package.json`：将打包后的 JS/CSS 交由 Trunk 管理。
- `server/src/events/mod.rs`：`tokio::sync::broadcast` + 有界环形缓存 + 持久层组合，支持快照 + 订阅。
- `frontend/src/components/stream_log_panel.rs`：按需连接 WS、前端有限缓冲、批量刷新、重连与组件卸载清理，可复用**设计经验**，但 AI 单向流优先采用 SSE，不照搬 WebSocket。
- 不跨仓库复制依赖或整块组件；先抽象符合 AI 语义的最小模块，确认许可证和依赖版本后再迁移。

## 2. 目标体验与不变量

在已完成题目的“AI 解析”页，用户能看到：

```text
AI 解析 · 正在生成
  ✓ 创建会话
  ✓ 查找题目与材料
  ◉ 正在调用工具：读取方法卡
  ✓ 工具调用完成
  ◉ 正在推理（仅显示允许公开的摘要或阶段，不展示内部原始推理）
  ◉ 正在输出答案……

【AI 解析正文】（逐步出现 Markdown）
……
```

**行为不变量：**

1. 原始解析仍是默认标签；打开 AI 标签、SSE 订阅和页面重连都不自动触发新 Turn、不额外消耗 Codex 额度。
2. 只有用户显式点击“开始 AI 解析”或“追问”才启动；重复请求保持既有幂等/忙碌语义。
3. 题目切换、标签切换、页面卸载与网络断连只关闭**订阅**，不取消服务端任务。只有既有停止接口触发 `turn/interrupt`。
4. 旧的历史记录与追问链保持可用；`(question_id, stem_fingerprint)` 仍是题目身份/版本隔离键，不引入跨题串流。
5. 未交卷答案隔离、临时图片上传限制、CSRF、登录权限、限额、并发上限和现有数据兼容不回退。
6. 不将不完整的模型输出误写为最终解答；失败、中断、重试都要有明确状态。

## 3. 端到端架构

```text
PC: Codex App Server (WebSocket / JSON-RPC)
          │   Thread / Turn / Item notifications
          ▼
RK3568: codex_app_server.rs（协议适配）
          │   事件筛选、同一 thread/turn 关联、脱敏
          ▼
        AI Event Mapper（AG-UI 风格的最小版本化模型）
          │
          ├── QuestionBankAiService / active run（生命周期、run_id）
          │          ├── 有界 broadcast + 可重放环形缓存
          │          └── SQLite：已有消息 / 状态，按需要保存最终结果
          │
          └── Axum GET /api/v1/question-bank/practice/questions/{id}/ai/events
                         │ 同源、受保护 SSE
                         ▼
Browser: EventSource（Yew/Rust WASM）
          │
          ├── Agent 过程组件（阶段、工具、错误、重连状态）
          ├── Markdown 追加缓冲 + 节流渲染 + KaTeX + 现有安全 SVG 机制
          └── [可选] Vite 打包的 TS/npm Bridge（例如 AG-UI 解析器）
```

**传输选择：** Codex ↔ RK3568 继续 WS；RK3568 → 浏览器以单向 SSE 为主。现有 POST 仍负责执行/停止/追问。不会为了流式 UI 额外启动 Node、Bun、React 服务。

## 4. 事件契约与映射

先实现 `AiStreamEventV1`（序列化为 JSON），字段包括：`version=1`、`run_id`、`seq`（run 内递增）、`question_id`、`kind`、`timestamp_ms`、`payload`。Event ID 使用 `run_id:seq`；不要把原始 Codex Thread ID、图片真实路径或访问 Token 作为客户端可控路由参数。

| Codex 通知（存在/条件性发送） | 对外事件（建议） | 显示 |
| --- | --- | --- |
| `turn/started` | `run_started` | 开始解析 |
| `item/started`，类型为 `commandExecution` / `mcpToolCall` / `webSearch` | `tool_started` | 工具类型、脱敏后的简短描述 |
| 对应 `item/completed` | `tool_finished` | 成功、失败、耗时（如有） |
| `item/reasoning/summaryTextDelta`（仅支持时） | `reasoning_summary_delta` | 可公开的摘要；不可用时只显示“正在思考” |
| `item/agentMessage/delta` | `text_delta` | 答案正文增量；见 `5 的输出格式决策 |
| `item/completed`，`agentMessage` | `message_finalized` | 权威正文，去重/校正先前增量 |
| `turn/completed` | `run_finished` / `run_failed` / `run_stopped` | 最终状态和持久化对齐 |
| 未知通知 / 暂不支持的 tool 类型 | 安全忽略或转换为通用阶段 | 不泄露原始包 |

映射注意点：

- 只处理 **当前 thread_id + turn_id + item_id** 归属正确的通知；不要把旧 Turn、别的任务、`initialize` 杂项、JSON-RPC ACK、服务端请求混入用户流。
- `item/completed` 的最终正文是权威值。增量可能缺失、重放或与最终正文有差异，前端按 `run_id + item_id + seq` 去重并允许最终内容替换。
- 显示的工具步骤是**实际已收到的事件**，不能为了体验编造“正在查找方法卡”这样的真实调用记录；缺少结构化事件时显示通用阶段即可。
- AG-UI 兼容性采用增量适配：`RUN_STARTED`/`RUN_FINISHED`、`TEXT_MESSAGE_*`、`TOOL_CALL_*`、`CUSTOM` 等按需建立单向映射；不能映射的通知保留在**内部**模型。先做官方协议 fixture 兼容性检查，再决定是否声明 AG-UI 标准互通，不机械套用完整 Agent 框架。
- **不传递 `item/reasoning/textDelta` 的原始内部推理**，不向浏览器输出命令原文、环境变量、工具参数、私有文件路径、上传图片路径或原始工具输出；工具信息采用服务端允许列表 + 长度限制 + 脱敏策略。
- 一律对 Codex 不可信输出按数据处理；Yew Markdown、代码块、SVG 现有安全渲染路径继续生效。

### SSE 示例（示意契约，非已上线接口）

```text
id: run-opaque-1:1
event: ai
data: {"version":1,"run_id":"run-opaque-1","seq":1,"kind":"run_started","payload":{}}

id: run-opaque-1:2
event: ai
data: {"version":1,"run_id":"run-opaque-1","seq":2,"kind":"tool_started","payload":{"item_id":"item-1","label":"读取相关资料"}}

id: run-opaque-1:3
event: ai
data: {"version":1,"run_id":"run-opaque-1","seq":3,"kind":"text_delta","payload":{"item_id":"item-2","delta":"这道题考查……"}}
```

## 5. Markdown 增量与 `outputSchema`（独立风险门槛）

当前 `turn_review` 的 `outputSchema` 将全部 Markdown 放在 JSON `answer` 字符串中，不能直接把收到的原始 delta 当 Markdown 显示。需要在实现正文流之前完成验证：

1. **优先尝试：仅 AI 解析 Turn 走纯 Markdown 输出**，在 Codex App Server fixture 与实际兼容性验证后评估是否取消该 Turn 的 `outputSchema`；保留 AI 出题等原有结构化 schema，不统一更改。
2. 如必须保持结构化 schema，**使用经过测试的增量 JSON 字符串解码器**：处理转义、Unicode、分块边界、数组外噪声/不完整 JSON，并仅提取 `answer` 值。不能用简单正则或手工去掉花括号。
3. **无论选哪条路，完成时都以已有最终解答的解析/校验结果为准**；不完整结果只属于临时 UI 草稿，绝不污染已完成消息。Codex 若不提供可用 delta，仍展示过程事件，最后一次性显示完整解析。

渲染策略：按 30–100 ms 或相近合理批次聚合 delta，避免每 token 触发 Yew 全量重绘；代码围栏、KaTeX 分隔符或 SVG 未闭合时仅安全显示文本/延迟水合，最终消息完成后调用既有 `hydrate_math` 与 `render_ai_diagrams`；保留“查看源码/复制源码”。

## 6. 服务端、订阅与持久化语义

**路由：**新增同源只读 `GET /api/v1/question-bank/practice/questions/{id}/ai/events`，保留现有 `GET /ai`、`POST /ai`、`POST /ai/messages`、`POST /ai/stop`、`DELETE /ai`。

- 该 GET 与既有 `ai_read` 使用同等管理员 Session 校验和题目可见性/版本校验；`EventSource` 走同源 Cookie，不依赖可泄露到 URL 的鉴权 Token。POST/DELETE 保持现有 CSRF/Origin 校验。
- 响应 `Content-Type: text/event-stream`、`Cache-Control: no-cache, no-transform`；保持心跳（例如 Axum `KeepAlive`），检查反向代理/路由缓存不缓冲 SSE。
- 每个活跃 Turn 一个服务器生成的 `run_id`；有界内存事件队列与 `broadcast` 仅缓存脱敏事件，给慢客户端明确 `resync_required` 或 snapshot，不允许无界队列/无限增长。
- 重连：先读取既有 `GET /ai` 作为权威历史状态；订阅建立后接收当前 run 的 snapshot + 后续事件；支持 `Last-Event-ID` 或同等游标进行有界重放。明确快照与实时消息之间的原子一致性/排序，避免事件丢失或重复。
- 取消：断开 SSE、切页、网络离线 **不发 `turn/interrupt`**；停止按钮仍走既有 `/ai/stop`，等待终止确认后发 `run_stopped`。
- 服务重启：沿用当前把“运行中”改成失败可重试的规则；无完整最终答案时不写 assistant 成品消息；新页面看得到已经持久化的历史及明确失败说明。
- 持久化：第一阶段继续用 `question_ai_reviews` + `question_ai_messages` 保存**最终内容**；过程事件只存在活动任务的有界内存缓存。若后续明确要永久回放过程，再单独设计带保留期、脱敏、空间配额的事件表，不在本阶段无边界保存命令输出。
- 埋点：记录事件投递数量、丢弃/重同步次数、首个事件耗时、流持续时间、异常类别；**不**把敏感正文和工具输出写到 telemetry。

## 7. TypeScript Bridge 方案（参考 Device Hub，确定引入标准构建链，库按需增加）

### 引入时机与职责

SSE + 基础过程 UI 先使用 `web_sys::EventSource` 和 Yew 实现，不必为了调用标准浏览器 API 创建 JS Bridge。

确定增加独立的、由 Vite 产出的 **browser-only TS bundle** 作为标准接入点；**首个真实 npm 依赖选 `@floating-ui/dom`**，在现有 `AiReply` 回复操作菜单中试点浮层定位，覆盖 JS 包导入、Rust 调用、DOM 生命周期、清理与 iOS 兼容。之后按需增加 AG-UI 事件验证器、图片预览、输入框增强或流式 Markdown 增强功能，**不要为了使用 npm 库而将消息状态从 Yew/Rust 迁到 TS**。职责边界：

- **Rust**：后端 AI 任务、数据模型、授权、Agent 事件筛选、SQLite、Yew 组件状态与生命周期。
- **TypeScript**：npm 专用库、浏览器复杂组件、必要的事件解码/视图插件；不复制 Rust 题库状态机，不持有 Codex Token。
- **WASM Bridge**：只暴露小型类型化接口（`wasm-bindgen`），统一初始化、销毁、错误返回；`Drop`/unmount 清理 JS 监听器和资源。
- **Node/npm/Vite 只存在构建期**；RK3568 发布的是静态 ES Module 与 WASM，不依赖 CDN、在线 npm 或 Node 常驻。

### 预计工程变更

```text
apps/rust/things/
  package.json                          # 复用现有 npm 工作区，增设 build:ts
  package-lock.json                     # 保持单一 lock；新增 @floating-ui/dom 和 Vite/TS
  frontend/ts/
    vite.config.ts
    src/index.ts                        # 稳定 Bridge API 导出
    src/floating-menu.ts                # 第一个 npm 实例：回复“⋮”菜单
    src/agent-events.ts                 # 后续可选：AG-UI/npm 的边界
  src/web/
    bindings/browser_bridge.rs          # Rust/WASM ↔ JS：统一初始化与销毁
    components/ai_run_progress.rs       # 后续示例，依照现有模块风格决定
  tools/build-frontend-bundle.sh        # 先 npm build:ts 再 Trunk，验证 JS/CSS
  tools/frontend-bundle-cache.py        # 纳入 TS 源码、lock、toolchain、产物指纹
```

Vite 产物必须由 Trunk 作为**本地静态 module** 引入，并由现有 tar/embed 链路交付；沿用 hyz-things 当前严谨的 `script-src 'self' 'wasm-unsafe-eval'` 与禁内联脚本 CSP，禁止全局 CDN/`unsafe-inline`。Device Hub 的 `window.deviceHub*` 只是可参考的桥接形式，原则上优先选择明确 module 生命周期、避免新增无约束全局变量。

对 `@ag-ui/core` 做一次独立决策：验证其浏览器 bundle、运行时依赖、许可、协议版本、Tree shaking 和产物体积；若仅需少量事件类型且 npm 包带来过量依赖，就先使用 Rust `serde` 类型和本地 TS 轻量 DTO，不强加包。

## 8. 分阶段实施（建议拆为后续功能 PR）

### 阶段 0：Vite + TypeScript 标准接入工程（独立前置 PR）

- **复用已有** `apps/rust/things/package.json` + `package-lock.json`，不建立第二套 Node/npm 工作区；新增锁定的 Vite、TypeScript 和首个 browser-only 依赖 **`@floating-ui/dom`**。复用 Device Hub 的 ES Module + `wasm-bindgen` 模式，但统一 Bridge 的导出接口、错误返回、版本边界与组件释放。
- **先做一个真正的业务落地点：**给现有 `AiReply` 的回复操作「⋮」菜单接入 `computePosition` + `flip` + `shift` + `autoUpdate`；菜单显隐、键盘 Escape、焦点和点击外部关闭由 Yew 处理，TS 只负责定位；菜单打开时订阅、关闭或卸载时执行 cleanup，不增加第二套状态管理。
- 将 `npm run build:ts`（基于 Vite）整合进当前 Trunk/前端 tar/embed 产物；单一入口、相对资源路径、静态 JS/CSS、CSP 和构建缓存输入指纹全部被现有发布链识别。明确 dev 与 release 一致，不依赖 Node/Bun 运行时、CDN、内联脚本或设备端安装 npm 包。
- 编写 `web/bridge` 的最小类型化 Rust 接口，并做一项自动化双向调用/错误处理/销毁 smoke test；以后新增 npm 库走同一入口、锁文件、构建和测试规范，不继续在 Yew 页面增加散落的 window 全局方法。
- 验证长解析列表、窄屏、iOS Safari、滚动容器、旋转/窗口大小变化、返回前台、旧前端缓存与更新后的 module 资源一致；记录生产 bundle 体积与构建失败回退。
- **AG-UI 暂不是首个验证库**：当前 Codex 事件还没转换为有保证的 AG-UI schema。先完成 Bridge 与 UI 实例，再在阶段 A/B 事件稳定时评估 `@ag-ui/core`；`@ag-ui/core/schemas` 的运行时校验还需要核对 Zod 依赖版本。
- 可先于阶段 A 实施，也可与 A 并行；它的存在不要求基础 SSE 必须改用 TypeScript。

**验收：** `npm ci` + `npm run build:ts` + Trunk release + 静态资源嵌入/CSP + Playwright E2E 通过。真实 Yew AI 回复菜单能翻转/避让边界，卸载后不残留 DOM 监听器。添加第二个示例模块无需修改构建主链，只需新增导出与依赖声明。

### 阶段 A：事件契约与 Codex Adapter（后端 PR 1）

- 明确 `AiStreamEventV1`、Run/Item 关联、允许暴露字段和字段最大长度。
- 为 `AiReviewTurn` 添加可测试的事件 sink，不破坏已存在的出题接口。
- 对 `item/started` / `item/completed` / `item/agentMessage/delta` / `item/reasoning/summaryTextDelta` / `turn/completed` 做映射；未知通知不中断会话。
- 复用 Device Hub 的有界 EventBus **原则**，在题目 AI 服务中实现按 Run 隔离的事件广播、snapshot、游标去重与缓冲。
- 新增 Axum SSE 路由、同源授权、心跳、取消不关联 SSE 断开、可控的重连/终态逻辑。
- 先在“正文继续一次性显示”的模式下验证运行时过程，**不**冒进修改 `outputSchema`。

**验收：** mock Codex 端按顺序、乱序、无 delta、多个 Thread、失败与停止发包时，SSE 能按当前 Run 正确展示；浏览器离开并返回不启动第二次 Turn。

### 阶段 B：Yew 过程 UI 与文本增量（前端 PR 2）

- 在 `question_bank_practice.rs` 旁提取专用组件/Hook，避免把新的连接管理全部堆入现有大型 page。
- 用事件订阅替代运行时 1500 ms 常规轮询；API GET 继续作为加载/恢复/兜底。
- 工具步骤、状态、可选公开推理摘要与正文分区显示；折叠过程面板，明确失败/重连/已停止。
- 验证并选择 `5` 的纯 Markdown 或增量 JSON 方案；避免不完整围栏造成闪烁、XSS 或损坏 SVG。
- 保留当前历史追问、复制源码、图片附件、题目导航和既有测试选择器。
- 若浏览器不支持 EventSource 或 SSE 不可达，可回退现有查询/最终结果展示，但不重复触发生成。

**验收：** 第一段有效正文到达即出现（不等待 `turn/completed`）；完成时最终全文与 SQLite 一致；断线、切题、停止、追问均无重复内容与额外 Turn。

### 阶段 C：TS/npm 生态按需扩展（后续独立 PR）

- 复用阶段 0 已完成的 Vite+Trunk 工程与基础 Bridge；选择确有收益的浏览器库（例如流式 Markdown/代码高亮/编辑器），不重复增加第二套 TS 构建系统。
- 评估并按需接入 `@ag-ui/core`；若引入，则做 TS 事件校验并保留 Rust DTO 的等价测试。
- 保障生产本地静态资源、无外部运行时依赖、CSP、双端构建缓存和更新资源可用。
- 严格限制 Bridge 只负责 JS 生态能力，不让其承担题目业务逻辑。

**验收：** `npm ci`、TS build、Trunk release、前端 bundle 归档/内容指纹、CSP Playwright 全部通过；没有 Node 运行时依赖。

### 阶段 D：聊天交互回归与上线验收

- Rust 单元/集成：事件适配、身份隔离、广播积压、断线、停止、任务重试、服务重启、旧表兼容、无 Codex 配置/配额错误。
- E2E（沿用/扩展 `apps/rust/things/e2e/question-bank.spec.ts`）：模拟后端 SSE 多片段、工具生命周期、可折叠 UI、跨题切换、重新进入、完成后数据一致、无权限拒绝与防 XSS。
- 确认不向未交卷练习泄露答案、无浏览器侧 `turn/start`/停止误调用、未经授权用户无法订阅其他题的事件。
- 对设备端仅在准备部署时做实际测试：弱网、PWA 回前台、iOS Safari、浏览器断网/唤醒、局域网代理 SSE 不缓冲；检查二进制/静态资源体积增量。
- 最终合并与部署遵循仓库既有 PR/CI 流程；**本计划 PR 不执行任何部署**。

## 9. 风险清单与门槛

| 风险 | 处理方式 |
| --- | --- |
| `outputSchema` 导致 JSON 字符串碎片不能直接渲染 Markdown | 先做正文流原型和 golden fixtures；无验证结果不取消 schema、不发布“流式 Markdown”。 |
| 现有 Codex Thread 是 `danger-full-access` | 这是**现状**而非本计划建议；SSE 只暴露脱敏可视事件，不公开命令、原始工具参数、绝对路径与 Token。另设权限最小化审计，不隐式更改当前产品行为。 |
| AG-UI 类型与 Codex 事件不一一对应 | 在 Rust adapter 做显式映射、未知事件降级；先声明内部子集，经过兼容测试再宣称 AG-UI 互通。 |
| 多题并发、旧 Turn 迟到和断线重放串流 | `run_id + thread_id + turn_id + item_id` 关联，`seq` 去重，快照与订阅原子顺序，按 `question_id + stem_fingerprint` 隔离。 |
| 广播积压、内存上升及频繁 DOM 重绘 | 有界广播 + bounded ring、慢客户端 resync、delta 批处理、不可见组件退订。 |
| SQLite 与屏幕显示不一致 | 只将合法完整的最终 answer 视为持久权威；终态时以服务端最终快照纠正增量。 |
| Vite 引入导致 CSP 或缓存失效 | 本地打包、Trunk 静态 module、npm lock、指纹和 CSP 检查；不靠运行时 CDN。 |
| WebKit/iOS 浏览器后台挂起 SSE | 监听可见性并重连快照；任务持续在 Rust 服务端运行，不依赖页面保持在线。 |
| 早期文档与当前实际路由/身份键/权限不一致 | 以 `main` 最新源码为准；文档引用要标明历史设计与当前实现的差异。 |

## 10. 最小完成定义（DoD）

- [ ] AI 解析/追问支持**真实** Codex 过程事件（工具、阶段、终态），未收到事件不造假。
- [ ] SSE 实时流不再靠 1.5 秒轮询探测进度；老接口仍能取历史状态并作为降级回退。
- [ ] 支持增量正文且最终完整文本与持久结果一致；不存在 JSON 包装泄漏到 Markdown UI。
- [ ] 订阅断连/切页不停止 AI；重连可同步最新状态；用户停止仍能可靠发出 `turn/interrupt`。
- [ ] 模拟的失败、停止、乱序、重复、积压、重启都不跨题串内容或泄漏未提交答案。
- [ ] 过程事件按最小权限暴露，不显示内部原始推理、秘密和非必要工具输出。
- [ ] Vite/TS Bridge 作为标准浏览器依赖构建链已接入，且不破坏 CSP、静态资源嵌入和缓存完整性；具体 npm 库维持按需选择。
- [ ] Rust 测试、Playwright E2E、前端构建与 CI 通过；无不相关的设备功能/题库数据改动。

## 11. 本 PR 范围

**仅新增这一份计划文档。** 不修改运行代码、依赖、数据库、SDK、题库或设备配置；不执行迁移、不部署、不合并。本 PR 审核通过后，按 `8` 拆分后续实施 PR。

## 参考

- 当前代码：`apps/rust/things/src/adapters/outbound/codex_app_server.rs`、`apps/rust/things/src/adapters/outbound/question_bank_ai.rs`、`apps/rust/things/src/web/pages/question_bank_practice.rs`、`apps/rust/things/tools/build-frontend-bundle.sh`。
- Device Hub：`hyz123a/device-hub/frontend/bundle/index.ts`、`frontend/src/components/xterm_terminal.rs`、`server/src/events/mod.rs`、`frontend/src/components/stream_log_panel.rs`。
- Codex App Server 协议（参考）：https://learn.chatgpt.com/docs/app-server
- AG-UI 事件协议（参考）：https://github.com/ag-ui-protocol/ag-ui/blob/main/docs/concepts/events.mdx
- Axum SSE（参考）：https://docs.rs/axum/latest/axum/response/sse/

## 12. AI 聊天/追问交互专项审计与重构（跟进审计：2026-11-11）

本节基于 `main` 当前的 `question_bank_practice.rs`、`collection_review.rs`、`question_bank_ai.rs` 与 Playwright 测试补充。**这是静态源码确认的交互风险与设计建议，尚非用户环境中已复现的端到端故障。**

### 12.1 当前事实与风险

| 源码确认的行为 | 用户影响 | 优先级 |
| --- | --- | --- |
| `ai_composer` 渲染在全部历史消息之后；并未见聊天专属自动跟随/“返回最新消息”逻辑 | 初次解析或多轮追问正文很长时需要反复滚动寻找输入框；题目阅读区与聊天阅读区共享行为 | P0 |
| 仅 `has_answer && !running` 时渲染追问框 | 生成阶段看不到输入框，无法准备下一轮问题，难以建立聊天连续感 | P0 |
| `send_follow_up` 在 HTTP 成功返回前不在消息列表中呈现草稿/待发送消息，状态只有 `submitting` | 发送反馈滞后，网络慢时不容易判断是否已发送 | P0 |
| 当前运行状态依赖 1500ms 轮询；服务端只在最终完成时写 assistant 消息 | 用户看不到实时正文和执行步骤；UI 无独立的“等待/运行中/首 token/工具”时间线 | P0 |
| 父组件在 `ai_tab` 切换时条件卸载 `QuestionAiReview`，追问草稿在组件本地 `use_state` | 切换“原始解析”回来尚未发送的文字与已选未发图片会消失 | P0 |
| `on_start_key` / `on_follow_up_key` 直接调用发送回调；回调只看 `submitting`，没有检查 `uploading_start` / `uploading_follow` | 虽然按钮禁用，Ctrl/⌘+Enter 仍可能在图片上传中提前发送，丢失本次附件 | P0（明确代码缺陷） |
| `send_follow_up` 要求非空文字，UI 发送按钮也按 `follow_up.trim().is_empty()` 禁用 | 虽有附件选择与图像上传能力，但不支持“仅发送图片进行追问” | P1（产品能力） |
| 停止/失败后显示“重新分析”，其按钮调用 `start`，不是重试刚才那条失败的追问 | “重试上次回答”和“重新解析整题”语义混淆，可能产生非预期的再次生成 | P1 |
| `use_question_ai_tab` 在发现已保存 AI 回复后自动切到 AI 标签 | 与“始终默认原始解析”的旧产品文档存在差异；是否保持自动恢复需确认产品意图，不直接判定为 bug | P1（体验决策） |
| E2E 目前主要覆盖消息结果、操作顺序、输入与按钮的边框/相对位置 | 尚未充分覆盖长记录滚动、草稿恢复、上传快捷键竞态、IME、iOS 软键盘与流重连 | P0（测试缺口） |

### 12.2 推荐的聊天交互目标

- **消息区与输入区分离**：题目/材料与 AI 对话分别有明确滚动上下文；AI 对话有独立滚动容器，输入框贴底（受 safe-area、iOS 软件键盘影响），而不是排在整个 Markdown 历史之后。移动端不能让 composer 遮挡原题正文，也不能制造嵌套滚动陷阱。
- **发送即时反馈**：点击后立即出现用户消息的本地 `sending` 状态；服务端确认后 `sent`，失败 `failed` 并保留可编辑/重试草稿。并发仍遵守后端**单题单 Turn**约束；不能通过前端 optimistic UI 触发重复调用。
- **生成中允许编辑下一条草稿，但禁止同时提交第二个 Turn**：发送按钮随 `running` 禁用或显示“等待本轮完成”；停止操作独立可见；生成中可上传并准备附件，最终发送仍由后端忙碌校验守护。
- **草稿按题/版本隔离**：按 `question_id + stem_fingerprint`（或不暴露指纹的安全等价稳定键）保存未发送文字和附件选择状态，切换 AI/原解析、切题、返回时恢复；不要把 A 题草稿错放到 B 题。过期附件需要重新确认，而不是静默沿用失效的临时 ID。
- **滚动策略**：靠近底部（阈值可配置）才自动追随增量；用户主动上滚后暂停自动滚动并显示“有新内容，回到底部”；消息历史重载后恢复合理位置；新问题发出时滚动到最新一轮，不能每个 token `scrollIntoView` 干扰阅读。
- **输入法与附件**：保留 Ctrl/⌘+Enter 的稳定发送动作，跳过 IME 组合输入；点击与快捷键统一通过校验层，上传未完成时**一律禁止发送**。支持图片-only 追问需要明确空文本的后端规则、隐式提示词、大小限制及 E2E。
- **对话语义**：把“首次解析”“继续追问”“停止当前生成”“重试失败的最后一轮”“从头重新解析”分成不同操作及确认语义；保留旧消息、Thread 与附件上下文，不把停止后的追问意外改成初始解析。
- **消息归属与状态**：给客户端显示状态使用稳定 `message_id` / `run_id`（可以不改旧消息表时先做前端适配），明确区分 user、assistant、tool（工具事件不伪装成 assistant 正文）。可折叠工具执行过程不能导致正在读正文的滚动位置跳跃。
- **不要用 React UI 框架替代 Yew**：如 `assistant-ui` 等提供优秀的参考交互设计，但其 React 组件若作为 React Island 接入会带来第二套 DOM/状态生命周期。首期优先使用 Yew 组件和 browser-only TS 库；确需框架组件再做独立包体积/可访问性/生命周期评估。

### 12.3 实施切分建议与验收

将阶段 B 明确拆为两条并行工作流：

**B1：聊天 UI 状态机与交互（不依赖流式协议）**

- 抽离 `QuestionAiReview` 中的 `ConversationMessageList`、`AiComposer`、`AiRunControls` 组件；集中处理提交、上传、失败重试、切题/切标签、草稿保留与滚动。
- 消息 pending/failed 状态的归并以服务端回包为准，解决请求完成、UI 卸载、题目身份变化时的旧响应竞态。
- 修复 Ctrl/⌘+Enter 上传竞态；补 E2E：发送中态、长对话输入框可达、滚动不抢焦点、附件在途、图片-only 行为、草稿恢复、IME 组合输入。

**B2：SSE 和 Agent 过程视图（依赖阶段 A）**

- 将 `run_started`、`tool_started`/`tool_finished`、`text_delta`、`run_finished` 映射到 B1 的消息状态机，而不是另外叠加一个独立聊天状态机。
- 完成时用服务端最终 `answer` 纠正增量、去重、落盘；切题重连时先同步消息快照再继续更新。
- E2E 在 iOS 竖屏/桌面、短答/超长 Markdown、多工具事件、断线、Stop/Retry 和图片上传组合下验收布局与状态。

**更新后的推荐落地顺序：阶段 0（Vite/TS 标准入口）→ B1（独立聊天交互整治，可与阶段 A 并行）＋ A（后端事件/SSE）→ B2（流式聊天）→ C（按需引入更多 npm 库）→ D（全量验收）。**

### 12.4 新增 DoD

- [ ] 长回答或多轮聊天时输入框无需滚到历史末尾才能使用，且不遮挡题目材料。
- [ ] 生成中可编写下一条草稿；不能并发发出第二个 Turn。
- [ ] 切换解析标签/题目不丢失草稿且不会跨题错配；历史消息和正在运行任务保持不变。
- [ ] 发送有即时 pending/failed 反馈，不能因 HTTP 响应延迟导致重复发送或假成功。
- [ ] 点击、键盘快捷键与上传禁用态一致；IME 不会意外提交。
- [ ] 用户上滚时不会被流式 delta 抢走阅读位置，新内容可一键跳到最新。
- [ ] 图片-only 产品决策明确，失败追问与重新解析语义不同。
- [ ] Playwright 补齐移动端软键盘、附件竞态、长对话、切题、断线重连等交互回归。
