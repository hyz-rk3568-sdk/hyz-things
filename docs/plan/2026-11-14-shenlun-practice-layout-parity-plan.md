# 申论训练：对照专项练习的一次性布局与菜单整改 Plan

**状态：实施及本地验证完成。** Rust 集成测试 10/10、严格 Clippy 与 80 项 Playwright/WebKit 回归通过；五种视口截图已完成对照。PR #125 包含本计划对应的页面、会话隔离与 E2E 实现。
**参照源：** `apps/rust/things/src/web/pages/question_bank_practice.rs`（4100 行）及其依赖；**目标：** `question_bank_shenlun.rs`（替代原 `shenlun_training.rs`）。

## 0. 已阅读并对照的现有实现

- `question_bank_practice.rs`：工作区根节点（约 316 行、3106—3204 行）、题目头和 AI 菜单（580—682 行）、AI 会话状态与删除可用性（843—979 行）、训练状态、计时、导航和滑动（约 1919—3204 行）、目录与历史相关逻辑；其他辅助区域不作为申论业务原样移植的目标。
- `shenlun_training.rs`：当前材料/题目栏、四种模式、菜单、顶栏、答题卡和目录（全文）。
- `training.rs`：训练 shell 与沉浸模式、`FIXED_TRAINING_LAYOUT`、`TrainingWorkspace` 对申论/专项练习的挂载。
- `training_split_workspace.rs`、`split_resize.js`、`frontend/question-bank.css`：分栏 DOM、拖拽比例、CSS 网格断点、手柄尺寸和基于容器 class 的样式。
- `question_header_actions.rs`、`ai_composer.rs`、`shenlun_ai_panel.rs`：复用按钮样式、Composer、OCR/来源答案。
- `e2e/question-bank.spec.ts` 与 `e2e/shenlun.spec.ts`：现有断言的范围；专项练习已经有多个视口的面板/Composer 几何测试，申论仍缺少同级别覆盖。

## 1. 已明确的代码差异与直接复制点

| 项目 | 专项练习已有实现 | 申论现状及整改方式 |
| --- | --- | --- |
| 沉浸式容器和左右留白 | `WORKSPACE = "question-bank-workspace training-workspace fixed inset-0 z-[70] flex h-dvh w-screen flex-col overflow-hidden bg-base-200 px-3 py-2"`，最外层负责视口与外边距 | 当前 `shenlun-workspace` 仅 `flex h-full min-h-0 flex-col gap-2`。在确认训练 shell 的 fixed 上下文后，**直接复用相同的工作区容器契约**；避免双重 fixed 与重复 padding |
| 面板样式 | `question-bank.css` 中 `.question-bank-workspace .question-bank-pane-header` 及特定 `data-testid` 阅读区选择器 | 申论没有 `question-bank-workspace` 根 class；即使复用 `QuestionBankQuestionHeader`，部分既有 CSS 不会生效。统一根 class，同时明确申论材料/题目选择器或共享语义 class，不滥用测试 ID |
| 中间双栏 | `TrainingSplitWorkspace` 使用 3:2 默认比例，分隔把手占 `.5rem`；768px 改左右布局，较窄时上下布局 | 组件已经复用，**不重新实现网格**；只检查外层可用宽度、`min-w-0`、两卡片与分隔条的边界，不用负 margin 硬挤 |
| 顶部与底部 | `TrainingHeader`、`StudyGoalBars`、`CodexLimitBars`、剩余空间主区、`CompactQuestionNavigation` | 复用结构，固定 header/nav 占位和主区 `min-h-0 flex-1 overflow-hidden`；是否添加计时/状态须以实际业务数据为准，不能伪造 |
| 题目工具栏 | `QuestionBankQuestionHeader` + `QuestionHeaderActions` + `QuestionAiHeaderActions`：操作位始终靠右、三点始终存在，`can_delete` 控制删除 | 保留申论的四个功能入口，但把菜单显示和状态生命周期**照抄成熟实现原则**，不再自行根据 `mode=="ai" || mode=="grade"` 直接显示删除 |
| AI 对话 | `QuestionAiReview` 用 `on_delete_availability` 回报是否存在可删除会话；`delete_signal` 驱动删除流程 | 申论未消费 `on_delete_availability`，导致无对话也可点删除；应复用相同能力，并确保切题/切模式后清空状态 |
| 滑动与阅读 | `TrainingSwipeSurface`、双面板滚动、`InkOverlay`、`touch-pan-y` 与多视口测试 | 申论已有滑动组件但滚动 class/涂鸦包装不完整。以专项练习已有机制为准，避免手势抢占 OCR 文件选择或 AI 文本输入 |

