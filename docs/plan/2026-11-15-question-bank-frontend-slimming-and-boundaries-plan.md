# Question Bank 前端瘦身与模块边界重构 Plan

> 状态：**提案 / 待审阅**。本 PR **只提交此 Plan，不修改业务代码、SQL、测试或部署**。
> 日期：2026-11-15；审查基线：`main@d54aa6b94e4eadfa4d4cd1b6263586fe4c22162c`。
> 原则：**以当前已实现的代码、HTTP 路由、E2E 所表达的最新产品行为为准**；历史 Plan 仅供追溯，不作为新增功能或兼容要求。

## 1. 目标、非目标和关键决策

**目标**

1. 纠正 `pages/` 目录中“页面入口”和“训练业务组件”混放的问题，让文件名直接反映职责。
2. 将行测与申论统一放在一个 `question_bank` 前端业务模块下。申论是科目（`subject=shenlun`），不是与 Question Bank 平级的产品模块；归纳概括等才是具体题型。
3. 在**不改变当前用户体验**前提下，拆分单体 `QuestionAiReview`、解耦手写板定位、用强类型表示申论 UI 模式。
4. 审计并删除旧 Plan 遗留、现行流程不再使用的申论 Attempt/Job/确认转写实现及过期测试模拟，减少实际代码，而非仅搬文件。
5. 保留未来 React + TypeScript 可按同一业务模块边界逐块迁移的可能性；优先稳定 HTTP、数据 DTO 和持久化契约。

**非目标（本轮不做）**

- 不引入 React、Next.js、完整 Feature-Sliced Design、全局状态管理或第二套申论业务模型。
- 不新增正式申论考试 Session、交卷、二稿对比、结构化打分引擎、OCR 确认流。
- 不改变 AI Markdown 报告内容、既有 Skill 提示词、Codex 权限或用户可查看来源答案的现行规则。
- 不因“追求小文件”拆出大量一行转发包装器；不对整个 `web/` 一次性批量改名。

**架构决策**

- `pages/`：仅负责实际路由（如 `StudyView::Training`）对应的页面外壳、导航与装配。一个界面被切换展示，不等于它是独立 Page。
- `web/question_bank/`：管理行测、申论共享的题库前端功能，按行为差异保留专用组件，不再建顶层 `shenlun/` 或 `xingce/`。
- `web/components/`：只存放没有题库专属含义、在其他模块也复用的通用 UI。
- `src/domain/question_bank.rs`：继续存业务 DTO / 可纯测试的规则，**不放 `ShenlunMode` 这类界面标签状态**。`application/` 与 `adapters/` 保持后端职责。
- 目录只是依赖边界：不因为搬文件就改变 HTTP 路径、持久化键、已有 `data-testid` 或对外组件行为。

## 2. 当前基线与具体问题

| 文件 / 层 | 已确认现状 | 需要的动作 |
| --- | --- | --- |
| `web/pages/question_bank_practice.rs` | 约 4,153 行；`QuestionAiReview` 单组件约 1,076 行；还承载题库练习、共享收藏与题目头 | 拆业务复盘逻辑，迁入 `question_bank/`；保留练习行为 |
| `web/pages/question_bank_shenlun.rs` | 约 549 行，`QuestionBankShenlun` 管理目录、选题、模式、收藏、Flashcard、材料及右栏 | 作为 `question_bank/shenlun.rs`，保留单个工作区、不额外造 Page |
| `web/pages/shenlun_ai_panel.rs` | OCR 面板与来源答案面板共处一文件 | 分别命名为 `ocr_panel.rs`、`source_answer_panel.rs` |
| `web/pages/ai_review_activity.rs` | 管理 `/runs`、`/runs/{id}/events` 历史、实时事件合并、聊天消息及工具轨迹 | 命名为 `ai_review_timeline.rs` 更准确；保留必要的加载/分页逻辑 |
| `web/pages/ai_review_stream.rs` | SSE 订阅和重连 Hook，名称准确 | 迁入题库模块，名称可不变 |
| `web/pages/ai_composer.rs` | 供题库 AI、申论 OCR 复用的输入与图片 UI | 迁入题库模块，名称可不变 |
| `web/pages/scratchpad.rs` | 约 1,063 行；`pane_selector` 用多个跨页面的 `data-testid` 决定生产书写目标 | 抽离语义定位；之后再决定是否移动到通用 `web/ink` |
| `web/pages/training.rs` | 路由训练外壳与 Flashcard 练习实现同处约 1,320 行 | 保留真正页面入口；Flashcard 拆分列为后续可选项目，不扩大本轮范围 |
| `web/pages/mod.rs` | `mod` 与大量 `use ...::*` 混用，存在隐式共享导入 | 改为显式小范围导入 / 导出，避免循环和跨页面依赖 |

