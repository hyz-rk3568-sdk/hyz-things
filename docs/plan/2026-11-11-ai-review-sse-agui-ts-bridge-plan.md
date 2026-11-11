# AI 解析实时对话、Agent 事件与 Vite/TypeScript 一体化实施计划

日期：2026-11-11  
仓库：`hyz-rk3568-sdk/hyz-things`  
目标分支：`main`  
交付方式：**全部功能在 PR #110 中完成、测试、验收；不拆分后续功能 PR。**  
当前状态：**实施计划，尚未修改业务代码或部署。**

## 1. 产品目标与一次性交付范围

把现有“提交 AI 解析 → 等待 → 展示完整结果 → 追问”的流程升级为日常可用的**实时、连续、多轮对话**。这项功能是本次 PR 的核心，而不是一个附属的技术试验。

完成时必须同时交付：

1. **正式的 Vite + TypeScript 前端基础设施**：复用现有 npm 工作区和锁文件，建立稳定的 Rust/Yew/WASM ↔ TS 接入点，以一个实际浏览器库验证全链路；后续引入 npm 包不再临时写散落脚本。
2. **重新组织 AI 解析与追问聊天**：独立滚动消息区、可持续编辑的底部输入框、每题草稿、发送状态、图片附件、停止/重试、正确的焦点与滚动行为。
3. **真实的 Rust SSE 流式通道**：将 PC Codex App Server 已收到的执行事件经过筛选、脱敏，推送到浏览器，展示工具调用、运行阶段和答案增量。
4. **端到端集成测试**：Rust 单元/服务端 HTTP 测试、前端构建/CSP 校验、Playwright 长对话与断线/切题/附件测试。

**架构决定**：保留 Rust Axum + Yew/WASM + SQLite；Vite/TS 负责 npm 库与浏览器 DOM 能力，不替换 Yew 的业务状态管理，不引入 React/Vue，不运行设备端 Node/Bun。**直接将 AI 运行时进度从 1.5 秒轮询切换到 SSE**，不保留双轨方案。断线后的 snapshot/事件重同步是 SSE 正常流程，不是第二套传输方式。

不在本 PR 中迁移其他页面到 TS、不替换全站 Markdown 引擎、不修改题库原文或练习答案、不执行固件/设备部署，也不扩展到通用多 Agent 平台。

## 2. 以当前源码为基准的已知事实

| 文件 | 当前实现 | 本次改动 |
| --- | --- | --- |
| `apps/rust/things/package.json`、`package-lock.json` | 已有 Node 20、Tailwind、KaTeX、Playwright；无 Vite/TS | 在**同一 npm 工作区**增加 Vite、TypeScript 和首个实际浏览器库 |
| `frontend/index.html`、`tools/build-frontend-bundle.sh`、`tools/frontend-bundle-cache.py` | Trunk 构建 WASM；前端 tar 嵌入原生 Rust；严格 CSP 与输入指纹 | 将 TS bundle 纳入确定性构建、嵌入和缓存逻辑 |
| `src/web/pages/study_markdown.rs`、`src/web/ai_diagram.js` | `pulldown-cmark`、KaTeX、安全 SVG 和本地题库图片规则 | 保留统一 Markdown 解析与安全规则，增强未完成文本的流式展示 |
| `src/adapters/outbound/codex_app_server.rs` | 原生 WebSocket/JSON-RPC；当前只收集完成的 `agentMessage`；解析 Turn 的 `outputSchema` 把 Markdown 放进 JSON `answer` | 为 AI 解析暴露实际通知和 Markdown delta，最后消息仍为权威值 |
| `src/application/question_bank/practice.rs` | `AiReviewBackend` / `AiReviewTurn` 抽象 | 扩展类型化事件 sink，便于模拟 Codex 做单元测试 |
| `src/adapters/outbound/question_bank_ai.rs` | 后台生成、`watch` 停止、SQLite 持久消息、按 `(question_id, stem_fingerprint)` 隔离；`MAX_ACTIVE=2` | 为每个 Turn 分配 `run_id`，内存事件快照与有界广播，保留服务端任务所有权 |
| `src/adapters/inbound/http/question_bank.rs`、`mod.rs` | `GET/POST/DELETE /practice/questions/{id}/ai` 与 `/ai/messages`、`/ai/stop`、`/ai/attachments` | 在同资源下增加只读 SSE 端点，执行仍由现有受控 POST 触发 |
| `src/web/pages/question_bank_practice.rs`、`collection_review.rs` | `QuestionAiReview` 挂在 AI 标签；1.5 秒轮询；发送区排在消息末尾；生成中追问框消失；草稿存在组件本地 | 提取聊天组件及稳定状态，统一任务/消息/草稿/附件/滚动逻辑 |