**额外注意：** `question-bank.css` 只有 `.question-bank-workspace [data-testid="question-bank-practice-material-scroll"]` 和 `question-bank-practice-question-scroll` 等特定选择器会给阅读区更大内边距。只补 `question-bank-workspace` class 仍不能让申论内容样式完整一致，应提取共同样式语义或显式覆盖申论真实节点。

## 2. 实施内容（按顺序一次性完成，不拆碎修）

### A. 将专项练习的工作区骨架原样对齐

1. 以专项练习 `WORKSPACE` 为唯一外层布局基准。核对训练 shell 在沉浸状态的 positioning，确保 `shenlun-workspace` 使用**同一固定视口规则、左右 padding、背景和 overflow**。确认左、右两侧距离一致，内容不触边。
2. 保留 `TrainingSplitWorkspace` 与原 `split_resize.js`；不另写分栏逻辑。沿用 3:2 网格、手柄与 768px 断点，并验证拖动/刷新后的宽度；原卷材料、长标题、按钮多、AI Composer 上传图片状态下仍不得撑破 `min-w-0`。
3. 直接沿用专项练习的 `TrainingHeader -> 主区 flex-1 -> 底部 CompactQuestionNavigation` 层级。材料栏/题目栏头、圆角、边框和内部滚动区统一按同一套样式。各自滚动、全文不横向溢出、不因切换 AI/OCR 改变左右留白。
4. 申论继续是“原卷复盘”，**不复制交卷/暂停/虚假题量或计时状态**。`InkOverlay` 若复用，需检查可写区域、题目切换持久键及 OCR/AI 输入冲突后再启用。

### B. 照抄成熟 AI 菜单的**状态判断**，修复非 AI 页的删除项

1. **三点菜单仅在非草稿模式常驻，并固定为该模式工具栏最右侧按钮。** 当前 `shenlun_training.rs` 将 `{menu}` 放入 `QuestionBankQuestionHeader.tabs` 末尾，而共享 `QuestionBankQuestionHeader` 在 tabs **后面**追加 `<QuestionInkToggle />`，因此三点不是最右侧；进入 `scratch_open` 草稿模式后，原有 tabs 被切换成草稿工具条，三点消失，此时**应当消失**，不是缺陷。整改时优先复用专项练习 header 的普通模式/草稿模式切换规则：`!scratch_open` 时四类内容页的三点按钮始终渲染、位于涂鸦切换钮右侧且是最后一个操作位；`scratch_open` 时只显示草稿工具条及退出草稿控件，**不出现三点菜单**。如需扩展共享 header 的可选尾部 slot，应确保只在 `!scratch_open` 分支渲染，不能用 CSS 强行盖过切换规则，也不能给专项练习造成重复菜单。
2. **触发器与菜单项分开判断。** 非草稿模式下，不论 `grade/ai/ocr/source`，三点按钮均始终可见；仅当 **当前展示 AI 会话 + 当前会话实际可删除** 才在菜单中显示“删除 AI 对话”，否则展开显示“暂无可用操作”。进入草稿模式时，菜单触发器也随普通工具栏一起隐藏；退出草稿模式时恢复。OCR、来源参考答案、加载/空会话不得出现删除项。
3. 参照 `QuestionAiReview::on_delete_availability` 把可删除状态回传父组件；切题、切模式、删除完成、异步响应返回、退出原卷均要清零或校验状态来源，避免旧题会话的按钮泄漏到新题或非 AI 模式。
4. **后端会话隔离不可遗漏：** 当前 grade/analysis 都指向 `/practice/questions/{id}/ai`，会出现内容交叉。实施时先设计真实 `purpose`/scope 及独立读写/删除（含草稿键和事件流），再允许按各自会话显示和删除。不能只改前端文案就声称隔离完成。将此项纳入同一个代码整改任务的功能验收；如后端隔离未实现，PR 不能宣称已完成。

### C. 将「批改」与「解析/追问」拆成不同页面和真正不同的 Codex Thread

**现状证据（已经核对 main）：**