**跨模块复用必须保留**

- `QuestionAiReview` 还被 `question_bank_shenlun.rs`、`collection_review.rs` 使用；不是“行测私有”组件。
- `QuestionBankQuestionHeader` / `use_practice_favorite` 也被其他题库/收藏工作区复用。
- `tool_execution_item()` 除 AI 时间线和 OCR 外，还被 `generation_activity.rs` 使用；不要直接设为题库私有后让其他模块反向依赖 Question Bank。
- 目前 `ai_review_activity.rs` 用 `super::question_bank_practice::ai_scope_url` 构造 URL；必须先把 scope URL 辅助函数提到题库 AI 共享层。

## 3. 推荐目标结构（是职责图，不是立即强制创建的所有文件）

```text
apps/rust/things/src/
├── domain/
│   └── question_bank.rs       # 跨层 DTO 与业务规则，保留
├── application/question_bank/ # 后端用例，保留
├── adapters/                 # HTTP、SQLite、Codex，保留
└── web/
    ├── pages/
    │   ├── study.rs          # 路由入口
    │   ├── training.rs       # 路由入口 / 训练模式装配
    │   └── ...
    ├── question_bank/
    │   ├── mod.rs            # 显式暴露供训练/收藏使用的组件
    │   ├── practice.rs       # 行测等题库练习工作区
    │   ├── shenlun.rs        # 申论工作区（科目专用组件，而非 Page）
    │   ├── ai_review.rs      # AI 复盘主组件及必要编排
    │   ├── ai_review_stream.rs
    │   ├── ai_review_timeline.rs
    │   ├── ai_composer.rs
    │   ├── ai_review_api.rs  # scope URL / 请求类型 / 请求幂等等共享逻辑
    │   ├── ocr_panel.rs
    │   └── source_answer_panel.rs
    └── components/
        └── ...               # 跨题库与生成工具共享的展示组件
```

落地时只添加确有职责的文件：例如 `ai_review_api.rs` 若仅 1 个很短的辅助函数，可暂放在 `ai_review.rs`；不要创建空模块。`question_header`、`favorites` 的复用接口应迁移到题库共享位置，具体文件数量以减小依赖和编译通过为准。

## 4. 实施顺序与变更清单

### 阶段 A — 用类型表示申论 UI 模式（低风险起点）

- 在 `web/question_bank/shenlun.rs`（移动前可先在原文件）引入 `enum ShenlunMode { Analysis, Grading, Ocr, Source }`，`Copy + Clone + PartialEq + Eq`。
- 把 `use_state(|| "ai".to_owned())`、所有字符串匹配、标签切换回调改为枚举；集中实现 `ai_scope() -> Option<&\u0027static str>` 或同等转换。
- `analysis` / `grading` / `ocr` 仍作为**现有 API 边界**上的字符串；不更改后端字段、草稿存储 key、DOM 测试选择器和组件重挂载行为。
- 验收：切题后模式记忆、四标签切换、批改与解析的独立会话/删除/草稿、OCR 及来源答案行为全部不变。

### 阶段 B — 手写板从测试 ID 脱钩

- `scratchpad.rs::pane_selector` 目前使用 `analysis-material-scroll`、`shenlun-material-scroll`、`question-bank-ai-messages` 等多组 `data-testid`。
- 在真实可书写滚动容器上增加 `data-ink-pane="material"` 或 `data-ink-pane="answer"`；由手写板查询明确的语义属性，不再把测试选择器当生产接口。
- 保留原有 `data-testid`，避免无谓改动 Playwright。针对多个挂载但隐藏的训练工作区，把选择器限定到当前书写覆盖层所属的工作区，避免选中后台隐藏容器。
- **绝不修改**当前每题持久化 scope（如 `shenlun-{paper_id}-{question_id}`）、坐标转换、撤销/重做、IndexedDB 串行写队列与 `flushInk` 语义。
- 验收：行测、申论、收藏复盘的材料/题目书写与滚动定位正常；切题笔迹恢复；iPad WebKit 触控、Apple Pencil、布局切换和持久化正常。

### 阶段 C — 拆 `QuestionAiReview`，整理命名和依赖

