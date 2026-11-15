# Question Bank 前端瘦身与模块边界重构 Plan

> 状态：**阶段 A—E 已实现，Rust 静态检查、目标单测与相关 WebKit E2E 已通过；真实 iPad/手机与旧库升级实测尚未执行**。实现、测试与 E2E 瘦身全部提交在同一个 PR #128 的同一分支，未另拆实现 PR，也尚未合并。
> 日期：2026-11-15；审查基线：`main@d54aa6b94e4eadfa4d4cd1b6263586fe4c22162c`。
> 原则：**以当前已实现的代码、HTTP 路由、E2E 所表达的最新产品行为为准**；历史 Plan 仅供追溯，不作为新增功能或兼容要求。

## 0. 历史 Plan 与当前行为的优先级

本 PR 已在两份旧申论 Plan 中**保留原文并用 `~~...~~` 划掉明确过时的条款**，紧跟当下 E2E/在用实现所表达的现行规则：

- [2026-11-13 申论 AI 复盘实施 Plan](2026-11-13-shenlun-grading-specialized-training-plan.md)：划掉“解析/批改共享同一会话”“参考答案须先批改”“共享对话”等过时描述；保留历史提交记录和未冲突的 OCR、评分 Skill 说明。
- [2026-11-14 申论布局与菜单 Plan](2026-11-14-shenlun-practice-layout-parity-plan.md)：划掉“独立页面”措辞、独立二稿提交歧义和错误的 `grade` scope 示例，明确同一工作区不同模式及 `grading/analysis` 两个 scope。
- [2026-11-09 申论真题 Schema/导入 Plan](2026-11-09-shenlun-zhenti-schema-import-plan.md) 是当期数据导入/试点范围，不应把当时的“不做申论阅卷”追溯性地当作当前产品约束，因此**本轮未机械划改**。

对旧文档中的**历史基线、当时的测试/验收记录**不做回溯性伪改；对 E2E 尚未验证的陈述不擅自判定无效。当前重构的唯一行为基线为在用代码、HTTP 路由和现行 E2E；旧 Plan 不再覆盖后来的实现决策。

## 1. 目标、非目标和关键决策

**目标**

1. 纠正 `pages/` 目录中“页面入口”和“训练业务组件”混放的问题，让文件名直接反映职责。
2. 将行测与申论统一放在一个 `question_bank` 前端业务模块下。申论是科目（`subject=shenlun`），不是与 Question Bank 平级的产品模块；归纳概括等才是具体题型。
3. 在**不改变当前用户体验**前提下，拆分单体 `QuestionAiReview`、解耦手写板定位、用强类型表示申论 UI 模式。
4. 审计并删除旧 Plan 遗留、现行流程不再使用的申论 Attempt/Job/确认转写实现，减少实际代码，而非仅搬文件。
5. 最后一阶段系统重构和瘦身题库相关 E2E：删除旧模拟、去除重复初始化与巨型 Mock、按行为重组测试；**保留甚至强化真正的回归保护**。
6. 保留未来 React + TypeScript 可按同一业务模块边界逐块迁移的可能性；优先稳定 HTTP、数据 DTO 和持久化契约。

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
- 把 `use_state(|| "ai".to_owned())`、所有字符串匹配、标签切换回调改为枚举；集中实现 `ai_scope() -> Option<&'static str>` 或同等转换。
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
| `e2e/shenlun.spec.ts` | `Attempt` / `draftAttempt()` / 旧 `/shenlun/attempts`、`/jobs` 模拟 | 在最终 E2E 瘦身阶段统一删除成功 Mock，用明确负向断言表达“旧接口不得调用” |

**数据兼容是阻断项**：`question_bank_sqlite.rs::daily_answered_counts` 和 `daily_answered_breakdown` 当前都读取 `shenlun_attempts`。不能先删表/迁移再让统计运行时报错。若历史数据仍有价值，保留只读统计并隔离旧表；若确实决定彻底移除，应在单独数据迁移设计中明确历史数据处理、新旧数据库验证和回滚策略。