**数据身份**：题目对话以 `question_id + stem_fingerprint` 为后端身份边界；不能沿用早期文档按 `session_id` 隔离的旧设计。出题等其他 Codex 使用场景仍走其当前各自的接口，本次只修改 AI 解析通路。

## 3. 最终架构与所有权

```text
PC：Codex App Server（WebSocket / JSON-RPC）
            │ 当前 Thread + Turn 的真实通知
            ▼
RK3568：codex_app_server.rs
            │ 过滤归属、筛选事件、限制大小、脱敏
            ▼
      AI Review Event Mapper
            │ run_id / seq / item_id
            ▼
      QuestionBankAiService
            ├── 现有后台任务 / 停止 / 追问 / SQLite 最终消息
            └── 每个 Run 的有界事件缓存 + broadcast
                                      │
                        Axum GET .../ai/events
                                      │ text/event-stream
                                      ▼
浏览器：Yew/WASM 单一聊天状态机
            ├── History & Run snapshot（初次进入和重连）
            ├── SSE 工具事件 / Markdown delta / 终态
            ├── 独立消息列表与底部 Composer
            └── Vite/TS browser bridge（npm 库与 DOM 增强）
```

所有业务状态（题目、Thread、Turn、run、消息、发送、草稿）均有唯一责任方：**Rust 后端拥有任务与落库，Yew 负责当前页面视图状态，TS Bridge 不再维护第二份聊天状态。**

## 4. Vite + TypeScript：现在就接入标准基础设施

### 4.1 目录和依赖

```text
apps/rust/things/
├── package.json                   # 增加 build:ts / typecheck:ts
├── package-lock.json              # 继续只有一个 npm 锁文件
├── frontend/
│   ├── index.html                 # 注册本地 TS module
│   ├── ts/
│   │   ├── vite.config.ts
│   │   ├── tsconfig.json
│   │   └── src/
│   │       ├── index.ts           # 唯一 bridge 入口与版本化 API
│   │       └── floating-menu.ts   # 首个实际 npm 组件
│   └── ts-dist/                   # 构建产物；gitignore 且不参与输入指纹
├── src/web/
│   └── bindings/browser_bridge.rs
└── tools/
    ├── build-frontend-bundle.sh
    └── frontend-bundle-cache.py
```

依赖：`vite`、`typescript`（开发依赖），`@floating-ui/dom`（运行依赖）。第一处业务落地点是 AI 回复“⋮”操作菜单，调用 `computePosition / flip / shift / autoUpdate`，验证滚动容器、窄屏边界和组件释放；**不要因为要引入 TS 就替换现有菜单状态机**。

- 统一的、有类型约束的 `hyzThingsUi` 浏览器入口；`wasm-bindgen` 通过一个清晰、版本化、可测试的桥接模块访问，处理尚未初始化、异常返回和资源释放。避免多个页面各自创建全局方法。
- 模块由同源静态产物加载，验证 Vite ES Module 与 Trunk 的装载先后顺序；必要时在桥接层显式等待就绪，不把依赖初始化竞态散落在组件内部。
- 构建流程在 `trunk build` 前运行 `npm run build:ts`；`ts-dist` 被 Trunk 打进现有发布 tar。缓存指纹包含 TS 源文件、配置、`package-lock.json` 和脚本，**必须排除**生成的 `ts-dist`，防止产物反向污染源指纹；验证输出文件真实存在。
- 保持既有 `script-src 'self' 'wasm-unsafe-eval'` 等 CSP；不新增外部 CDN、内联脚本、`unsafe-inline` 或设备端 JS 服务。
- 第二个 TS 模块只需增加 npm 依赖、实现并导出桥接接口和测试；不再修改 Rust 打包总链路。后续是否使用 `@ag-ui/core` 以实际 SSE 协议/事件映射需求决定，不能为了依赖数量而引入。
- **Markdown** 继续由 `pulldown-cmark`、KaTeX 和现有安全 SVG 机制处理；本 PR 不额外引入 `marked` / `markdown-it`，避免两套 HTML/图片/数学语义。