- 先抽共享请求/作用域 URL、请求去重/重试辅助逻辑，再将聊天展示 + 执行轨迹从巨型组件分离。避免仅把 1,076 行原样转移到另一个文件。
- 统一三个职责：① AI 控制状态/请求（启动、追问、删除、重试、断线与历史恢复）；② SSE 快照；③ 时间线展示 / 输入组件。
- `ai_review_activity.rs` 推荐改名 `ai_review_timeline.rs`：因为它既展示消息又展示运行轨迹；保留 `RunActivity` 作为内部组件。
- `tool_execution_item` 被 `generation_activity.rs` 复用：如有必要移到通用 `web/components/tool_execution.rs`，不做题库向生成工具的反向依赖。
- `shenlun_ai_panel.rs` 拆成 OCR 和来源答案两个明确文件；`QuestionBankShenlun` 继续只负责工作区编排。
- 将题库相关代码从 `pages/` 迁往 `web/question_bank/`；`pages/training.rs` 只负责装配 `QuestionBankTraining`、`QuestionBankShenlun` 等模式。精简 `pages/mod.rs` 的 `pub use ...::*`。
- **稳定性红线**：AI `analysis` / `grading` 两个 scope 的 thread、SSE、历史、附件、删除、草稿绝不可串；现有 HTTP endpoint、幂等 request-id、sessionStorage/IndexedDB key 不变。
- 拆分以业务边界和可测试性为目标，不设置“每文件最多 N 行”的人为指标。

### 阶段 D — 清理旧申论 Attempt / Job / OCR 确认原型

**当前有效路径（保留）**

- `GET /api/v1/question-bank/shenlun/papers`、`GET /papers/{id}`；
- `POST /api/v1/question-bank/shenlun/questions/{id}/ocr`：一次性 Codex Markdown 转写，不保存作答，也不要求确认；
- `GET /api/v1/question-bank/shenlun/questions/{id}/source-answer`；
- 共享 `practice/questions/{id}/ai?scope=analysis|grading` 及相关历史、SSE、附件、删除、追问请求。

**待核对清理（不能只凭文件名删除）**

| 位置 | 候选遗留 | 必须核对的依赖 |
| --- | --- | --- |
| `application/question_bank/shenlun.rs` | `create_shenlun_attempt`、`shenlun_attempt(s)`、`save_shenlun_draft`、`submit_shenlun_attempt` | **保留** `shenlun_papers`、`shenlun_paper`、`shenlun_source_context` 及仍使用的 source hash 逻辑 |
| `application/question_bank/mod.rs` | `QuestionBankRepositoryPort` 的 5 个 `shenlun_*attempt` 方法 | 确认无当前应用用例和外部调用；移除 SQLite 实现及相关 mock/test |
| `domain/question_bank.rs` | `ShenlunJobRequest`、`ShenlunJob`、`ConfirmShenlunTranscription`、`CreateShenlunAttempt`、`SaveShenlunDraft`、`ShenlunAttempt` | 全仓库调用、序列化和文档契约查找；保留 `ShenlunSourceAnswer`、试卷及题目模型 |
| `application/question_bank/shenlun_grading.rs` | 旧逐页 JSON `validate_transcription`、孤立 `SKILL_VERSION` | 当前 OCR HTTP 响应为 Markdown + events，先确认该校验已不在运行路径 |
| `adapters/outbound/question_bank_sqlite.rs` | Attempt 的新增、读取、保存、交卷 SQL 及旧测试 | **学习统计仍在两处查询 `shenlun_attempts`**；先决定历史数据只读兼容或受控迁移 |
| `migrations/question_bank/20261113*shenlun*.sql` | 已废弃 Attempt/Job 表、列、索引 | **已执行迁移不得删除或重写**；新库初始化、旧库升级都应可用 |
| `e2e/shenlun.spec.ts` | `Attempt` / `draftAttempt()` / 旧 `/shenlun/attempts`、`/jobs` 模拟 | 将“不应访问旧接口”改成可失败的负向约束；不要保留误导性成功 mock |

**数据兼容是阻断项**：`question_bank_sqlite.rs::daily_answered_counts` 和 `daily_answered_breakdown` 当前都读取 `shenlun_attempts`。不能先删表/迁移再让统计运行时报错。若历史数据仍有价值，保留只读统计并隔离旧表；若确实决定彻底移除，应在单独数据迁移设计中明确历史数据处理、新旧数据库验证和回滚策略。

**清理规则**：先做调用图检索，再检查动态注册 / SQL / 测试 / 数据库迁移；未证明无用之前列为“待确认”，而非宣称已删干净。保留服务鉴权、OCR 错误处理、来源答案保护以及实际使用的 Codex service。