**清理规则**：先做调用图检索，再检查动态注册 / SQL / 测试 / 数据库迁移；未证明无用之前列为“待确认”，而非宣称已删干净。保留服务鉴权、OCR 错误处理、来源答案保护以及实际使用的 Codex service。

## 5. E2E：当前行为基线 + 最终独立瘦身阶段

**当前测试规模和可维护性问题（静态审查）**

- `e2e/shenlun.spec.ts` 约 **859 行**；反复出现 `loginAsAdmin`、`page.route('**/api/v1/question-bank/shenlun/**', ...)`、试卷目录及来源答案模拟。开头仍有旧 `Attempt`、`draftAttempt` 模型，实际流程不再创建它。
- `e2e/question-bank.spec.ts` 约 **2,338 行**，混合题库练习、AI 对话/SSE、附件、Markdown 布局、断线重试、移动端交互等大量不同业务场景。应按测试**行为领域**分组；已有其他独立 question-bank spec，不复制相同覆盖。
- `e2e/favorite-flashcard.spec.ts` 约 **935 行**，包含收藏/Flashcard 主流程及大量 AI、手写板交叉验证；可复用部分题库 AI Mock/导航 helper，但不能把收藏专属断言迁丢。
- `e2e/comprehensive.spec.ts` 约 **1,358 行 / 24 个用例**，覆盖 AI 出题（资料分析）的准备、恢复、暂停续答、用户出题要求，以及 Ink/IndexedDB/PWA 安全区等横切行为；生成流程与手写/布局断言混在同一文件。
- `e2e/logic-judgment.spec.ts` 约 **664 行 / 19 个用例**，覆盖 AI 出题（判断推理与数量关系）的生成、交卷、复盘、计时恢复和手写命中；与综合训练共享“生成试卷 → 进入训练 → 停走恢复”的同类 setup。
- `e2e/support.ts` 约 **127 行**，主要是通用手势/计时辅助；**不应**将题库专用的所有复杂模拟塞进这个通用文件。若提取 helper，放在 `e2e/helpers/question-bank.ts` 或邻近小模块，fixture 数据与请求行为分开。
- 文件行数本身不是缺陷：重点是**删除过期测试模型、减少重复初始化与 Mock、保留测试独立性和失败定位**。行数减少不是验收唯一指标。

**现有 `e2e/shenlun.spec.ts` 已覆盖的重要约束**

- 申论复用原卷目录、统一答题卡、题目切换与分栏；不存在独立交卷/二稿入口。
- 每题手写保存、切题恢复、收藏与 Flashcard。
- 解析和批改使用独立 `analysis/grading` 会话、草稿、历史、追问及删除状态。
- OCR 在当前复盘内保留多次转写结果并流式显示工具进度；结果只保存在内存，不创建作答或确认，退出复盘后清除，也不发生 legacy write。
- 多视口布局、固定 Composer、内部滚动与题库共享工作区的外观一致性。

**在实现阶段 A—D 期间，按改动持续回归，不等到最后**

1. 将旧 Attempt/Job 写请求视为非法：不再返回成功 Mock，直接失败并记录调用次数应为零；该测试修改可与后端清理一起完成，最后 E 阶段统一整理结构。
2. AI 各 scope 的历史/事件、删除及晚到响应分别断言；跨题切换和后台请求完成后不泄漏错误内容。
3. `data-ink-pane` 语义绑定的测试不替换旧 `data-testid`；加入隐藏工作区、屏幕旋转/滚动后的书写命中测试。
4. SQLite 统计测试：空数据库、带历史 `shenlun_attempts` 数据库、新安装与升级数据库；核对 `daily_answered_counts` 和 breakdown。
5. 模块移动后同步更新 `apps/rust/things/tools/select-related-e2e.sh` 路径匹配，避免 E2E 选择器漏跑；共享组件变更同时跑 Question Bank、收藏、生成相关用例。

**最终阶段 E — E2E 专项重构与瘦身（本轮明确要实施）**

