# 资料分析 Codex App Server 出题 Agent Plan（2026-10-21）

状态：用户批准，实施中。本文替换 \`hyz-things-comprehensive-data-analysis-plan.md\` 的模型 Provider 设计，保留原有训练规格。

## 部署决策

- 电脑兼任 SMB Flashcard 服务器和 Codex App Server，固定地址 192.168.8.202。
- 电脑执行 \`codex login --device-auth\` 一次，凭据由 Codex 在电脑持久化，开机自动启动服务，不每次重新登录、不主动 logout。
- 官方 Codex App Server 监听 \`ws://0.0.0.0:1457\`，开启 \`--ws-auth capability-token --ws-token-file $HOME/.codex/app-server-token\`。
- RK3568 固定连接 \`192.168.8.202:1457\`；不安装 Codex、不运行 Grok OAuth 插件、不走 SSH 隧道/代理。
- 电脑建议用 \`umask 077; openssl rand -hex 32 > "$HOME/.codex/app-server-token"\` 生成高熵 Token；应用不限制 Token 长度，但只接受安全的 Bearer Token 字符。RK3568 把与 \`--ws-token-file\` 完全相同的内容写到 root-owned 0600 的 \`/userdata/hyz-things/.env\`：
  \`HYZ_THINGS_CODEX_WS_TOKEN=<same bearer token>\`。
- WebSocket Token 只证明局域网客户端身份，**不是** ChatGPT OAuth Token，无需每次自动同步；主动轮换时两端同步修改。
- 删除 \`HYZ_THINGS_AI_BASE_URL\`、\`HYZ_THINGS_AI_API_KEY\`、\`HYZ_THINGS_AI_MODEL\`、旧 \`AiConfig\` 与 \`/chat/completions\` 请求路径。连接地址常量固定，浏览器不得配置任意 URL。
- \`ws://\` 不提供加密，按用户对可信局域网的安全边界要求使用。建议防火墙仅允许板子访问端口；官方 WebSocket 传输仍属实验能力。

## 架构

iPad → RK3568 hyz-things（官方资料缓存、Flashcard 薄弱点、Rust 验证、SQLite） → WebSocket JSON-RPC + Bearer Token → 电脑 Codex App Server（持久 ChatGPT 登录） → Codex 云端模型。

## Agent 工作流

1. RK3568 读取国家统计局年度公报的缓存章节，提取主题和可引用原文。
2. 读取最多 10 张遮蔽数值的 Flashcard 错题，并按历史成绩选择薄弱考点。
3. 启动 WebSocket 协议：Upgrade 握手校验 HTTP 101 / Sec-WebSocket-Accept → initialize → initialized → thread/start（ephemeral、sandbox=readOnly、approvalPolicy=never）→ turn/start（严格 JSON Schema）。
4. Codex 返回改写材料及恰好 5 条含 evidence、operation、a、b、unit、explanation 的候选计算题。必须依据原文，禁止综合判断。
5. Rust 执行来源数字、题型、算式、唯一正确项、题量及至少 3 种计算考点的验证。模型不生成最终 A/B/C/D，程序生成。
6. 验证不通过时，将失败原因作为同一 Thread 的第二个 Turn 反馈，最多修复一次；仍失败拒绝该组。
7. 严格校验的 4 组×5 题入库后才开放训练。20 分钟计时、iPad 双栏与透明手写层、成绩持久化和 Flashcard FSRS 均不改动。

Agent 不获得网页任意命令和路径，禁止命令审批、写文件和后台任意资源调用。Token、Prompt 和模型原文不写入前端或普通日志。WebSocket 握手、帧长度、总事件数和单次生成均限时有界。

## 改动文件

- 新增 \`apps/rust/things/src/adapters/outbound/codex_app_server.rs\`：固定 LAN WebSocket + JSON-RPC 客户端。
- 修改 \`apps/rust/things/src/adapters/outbound/data_analysis.rs\`：旧 Provider 改成 Codex Session；继续使用现有 Rust 验算、题目缓存与训练业务。
- 修改 \`apps/rust/things/src/main.rs\`、HTTP status 和 \`src/web/pages/comprehensive.rs\`，删除旧配置提示。
- 修改 \`apps/rust/things/.env.example\`，新增 \`scripts/run-codex-app-server.sh\`。

## 验收

- fake WebSocket server 验证握手、Token、JSON-RPC 初始化、临时 Thread、结构化 Turn 事件及异常处理，CI 无需真实账号或外网。
- 原有 SQLite 20 题生命周期、权限/CSRF、选项生成和浏览器 Playwright 回归。
- 按 AGENTS.md 的 Red–Green–Refactor 完成，独立分支 + PR；合入前必须最终 HEAD 全部相关 GitHub Actions 通过。
- 部署电脑并启动 Codex 后，真实验证 RK3568 → 192.168.8.202:1457 出题并在 iPad 完成训练。
- CI 成功不等于真实电脑 OAuth、局域网连接和板端部署成功；三者需独立验收。

## 2026-10-21 增量：最多四组有限并行（已批准）

- 将此前每组串行等待的生成循环改为**分波次并行**：每波最多 4 个不同来源/主题候选，每组分别建立 WebSocket 和临时 Codex Thread，互不共享 JSON-RPC 流。
- 优先选取不同年份及不同主题；失败时从剩余官方材料补位，第二遍仅放宽年份/主题重复，不重复使用相同来源/章节，不重复请求已经失败的候选。原有单次最多 12 次新生成上限保留。
- 组结果可乱序结束，但按候选顺序写入 SQLite，并以 4 × 5 题通过校验为正式训练准入条件。独立题组验算和同 Thread 单次修复保留。
- **暂不复用单个 WebSocket**：多个 Thread 复用一条 socket 需要独立消息分发器、JSON-RPC request ID 与 event stream 路由，容易串线；局域网内 4 次握手开销远小于模型生成时间。后续如出现瓶颈，再独立规划复用连接。
- 验收必须包含：选择规则单测、真实并发屏障单测、原有 mock App Server 协议测试、GitHub Actions Rust fmt/test/clippy、前端单测/构建以及 **Playwright E2E 全量执行**。不允许以“构建通过”代替端到端验收。
- 同一 PR 顺带清理旧构建日志中 Canvas 过时 API、仅测试用字段 dead_code 和 training 测试 unused import 警告；不更改实际绘画和速算行为。

- 并行度是**最多**四组，不是始终四路：优先使用已验证的 SQLite 缓存，只有尚未缓存的材料才需要新 Codex 会话；失败的候选不会占据最终四组名额。实际速度由电脑端模型限流和出题/校验时间决定。
