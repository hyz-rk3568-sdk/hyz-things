# hyz-things 专项训练与 Flashcard 错题复练 — 一期实施 Plan

> 日期：2026-10-21
> 状态：已批准实施。以本 Plan 和既有速算 / Flashcard 验收为准。
> 基线：main acb7b2c76693b26564f7925d71c9f63dcc56cfc3

## 1. 目标与非目标

用户批准：将「资料分析 · 速算训练」更名为「专项训练」，包含「速算训练」与「Flashcard 错题复练」；**仅专项训练可以 Apple Pencil 手写**，原 Flashcard 卡片库与 FSRS 间隔复习完全保持不变。保留原速算题目生成、四选一判题和心算精度规则。

一期不开发申论、OCR/AI 自动判卷、训练历史持久化、PDF、橡皮擦/撤销、Flashcard Web 制卡及新的统一 SQLite schema。不修改 router、camera、proxy。

## 2. 数据源与流程

已同步 Flashcard 的 Markdown 是 source of truth，SQLite 保存索引和 FSRS 状态；题卡唯一 ID 来自已有 UUIDv7。复练仅读取 GET /api/v1/study/cards、/decks、/tags，不调用 /api/v1/study/review POST。

训练流程：选择模式 → 选择 Deck + Tag (默认错题) → 读取当前所有 active 候选卡 → 随机抽取不重复卡片 → 展示 Front + 手写草稿 → 显示 Back → 做对 / 做错 / 跳过 → 下一题清空草稿 → 当前会话成绩和错题回看。

不猜测 Front/Back 中的 A/B/C/D 标准答案；纯技巧卡允许跳过。显示解析时冻结作答用时，阅读解析时长不计入答题。跳过单独统计；正确率与 FSRS 评分不是一个概念。

候选分页每次200张，最多1000张；大于上限提示缩小范围，不能静默截断。题数10/20/50，少于选择题量时按实际张数练习并告知。候选为0或 API 错误可重新筛选，不影响浏览器端速算。

## 3. Web 边界与路由

一级「学习」导航不变。新地址 #/study/training，旧地址 #/study/calculation 兼容并规范化到新地址，默认进入速算模式；原 #/study/cards、#/study/review 保留。

保留 CalculationTraining 中生成随机计算题及统计的实现。新增 TrainingWorkspace / FlashcardPractice；最小共享 Markdown / KaTeX / 图片安全渲染函数。不允许执行卡片 raw HTML 或跳出卡片资产目录。FSRS 的 review/sync 后端、权限、CSRF 不做变更。

Canvas 完整保留 900×450、lineWidth=3.8、round lineCap/lineJoin、白色笔迹、深色背景、直接 pointermove lineTo、Apple Pencil 连续落笔与 Safari 手势防吞笔逻辑；不做压感和自动平滑。每次切换题目重置当前草稿。

## 4. 实施粒度

1. 在独立 feature 分支上新增路由与入口、保留速算回归。
2. 提取共享安全 Markdown 渲染组件、构建专项训练壳。
3. 加入错题复练全流程、Deck/Tag 筛选、分页、随机顺序、手写草稿、自评和当轮统计。
4. 调整/新增 Rust 单测与 Playwright E2E：旧入口、速算精度、Apple Pencil 事件行为、无 review POST、清空、Front/Back、筛选和成绩；最终 HEAD 在 GitHub Actions Portal CI 完成验证。

遵守仓库 AGENTS.md：逐行为 Red → Green → Refactor，跨模块功能使用 PR，CI 校验通过后才允许合入 main。

## 5. 验收

- [ ] Flashcard 原复习与 Deck+Tag、FSRS、同步逻辑不变。
- [ ] 原三类速算和10/20/50选择、手写、计时、结果维持既有表现。
- [ ] 新旧训练链接可用，原系统一级导航无变化。
- [ ] 预选错题 Tag；Deck/Tag 可组合；0、少题、大题量、失败均明确提示。
- [ ] 原卡片 Markdown/图片/LaTeX 可渲染，Back 揭晓前不可见。
- [ ] 手写连笔，切题清空，自评隔离于 FSRS，统计区分错与跳过。
- [ ] GitHub Actions Portal CI 在最终 HEAD 通过。
