# 资料分析 QuestionPlan：SQLite 精确选卡与全卷约束（2026-10-24）

## 背景和目标

现有 `QuestionPlan` 仅在四篇材料之间随机均衡分配选中的错题分类，并把普通知识点**候选分类名单**交给 Codex；无法保证全卷具体卡片唯一、同篇分类唯一，也无法准确追踪所用卡片。

本次改为 **SQLite 选卡 → Rust 完整规划 20 个题位 → 四个 Codex Thread 按指定卡片命题 → Rust 对照规划验证身份并入库**。不改用户自定义 Prompt、卡片 Markdown 格式、FSRS、4×5/20 分钟、暂停/恢复及历史重做；不引入额外 AI 选卡调用或 Rust 数学验算器。

## 约束与语义

- 用户勾选 1～12 个 **错题分类**。每个选中分类保底至少一张来自 `资料/错题/<分类>/` 的真实卡；这里「保底」指至少一张卡，不要求恰好一张。
- 保底之后，所有剩余 `资料/错题/<分类>/` 与 `资料/<分类>/` 卡片**统一候选**；不再指定错题/普通题配额，也不屏蔽未选错题分类。
- 四篇各五题；**同篇五张卡所属分类两两不同**（来源目录不影响分类相等性）；**全卷二十个 card_id 两两不同**。
- 跨篇允许同一分类，不允许同一卡。尽量覆盖不同分类、平衡分类出现次数；同等分数随机打散。
- 卡片池不足/无满足硬约束的分配时明确报错，不静默违反约束。
- V1 是 ID 级去重、分类级互斥，**不承诺**不同卡的具体考法语义去重；历史频率、`concept_key` 进入后续 V2。

## 设计

### 1. SQLite 卡片快照

从 `flashcards WHERE active=1` 提取 `id, source_file, deck_path, front_markdown, back_markdown, source_hash`。仅接受合法的资料分析普通/错题路径、真实且非空的内容、有效卡片 ID。解析来源与类别使用现有路径函数，不在出题阶段扫描 `/mnt/hyz-cards`。

一次生成任务开始时加载一次卡池并冻结；Rust 持有文本快照并传给 Codex，避免运行期间同步导致内容错配。数据库的时效由已有 Flashcard 同步流程负责；如未同步，明确以已同步卡为准。不修改源文件或同步器。

### 2. 题目目标与规划

```rust
struct CardTarget {
    target_id: String,   // g1-q1 ...
    card_id: String,
    card_path: String,
    source_hash: String,
    category: String,
    source_kind: SourceKind, // Wrong | Knowledge
    front_markdown: String,
    back_markdown: String,
}
struct QuestionPlan {
    groups: [Vec<CardTarget>; QUIZ_GROUPS],
}
```

先根据用户选中的错题分类构造保底目标，再从剩余统一卡池补足二十题。采用**全卷级搜索/约束分配**，不能逐篇贪心造成后续无解；根据分组占用的分类集合和全卷已用 card_id 剪枝，优先选择分类覆盖大、出现次数少的分配；有多种可行解时用任务 seed 打散。同样快照+seed 产生相同计划。全部目标生成完毕才启动 Codex。

注意：仅用「至少5类、至少20卡」无法证明约束可满足，还需最终完整规划校验。

### 3. Codex 命题和绑定

`generate_group()` 将每篇固定五个 `CardTarget` 的 `target_id/card_id/category/source_kind/card_path` 和 Front/Back 原文发送给 Codex；不再传整套候选分类或固定 wrong/knowledge 数量。仍保持每篇材料一次生成五题、最多四篇并行。

在 Codex 输出 JSON Schema 中为每题增加必需 `target_id`。允许调整题目展示顺序，但五个 target_id 必须形成与计划相等的集合，不能重复/缺失/替换。Rust 根据 `target_id` 回填和验证 `skill`、`source_kind`、`card_id` 等可信来源属性，不能仅相信模型声明。保留现有题数、四选一及答案索引校验；**数学答案与是否真正应用卡片方法仍由 Codex 自检**。

部分失败时只重试对应篇，并维持这篇相同卡片集合与目标 ID，不改变成功篇。仅通过校验的题组入库。

### 4. 来源、隐私与兼容

`VerifiedQuestion` 增加可选 `card_id/card_path/source_kind/source_hash`（用 serde default），来源由 Rust 回填；`QuestionGroup.version` 从 4 改为 5。答题中的 `PublicQuestion` 不暴露答案或卡片来源，交卷后可追溯。旧 `groups_json`、历史回看、重做仍可反序列化，重做不重新抽卡。原则上无需新数据库表。

## 实施步骤

1. 在 `data_analysis.rs` 增加从 SQLite 读取完整卡片的读取器、路径过滤、卡片有效性检查；单次任务冻结快照。
2. 重构 `QuestionPlan`、添加保底错题分配 + 全卷补齐/互斥/回溯规划、最终约束校验。
3. 更新 Codex 固定动态上下文和 `codex_app_server.rs` 输出 Schema，增加 `target_id`。
4. 按计划目标严格核对 Codex 响应、由 Rust 回填来源；失败重试保持原计划。
5. 扩展 `VerifiedQuestion` 可选元数据，升级新组版本，确认旧试卷 JSON 兼容。
6. 编写单测、回归测试；运行可用的 Rust 格式/编译/测试与端到端验证。

## 验收标准

- 选择 1～12 类时（满足卡池可行性前提）都能生成完整四篇×五题，选中错题分类全部保底；保底后的卡池不区分来源配额。
- 每篇五个互异分类，全卷二十个互异 card_id；跨篇允许同分类不同卡。
- 同一数据快照与 seed 输出相同计划；无解时明确失败。
- Codex 返回缺失/未知/重复 `target_id` 不能入库，重试仍用原目标。
- 新题存储稳定来源、旧试卷仍可读；用户 Prompt、计时、历史重做、FSRS 不受影响。
- V1 不把卡片 ID 去重宣称为具体考法去重。

## 后续 V2（本 PR 不实施）

按最近真实生成过的试卷进行卡片出题频率降权（历史重做不计新出题）；设计独立 `concept_key` 后再考虑跨卡片考法去重。
