# hyz-things 专项训练学习计时与统计 — 实施 Plan

> 状态：实施中（PR #88，尚待 CI 验证和合并）
> 目标仓库：`hyz-rk3568-sdk/hyz-things`
> 涉及应用：`apps/rust/things`（Rust + Yew + Axum + SQLite）
> UI 基准：已确认的专项训练计时原型 v2；以用户最新确认的“两个计时器完全独立”为准。

## 1. 目标

在现有「专项训练」内新增全局**每日有效学习正计时**和第六个**「学习统计」**标签，统计五种专项训练的学习时长，支持每日/周统计及学习热力图。只移除旧的「上午 / 下午 / 晚上」自定义倒计时（含其画中画入口），不得移除考试日期倒计时及其共享画中画能力。保留试卷自身的限时倒计时及既有暂停、继续、交卷逻辑。

### 交互验收红线（必须先写测试）

1. 学习正计时以**粉色 `HH:MM:SS`** 显示；试卷倒计时以**白色 `MM:SS`** 显示（最后一分钟保留现有错误/警示颜色）。两个时间区域均**只占一行**，不显示“今日学习”“倒计时”等附加文字，也没有图标、额外卡片或单独暂停按钮。保留辅助技术可读的 `aria-label`、`title` 和 ≥40px 点击热区，不增大视觉体积。
2. **点击粉色学习时间**：只暂停/继续学习正计时，试卷倒计时不变。
3. **点击白色试卷时间**：只执行现有试卷暂停/继续，不改变学习正计时状态；即使试卷暂停并隐藏题目，学习计时仍按其自己的状态继续走。此规则优先于“试卷暂停自动暂停学习”的任何旧设想。
4. 关闭答题页或点击「暂停并退出」回到非学习界面时，结束当前有效学习片段；它不同于**仅点击试卷倒计时**暂停而仍停留在答题页。手动暂停学习后不得因切换模块或进入答题页而自动恢复。
5. 原有五个入口「速算训练 / 错题复练 / 资料分析 / 判断推理 / 数量关系」保留，**「学习统计」作为第六个普通文字标签**，沿用现有横向滚动 Tab，不单独增加图标或悬浮入口。进入统计页面时不累计学习时长。
6. 学习总时长 = 各有效学习片段的并集，单题耗时、试卷耗时和模块耗时是总时长的细分指标，**不能重复相加**。

## 2. 现有代码边界（已核对）

- `src/web/pages/training.rs`：`TrainingWorkspace`、`TrainingMode`、五种模式切换、`training_immersive`；主集成点。
- `src/web/pages/comprehensive.rs`：资料分析沉浸式答题，现有 `analysis-clock` 及服务端试卷 deadline/pause。**不重写试卷 deadline 计算。**
- `src/web/pages/logic_judgment.rs`：判断推理和数量关系共用页面，`logic-clock` / `quant-clock` 及暂停逻辑。
- `src/web/pages/calculation.rs`：速算训练和已有本题耗时。
- `src/web/pages/training.rs` 内 `FlashcardPractice`：错题复练及已有卡片思考用时。
- `src/web/pages/training_metrics.rs`：现有训练成绩样式，可参考但不混淆成绩与学习计时。
- `src/web/pages/study.rs`：Dashboard/Cards 存在旧的 `<CustomCountdownPanel />`，首页「专项训练」说明仍写“3种模式”。
- `src/web/hooks/countdown.rs`：旧三个自定义倒计时、localStorage 编解码和 PiP；同时包含考试日期倒计时与共享 PiP，**删除前必须逐个核查引用。**
- `frontend/app.css`：`.training-topbar-inner`、`.training-modes-inner`、沉浸式 `.analysis-header` 布局。
- `migrations/`、`src/adapters/inbound/http/`、`src/application/`、`src/adapters/outbound/`：既有 SQLite / HTTP 分层与校验惯例。

## 3. 计时状态和业务规则

### 3.1 两套互不干预的状态机