**本节验收**：`npm ci`、TS 类型检查、Vite build、Trunk release、静态资源检查与 CSP E2E 通过；实际浮层成功定位；关闭/卸载后 `autoUpdate` 无残留监听器；无 Node 运行时依赖。

## 5. 后端：Codex 真实事件 → AI Run → SSE

### 5.1 事件模型

定义服务端类型 `AiReviewStreamEventV1`，外发最少包含：

```json
{
  "version": 1,
  "run_id": "opaque-run-id",
  "seq": 12,
  "kind": "tool_started",
  "timestamp_ms": 1794370000000,
  "payload": {
    "item_id": "opaque-item",
    "label": "读取题目相关资料"
  }
}
```

对外事件类型：

| kind | 含义 |
| --- | --- |
| `run_started` | 服务端已创建此轮任务 |
| `phase_changed` | 排队、连接、执行、生成正文等真实阶段 |
| `tool_started` / `tool_finished` | 收到匹配本轮的工具开始/结束通知；只输出允许公开的类型与简短状态 |
| `reasoning_summary_delta` | 仅当 Codex 实际提供公开摘要才输出；无摘要则仅显示“正在思考”状态 |
| `text_delta` | 已验证为 Markdown 正文的增量 |
| `answer_final` | 最终、完整、规范化 Markdown；用于覆盖流式草稿 |
| `run_finished` / `run_failed` / `run_stopped` | 与服务端持久化结果一致的终态 |

映射依据：`turn/started`、`item/started`、`item/completed`、`item/agentMessage/delta`、`item/reasoning/summaryTextDelta`、`turn/completed` 等 Codex 通知；**具体字段名与格式必须用实际协议 fixture 测试确认**。只消费当前 `thread_id + turn_id` 对应通知，工具使用 `item_id` 配对；不把初始化消息、ACK、其他 Turn、未知或迟到的包当作本次事件。

**安全边界**：不暴露原始内部推理、原始命令/工具参数、环境变量、Token、PC 路径、图片临时路径或未过滤的工具输出；只发送白名单工具类别与脱敏标签。未收到的工具事件不能虚构为已执行。Codex 线程的 `danger-full-access` 是当前实现事实，保持服务端隔离和最小对外披露，不通过浏览器提供新的远端执行入口。

### 5.2 Markdown 不再封装为 JSON 字符串

当前 AI 解析的 `outputSchema = { answer: string }` 使 `agentMessage/delta` 可能是 JSON 转义片段，不能直接显示。

**本 PR 明确改为：AI 解析专用 Turn 输出纯 Markdown（不传 `outputSchema`），服务端逐片段校验、转发，并以最终 `agentMessage` 做完整性和长度校验。** 仅此通路移除 JSON 包装要求；其余需要结构化输出的 Codex 调用保留原有格式。更新 `AI_REVIEW_ENVELOPE` 与相关 Rust tests。

协议演进、输出异常和安全情况要在 mock App Server fixture 中覆盖；只允许被确认为 agent 正文的 delta 进入聊天区。**不设计 JSON 字符串流的第二套解码器，也不保留 AI 解析旧输出格式双轨。**

### 5.3 SSE 路由、顺序与持久化

新增 `GET /api/v1/question-bank/practice/questions/{id}/ai/events`：

- 同现有 `ai_read` 的敏感读取授权、题目权限和版本检查；同源 Cookie 认证，URL 不放 access token。生成、追问、停止仍使用后端授权与 CSRF 校验的 POST。
- `Content-Type: text/event-stream`；禁缓存/代理缓冲，心跳持续连接；每条事件带 `id: run_id:seq`，`data` 为上述 JSON。浏览器用标准 `EventSource` / `web_sys` 消费，不强行依赖 TS 事件客户端。
- 每个 Run 有独立 `run_id`、连续 `seq`、有界事件环形缓存和 `broadcast`。连接时得到**原子一致的 snapshot + 之后的事件**，断线利用 `Last-Event-ID` 与当前 run 快照重同步；重复序号去重，落后的订阅者拿最新快照，不持有无限缓存。
- 初次进入页面使用已有 `GET /ai` 一次性加载**持久历史**；进度更新**仅由 SSE** 驱动，不再运行 1500ms GET 轮询。服务重启后清理僵尸 `running`，前端通过新的服务端状态呈现可重试，不重建不存在的 Run。
- `question_ai_messages` 继续只写最终消息，流式草稿只在内存；成功时先完成服务端持久化，再发 `answer_final` 和终态。错误/停止不把不完整 Markdown 写成最终答案。
- 用户离开页面/切题/断线仅关闭 SSE；后台任务继续。只有“停止生成”走 `turn/interrupt`；停止、删除、追问与当前 `active` 任务锁遵守单题单 Turn。

