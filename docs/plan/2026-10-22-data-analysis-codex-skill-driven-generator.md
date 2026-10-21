# 资料分析综合训练：Codex Skill 题型驱动出题引擎（2026-10-22）

状态：用户批准并实施。本文件为实施记录和验收边界；原历史设计保留。

## 核心决定

- 将流程由「先改写官方材料 → 从原文数字凑题 → Rust 验算」改为「Codex 读取 hyz Skill/Flashcard → 先规划5题型 → 构造自洽模拟数据 → 组织材料和四选一 → Codex 自检 → 应用展示」。
- 当前保留每次4组×5题、20分钟、暂停/恢复、历史回看、同材料找数5题和跨模式去重。
- 每组以独立 ephemeral Codex Thread 生成，最多4组并发；软目标全卷12道个人易错、6道其他核心、2道基础，按不同官方统计主题分组。Codex 能自由使用只读工具，不强制 Python。
- 使用电脑仓库 `.agents/skills/行测做题/SKILL.md` (name: `hyz-xingce`) 和 `flashcards/资料/` 的**当前文件**。不把卡片正文预先复制到 RK3568 的 Prompt；Codex 自主读取。
- 允许构造模拟值；官方公报提供年份、主题、主体、指标、统计口径和写作风格，不声称模拟数值为国家统计局真实数据。网页与材料明确标识模拟训练。
- Codex 直接返回 `question/options[4]/correct_index/answer/skill/evidence/explanation`；不再由 hyz-things 通过 `Operation` / `verify_recipe` 生成正确项、干扰项或验算。
- **取舍**：模型可能生成有误试题；这是用户明确选择的无 Rust 内容校验方案。Rust 仅作 JSON 解析、5题/4选项/索引可渲染等结构限制、权限/CSRF/超时/资源限制、持久化和成绩逻辑。并不宣称数学正确性由程序保证。

## RK3568 配置

`/userdata/hyz-things/.env`：须为 root 拥有、权限 0600 的普通文件，不允许符号链接，继续沿用旧版 Token。

```dotenv
HYZ_THINGS_CODEX_WS_TOKEN=<与PC端codex app-server token-file相同>
HYZ_THINGS_CODEX_CWD=/home/<PC用户>/hyz_things
```

- CWD 是**电脑上 Codex App Server 进程能够读取的真实绝对路径**，由 RK3568 后端原样作为 `thread/start.cwd` 发给 App Server；不在板端展开 `~` 或判断电脑目录是否存在。不能从浏览器传入 CWD。
- 配置校验只允许绝对路径、禁止 `..` 及控制字符；必须同时配置 Token 与 CWD，避免隐式回退至 `/tmp`。
- PC 上执行 `cd /home/<PC用户>/hyz_things && pwd -P` 确认真实目录，并确认 `AGENTS.md`、`.agents/skills/行测做题/SKILL.md`、`flashcards/资料/` 可读。PC 端按已有 `scripts/run-codex-app-server.sh` 启动。
- 保持 `sandbox: "read-only"`、`approvalPolicy: "never"`、`ephemeral: true`。Codex 可自行决定读取文件、执行可用的只读工具，客户端不批准交互式写操作或越权请求。
- 更新电脑工作树中的 Skill/Flashcard 后，**下一次新 Thread** 可读取新版；正在执行中的 Thread 不保证热更新，RK3568 单独更新卡片数据库不等于 PC 文件已同步。
- 新出题结果使用 `QuestionGroup.version = 4`，旧版结果和练习历史保持兼容；旧版缓存不会被新生成器当成新题使用。

## 材料与去重

- 旧的官方原文改写仍按 `source_url + section` 识别已见材料。
- 新的模拟材料基于材料文本 SHA-256 去重：同官方主题但不同模拟数据可视为陌生材料，同一模拟材料跨综合/找数会认作已见。
- 新旧数据仍存于 `data_analysis_groups`，找数仍复用已保存材料。前端官方参考资料链接明确为“口径参考”，不代表模拟数值是真实官方数据。

## 验收

1. 配置文件 Token+CWD 缺一不可，非法路径拒绝；PC 端路径不在 RK3568 校验其文件存在性。
2. 假 App Server 测试验证 `thread/start.cwd`、只读沙盒和最终回答 schema；只读命令事件不被误认为交互审批请求。
3. Native 单测确认新版本直接消费 Codex 提供的选项与正确下标，不进行 Rust 数学复算。
4. 保持完整20题生命周期、暂停、结果、历史、找数训练；不同模拟材料跨模式去重。
5. 最终提交在 GitHub Actions 上通过静态、Rust fmt/test/Clippy、Web build/unit、Playwright E2E。
6. **CI 无法替代真实 PC/RK3568 冒烟**：部署后还需确认 App Server 能读取 Skill 和最新 Flashcard、生成题型驱动的模拟卷，实际用户选择需以线上验证为准。

## 实施限制与后续

- 使用每篇独立 Codex Thread 规划5题，整套20题由后端选4个主题并提供整卷题型配比软目标；不是再引入第二套 Rust 题型规划器。
- 不强制命题阶段运行 Python，不要求有工具执行证明；错误题目依赖 Codex 自检、用户人工复盘，而非 Rust 校验器。