- 学习计时：`idle → running ↔ paused → ended`，在真实训练/复盘阶段启动。点击**学习时间**独立切换 `running/paused`。
- 试卷计时：沿用各模块已有 `prepared / active / paused / submitted` 与服务端 authoritative deadline；点击**试卷时间**仅触发原有试卷 `pause/resume`。
- 不使用单个 `isPaused` 同时控制两者，也不得在试卷 `toggle_pause` 里写学习 `pause`。保留试卷暂停的遮挡题目行为。
- 启动新一轮训练、恢复未完成的训练时，如果用户尚未手动暂停学习，则自动开启有效学习片段；单纯查看配置、出题等待、历史记录和统计不计时。可选的答案解析/错题复盘属于 `review` 片段。
- 模式切换时关闭旧模块片段并开启新模块片段；全局今日累计数字连续，不清零。取消/交卷不清空当日时长。
- 后台/离线不可靠期间不能无限累加：采用可配置的活动心跳和超时封口；返回时明确恢复或确认待计时段。**不能仅凭题目长时间无点击就断定没有学习**，以免误砍资料分析阅读时间。

### 3.2 显示和计算

- UI 每秒刷新，但**不能**按 `setInterval` 累加一秒。由已保存累计毫秒 + 当前活跃片段的时间差计算；使用客户端单调时钟改善前台平滑显示，并用服务端时间定期校准。
- SQLite 保存毫秒级 UTC 时间戳；每日统计按固定 `UTC+08:00` 日界切分，避免跨午夜被算到错误日期。
- 一个账号同时多设备/多标签页：同一时刻只允许一个可计费学习片段（按现有认证身份实现单活跃租约/抢占或明确拒绝），服务端幂等处理重复事件。

## 4. 持久化与 API

新增独立 `training_time` 领域/应用/适配器模块，不把计时逻辑塞进现有题目服务。

建议新增 SQL migration（名称以开发时最新序号为准）：

- `study_time_sessions`：`id`, `owner_scope`, `device_id`, `status`, `created_at_ms`, `last_seen_at_ms`, `ended_at_ms`, `revision`。
- `study_time_segments`：`id`, `session_id`, `module`, `phase` (`answer`/`review`), `training_run_id`（可空）, `started_at_ms`, `ended_at_ms`, `effective_ms`, `close_reason`。
- `study_time_events`（如需独立去重表）：`event_id UNIQUE`, `session_id`, `operation`, `server_at_ms`。如仅用 session revision 即可保证请求幂等，可省去此表。
- 索引覆盖 `session_id`、`started_at_ms`、`module`；严禁跨设备重复累计；跨零点查询按日拆分片段。

建议沿用当前 `/api/v1/study/` 路径和管理员认证/CSRF/Origin 约束：

- `GET /api/v1/study/training-time/current`：返回当前会话状态及今日累计。
- `POST /api/v1/study/training-time/events`：类型化 `start/pause/resume/switch/end/heartbeat` 事件；有 `event_id` 和乐观版本号以便去重和冲突处理。
- `GET /api/v1/study/training-time/stats?from=YYYY-MM-DD&to=YYYY-MM-DD`：每日学习总时长、模块/阶段拆分、连续天数及热力图数据。

不能从客户端传入可任意覆盖累计分钟数的字段。服务端控制有效时长、片段闭合、超时规则、归属和权限。对 API 增加固定路由、严格 body 校验、CSRF、同源和冲突响应测试。

## 5. Web 实现步骤

### P1. 共享学习计时器（先完成独立性）

1. 新增 `src/web/hooks/training_time.rs`（或更适合的独立状态组件），为专项训练提供共享状态、事件回调及每秒显示快照。
2. 在 `TrainingWorkspace` 安装单一 Provider/Context；状态由服务端 `current` 恢复，避免每个模式各自开一套计时器。
3. 新建复用组件 `StudyTimeInline`，**只渲染一个粉色时间值**，点击仅切换学习计时；普通专项训练顶栏和沉浸式答题顶栏都使用它。
4. 把共享计时器接入资料分析、逻辑判断、数量关系、速算训练、错题复练的 `answer/review` 生命周期。沉浸式界面使用相同 Context，不复制时钟状态。
5. 仅调整现有试卷时间的样式和点击区（白色、单行）；现有 `analysis-clock` / `logic-clock` / `quant-clock` 与暂停 API 行为保持不变。
6. iPad 横屏及手机竖屏测试点击热区、对齐、不换行、不遮挡「交卷」「返回」和题目导航。

### P2. 后端 SQLite 与同步

1. 添加 migration、纯计时规则的 domain 模块和单元测试。
2. 加 application service + port、SQLite outbound、Axum inbound，复用既有依赖注入和管理员权限机制。
3. 加上幂等 event、心跳、跨日拆分、服务端重启恢复、多设备单活跃策略。
4. 前端请求失败时显示可恢复状态；不能静默丢弃已确认的片段，也不能离线无限计时并在恢复时直接上报未经校验的累计时长。

