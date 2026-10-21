# hyz-things 资料分析综合训练 · 20题20分钟

状态：经用户批准实施；完整设计规格见对话中 2026-10-21 的 Review Plan。
一级学习导航不变，专项训练增加第三模式「综合训练」。
首篇官方文字资料：https://www.stats.gov.cn/sj/zxfb/202602/t20260228_1962662.html
固定题量：4组×5题；限时1200秒。文章12章、细主题与行测考点分别建模，资料缓存可复用。
接口必须由后端发起官方抓取和 OpenAI-compatible API 调用；最终仅经校验的题可用于正式训练。
RK3568 配置路径：/userdata/hyz-things/.env；变量为 HYZ_THINGS_AI_BASE_URL、HYZ_THINGS_AI_API_KEY、HYZ_THINGS_AI_MODEL。
主要终端 iPad Safari：横屏44/56双栏材料+题目、两个独立透明笔层，竖屏堆叠，超时自动交卷。
后端保存训练成绩与主题考点表现，不触动 Flashcard FSRS；权限、CSRF、安全脱敏与离线CI为上线条件。
