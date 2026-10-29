# 逻辑判断专项训练：VCNT V2 复用与专项训练页面接入（2026-10-29）

**2026-10-29 分类调整**：依据小P形式逻辑第1—4课，原「分析推理」方法卡并入 `flashcards/判断/形式推理/card.md`；专项训练只保留形式推理 `formal`，排除一拖五。下文记录的是首次实现时的旧设计，供历史追溯。

状态：功能代码已实现，主机测试、契约夹具浏览器验证与网络 ADB 热部署基础验收已完成；真实 Codex 出题及 iPad 真机验收待执行。命题正确性与自检按用户当日调整交给 Codex。

## 用户确认范围

- 入口：**学习中心 → 专项训练 → 逻辑判断**，作为现有 \`TrainingMode\` 第四个模式。
- 不增加顶层应用/导航入口，不改资料分析既有「速算训练」「错题复练」「综合训练」行为。
- 仅 **形式推理 + 分析推理**；排除论证推理、原因解释、图推、定义、类比、日常结论、分析推理的一拖多/多条件综合题组。
- 每卷 **10 道四选一独立题，10 分钟**，题型比例由 VCNT V2 自动调度决定，**不强制 5:5**。
- 依据现有 \`flashcards/判断/形式推理/card.md\`、\`flashcards/判断/分析推理/card.md\` 和对应 \`错题/{形式推理,分析推理}/cards.md\` 的真实、已同步卡片命题。题干使用当次生成的新内容，不直接把方法卡当选择题展示。
- 沿用 hyz 原有做题套路；已有卡片与 Skill 中源自用户的规则优先。新增“基础通用规则（待实战复盘）”不可宣称是用户本人方法。

## 页面集成

当前入口代码：\`apps/rust/things/src/web/pages/training.rs\`。现有模式枚举：\`Calculation\`、\`Flashcards\`、\`Comprehensive\`。

新增 \`LogicJudgment\` 模式及「逻辑判断」切换按钮，模式内容由单独 \`pages/logic_judgment.rs\` 组件承载，避免继续增大 \`training.rs\`。

- 设置/待开始：显示 10题/10分钟、形式+分析、VCNT 自动选卡、卡片池有效数量、出题状态和历史记录。
- 出题：管理员权限要求与现有 Codex 综合训练一致；从后端创建任务、轮询/恢复状态。切换页面不丢失已准备任务。
- 作答：复用固定高度沉浸布局、Apple Pencil 透明草稿层、题号导航、上一题/下一题、全卷倒计时；无需资料分析的 **60:40 材料区**，改为单题干/选项区。
- 交卷：不展示未提交试卷的答案；提交后查看正确率、每题作答/正确答案、解析、关联卡片、错题。
- 恢复：后台存储试卷、答案与倒计时截止时间。刷新可恢复，历史试卷可重做，但重做不增加 VCNT。

## 调度复用，避免复制出第二套算法

当前事实依据：\`docs/plan/2026-10-26-data-analysis-global-vcnt-v2.md\` 与 \`apps/rust/things/src/adapters/outbound/data_analysis.rs\`。

抽出可单测的 **公共卡片级 VCNT 状态与权重计算**；资料分析继续使用现有 4×5 分组/同组知识点互斥的最小费用流，不应为了逻辑判断破坏其既有输出。

逻辑判断只有十个平铺题位且没有“同组互斥”约束；每次选十张不同、有效的卡片，按全局最低 VCNT 排序，在并列时用 task seed 可复现地打散，**无两类题型硬配额**。方法卡与对应错题卡合并为同一调度池。

\`\`\`text
p = (correct + 2) / (answered + 4)
w = clamp(1 + 0.8 * (0.5 - p), 0.7, 1.3)
delta_vcnt = round(1024 / w)
\`\`\`

- 新卡从逻辑判断候选池现有最小 VCNT 起步。
- **仅完整新卷成功入库**，所选卡片 \`selected_count+1\`、\`vcnt += delta_vcnt\`；失败、取消、重做不增加。
- 计算增量时使用任务规划期间冻结的、已交卷且**实际作答**的卡片成绩；未答题不得记错。
- 保留更新前 VCNT 快照，入库事务中乐观校验；并发冲突回滚并重选。
- 选择时校验至少10张不同、带有效 UUID、Front/Back 非空的已同步卡；缺少则显示明确错误，不重复用同一卡凑数。
- 资料分析与逻辑判断分别存储 VCNT、出题记录和作答成绩，**不混用统计表**；也不改普通 Flashcard/FSRS。
- 无需复用现有 \`data_analysis_card_usage\` 表本身。公共化的是调度**逻辑**和数据库事务模式，不是两种训练的状态。

## 命题验证

- Codex 按指定的 \`target_id/card_id/card_path/front/back\` 逐卡生成一题，10题恰好覆盖计划目标，不允许擅自换卡或重用目标。
- 独立题均须四个答案选项且只有一个正确项；不能出现一拖多或阅读材料题组。
- **2026-10-29 用户调整：题目正确性与语义审核由 Codex 完成，沿用资料分析出题方式。** Codex 自检真假、逆否、集合、平行结构、分析约束与答案唯一性；应用只接收四选一题目、正确答案和解析。
- 应用只检查入库必需的输出格式、题数、选项数量、答案索引范围、内容非空以及计划目标一一对应。题目逻辑、自然语言歧义和答案唯一性均交给 Codex，不要求结构化命题模型、枚举器或额外规则验题。
- 命题提示保留用户原有方法优先与题型排除要求，由 Codex 完成出题及自检。
- 后端统一回填卡片来源，入库/发卷端要防止在交卷前暴露 \`answer_index\`、解析、卡片 Back。
- 出题失败或输出格式、目标不完整时，整卷拒收入库并保留重试入口；此类失败不产生答卷成绩。题目质量由 Codex 负责自检。

## 后端组织

建议从现有 \`data_analysis\` 垂直模块抽出最少必要的公共件：\`vcnt\` 权重、调度选择、快照读取与统计，但不要在第一步重写或迁移资料分析已有数据库表。

新增独立 \`domain/logic_judgment.rs\`、\`adapters/outbound/logic_judgment.rs\`、\`adapters/inbound/http/logic_judgment.rs\`、\`web/api/logic_judgment.rs\`、\`web/pages/logic_judgment.rs\`；名称可按实际项目模块组织适当调整。

建议独立 REST 命名空间：\`/api/v1/study/logic-judgment/*\`，其中包含：
- status、prepare、prepare/{job_id}；
- runs/{id}、runs/{id}/start、runs/{id}/answers、runs/{id}/submit、runs/{id}/result；
- unfinished、history、card-performance、user-prompt。

后端沿用现有认证、CSRF、并发控制、停机重启恢复和事务安全机制。

## 实施次序与验收

1. 抽出公共的 VCNT 加权计算与可测试调度逻辑，保持资料分析已有测试/行为不变。
2. 新增逻辑判断专用的持久化表、真实卡池读取、10卡调度、快照与事务更新测试。
3. 实现10道独立题的 Codex 生成与自检、输出格式检查、准备任务及恢复。
4. 实现10分钟答卷、批改、重做、历史/统计 API。
5. 在专项训练中挂载第四个逻辑判断模式和移动端/Apple Pencil 答题界面。
6. Rust 单测、fmt、clippy、前端构建、Playwright E2E、已有资料分析回归；验证超时、恢复、并发计数、题型来源排除、相同卡片不重复、格式错误重试，以及 Codex 提示中的答案唯一性自检要求。

**已完成门户网络热部署基础验收；完整真实 Codex 出题与 iPad 真机验收仍需执行。**


## 实施与验证记录（2026-10-29）

- 共享 `domain/vcnt.rs` 提供卡片级状态、原有加权公式与 seeded 去重选择；资料分析保留原有 4×5 最小费用流和统计表。
- 逻辑判断新增独立持久化表、生成端口和管理员 HTTP API；真实已同步卡片参与十卡调度，完整新卷事务入库后更新 VCNT。准备失败、停止、快照冲突和原卷重做均有测试覆盖。
- 服务由 `main.rs` 构造，启动恢复中断任务，LAN/Tailscale 共用服务，关闭时取消生成任务。
- 第四模式已接入专项训练，包含单题沉浸作答、透明草稿、服务器答案与截止时间恢复、结果/卡片/历史/统计、原卷重做及用户提示。沿用现有 PWA 安全区样式，保存/恢复默认提示时回填服务器确认值。
- `make check-static`、`cargo fmt --check`、native 全量测试（141 个 library + 9 个 integration）、包含 e2e 入口的 native 严格 Clippy 均通过；资料分析 24 个 adapter 测试包含在内。
- release 前端构建通过。Chromium 三个训练测试文件共 41 项通过：逻辑判断 12 项、综合训练 16 项、闪卡/速算/错题复练 13 项；覆盖桌面、iPad 尺寸、手机尺寸、Pencil pointer、PWA 安全区、失败重试、到时保存竞态和刷新恢复。测试整理后再次通过默认提示/PWA 两项检查。
- 浏览器逻辑判断 API 使用 DTO 契约 mock，生成服务使用 application-level fake port；网络热部署基础验收见下节，完整真实 Codex 出题及 iPad Safari/Apple Pencil 真机验收待执行。
- 额外执行的 Web/WASM `clippy -D warnings` 被既有 9 条诊断阻塞，涉及未改动的 `web/ui.rs`、`web/protected_control.rs`、`web/resources.rs`、`web/pages/scratchpad.rs`。本轮保留这些模块的原有实现。


## 网络 ADB 热部署验收（2026-10-29）

- 复用现有交叉工具链，通过 `make -o toolchain things-app` 构建 ARM64 门户 ELF；使用 `deploy-app.sh deploy things <ELF>` 执行网络 ADB 热部署。
- 主机、设备 `/usr/bin/hyz-things` 与注册表 SHA-256 一致：`d5d6bfc9f12c15c0eeff70a5eb7c25db48306ea74937a2f2dd53f83c304d75aa`；协议保持 router 13 / camera 3。
- 部署仅重启门户；router、camera 的 PID/启动时间及 boot ID 保持一致，router/things ready 标记均存在。
- 迁移后 `20261029120000` 已应用，五张逻辑判断表存在，数据库 `quick_check` 通过。
- 迁移前数据库通过 SQLite Backup API 在线快照并校验，保存在 `/userdata/hyz-things/apps/backups/flashcards-pre-logic-20261029T083446Z.db`，SHA-256 为 `668b23c8b50d3d1ff99ec077f971c5305a43597085eb85885b70b7dd81115aa0`。旧 ELF `21e42b69f1ba913759b704edc67cab0ae6cc74b16a5d7f7a2de5aace0aaf6f13` 已保留并登记为 previous。
- 本次新增学习数据库迁移。回退旧版学习功能时应先保存回退时的当前数据，再配套恢复迁移前数据库与旧 ELF；数据库回退会丢弃备份点之后的学习记录，需要确认数据回退范围。
- 真实 HTTPS 验证：未登录读取逻辑状态返回 401；管理员读取 status、unfinished、history、card-performance、user-prompt 均返回 200/no-store。桌面/手机浏览器完成第四模式切换、用户提示读取、综合训练/速算切换和刷新恢复。
- 首次验收时 `configured=true`、`eligible_cards=4`，出题按钮按至少十张卡片的约束禁用；随后同步后复核为 `eligible_cards=16`，真实页面生成按钮启用，桌面/手机与刷新检查再次通过。本次验收没有创建新卷或增加 VCNT；原有暂存卡片及同步补充的 UUID 改动单独保留。
- CI 能力套件已登记新增的 12 项逻辑判断测试，能力基线为 99 项声明 / 98 项可运行；含独立 PWA 套件的 Playwright 实际发现 101 项测试。