1. **删除遗留**：移除 `Attempt`、`draftAttempt`、旧 `/shenlun/attempts` 和 Job 成功模拟及无意义的假状态；对被废弃的写接口使用 `route.abort()` / 显式 throw 或请求监听计数，确保发生请求就使测试失败，而不是默默成功。
2. **收敛重复 setup**：把申论试卷目录、纸张题目、来源参考答案的基础 fixture 与常用 `openShenlun` / `installShenlunRoutes` 收敛为小型辅助。支持各测试覆盖个别响应和延迟场景，不创建一个需要几十个选项的万能 Mock 服务。
3. **按行为组织套件**：题库练习、共享 AI（流与请求生命周期）、申论、收藏 Flashcard、手写及布局等各有清楚的归属；可以将 `question-bank.spec.ts` 的 AI 大段拆进 `question-bank-ai.spec.ts`，但首先排查与现有 `question-bank-*.spec.ts` 重复的测试。切忌只拆文件不消除重复逻辑。
   - 专项训练中的 AI 出题页面一并纳入：`comprehensive.spec.ts`（资料分析）与 `logic-judgment.spec.ts`（判断推理、数量关系）。优先抽出共用的“状态/出题要求/准备任务/历史”与“未计时试卷进入训练”fixture，并按“生成与恢复”“答题与交卷”“手写与布局”拆分大文件；各科目差异化的 payload 和断言留在对应 spec，不建立万能 Mock。
   - 拆分或抽 helper 后同步更新 `e2e/support.ts` 的能力套件清单与 `tools/check-e2e-suites.py` 的计数，确保 CI 实际执行新文件。
4. **保留不同测试层次**：真实 HTTP 上传/鉴权/CSRF 与数据库集成测试继续跑真实路径；Mock 用于稳定重现异步/竞态/错误，不可用 Mock 全绿替代真实路径验证。不要把需要验证的响应模型全部硬编码到 helper 里掩盖协议变化。
5. **保持核心断言**：各科目选题/切题、AI `analysis/grading` 隔离、SSE 重连、请求幂等/冲突、独立草稿、Ink IndexedDB 恢复、OCR Markdown 与工具输出、来源答案、收藏和 Flashcard、WebKit 几何；不要为减少 case 数量而删除边界/失败路径覆盖。
6. **维护测试执行选择器**：若拆分 `question-bank.spec.ts` 等 suite，同步更新 `tools/select-related-e2e.sh`、CI 指定 spec 与受影响的测试命令。检查新 spec 在 CI 下实际被选中执行，而不是只有本地可以运行。
7. **验证方式**：E 阶段前后记录测试清单、关键场景映射、用例数量及实际执行结果；对比删掉的仅是重复/过时断言。`sh e2e`/Playwright 通过且与真实接口测试结合，才可认定“瘦身不降覆盖”。

**验证建议（在后续实现 PR 中执行，不能以本计划代替实际结果）**

- Rust `cargo fmt --check`、Clippy、目标单元与集成测试；
- Playwright/WebKit `e2e/shenlun.spec.ts`、`e2e/question-bank.spec.ts`、`e2e/favorite-flashcard.spec.ts`，并按影响增加相关生成工具测试；
- 真实 iPad / 手机检查手写、OCR 上传、长 Markdown、SSE 断线重连、切模式与滚动；
- 查看 SQLite 新库与旧库升级情况，禁止测试仅 Mock 全绿就直接宣称数据库兼容。

## 6. **同一个实现 PR、分阶段 commit**

**按用户决策，本重构不拆多个实现 PR。** 当前 Draft PR #128 先供审阅 Plan，审阅确定后可在该 PR 继续追加 A—E 的代码提交；每一阶段对应独立的 commit（必要时数个小 commit），完成后统一 review、统一合并，绝不在某一阶段未验收时提前宣称全部完成。