## 5. E2E 是当前行为基线，不是全部业务定义

**现有 `e2e/shenlun.spec.ts` 已覆盖的重要约束**

- 申论复用原卷目录、统一答题卡、题目切换与分栏；不存在独立交卷/二稿入口。
- 每题手写保存、切题恢复、收藏与 Flashcard。
- 解析和批改使用独立 `analysis/grading` 会话、草稿、历史、追问及删除状态。
- OCR 是一次性图片转写，结果为 Markdown，显示工具输出，无转写确认或 legacy write。
- 多视口布局、固定 Composer、内部滚动与题库共享工作区的外观一致性。

**后续应增加 / 加强**

1. 把旧 Attempt/Job 成功模拟替换为异常请求失败 + 请求次数为零的负向断言；不要测试一个实际已不支持的接口“能用”。
2. AI 各 scope 的历史/事件、删除及晚到响应分别断言；跨题切换和后台请求完成后不泄漏错误内容。
3. `data-ink-pane` 语义绑定的测试不替换旧 `data-testid`；加入隐藏工作区、屏幕旋转/滚动后的书写命中测试。
4. SQLite 统计测试：空数据库、带历史 `shenlun_attempts` 数据库、新安装与升级数据库；核对 `daily_answered_counts` 和 breakdown。
5. 模块移动后更新 `apps/rust/things/tools/select-related-e2e.sh` 路径匹配，避免 E2E 选择器漏跑；共享组件变更同时跑 Question Bank、收藏、生成相关用例。

**验证建议（在后续实现 PR 中执行，不能以本计划代替实际结果）**

- Rust `cargo fmt --check`、Clippy、目标单元与集成测试；
- Playwright/WebKit `e2e/shenlun.spec.ts`、`e2e/question-bank.spec.ts`、`e2e/favorite-flashcard.spec.ts`，并按影响增加相关生成工具测试；
- 真实 iPad / 手机检查手写、OCR 上传、长 Markdown、SSE 断线重连、切模式与滚动；
- 查看 SQLite 新库与旧库升级情况，禁止测试仅 Mock 全绿就直接宣称数据库兼容。

## 6. 建议拆分为多个实现 PR，而不是一个大爆炸式重构

| 实施 PR | 范围 | 合并前必须确认 |
| --- | --- | --- |
| A | `ShenlunMode` 枚举 + E2E 明确当前模式契约 | 行为不变、scope/草稿隔离 |
| B | 手写板 `data-ink-pane` 解耦 | 行测/申论/收藏、笔迹恢复及 WebKit |
| C | `QuestionAiReview` 提取 + `ai_review_timeline` 命名 + `question_bank/` 目录归位 | 不改变网络/存储契约；共享调用方编译和测试 |
| D | 旧申论 Attempt/Job/转写校验清理 + 测试 Mock 瘦身 | 无活跃调用；学习统计与旧库兼容方案先通过测试 |

以上每个 PR 都应是可以独立回滚和复查的行为保持型改造；如 B 与 C 的模块移动冲突，应先提语义选择器再移动文件。不可为了追求单 PR 完成而混合大规模迁移、存储改造和 UI 功能新增。

## 7. React 迁移预留（只约束边界，不提前实现）

- 将来可新建独立 React/TypeScript 前端，把当前 `web/question_bank/` 映射为 `src/modules/question-bank/`，`pages/` 仍是路由入口。
- 继续使用 Rust `domain / application / adapters`；React 的界面模式状态不放进 Rust `domain`，也无需复制一份同名大 `domain/`。
- 保持题目 ID、HTTP DTO、AI scope、request-id、SSE events、Ink 本地存储格式和键名稳定，使两个前端可用同一组 E2E 行为校验。
- React 的 `useAiReview`/`useInk` 需要重写；此 Plan **不会**使 Yew 组件代码自动变成可复用的 React 组件。

## 8. 完成标准与状态

- [x] 本 Plan 对照当前路由、申论 E2E、申论 HTTP endpoint、题库 AI/Ink、SQLite 统计引用进行静态审查。
- [ ] 实现 PR A-D（需要独立审查与实际验证）。
- [ ] 对公共类型、组件依赖及本地存储键进行回归验证。
- [ ] 完成旧库和真实设备测试；未执行前一律记录为“未验证”。

**本 PR 只供审阅重构范围、命名、顺序和清理策略；不授权自动删库、改行为或合并。**
