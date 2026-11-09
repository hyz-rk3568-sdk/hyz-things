# 申论真题导入、Schema 扩展与图片 AI 解析试点计划

日期：2026-11-09。**状态：已实施；本地源码、迁移、测试、图片上传与 OCR 服务验证通过，设备热部署验收进行中。**

## 一、范围与约束

本批次仅完成以下设计对应的后续实施：

1. 从 [ProfessorZhi/gwy-zhenti](https://github.com/ProfessorZhi/gwy-zhenti) 筛选 **2017—2026 年**国考、湖南省考的申论真题。
2. 将本批申论试卷转换成新的 `hyz-paper/v2` 格式，放入既有 `question_bank/papers/` 目录；已有行测 `hyz-paper/v1` 文件原样保留。
3. 为原始题库增加最低限度的科目、作答类型、题型、判分模式和分值字段，以及主观题参考答案的独立存储位；增加相应 SQLite migration 和解析/同步适配。
4. 保留原卷给定材料、作答要求、参考答案与解析，并做可重复的结构和图片校验。
5. 扩展**现有真题 AI 解析**的图片上传和视觉追问输入，新增项目内 PaddleOCR Skill、宿主机本地 OCR 服务管理方案，并按用户要求将 Codex App Server 线程统一改成 danger-full-access。

**不做：** 不新增第三种 `kind`；不加入职测、综应；不增加 `source_url`、`bank_id`、`exam_series`、字数结构化字段等非必要元数据；不新增申论业务页面或阅卷功能；本次图片试点仅扩展现有 AI 解析输入与临时附件传输；图片仅放临时目录，不修改题库共享、设备挂载和现有永久题库数据。

**版本原则：**已有 `hyz-paper/v1` 是历史只读格式（允许现有 UUID 补写），缺省字段在解析及 SQLite migration 中补齐，不批量回写旧 Markdown；本批新申论真题与此后新建的所有试卷均使用 `hyz-paper/v2`，不再新建 v1 文件。一次 migration + 一次同步即可完成既有数据兼容（正式同步需待解析器升级后执行）。原有行测真题与 UUID、练习历史、收藏、错题不作重置或迁移改写。提交代码前必须先审计测试现有兼容行为。

## 二、来源范围与固定版本

上游：`ProfessorZhi/gwy-zhenti`，正式实施时先固定 Git commit，保存到本地导入清单，避免重复导入时源数据无提示改变。来源仓库一卷一个 Markdown，图片位于年份下的 `图片/` 目录，文件中包含给定材料、题目、折叠参考答案/解析；其内容不等同于经官方认证的原卷或官方答案。

| 地区 | 上游路径 | 年份 | 已核对的申论文件 |
| --- | --- | --- | ---: |
| 国考 | `国考/<年>/申论-*.md` | 2017—2026 | 25 |
| 湖南省考 | `省考/湖南/<年>/申论-*.md` | 2017—2026 | 23 |
| **合计** | | | **48** |

逐年扫描实际文件名，不假定每年卷型固定。国考含副省级、地市级，以及有来源文件的行政执法卷；湖南保留实际出现的统考、通用卷、乡镇卷、县乡卷、省市卷、行政执法、选调生等。**湖南 2026 年在当前来源中仅有县乡卷**，不能虚构其他卷型。既存文件可能存在缺题、缺材料、缺图或人工核验未完成状态；逐卷报告。

## 三、落盘目录与来源边界

不新增 `question_bank/shenlun/`，沿用原有年份路径：

```text
question_bank/
├── papers/
│   ├── 国考/
│   │   ├── 2017/申论-副省级.md
│   │   └── 2026/申论-行政执法.md
│   └── 省考/
│       └── 湖南/
│           ├── 2017/申论-统考.md
│           └── 2026/申论-县乡卷.md
├── assets/                  # 仅原卷实际引用的图片，按 SHA-256 命名
└── collections/             # 原有 kind: drill 专项合集，保持不变
```

国考、湖南只是 `region`/路径分组；申论真题仍为 `kind: exam`。不把整卷导入 `collections/`，不影响现有 `kind: drill` 语义。

**权利边界：** 上游 README 将内容限定为学习研究用途，试题、参考答案、解析和图片还涉及第三方权利，不能把仓库可见当作允许整库再分发。实施时先导入**本地私有工作区**并审计授权；未确认分发许可前，公开仓库只提交必要源码/迁移和不含第三方全文的清单，不提交完整申论卷、图片或转换脚本。

## 四、引入 `hyz-paper/v2`，同时向后兼容 v1

**新写入统一使用 `hyz-paper/v2`，旧 v1 保持可读。** 解析器须同时支持 v1、v2：v1 缺失 `subject`、`answer_type`、`grading_mode` 时分别默认 `xingce`、`objective`、`exact`，不自动回写这些新字段；v2 则强制声明试卷级 `subject`，以及每题的 `answer_type`、`grading_mode`，缺失或未知值应拒绝导入。`question_type`、`max_score` 仍按来源可选。现有 `Header` 解析器支持 `title`、`year`、`region`、`variant`、`role`、`kind` 等；题级元数据已有 `module`、`category`、`topics`、`materials`。

### 4.1 试卷级

**v2 必须声明 `subject`**，本批允许 `xingce` / `shenlun`。v1 缺失时解析默认 `xingce`，不修改旧文件。

```yaml
---
schema: hyz-paper/v2
kind: exam
subject: shenlun
title: 2026年国考申论（副省级）
year: 2026
region: 国考
variant: 副省级
role: primary
---
```

v2 不定义 `status` 字段；历史 v1 文件里已有的 `status` 继续被忽略。本次不新建来源 URL 等元数据。该示例仅演示结构。

### 4.2 题目级

在每道题现有 `yaml` 块中增加：

| 字段 | 取值 | 兼容逻辑 |
| --- | --- | --- |
| `answer_type` | `objective` / `subjective` | v2 必填；仅 v1 缺省 `objective` |
| `question_type` | 非空字符串或缺省；申论可用 `summary`、`analysis`、`measures`、`document_writing`、`essay`、`other` | 不明确则 `other` 或留空；不凭题号猜作文 |
| `grading_mode` | `exact` / `ai` / `manual` | v2 必填；仅 v1 缺省 `exact`，本批申论题标记 `ai`，不意味着已实现 AI 评分 |
| `max_score` | 正数或缺省 | 仅源题明确给出分值时写入 |

原有 `module`、`category`、`topics`、`materials` 保留。申论材料关联必须准确：明确关联的按来源对应材料编号映射；全卷适用的给定材料保持全卷可访问；无法核实时报告而不是编造引用。

### 4.3 原题与参考答案分离

当前 `question_fields()` 识别 `Stem / Options / Answer / Explanation / Notes`；`Answer` 当前按选择题字母解析。**本批新增 `Reference Answer` 区段**，只用于保存主观题原始参考答案/范文；`Explanation` 保留原始解析，`Answer` 对申论留空。

转换后的结构示意（不代表真实题目或分值）：

~~~~markdown
## Material M1

（给定材料 1 原文）

## Question 1

```yaml
module: 申论
category: 概括归纳
answer_type: subjective
question_type: summary
grading_mode: ai
max_score: 10
materials: [M1]
```

### Stem

（完整题干和作答要求）

### Options

### Answer

### Reference Answer

（来源提供的主观题参考答案）

### Explanation

（来源提供的解析；没有则保留缺失状态）
~~~~

原来的 `hyz-paper-id`、`hyz-question-id`、`hyz-material-id` UUIDv7 注释机制不变。转换器应输出可被现有补写机制补齐身份标记的 Markdown；重复导入不得更换已存在的 ID。**`question_fields()` 必须先显式支持 `Reference Answer`，否则新章节可能被错误拼进上一字段。**

## 五、SQLite migration（独立 question-bank 数据库）

位置：`apps/rust/things/migrations/question_bank/`。新增**一个** migration，不修改任何已执行的 migration。以下是拟议 SQL，实际提交前需检查现有索引、metadata JSON 与 SQL 查询兼容性。

```sql
ALTER TABLE papers
  ADD COLUMN subject TEXT NOT NULL DEFAULT 'xingce';

ALTER TABLE questions
  ADD COLUMN answer_type TEXT NOT NULL DEFAULT 'objective';

ALTER TABLE questions
  ADD COLUMN question_type TEXT;

ALTER TABLE questions
  ADD COLUMN grading_mode TEXT NOT NULL DEFAULT 'exact';

ALTER TABLE questions
  ADD COLUMN max_score REAL;

ALTER TABLE questions
  ADD COLUMN reference_answer TEXT;

CREATE INDEX IF NOT EXISTS idx_bank_papers_subject
  ON papers(subject);
```

- Migration 为已有 v1 行补齐默认值：旧 `papers.subject = xingce`、`questions.answer_type = objective`、`questions.grading_mode = exact`；旧文件不需要逐份补写 YAML。一次同步后 SQLite 保持统一结构。**v2 不依赖这些数据库默认值，文件中必须明确填写必填字段。**
- `grading_mode` 是题目声明的**目标评分方式**，不代表当前具备相应的执行能力。`exact` 表示按标准选项精确判分，`ai` 表示预留 AI 主观评判方式，`manual` 表示人工评阅；与 `answer_type` 独立，不能只靠主客观类型推断评分模式。当前版本不新增 AI 评卷或人工评卷业务逻辑。
- `correct_answer` 仍用于客观选项，`reference_answer` 是单独的主观题参考内容；不得向客观判分字段写入范文。
- 现有 `gradable` 保持“有可用选择题结构与答案”的含义，**不能根据 `grading_mode: ai` 将其设置为 1 或接入现有选择题判分**。申论题不具备客观选项结构时为 `0`，不能由此推断题目无法保存/阅读。
- 同步层必须把 Markdown `subject` 写入 `papers.subject`，题级新增字段写入 `questions`；读接口/DTO 需保证一致性。当前 `papers.metadata` 仍保存部分索引信息，读写 SQL 必须检查，避免数据库字段与 JSON 快照产生不同值。
- 现有 `practice_sessions`、`practice_items`、`practice_attempts`、`question_progress`、`favorites` 保持原样；本批不新增作答或阅卷表。
- 迁移前备份 `question-bank.db`，验证旧数据库增量升级和新数据库初始化；不要通过删除数据库重建来“升级”。

## 六、转换器与同步适配

### 6.1 一次性临时转换脚本

转换器作为一次性临时脚本运行，放在项目根 `tmp/`（已 `.gitignore`）或 `/tmp`，不加入 `scripts/`，也不提交到仓库。仓库中不需要新增长期维护的申论导入工具；脚本只负责结构转换、图片复制和审计报告，解析规则仍以设备端 Rust 解析器为准。

处理步骤：

1. 固定上游 commit，并只扫描国考、湖南省考 2017—2026 年 `申论-*.md`。
2. 读取上游中文 frontmatter，核对年份、地区、科目、卷型；同名目标文件冲突拒绝静默覆盖。
3. 转换 `## 给定材料 / ### 材料 N` 为 `## Material MN`，保持材料编号、原文和次序。
4. 转换 `## 题目 / ### 第 N 题` 为 `## Question N`，保留题号、题干、字数要求和明确分值。
5. 将折叠答案块拆成 `Reference Answer` 与 `Explanation`；允许一题有多个来源参考答案，忠实保存，禁止 AI 改写。
6. 图片复制到现有 `question_bank/assets/<sha256>.<ext>`，重写正文相对路径；验证每一个实际引用图片存在且哈希匹配。
7. 做完整性审计：材料数量和编号、题目数量、缺题占位、答案缺失、图片引用、UTF-8 和解析器可读性。
8. 输出 dry-run 差异、冲突和缺项报告到本地临时目录；经确认后执行增量导入。保留已存在 UUID，不覆盖人工编辑；再次执行应幂等。

源内容可能含不可信 Markdown/HTML，导入过程仅作为数据解析和渲染，不执行其中任何命令、脚本或外链操作。

### 6.2 现有代码的必要适配点

- `apps/rust/things/src/application/question_bank/document.rs`：版本分支解析、v2 必填字段校验、v1 缺省兼容、`Reference Answer` 解析、唯一编号与 UUID 校验。
- `apps/rust/things/src/domain/question_bank.rs`：需要展示/传递的 `subject` 和题目属性 DTO。
- `apps/rust/things/src/application/question_bank/mod.rs`：`StoredQuestion` / `PaperContent` / `IndexedPaper` 元数据投影更新。
- `apps/rust/things/src/adapters/outbound/question_bank_sqlite.rs`：同步插入/更新、读取和 metadata 一致性，避免旧字段被覆盖。
- `apps/rust/things/src/adapters/outbound/question_bank_source.rs`：保持现有目录扫描、文件大小、图片哈希及 UUID 写回规则，不为第三方源增加运行时网络访问。

不修改现有训练页面与答题逻辑；如现有客观题查询会混入申论题，应在**现有查询范围**加 `subject = 'xingce'` 约束，以确保原行为不变，而不是增加新训练入口。

## 七、申论真题与 Schema 验证

**必须通过：**

- [ ] 2017—2026 国考 25、湖南 23 份申论源文件全部逐份进入导入审计清单；每份标记成功/缺项/失败，不用总数掩盖缺损。
- [ ] 解析器同时接受 v1、v2；旧 `hyz-paper/v1` 行测解析、UUID 写回、内容指纹/同步和重复导入回归通过；旧 Markdown 不因新增字段发生批量改写。
- [ ] v1 缺省的三个字段由 migration / 解析兼容补齐；新迁移能升级旧数据库且不重建个人历史表；v2 缺少 `subject`、`answer_type`、`grading_mode` 必须拒绝导入。
- [ ] 申论材料序号与题号完整对应，题干要求、参考答案、解析不混淆、不丢失。
- [ ] 所有保留的图像引用经 SHA-256 文件与引用路径校验。
- [ ] 主观题 `Answer` 不被当作选择题选项解析；`Reference Answer` 不进入作答前题目 DTO。
- [ ] `grading_mode` 完整经过 v2 Markdown 解析、SQLite migration、同步和读取；v1 旧题默认为 `exact`，新申论 v2 显式保存 `ai`，不触发任何未实现的评分任务。
- [ ] 现有 `exam / drill` 语义、行测客观判分和练习统计保持不变。
- [ ] 未确认再分发许可前不将第三方完整原文/图片加入公开仓库；临时转换脚本和逐卷审计产物留在本地 `tmp/` 或 `/tmp`，不提交。


## 八、现有真题 AI 解析图片试点

**范围只限已有真题 AI 解析入口**：允许在首次解析指令及后续追问添加 JPG/PNG 图片（初定每次至多 3 张、每张不超过 8 MiB，最终以设备测试确定）；文字和图片随同一条消息发送 Codex。暂不支持 PDF、不增加申论作答或阅卷页面。

### 8.1 使用项目内 `tmp/`，不经过 SMB

**图片是临时 AI 消息附件，不属于题库原始资料；SMB 只是此前复用现有共享的设想，实际没有必要。** 本试点采用独立于题库的临时上传：

~~~text
浏览器 JPG/PNG + 文字
  → RK3568 things API（管理员认证 / CSRF / 流式校验并转发；仅内存缓冲、不写设备磁盘）
  → PC 上的受控临时图片接收端（RK → PC 的授权 HTTP 请求）
  → <Codex CWD>/tmp/hyz-ai-uploads/<server-generated-uuid>.png
         ├─ Codex App Server turn/start: text + localImage（绝对路径）
         └─ 按需由 hyz-image-ocr Skill 读取项目内原图并请求 127.0.0.1:8080 PaddleOCR
  → 任务结束或 TTL 到期后清理临时文件
~~~

- **只在 PC 上 Codex 工作目录（当前项目根）的 `tmp/hyz-ai-uploads/` 落盘一次。** RK3568 的 HTTP 后端只鉴权、执行流式校验与转发，最多使用受限内存缓冲，**不得写入 RK3568 的 `/tmp`、其他磁盘目录、SMB 或题库目录**。如果 HTTP 框架需要落盘缓冲，应改为流式接收/转发实现，而不是用设备临时文件代替；超限或断流立即终止。
- 由于 RK3568 和 Codex App Server 位于两台设备，**只在 RK3568 的 `/tmp` 保存文件，PC 上的 `localImage` 与 PaddleOCR Skill 仍无法直接读取**。因此需要一个轻量的 **PC 临时附件接收端**：通过严格请求体和速率限制，仅接受已登录管理员触发的 RK 后端上传；接收端生成文件 ID 并写入项目内 `tmp/hyz-ai-uploads/`，不接受客户端指定路径。此端口不同于只监听 `127.0.0.1:8080` 的 PaddleOCR 本身。
- PC 接收端使用 `systemd --user` 托管，和 PaddleOCR 服务独立；项目内 `tmp/hyz-ai-uploads/` 权限 `0700`，文件权限 `0600`，执行用户与 Codex 一致且非 root。临时文件均有 UUID、最大占用和生命周期限制；以运行任务引用计数保护在用图片，按线程空闲 TTL 清理并保留到 TTL 以支持同一 Thread 的后续追问，另设开机/周期性过期清理处理崩溃或断线。项目根的 `tmp/` 已在 `.gitignore` 中，图片不会进入提交；`tmp/` 会跨重启保留，因此不能依赖系统清理。
- 由后端通过类型化附件 ID 记录当前请求中的图片，PC 端返回经过验证的项目内本机绝对路径，后端再生成 `localImage` 项。前端不得传入路径或自定义 Codex 输入类型；需要确保输入图片在 Codex 消费完数据且任何按需 OCR 完成前不会被删除。
- **本试点不增加永久的 `question_ai_attachments` 数据表，也不将图片 Base64 或原文件保存到 SQLite。** 文本聊天历史仍按原有机制持久保存；重新打开历史时，已过期图片仅显示“本次临时图片已清理”，需要重新上传，不能承诺历史附件可再次下载或重新 OCR。
- 单次限制建议 1–3 张 JPG/PNG、每张不超过 8 MiB，执行文件头/MIME 一致性、解码可读性、尺寸上限、目录安全、超时/并发/总磁盘配额校验。PC 接收端不执行文件内容，不让提示词控制文件路径。
- 如果后续决定**不需要 OCR 访问同一张原图**，可以简化为 RK 后端直接向 Codex `turn/start` 发送 `image` Data URL（Base64），省掉 PC 临时接收端；但**当前既要 Codex 看图、又要宿主机 Skill 在需要时 OCR 原图**，因此采用项目内 `tmp/` + `localImage` 的一致路径。

### 8.2 Codex App Server 统一使用 danger-full-access

目前 apps/rust/things/src/adapters/outbound/codex_app_server.rs 为持久 AI 解析线程设置 read-only，生成线程已经使用 danger-full-access。按本次明确要求，Codex App Server 的线程统一使用最高权限：

- 新线程的 thread/start.sandbox 设为 "danger-full-access"；生成线程与真题 AI 解析线程保持一致。
- 旧线程通过 thread/resume 继续时，在 turn/start 显式指定 sandboxPolicy.type = "dangerFullAccess"（核对部署版本的协议支持）。如果部署版本不支持 per-turn sandbox 覆盖，则不再恢复旧线程，改为新建 danger-full-access 线程，旧线程历史按只读内容展示。
- 此修改必须有新建/恢复线程协议回归测试。
- **风险承认**：danger-full-access 意味着 Codex 不再受到内建文件和网络沙箱限制；上传图片、OCR 输出和试题文本都可能触发提示注入，Skill 说明本身不能限制真实权限。宿主机必须以非 root 专用低权限账户运行 Codex，不暴露私密凭据或高价值目录；适用时在操作系统层再隔离进程。不得因为启用 full access 就关闭管理员认证、CSRF 或附件校验。

## 九、项目 PaddleOCR Skill 与宿主机服务生命周期

### 9.1 Skill 位置及调用

~~~text
.agents/skills/
├── 行测做题/SKILL.md           # 保留
└── hyz-image-ocr/
    ├── SKILL.md                # Codex 调用契约
    └── scripts/ocr_client.py   # 固定本机 OCR API 客户端
scripts/ocr-host/
├── README.md                   # 安装、版本检查、诊断、卸载
├── hyz-paddleocr.service       # PaddleOCR systemd --user 单元模板
└── hyz-image-receiver.service  # PC 临时图片接收端 systemd --user 模板
~~~

- Codex 默认直接查看用户手写原图；遇到数字、符号、分数线、上下标或中文手写歧义时才按需调用 hyz-image-ocr。
- Skill 只能处理本次请求由后端验证的本地附件；调用脚本固定请求 127.0.0.1:8080；输入图片必须来自 PC 项目内受控的 `tmp/hyz-ai-uploads/` 目录，不得访问任意用户提供的文件路径或 URL。
- OCR 输出只是辅助证据，必须和原图核对。用户速算过程、合理近似和不确定符号不擅自改写。
- OCR 不可用时返回明确状态，AI 仍可按原图尝试解读；Skill 不负责安装、启动服务或执行 sudo。
- 普通 OCR 产线不等同于数学公式专用识别。公式识别如需 PP-FormulaNet，应单独验证并配置专用产线，不假定通用 OCR 已能准确生成 LaTeX。

### 9.2 谁启动 PaddleOCR？

**由运行 Codex App Server 的 PC 宿主机上的 systemd --user 托管 PaddleOCR 和独立的临时图片接收端；不是 RK3568，也不是 Codex Skill。** 服务按宿主机用户启用后，在登录会话启动并在故障时自动重启。若必须无人登录就可用，管理员明确执行一次 loginctl enable-linger。

推荐流程：在 PC 上创建 ~/.local/share/hyz-ocr/venv/，安装兼容当前 Python/CPU 或 GPU 的 PaddlePaddle、PaddleOCR，以及一次性执行 paddlex --install serving；将仓库提供的 systemd 单元模板复制到 ~/.config/systemd/user/ 下，经核对实际虚拟环境路径后启用。

单元模板要点：

~~~ini
[Unit]
Description=hyz PaddleOCR local serving

[Service]
Type=simple
ExecStart=%h/.local/share/hyz-ocr/venv/bin/paddlex --serve --pipeline OCR --device cpu --host 127.0.0.1 --port 8080
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
~~~

管理员在 PC 上执行（**当前 PR 不执行**；图片接收端另用独立用户服务托管）：

~~~bash
systemctl --user daemon-reload
systemctl --user enable --now hyz-paddleocr.service
systemctl --user status hyz-paddleocr.service
curl -fsS http://127.0.0.1:8080/health
~~~

需要注销后仍自动启动时，管理员可选择执行 sudo loginctl enable-linger <PC-username>。切勿把 PaddleOCR 默认监听 0.0.0.0 的无鉴权服务直接暴露到 LAN。OCR 仅对本机 Codex/Skill 开放，固定 127.0.0.1:8080；第一次模型下载或加载时未就绪不能导致纯视觉追问失败。

## 十、图片试点验收

- [ ] 上传 JPG/PNG，原有文本追问、历史消息与答案结构保持兼容。
- [ ] 文件校验、管理员权限、CSRF、数量/大小限制、PC 项目 `tmp/hyz-ai-uploads/` 文件权限及到期清理通过；任何用户图片都不进入 SMB/题库，项目根 `tmp/` 保持在 `.gitignore` 中。
- [ ] RK 只进行内存级流式转发、不在设备磁盘或 `/tmp` 落盘；PC 接收端只在 PC 项目 `tmp/hyz-ai-uploads/` 落盘一次，同一原图供 Codex `localImage` 和 OCR Skill 读取；不依赖 CIFS/SMB。
- [ ] 在用临时文件不提前删除；重试、取消、超时和进程重启后按 TTL 清理，无孤儿临时文件和数据库 Base64 膨胀；旧文本历史不受影响。
- [ ] 新建与恢复的 Codex App Server Thread 均正确使用 danger-full-access；生成线程与真题 AI 解析线程一致。
- [ ] 宿主机 systemd --user 可以启动、检查、自动重启 OCR 与 PC 临时图片接收端；OCR 仅监听 loopback，接收端只接收 RK 后端转发的已登录管理员图片上传。
- [ ] 手写公式识别先看原图，OCR 可选且故障可降级；对照原图报告歧义，不凭 OCR 擅自修改速算步骤。

## 十一、本 PR 的边界

**该 PR 仍只有这一份 Plan Markdown 文档。** 不包含新代码、SQLite migration、Skill 文件、OCR 安装、systemd 服务启用、设备部署、转换脚本或第三方申论题目正文。后续实施需另行审阅。