**本节验收**：两题并发不串流；工具开始/结束准确配对；断线后快照顺序确定，终态和 SQLite 一致；断开 SSE 不触发中断；无权限和未交卷读取被拒绝；慢订阅者不会拖垮服务端。

## 6. 前端：把 AI 解析改成真正的聊天工作区

### 6.1 独立组件与状态

不继续向 `question_bank_practice.rs` 大文件追加完整的 SSE/Composer/滚动实现。建议抽出：

```text
src/web/
├── components/
│   └── ai_review/
│       ├── mod.rs               # Chat state / actions / keyed identity
│       ├── stream.rs            # EventSource + snapshot + seq 去重 + cleanup
│       ├── messages.rs          # 用户/助手消息、pending、流式正文
│       ├── composer.rs          # 文本、图片、粘贴、快捷键、发送状态
│       └── progress.rs          # Tool / phase timeline
└── bindings/browser_bridge.rs   # 只负责 npm/DOM 能力
```

两个已有复盘入口均复用同一个 `QuestionAiReview`，保持题目与 AI 对话的视觉边界，避免两份实现。

**消息状态机**：`idle → submitting → running → complete/failed/stopped`；用户点击发送后立即显示 `pending` 气泡，服务端确认后变成 `sent`，失败显示 `failed` 并保留内容/重试入口。使用客户端请求 ID + 服务端幂等键或等价约束避免响应延迟、重复点击和重连导致重复 Turn。**运行中允许编写下一条草稿，但不能同时启动第二个 Turn。**

### 6.2 聊天布局与滚动

- 聊天消息容器独立滚动；Composer 停靠在聊天区域底部，始终可达；考虑桌面/窄屏、`env(safe-area-inset-bottom)`、iOS 软键盘和复盘页面现有双栏/滑动手势。不得用整屏固定输入框盖住题目、材料或导航。
- 靠近底部才自动跟随 Markdown delta；用户主动向上读历史时暂停自动跟随，出现“有新内容 / 回到底部”入口。新追问成功发送时回到该轮，滚动调整遵守已有题目复盘滚动记忆，不相互抢夺位置。
- 生成过程可折叠，与正文独立呈现；工具进度按真实事件更新。工具事件不伪装成 assistant 回复，不反复把整个聊天滚动到末尾。
- Markdown delta 按短时间批次（例如 50–100ms）聚合；不完整 `$` / `$$`、代码围栏、表格和 SVG 以安全文本暂显、在语法完整或最终化后使用当前 Markdown / KaTeX / SVG 路径渲染；不可把不可信内容直接塞给 `innerHTML`。

### 6.3 草稿、图片与键盘

- 文字草稿按题目唯一身份保存到浏览器当前会话，切换 AI/原解析、题目与返回复盘时可以恢复；不可把 A 题草稿发到 B 题。图片仍由受控上传接口生成临时 ID，上传状态/失败/过期须明确展示；刷新后无法确认有效的临时附件要提醒重新上传。
- 点击发送和 `Ctrl/⌘ + Enter` 使用**同一校验入口**：发送中、模型运行中、图片上传中、内容超长或 IME 组合输入期间均不能误发；上传进行中禁止发送是当前已确认的缺陷修复。
- 支持**仅上传图片的追问**：至少一张附件即可发送（空文本允许），后端要求“文本非空或有有效图片”，并构造明确的图片分析提示；不可接受既无文字又无附件的空请求。
- 失败的**当前轮重试**与**从头重新解析**是两个不同操作：不得把“追问失败”错误地变成重放整题初始请求；保留已有消息与 Codex Thread 的业务上下文。
- 保留“开始 AI 解析需要用户明确点击”原则；订阅 SSE、恢复历史或切题均不自动触发 Codex 额度消耗。删除确认仍单独处理。