| 阶段 / 建议 commit | 范围 | 阶段检查 |
| --- | --- | --- |
| A `refactor(question-bank): type Shenlun UI modes` | `ShenlunMode` 枚举，模式映射显式化 | 行为不变、scope/草稿隔离、相关 E2E |
| B `refactor(ink): select panes by semantic attributes` | 手写板 `data-ink-pane` 解耦 | 行测/申论/收藏、笔迹恢复及 WebKit |
| C `refactor(question-bank): extract shared AI review and organize modules` | `QuestionAiReview` 拆分、`ai_review_timeline` 命名、`question_bank/` 归位 | 不改变网络/存储契约；共享调用方、编译及 E2E |
| D `refactor(shenlun): retire unused attempt and job prototype` | 旧申论 Attempt/Job/转写校验及未用代码清理；同时修正必要测试负向断言 | 无活跃调用；学习统计与旧库兼容验证通过 |
| E `test(e2e): slim and organize question bank regression suites` | **最后统一 E2E 瘦身**：过时 mock、重复 setup、巨型 spec、套件归属、CI 选择器 | 场景覆盖无下降；相关套件与真实 API/上传/数据库测试验证 |

**每阶段检查必须运行并如实记录**：静态编译与目标单测 + 涉及的现有 E2E；E 阶段负责测试结构整体瘦身和收尾回归，不意味着 A—D 可以不跑测试。保留阶段 commit 以便审阅和精准回滚，不要求拆独立 PR。若 B/C 文件移动冲突，先完成语义选择器替换再移动。

PR #128 在关键测试与最终 CI 通过以前保持 Draft；A—E 的实现提交、专项测试与 E2E 瘦身均已进入同一分支，验证结果见 §8。真实设备与旧库升级未验证前不宣称全部完成。

## 7. React 迁移预留（只约束边界，不提前实现）

- 将来可新建独立 React/TypeScript 前端，把当前 `web/question_bank/` 映射为 `src/modules/question-bank/`，`pages/` 仍是路由入口。
- 继续使用 Rust `domain / application / adapters`；React 的界面模式状态不放进 Rust `domain`，也无需复制一份同名大 `domain/`。
- 保持题目 ID、HTTP DTO、AI scope、request-id、SSE events、Ink 本地存储格式和键名稳定，使两个前端可用同一组 E2E 行为校验。
- React 的 `useAiReview`/`useInk` 需要重写；此 Plan **不会**使 Yew 组件代码自动变成可复用的 React 组件。

## 8. 完成标准与状态

- [x] 本 Plan 对照当前路由、申论 E2E、申论 HTTP endpoint、题库 AI/Ink、SQLite 统计引用进行静态审查。
- [x] **在同一个 PR #128 中按 A—E 阶段提交 commit 并验证**。A—E 均已实现；Rust 静态检查、单元/集成测试与相关 WebKit E2E 通过。
- [x] 对公共类型、组件依赖及本地存储键进行回归验证：`ShenlunMode` 只存在于 Web 层，HTTP 仍使用 `analysis`/`grading`/`ocr` 字符串；原 `data-testid`、AI scope、request-id、草稿 scope 与 Ink IndexedDB 键均未改变；`data-ink-pane` 仅新增语义属性。
- [ ] 完成旧库和真实设备测试；**未执行前一律记录为“未验证”**。

### 实施与验证记录

**阶段实现**

