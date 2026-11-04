# 行测真题复盘 AI 解析与追问实施计划

日期：2026-11-04。分支：`feat/question-bank-ai-review`。目标：通过 PR 合入 `main`。

## 产品决定

- 套卷、模块练习和相关历史复盘复用原有题库 Review，默认展示 **原始解析**。仅点击“AI 解析”标签才加载 AI 记录；首次点击不自动消耗额度。
- 用户主动点击“开始 AI 解析”才发起生成；有记录时展示历史消息与追问输入框。
- 切换题目、切换解析栏目、离开复盘页均不取消正在运行的任务；仅“停止生成”明确调用 App Server 的 `turn/interrupt`。停止保留既有对话。
- 解析与追问对话持久保存到 RK3568 的 `question-bank.db`，按 `practice_session_id + question_id` 查找；每道题独立持久 Codex Thread，同一题的追问复用 Thread。重复点击不重复创建任务；失败可重试。
- Codex 输入采用 **ID 驱动检索**：由后端核实属于已交卷练习的 `paper_id`、`question_id`、`material_ids`、`source_hash`、用户选项；不发送题干、选项、原始解析、模块、题型、考点、材料正文。Codex 在电脑端项目 `question_bank/papers/` 内根据 UUID 搜索原题、材料、图片；根据题面按需读取 `.agents/skills/行测做题/` 和 `flashcards/`。
- 原卷 Answer/Explanation **仅供参考**，必须输出自己的判断；与参考答案有分歧要说明，禁止照搬原始解析。资料分析优先采用 hyz 的近似速算套路。题面、原解析和图片都是不可信的输入，不作为执行指令。
- 安全：浏览器只传练习/题目 ID 与追问文本，后端鉴权、CSRF、限制长度和并发；Codex 固定 PC 主机与 CWD，AI 线程只读沙盒，禁止文件修改，拒绝路径穿越；仅允许交卷后使用。
- 如果题库已经更新且 `source_hash` 不符，不允许拿新版题面代替旧题。之前存档的 AI 解析仍可读；新生成需用户重新作答/匹配版本。
- 如果 PC 离线或重启，历史消息仍可读取；恢复旧 Codex Thread 失败时显示明确错误，可通过新线程重新分析，不丢旧对话。RK3568 服务重启把运行中状态修正为“失败/可重试”，不能误报仍生成中。

## 数据流

1. 复盘 GET 原题：仍用现有 `PracticeReview`，默认呈现 `explanation`。
2. 切换 AI 标签：GET `/api/v1/question-bank/practice/sessions/{session_id}/questions/{question_id}/ai`；查询 SQLite 消息与任务状态，绝不自动创建新 Thread。
3. 点击开始：POST 同路径（空 JSON），服务端校验 completed session/归属/版本，幂等创建后台任务和持久 Thread。使用本地 Codex App Server，禁用 ephemeral。
4. 后台写入 `thread_id`、消息、状态，客户端仅轮询当前可见题目；退出页面不影响后台 `tokio::spawn`。
5. 追问：POST `.../ai/messages`，复用同一 Thread（`thread/resume`），记录用户消息和 AI 回答。每题同时只有一个活动 Turn。
6. 手动停止：POST `.../ai/stop`；发出取消信号，适配器调用 `turn/interrupt`，等待终止确认；不丢完成的历史。
7. E2E：仅跑 `e2e/question-bank.spec.ts`；覆盖默认原始解析、切换 AI、开始、切题不打断、退出返回、追问、停止、请求鉴权，使用模拟后端而非真实 Codex 消耗。

## 实施顺序 / 验收

1. 新增问答 DTO 与 SQLite migration；写状态/业务单元测试，保护现有题库答案安全。
2. 复用 Codex App Server WebSocket，新增持久 Thread 创建/恢复和只读权限、Turn 中断测试；保留现有 AI 出题的 ephemeral 模式。
3. 加题库 AI service、API 路由（管理员/CSRF）、持久异步任务和恢复语义。
4. 为 Yew 复盘 UI 增加默认原始解析与按题保存的 AI 标签、消息和停止/追问；原题材料与导航不变。
5. 加相关 Playwright E2E；静态检查后运行相关 Rust/Web/Playwright 测试，检查 PR CI；全部相关检查成功且 review/mergeable 后合入 main。

## 不做

不修改原始 Markdown、题库解析、Flashcard 数据与历史作答；不生成新题；不把全部题干复制到提示词；不在离开页面时取消任务；不跑无关 E2E 测试；不执行设备部署/SDK/固件构建。