**本节验收**：长文本不再需要滚到历史底部才能输入；草稿切题不丢；图片上传时键盘快捷键不能发出；图片-only 正常；发送/生成/停止状态可辨认；上滚不会被流式输出抢走位置。

## 7. 统一实施顺序——一个 PR，内部按顺序提交

所有代码、测试、文档都在 **PR #110 同一分支**完成。顺序只用于降低风险和安排提交，不创建其他功能 PR。

1. **构建基础设施**：Vite/TS/npm + `@floating-ui/dom`、Rust Bridge、Trunk 静态 module、锁文件/缓存指纹/CSP tests。
2. **Codex 与服务端事件**：AI 解析纯 Markdown delta、`AiReviewStreamEventV1`、类型化 sink、`run_id`、有界广播、SSE 路由、权限与顺序测试。
3. **聊天 UI 重构**：组件拆分、底部 Composer、草稿、即时消息、图片-only、滚动、点击与快捷键统一校验、失败/停止语义。
4. **端到端接线**：Yew EventSource 订阅、run snapshot/重连、执行过程时间线、流式正文、Math/SVG 完整渲染和最终持久结果同步。
5. **集成验收**：Rust tests、TS typecheck/build、Trunk release、资源缓存检查、CSP、Playwright；修完失败项后再 review、合并。**不进行设备部署或主分支提前合并。**

## 8. 必须覆盖的测试与验收场景

| 维度 | 必测场景 |
| --- | --- |
| 构建 | `npm ci`、TS 类型检查、Vite + Trunk release、静态 tar 内容、CSP、构建缓存输入与产物一致性 |
| Codex | 真实字段 fixture、Tool 成功/失败、多 item、无 delta、超长输出、停止/超时、远端断连、错误 JSON-RPC、非本轮消息被忽略 |
| SSE | 仅指定题目可订阅、两题并发、`Last-Event-ID`、快照与广播边界、重复/乱序、缓冲溢出、心跳、服务重启、终态事件 |
| Markdown | 中文、表格、反引号围栏、未闭合公式、`\\(\\)`/`\\[\\]`、KaTeX、代码块、危险 HTML、允许的 SVG/不允许的 SVG、图片链接 |
| 消息 | 首次解析、追问、二次追问、即时 pending、失败重试、重复点击/请求幂等、最终与数据库一致 |
| 编辑 | 输入法组合、Ctrl/⌘+Enter、空文本、图片-only、多图、上传失败/超限、上传中误发、切题草稿与附件归属 |
| 交互 | 超长对话、底部输入框、滚动锁定/返回最新、原解析标签切换、离开页面不停止、显式停止、删除确认 |
| 环境 | 桌面、390px 手机、iOS Safari 软键盘、PWA 回前台、弱网/短暂断线、前端资源重新加载 |

E2E 优先扩展已有 `apps/rust/things/e2e/question-bank.spec.ts` 和相关题库复盘测试，使用模拟 Codex 通知及可控 SSE 夹具，不调用真实付费 Codex。实际 Codex 联调作为最后验收项，必须核实增量事件确实存在，不得用假的工具步骤或假的逐字动画充当成功。

## 9. 完成定义（DoD）

- [ ] Vite/TS 已作为正式构建链的一部分，至少一个真实 npm 库在 Yew 页面工作，Bridge 有明确生命周期，不产生第二套聊天状态。
- [ ] 一个 PR 同时包含聊天交互修复、真实 SSE、Agent 执行过程和 Markdown 增量；不是只有计划或半套功能。
- [ ] 用户发起解析/追问后能立即看到发送反馈、真实阶段与工具事件，并持续看到正文增长；最终完整内容与 SQLite 一致。
- [ ] 聊天输入框始终可达，草稿按题恢复，图片上传与快捷键可靠，支持图片-only。
- [ ] 长对话滚动不抖、不抢焦点；工具过程独立可折叠；桌面与移动端可以正常完成追问。
- [ ] 切页/断开 SSE 不取消任务，重连能得到当前 Run 快照；停止明确终止服务端 Turn；不跨题、跨 Turn 串消息。
- [ ] 不泄露未交卷答案、内部原始推理、私有命令或文件路径；现有授权和 CSRF 约束不减弱。
- [ ] Rust、TS、Trunk、CSP、Playwright 的相关检查全部通过；PR review/CI 完成后才合并。