- A：`web/question_bank/shenlun.rs` 引入 `ShenlunMode { Analysis, Grading, Ocr, Source }`，集中提供 `ai_scope()`、`prompt_mode()` 与 `key()`；模式状态改由枚举持有。
- B：可书写容器增加 `data-ink-pane="material"|"answer"`；`scratchpad.rs` 改用 `pane_element` 按语义属性定位，并跳过零尺寸的隐藏工作区；原 `data-testid` 保留。
- C：题库前端迁入 `web/question_bank/`（`practice`、`shenlun`、`collection_review`、`manage`、`setup`、`ocr_panel`、`source_answer_panel`）；`QuestionAiReview` 拆为 `ai_review`、`ai_review_api`、`ai_review_recovery`、`ai_review_stream`、`ai_review_timeline`、`ai_composer`、`ai_reply`；通用 `question_header_actions` 与 `tool_execution` 移入 `web/components/`。
- D：删除 `ShenlunJobRequest`、`ShenlunJob`、`ConfirmShenlunTranscription`、`CreateShenlunAttempt`、`SaveShenlunDraft`、`ShenlunAttempt`、SSE 之外的旧批改校验与 Attempt/Job 读写路径；`shenlun_attempts` 历史表保留，学习统计继续只读兼容，并新增 `learning_statistics_read_legacy_shenlun_attempts_without_attempt_writes` 覆盖。
- E：新增 `e2e/helpers/question-bank.ts` 与 `e2e/helpers/shenlun.ts` 收敛登录、选题、开始练习、AI 请求桩、申论 fixture 与 OCR EventSource；AI 用例拆入 `e2e/question-bank-ai.spec.ts`；`question-bank.spec.ts` 2,337 行降至约 1,074 行；删除旧 `/shenlun/attempts`、`/jobs` 成功 Mock，改为负向断言；同步 `tools/check-e2e-suites.py`、`tools/select-related-e2e.sh` 与 `AGENTS.md` 的本地并行分片说明。
- E 追加：专项训练中的 AI 出题套件（`comprehensive.spec.ts` 资料分析、`logic-judgment.spec.ts` 判断推理与数量关系）纳入瘦身范围。数量关系由 `logic_judgment.rs` 的 `quantitative` 参数共用同一生成/作答工作区，因此与判断推理共用 `logic-judgment.spec.ts`；它与综合训练只共享“生成任务 → 进入训练 → 停走恢复”的 fixture 模式，题型 payload 和复盘断言保留在各自 spec。
- F 追加：专项训练的 AI 出题页面归入 `web/ai_generation/`（`comprehensive`、`logic`、`activity`、`prompt`）。判断推理与数量关系继续共用同一套单题工作区；出题要求面板及其读取、保存、恢复状态收敛为 `GenerationPromptSettings` 与 `use_generation_prompt`，资料分析、判断推理、数量关系共享同一组件。
- 合并 `origin/main` 的申论 OCR 流式保留功能（`2baec77b`、`28156ab7`）时，保留其内存态多次转写、工具进度与“OCR 模式隐藏题干”的产品行为；Stage D 仅退役数据库 Attempt/Job 原型，两者不冲突。同步修正 main 中三处与新行为不一致的断言（OCR 模式题干、删除可用性跨模式保留、铅笔 SVG 图标）。

**本次实际执行**

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --locked --manifest-path apps/rust/things/Cargo.toml --all-targets --features e2e -- -D warnings` | 通过 |
| Web 单元测试 | 50/50 通过 |
| Native 单元 + 集成测试 | 通过（含 28 个 question bank 测试与 SQLite 历史统计兼容测试） |
| `tools/check-e2e-suites.py` | 通过：177 个声明用例 / 176 个可运行用例 |
| `tools/test-portal-ci.py` | 9/9 通过 |
| `make check-static`、`git diff --check` | 通过 |
| `e2e/shenlun.spec.ts` | 21/21 通过（含 main 的 OCR 流式保留用例；harness 启动时带 `HYZ_THINGS_E2E_SHENLUN_UPLOAD_FIXTURE=1`） |
| `e2e/question-bank.spec.ts` + `e2e/question-bank-ai.spec.ts` + reference + theme | 92/92 通过 |
| `e2e/comprehensive.spec.ts` + `e2e/flashcards.spec.ts` | 43/43 通过 |
| `e2e/logic-judgment.spec.ts` | 19/19 通过（含数量关系生成与手写用例） |
| `e2e/comprehensive.spec.ts` + `e2e/logic-judgment.spec.ts`（F 重构后） | 42/43 一次完整运行；唯一失败是共享出题面板的 `aria-label` 覆盖了可见 `<label>`，移除后该用例单独复跑通过 |

**未验证**

- 真实 iPad / 手机 WebKit 触控、Apple Pencil 与真实设备上的长 Markdown、SSE 断线重连：**未执行**。
- 旧设备数据库升级实测：**未执行**；目前只有旧表只读统计的单元测试作为兼容证据。
- 本轮 E2E 在本机以 `--shard` 并行验证；最终合并门槛仍以 PR 最终 HEAD 的必需 CI 为准。

**PR #128 现已包含 Plan 文档、A—E 实现、单元/集成测试与 E2E 瘦身。未执行真实设备与旧库升级验证前，不宣称“全部完成”；未获得确认前不合并。**