- `shenlun_training.rs` 362—368 行：`grade`、`ai` 都渲染同一个 `QuestionAiReview`，仅通过 `mode` 改变提示词。组件 `key="{question_id}:{mode}"` 只负责前端重挂载，**不隔离服务器会话**。
- `question_bank_practice.rs` 957 行：两种模式调用同一 URL `/practice/questions/{question_id}/ai`；`follow_key/start_key/request_key` 都未包含 mode，草稿和请求状态也会串。
- `question_bank_ai.rs` 248、274—279 行：持久化 Key 为 `(question_id, stem_fingerprint)`，`question_ai_reviews.thread_id`、messages、requests、events、stream/stop/delete 都用同一 Key。
- 申论 E2E 将共享工作区外观与会话隔离分开验收：解析/批改可复用同一套组件，但各自的 scope、Codex thread、历史/事件、请求和删除状态必须隔离。

**明确目标不是“两个 tab 换标题”**，而是在题目右栏分别进入独立的 **AI 批改页面** 与 **AI 解析及追问页面**，同一题目也有独立持久会话和独立 Codex thread：

1. **AI 批改页面：** 输入申论作答（文字/图片）、启动批改、显示评分和逐点反馈；在此页面追问扣分原因、提交二次作答，**只延续批改 thread**。保留复用的 AI Composer、消息流、进度与停止机制。
2. **AI 解析及追问页面：** 以题目和材料为上下文启动讲解，后续追问继续**解析 thread**；不得自动显示批改历史、批改草稿、评分结果，也不得复用批改上下文。
3. **服务端持久隔离：** 为申论定义并校验稳定 scope（如 `grade` / `analysis`），贯穿 GET/POST、follow-up、附件、history/runs/events、SSE/流、stop、retry、delete 和 Codex thread 恢复。设计时检查现有 SQLite 表的主键/索引/外键与历史记录迁移；不可仅在前端添加 query 参数、伪造两个页面或拼接未校验 key。其他科目的默认 analysis API 必须向后兼容。
4. **删除与切换：** 删除按钮由**当前页面 + 当前题目 + 当前 scope 的实际会话状态**共同决定；删除批改不会删除解析，反之亦然；切模式、切题、刷新、恢复未完成任务均只读取正确 scope。独立的客户端草稿、request-id、上传进度和过期附件状态也要按 scope 隔离。
5. **线程验收：** 启动同题批改和解析后，服务端存储的两个非空 `thread_id` 必须不同；批改追问沿用批改 ID、解析追问沿用解析 ID。交叉查询、历史列表、SSE、停止、删除都不能访问另一 scope；错误/重试及并发场景同样验证。

### D. OCR 必须呈现共享 AI Composer 的完整交互，而不是只嵌入一个上传控件

**现状证据（已经核对 main）：** `shenlun_ai_panel.rs` 157—179 行**确实调用了现有 `ai_composer(AiComposer { ... })`**；`read_only=true`、`max_length=0`、共享 `AiImagePicker` 和发送箭头已经存在。但 OCR 目前被包在普通滚动内容下的 `section.mt-3.space-y-3`，结果、工具活动和 Composer 顺序排列，尚未达到专项练习 AI 面板的**固定底部 Composer + 上方独立可滚动消息区**结构。

1. 继续直接复用 `ai_composer`、`AiImagePicker`、图片预览/移除、上传状态与已有发送箭头；**不新造选择文件按钮或第二个“转写”按钮**。文字框保持只读，不接受文字提交；无图片时发送禁用。
2. OCR 页面与批改、解析是独立视图。顶部展示题干与必要说明，中间单独滚动显示 OCR 结果及工具过程，**Composer 固定在右栏底部**；软键盘出现、窄屏、iPad 横竖屏及长 Markdown 时不挤出屏幕。不将 OCR 结果自动写入批改答卷。
3. 图片附件可提交一次临时 OCR 请求，由独立临时 Codex thread 生成 Markdown；不污染批改或解析的持久 thread。核对 `/shenlun/questions/{id}/ocr`、附件上传端点、CSRF、错误恢复/重复提交、防误触和真实 WebKit 图片上传行为，而不仅是 mocked 返回值。
4. 验收：图片选择→缩略图→发送→工具活动/Markdown→清空已上传图片；只读和禁用状态；OCR 结果不创建批改记录；切到批改/解析时各自 thread 和草稿保持独立。

### E. 复制成熟 E2E 的**几何验收方式**（不是只断言“元素可见”）

在 `e2e/shenlun.spec.ts` 中仅新增相关用例，参考 `e2e/question-bank.spec.ts` 的 `boundingBox`、`toHaveCSS` 与无横向溢出断言；不需要全量测试。

