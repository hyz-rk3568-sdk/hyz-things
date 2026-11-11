# AI 解析 SSE 流式输出、Agent 执行可视化与 TypeScript Bridge 实施计划

日期：2026-11-11。状态：**计划待评审；仅文档，不包含功能实现**。目标分支：`main`。

## 0. 决策摘要

**推荐方案：继续使用 Rust Axum + Yew/WASM；PC 侧 Codex App Server 仍通过 WebSocket/JSON-RPC 接入；RK3568 侧补充受控的 Agent 事件映射 + SSE；前端按需引入经 Vite 打包的 TypeScript/npm 生态。**

- **不**替换 Yew，不引入 React/Vue，不让 Node.js/Bun 常驻 RK3568，不重写 Codex Agent。
- **不**直接把 Codex 原始 JSON-RPC、内部推理文本、原始工具参数或执行日志透传给浏览器。
- **参考 AG-UI 协议、按实际需求实现可互操作的子集**，先完成内部有版本号的事件契约及端到端行为；只有确需 TS SDK 时才增加 `@ag-ui/core`，不以“全量兼容 AG-UI”作为未经验证的承诺。
- **优先完成服务端事件流与 Yew 原生 SSE 接收**；把 TypeScript Bridge 标准化与第三方 npm 生态接入作为独立、可验收的阶段，而非 SSE 的强制依赖。
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

## 7. TypeScript Bridge 方案（参考 Device Hub，渐进式）

### 引入时机与职责

SSE + 基础过程 UI 先使用 `web_sys::EventSource` 和 Yew 实现，不必为了调用标准浏览器 API 创建 JS Bridge。

确有需要时增加独立的、由 Vite 产出的 **browser-only TS bundle**，首个适配模块可选 AG-UI 事件验证器或流式 Markdown 增强功能。职责边界：

- **Rust**：后端 AI 任务、数据模型、授权、Agent 事件筛选、SQLite、Yew 组件状态与生命周期。
- **TypeScript**：npm 专用库、浏览器复杂组件、必要的事件解码/视图插件；不复制 Rust 题库状态机，不持有 Codex Token。
- **WASM Bridge**：只暴露小型类型化接口（`wasm-bindgen`），统一初始化、销毁、错误返回；`Drop`/unmount 清理 JS 监听器和资源。
- **Node/npm/Vite 只存在构建期**；RK3568 发布的是静态 ES Module 与 WASM，不依赖 CDN、在线 npm 或 Node 常驻。

### 预计工程变更

```text
apps/rust/things/
  frontend/ts/
    package.json
    package-lock.json
    vite.config.ts
    src/index.ts
    src/agent-events.ts                 # 可选：AG-UI/npm 的边界
  src/web/
    bindings/ai_stream_bridge.rs        # 仅当需要 npm 时
    components/ai_run_progress.rs       # 示例命名，依照现有模块风格决定
  tools/build-frontend-bundle.sh        # npm ci / Vite / Trunk 产物检查
  tools/frontend-bundle-cache.py        # 纳入 TS 源码、lock、toolchain、构建产物指纹
```

Vite 产物必须由 Trunk 作为**本地静态 module** 引入，并由现有 tar/embed 链路交付；沿用 hyz-things 当前严谨的 `script-src 'self' 'wasm-unsafe-eval'` 与禁内联脚本 CSP，禁止全局 CDN/`unsafe-inline`。Device Hub 的 `window.deviceHub*` 只是可参考的桥接形式，原则上优先选择明确 module 生命周期、避免新增无约束全局变量。

对 `@ag-ui/core` 做一次独立决策：验证其浏览器 bundle、运行时依赖、许可、协议版本、Tree shaking 和产物体积；若仅需少量事件类型且 npm 包带来过量依赖，就先使用 Rust `serde` 类型和本地 TS 轻量 DTO，不强加包。

## 8. 分阶段实施（建议拆为后续功能 PR）

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

### 阶段 C：TS/npm Bridge 工程化（独立 PR 3，可选前置小实验）

- 以 Device Hub Vite+Trunk 模式新增 TS 工程和构建钩子/缓存指纹，先接入一个最小可测 JS 模块。
- 评估并按需接入 `@ag-ui/core`；若引入，则做 TS 事件校验并保留 Rust DTO 的等价测试。
- 保障生产本地静态资源、无外部运行时依赖、CSP、双端构建缓存和更新资源可用。
- 严格限制 Bridge 只负责 JS 生态能力，不让其承担题目业务逻辑。

**验收：** `npm ci`、TS build、Trunk release、前端 bundle 归档/内容指纹、CSP Playwright 全部通过；没有 Node 运行时依赖。

### 阶段 D：回归与上线验收

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
- [ ] TS Bridge（如最终需要）沿用 Device Hub 的构建经验，不破坏 CSP、静态资源嵌入和缓存完整性。
- [ ] Rust 测试、Playwright E2E、前端构建与 CI 通过；无不相关的设备功能/题库数据改动。

## 11. 本 PR 范围

**仅新增这一份计划文档。** 不修改运行代码、依赖、数据库、SDK、题库或设备配置；不执行迁移、不部署、不合并。本 PR 审核通过后，按 `8` 拆分后续实施 PR。

## 参考

- 当前代码：`apps/rust/things/src/adapters/outbound/codex_app_server.rs`、`apps/rust/things/src/adapters/outbound/question_bank_ai.rs`、`apps/rust/things/src/web/pages/question_bank_practice.rs`、`apps/rust/things/tools/build-frontend-bundle.sh`。
- Device Hub：`hyz123a/device-hub/frontend/bundle/index.ts`、`frontend/src/components/xterm_terminal.rs`、`server/src/events/mod.rs`、`frontend/src/components/stream_log_panel.rs`。
- Codex App Server 协议（参考）：https://learn.chatgpt.com/docs/app-server
- AG-UI 事件协议（参考）：https://github.com/ag-ui-protocol/ag-ui/blob/main/docs/concepts/events.mdx
- Axum SSE（参考）：https://docs.rs/axum/latest/axum/response/sse/
