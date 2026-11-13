# 专项训练 · 申论 AI 复盘实施 Plan

日期：2026-11-13  
PR：[#122](https://github.com/hyz-rk3568-sdk/hyz-things/pull/122)（Draft）  
范围：本 PR 内完成全部改动、专项测试与验收；未使用的原型不做数据迁移、评分格式兼容或旧版 UI 兼容。

## 1. 业务目标与边界

在现有「专项训练」增加「申论训练」。沿用真题地区/年份/试卷组织、共享双栏、材料滚动、题目导航、收藏及 Flashcard，不额外开发正式考试 Session、二稿对比、独立采分点引擎或批改历史列表。

右栏四个业务入口：

- **AI 批改**：输入文字或上传手写照片，使用现有 `QuestionAiReview`。模型读取本题题干、给定材料、题型/文种/字数要求及来源参考答案（仅作线索），输出完整中文 Markdown 练习诊断与建议得分。
- **AI 解析与追问**：与批改使用同一个题目级 Codex 会话和完整流式工具轨迹。切换标签只改变输入目标；同一个会话里的后续追问不创建另一套历史或评分记录。
- **OCR 临时转写**：**独立的一次性 Codex 对话**。图片上传复用 Question Bank 的附件组件、校验和 PC 端转发；视觉模型直接输出逐页 Markdown 原文，保留错字/段落，对不清楚的字作明确标记。**不要求人工确认，不提交保存、不调用旧的 Job / attempt / OCR 确认接口，也不复用持久解析线程。**真实工具调用使用现有 `AiReviewProgress` 事件契约与 `tool_execution_item` 展示。当前采用单次 HTTP 响应，工具轨迹在任务完成后显示，不承诺实时 SSE 或历史恢复。
- **题库来源参考答案**：按 `question_id` 只读获取，不生成额外作答记录；本题已产生完整 AI 批改报告后才允许展示，注明来源和非官方性质。

三点菜单仅保留复用共享 AI 对话的**删除**（既有确认机制），无「新批改、重批、查看评分历史」。

## 2. Prompt 与 Markdown 报告

唯一 Skill：`.agents/skills/hyz-shenlun/SKILL.md`，只按需读取 `references/xiaoti.md`、`references/dazuowen.md`、`references/scoring.md`。与行测规则隔离。Prompt 只写业务要求、素材位置和输出格式；不把 Codex Thread、事件流等工程细节告诉 AI。

批改顺序：

1. 识别题型、作答对象、问法、材料范围、字数、文种及可信满分；先审题后评分。
2. 依据题干和原材料抽取不重复的采分点，来源答案仅作线索。正确识别同义概括和合理表达，不机械做关键词匹配、不重复扣同一个问题。
3. 小题按采分点分析；公文按任务身份、受众、格式与口吻，大作文按立意、结构、材料运用、论证与语言综合评估。
4. 只有在材料与分值可信时提供「AI 练习参考分（非官方）」，模型自己分配权重并核对总和；否则输出未评分诊断，不编造官方标准或虚假的确定性。
5. 引用真实的学生原话和材料依据。图片内容不清时标示不确定性，不能猜字；能辨认的内容可直接批改，**没有额外人工确认门槛**。
6. Markdown 固定章节：`# 申论批改`、`## 练习参考分`、`## 审题与作答要求`、`## 采分点与得失分`、`## 总体评价`、`## 修改建议`。小题优先使用明晰表格，大作文可改为分维度反馈。只有用户主动要求才写完整范文或整篇替代答案。

Rust 不再把 AI Markdown 转成 JSON 评分，也不计算点数/分值；保留既有请求鉴权、附件保护、Codex 状态、输入边界及来源保护。

## 3. 复用实现

- `QuestionAiReview` + `QuestionBankAiService`：批改、解析、Markdown 展示、图片、追问、停止、真实工具轨迹、断线恢复、删除。
- `AiComposer` + `AiImagePicker`：共享输入和上传组件；申论允许最长约 50,000 字，行测继续保持 2,000 字。
- `TrainingSplitWorkspace`、`QuestionBankQuestionHeader`、`MaterialScrollButton`、`CompactQuestionNavigation`、`QuestionHeaderActions`、现成收藏 / Flashcard。
- `ShenlunAiService::ocr`：只有一次性 Markdown OCR 与短期工具事件收集；图片转发和 Codex Session 依赖现有实现。无额外聊天表、OCR Job、确认结果落库或持久会话。
- `ShenlunAiService::reference_answer`：按原题身份保护来源答案。

## 4. 专项验收

- [x] 已提交：申论 Markdown 批改/解析共享对话，原卷双栏与答题卡、收藏、Flashcard、长文本输入、删除三点菜单、来源答案。
- [x] 已提交：临时 Codex OCR、Markdown 直接渲染、真实工具事件采集与同款工具卡片（**执行结束后展示**），删除旧 OCR 确认界面与相关路由。
- [x] 已提交：`e2e/shenlun.spec.ts` 覆盖选卷、切题、共享 AI 批改/追问/删除、来源答案、临时 OCR Markdown 与工具输出；CI 路径选择仅该专项用例。
- [ ] 最新 HEAD 的 Rust `fmt --check`、编译、Clippy 和相关单测无失败。
- [ ] 最新 HEAD 的 `npx playwright test e2e/shenlun.spec.ts` 真实运行通过；不跑全量 E2E。
- [ ] 真实设备/模型验收：多页手写图片、实际工具轨迹、申论小题/大作文与 iPad/手机的交互和长作文展示。
- [ ] 所有关键 CI 门禁成功，Review 通过后才考虑解除 Draft；不直接合并。

任何构建失败、测试未运行的项都不得标为通过。项目尚未投入使用，旧原型和其历史记录不作为兼容目标。