- 同一浏览器和视口比较两个页面：**1366×900、1133×744、1024×768、768×1024（iPad 竖屏）、390×844**；至少覆盖 iPad 横竖屏和窄屏。
- 左右外边距：`materials.x >= viewport safe inset`、`question.right <= viewport - safe inset`，且与专项练习同视口工作区容器的边界差不超过 1—2 CSS px；没有横向溢出。
- 宽度：>=768px 面板按共享分栏规则左右显示；<768px 上下显示，面板不相互覆盖。手柄可拖拽且比例可恢复默认；两个面板的标题栏高度与纵向对齐误差 <=1 CSS px（同专项练习现有测试）。
- 上下：header、内容、底部答题卡互不重叠；材料和 AI 解析超长时只有各自面板滚动；Composer 留在右栏底部，发送按钮不出边界。切换四模式、展开/关闭菜单后再次测量。
- 菜单：`scratch_open=false` 下四种模式分别验证三点按钮始终 `toBeVisible()` 且 `toHaveCount(1)`，并用 `boundingBox` 确认三点处于题目工具栏**最后一位**、在 `QuestionInkToggle` 右侧、没有超出面板；打开草稿模式后验证三点 `toHaveCount(0)`，退出草稿后恢复。空 AI 会话、已有可删会话、切题、切模式、删除后、异步响应晚到时再验证 **仅在对应有效 AI 会话显示删除项**；实际删除不会影响另一题或另一 scope。
- 对关键视口保留申论与专项练习的**并排截图基准**供人工视觉检查（留白、分栏、页头、按钮、底栏）。CI 绿但截图未验收，不能说“布局已对齐”。

## 3. 验收与提交约束

- 目标是“一次性整改”，不再按失败断言打补丁。先确认所有差异，再由同一后续实现 PR 一并修正。
- E2E 只跑申论相关用例；CSS/共用组件改动如影响专项练习，另外跑对应的**专项练习相关测试**，而不是盲目全量运行。
- Rust format / clippy / targeted tests 与 Playwright 通过，**还需 iPad 几何和截图对比**；两项都合格才可称为完成。
- **非草稿模式三点按钮常驻最右，草稿模式按专项练习逻辑隐藏三点**；按钮常驻与删除项有条件出现是两项独立阻断验收条件。
- **批改页与解析/追问页拥有不同 Codex thread**、OCR 共享 AI Composer 并固定底部、四模式菜单不串状态，均为阻断验收条件；任何一个未满足都不能宣布“已全部修复”。
- 本计划已用于 PR #125 的代码实现；最终完成状态以相关 Rust 检查、WebKit 多视口截图验收、PR 合并和网络 ADB Things 部署的实际结果为准。

## 4. 2026-11-14 最终验收记录

- PR #125 已合并到 `main`，merge commit 为 `d046895a1ef7d8b7b0200f3e8329602a4a0d3782`，源码树为 `c562a9823ed968d5e531d5dc5243fc97462c0d76`。代码提交带有 `[skip ci]`。
- 本地 `cargo fmt --check`、完整 Things native 测试、E2E feature 严格 Clippy、TypeScript 类型检查及 `make check-static` 通过。最后一轮相关 WebKit 回归 11/11、OCR 真实附件上传 1/1、默认/指定 spec fixture 选择验证 2/2 通过；申论布局覆盖 1366×900、1133×744、1024×768、768×1024 和 390×844。`make check-static` 中依赖设备 SDK 的检查因候选仓库未包含 `sdk/` 而跳过。
- GitHub 自动生成的 PR #125 merge commit 未继承源提交的 `[skip ci]`，因此 `Portal CI` 的 push workflow 启动。为遵循跳过 CI 的请求，运行随后被取消；本地日志显示聚合 E2E 检查因 Playwright shards 被取消而失败，不能记录为 CI 通过。静态/格式/单元测试/Clippy 和快速检查在取消前成功。
- 使用 Buildroot AArch64 linker 从上述合并源码构建的 Things ELF，其 SHA-256 为 `a477120622850da817d06fca7b7df2100b92a934d9fabc921846030ea5036869`。网络 ADB 热部署后 `/run/hyz-things/ready` 内容为 `8080`，设备 ELF 与主机 SHA-256 一致；HTTPS 门户、生成的 JavaScript 与 WebAssembly 均返回 200。Router readiness、ELF SHA-256 和进程 ID 与部署前一致。
