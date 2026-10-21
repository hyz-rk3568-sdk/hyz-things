# hyz-things 资料分析综合训练 · 20题20分钟

状态：经用户批准实施；完整设计规格见对话中 2026-10-21 的 Review Plan。
一级学习导航不变，专项训练增加第三模式「综合训练」。
首篇官方文字资料：https://www.stats.gov.cn/sj/zxfb/202602/t20260228_1962662.html
固定题量：4组×5题；限时1200秒。文章12章、细主题与行测考点分别建模，资料缓存可复用。
接口必须由后端发起官方抓取和 OpenAI-compatible API 调用；最终仅经校验的题可用于正式训练。
RK3568 配置路径：/userdata/hyz-things/.env；变量为 HYZ_THINGS_AI_BASE_URL、HYZ_THINGS_AI_API_KEY、HYZ_THINGS_AI_MODEL。
主要终端 iPad Safari：横屏44/56双栏材料+题目、两个独立透明笔层，竖屏堆叠，超时自动交卷。
后端保存训练成绩与主题考点表现，不触动 Flashcard FSRS；权限、CSRF、安全脱敏与离线CI为上线条件。


## iPad 与 RK3568 配置验收

- 在 RK3568 的 `/userdata/hyz-things/.env` 写入 `HYZ_THINGS_AI_BASE_URL`、`HYZ_THINGS_AI_API_KEY`、`HYZ_THINGS_AI_MODEL`。
- 配置文件必须由 root 拥有、文件权限 0600；修改后重启 hyz-things 服务，API Key 不经过浏览器。
- iPad Safari 进入「学习 → 专项训练 → 综合训练」。请先以管理员身份登录，再点击「智能出题并开始」。
- 题目生成必须从国家统计局公报获取文字和来源，缓存至 SQLite，严格验证后组成4组×5题。
- 开始训练时才启动20分钟计时；测试横屏左右双栏、选项保存、手写跨题保留、页面刷新恢复、到时交卷与成绩持久化。
- CI 使用 fake API 和虚拟数据执行，真实 DeepSeek 模型、RK3568 出站网络与 iPad Apple Pencil 手感必须在部署后另外验收；未通过不得标记为板端验证成功。