**当前提交仅重写计划文档并更新 PR 描述；上述复选框须由后续在同一 PR 中的代码提交逐项达成。**

## 10. Flashcard 入口：先定交互，后改代码

### 10.1 归属与位置

“创建 Flashcard”是**当前题目的操作**，不属于 AI 消息发送，也不等于“把这条 AI 回复存为卡片”。当前两处入口位于解析内容末尾：`question_bank_practice.rs`（`question-bank-practice-create-flashcard` / `question-bank-practice-manage-flashcard`）和 `collection_review.rs`（`collection-review-create-flashcard` / `collection-review-manage-flashcard`）。

本 PR 确定将它们统一放到 **`QuestionBankQuestionHeader` 的题目操作区，紧邻已有“☆ / ★ 收藏”按钮**。不移动训练导航和手写工具；不把 Flashcard 创建放进 AI Composer，也不在每条 AI 回复下增加创建入口。原有编辑工作区/资格查询/写入接口与卡片正文编辑流程不变。

### 10.2 只占一个图标位，准确区分状态

使用一致的“**两张叠放卡片**”图标（Lucide `layers-2`，或产品图标集中同语义的卡片图标），用右上角微标记区分状态；**不使用只有“+”而不带卡片含义的通用图标**：

| 资格与已有关联卡片 | 顶部入口 | 点击行为 | 无障碍文字 |
| --- | --- | --- | --- |
| `existing.length == 0` 且可创建 | 叠卡图标 + 小“+” | 打开**创建 Flashcard** 编辑工作区 | “为当前题目创建 Flashcard” |
| `existing.length >= 1` | 同一个叠卡图标 + **数量角标**（`1`、`2`、`9+`） | 打开**管理当前题目 Flashcard** 工作区；提供查看/修改/删除，允许创建更多时由工作区决定 | “管理当前题目的 N 张 Flashcard” |
| 无关联卡且不可创建 | 不显示可点击入口；解析区保留原有不可创建原因提示 | 不操作 | 原有原因说明可被读屏获取 |
| 资格/关联卡查询中 | 不显示可点入口，待加载确定后再呈现 | 不操作 | 不宣称已创建 |

判断**已创建优先于可创建**：即使还可以继续新建，已有卡也必须显示数量与管理入口，不能误显示为“从未创建”。用户操作只进入原有 Flashcard 工作区，删除必须在工作区二次确认；点击图标不直接删除任何卡片。

图标按钮保持手指可点的至少 36–40px 点击区域；移动端只显示图标与角标，不挤压题目标题。完整文字通过 `aria-label` / `title` 提供，颜色不是唯一状态区分方式。保留现有 `data-testid` 以便改写后的 E2E 精确验证“创建”与“管理”状态，不依赖展示文字。

### 10.3 与聊天优化的关系

聊天区固定职责为：历史消息、流式 Markdown、Agent 真实工具时间线、图片、草稿、发送、停止与重试。**Flashcard 是题目级动作，不与聊天输入框争夺底部位置**；原始解析/AI 解析标签维持本轮既定范围，不做全站学习助手、新建会话或上下文锁定。

### 10.4 本项验收

- [ ] 题目训练复盘与专项收藏复盘显示统一卡片图标；解析区末尾不再重复显示创建/管理按钮。
- [ ] 无卡显示 `+`；已有一张/多张显示准确数量；已有卡时能进入修改/删除，仍允许在资格许可时继续创建。
- [ ] 不可创建时保留原因；源文件变化和资格限制沿用已有服务端约束。
- [ ] iPhone 宽度下不遮挡题目、收藏、滚动或手写操作；键盘导航、读屏标签和点击目标符合可用性要求。
- [ ] 保留既有 Flashcard 数据与写入流程，E2E 同时验证两处页面的按钮状态和打开工作区行为。

**实施约束：本节仅确定设计，不代表已修改任何 Flashcard UI 代码。先完成计划评审，再在 PR #110 同一分支实施并验证。**