### P3. 「学习统计」第六个普通 Tab

1. 在 `TrainingMode` 加 `Statistics`，在原 `.training-modes-inner` 追加「学习统计」文字标签；默认仍进入速算训练。
2. 新建 `src/web/pages/training_statistics.rs`（或同级命名），复用既有 `TrainingSetupLayout`/暗色样式：今日、本周、最近 30 天、最长/当前连续学习天数。
3. 按 `date -> effective_ms` 画日历热力图；颜色深浅按学习总分钟数分级，点击任一天显示日期、总时长、各模块/答题复盘拆分。空白日期显示 0，不伪造数据。
4. 追加各模块时长条形分布、过去 7/30 日趋势。若尚无数据，显示有指导性的空状态，而不是全为假数据。
5. 学习统计只读取新统计接口，不把单题时间汇总后再次计入全局时长。

### P4. 清理旧三段倒计时

1. 删除 `study.rs` 中 Dashboard/Cards 对 `<CustomCountdownPanel />` 的调用；同步修正专项训练“3种模式”的过时文案。
2. 将 `countdown.rs` 中仅服务旧上午/下午/晚上的配置、localStorage 状态和渲染逻辑剥离；移除已无引用的 `CountdownEditor` 及自定义 PiP 分支（如果确实不再被其他代码使用）。
3. **保留** `EXAM_COUNTDOWN_TARGETS`、考试日期倒计时、共享画中画和 `frontend/pip-countdown.js` 中仍被考试倒计时依赖的代码。
4. 更新 `e2e/countdown.spec.ts`，验证“旧三段消失、考试倒计时和考试画中画仍可用”；删除孤立测试与 CSS，不保留死代码。

## 6. 测试与验收（TDD）

遵守仓库 `AGENTS.md` 的逐行为 **Red → Green → Refactor**；跨模块功能必须独立分支、PR，最终候选 HEAD 相关 CI 全通过后才合并。先静态检查；没有额外授权不主动启动完整 Rust/前端/固件构建或部署。

应新增/扩展：`e2e/training-time.spec.ts`、`e2e/training-subjects.spec.ts`、`e2e/logic-judgment.spec.ts`、`e2e/countdown.spec.ts`，并覆盖后端 domain/application/HTTP 测试。

**独立计时验收用例（最高优先级）：**

- [ ] 开始资料分析：粉色今日正计时递增，白色试卷倒计时递减。
- [ ] 点击白色倒计时：试卷暂停、遮挡题目；粉色学习时长继续增长。
- [ ] 点击白色倒计时继续：试卷恢复；学习时长无跳变、未重置。
- [ ] 点击粉色正计时：学习停止增长；白色试卷倒计时继续递减。
- [ ] 再点粉色正计时：学习恢复；试卷时间不被重置。
- [ ] 五种模式均有同一全局学习计时；模式切换不丢今日累计、不发生双计。
- [ ] 非训练配置/历史/学习统计页面不累计；手动暂停不会被页面切换自动解除。
- [ ] 页面刷新、Safari 后台、断网、服务重启、跨设备单活跃、重复事件及跨午夜均不重复/虚高累计。
- [ ] 热力图每日汇总正确，点击日期能显示模块明细；历史记录在新设备可读取。
- [ ] 「学习统计」是第六个普通文字 Tab，无独立图标；两个计时器仅数字、单行、不同色。
- [ ] 旧自定义上午/下午/晚上倒计时消失；考试日期倒计时及 PiP 和试卷限时机制保持正常。
- [ ] 既有成绩、Flashcard FSRS、试卷原题重做、跨设备答卷恢复、Apple Pencil 手写行为无回归。

## 7. 完成交付

1. 独立 `feature/training-time-statistics` 分支和 PR，不直接改 `main`。
2. PR 包含 migration、新计时模块/API、共享 Web 计时器、学习统计页面、旧三段倒计时清理以及完整测试。
3. PR 描述列出交互/边界行为、数据库迁移兼容性、测试证据、可能风险与回滚方法。
4. `main` 仅在 PR 通过最终 HEAD 的相关 CI、人工确认原型效果后合并；不自动部署设备。

## 8. 非目标

- 不改已确认的考试日期；不引入新画中画学习计时器。
- 不改 FSRS 复习打分、出题逻辑、成绩公式或试卷倒计时的权威 deadline。
- 不把普通浏览页面停留时长直接算入有效学习，不对过去未记录的日期伪造历史。
- 不改 router、camera、proxy、系统导航或设备部署流程。
