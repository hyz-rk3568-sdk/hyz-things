# 国考与湖南省考近十年申论真题导入、Schema 扩展与 Migration 计划

日期：2026-11-09。**状态：仅设计评审，尚未实施。**

## 一、范围与约束

本批次仅完成以下设计对应的后续实施：

1. 从 [ProfessorZhi/gwy-zhenti](https://github.com/ProfessorZhi/gwy-zhenti) 筛选 **2017—2026 年**国考、湖南省考的申论真题。
2. 将试卷转换成现有 \`hyz-paper/v1\` 兼容格式，放入既有 \`question_bank/papers/\` 目录。
3. 为原始题库增加最低限度的科目、作答类型、题型和分值字段，以及主观题参考答案的独立存储位；增加相应 SQLite migration 和解析/同步适配。
4. 保留原卷给定材料、作答要求、参考答案与解析，并做可重复的结构和图片校验。

**不做：** 不新增第三种 \`kind\`；不加入职测、综应；不增加 \`source_url\`、\`bank_id\`、\`exam_series\`、\`grading_mode\`、字数结构化字段等非必要元数据；不新增训练页面、作答表或任何申论业务功能；不修改设备、挂载或部署配置。

原有行测真题与 UUID、练习历史、收藏、错题不作重置或迁移改写。提交代码前必须先审计测试现有兼容行为。

## 二、来源范围与固定版本

上游：\`ProfessorZhi/gwy-zhenti\`，正式实施时先固定 Git commit，保存到本地导入清单，避免重复导入时源数据无提示改变。来源仓库一卷一个 Markdown，图片位于年份下的 \`图片/\` 目录，文件中包含给定材料、题目、折叠参考答案/解析；其内容不等同于经官方认证的原卷或官方答案。

| 地区 | 上游路径 | 年份 | 已核对的申论文件 |
| --- | --- | --- | ---: |
| 国考 | \`国考/<年>/申论-*.md\` | 2017—2026 | 25 |
| 湖南省考 | \`省考/湖南/<年>/申论-*.md\` | 2017—2026 | 23 |
| **合计** | | | **48** |

逐年扫描实际文件名，不假定每年卷型固定。国考含副省级、地市级，以及有来源文件的行政执法卷；湖南保留实际出现的统考、通用卷、乡镇卷、县乡卷、省市卷、行政执法、选调生等。**湖南 2026 年在当前来源中仅有县乡卷**，不能虚构其他卷型。既存文件可能存在缺题、缺材料、缺图或人工核验未完成状态；逐卷报告。

## 三、落盘目录与来源边界

不新增 \`question_bank/shenlun/\`，沿用原有年份路径：

\`\`\`text
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
\`\`\`

国考、湖南只是 \`region\`/路径分组；申论真题仍为 \`kind: exam\`。不把整卷导入 \`collections/\`，不影响现有 \`kind: drill\` 语义。

**权利边界：** 上游 README 将内容限定为学习研究用途，试题、参考答案、解析和图片还涉及第三方权利，不能把仓库可见当作允许整库再分发。实施时先导入**本地私有工作区**并审计授权；未确认分发许可前，公开仓库只提交转换器、必要源码/迁移和不含第三方全文的清单，不提交完整申论卷及图片。

## 四、\`hyz-paper/v1\` 向后兼容扩展

**继续使用 \`hyz-paper/v1\`**，本批只增加可选字段并补充解析校验；不引入破坏性 v2。现有 \`Header\` 解析器已支持 \`schema\`、\`title\`、\`year\`、\`region\`、\`variant\`、\`role\`、\`kind\` 等；现有 \`Metadata\` 支持 \`module\`、\`category\`、\`topics\`、\`materials\`。

### 4.1 试卷级

**仅新增 \`subject\`**，当前允许 \`xingce\` / \`shenlun\`；旧文件未声明时视为 \`xingce\`。

\`\`\`yaml
---
schema: hyz-paper/v1
kind: exam
subject: shenlun
title: 2026年国考申论（副省级）
year: 2026
region: 国考
variant: 副省级
role: primary
status: unverified
---
\`\`\`

\`status\` 沿用现有真题文档中的状态信息，本次不新建来源 URL 等元数据。该示例仅演示结构。

### 4.2 题目级

在每道题现有 \`yaml\` 块中增加：

| 字段 | 取值 | 兼容逻辑 |
| --- | --- | --- |
| \`answer_type\` | \`objective\` / \`subjective\` | 旧题缺省 \`objective\` |
| \`question_type\` | 非空字符串或缺省；申论可用 \`summary\`、\`analysis\`、\`measures\`、\`document_writing\`、\`essay\`、\`other\` | 不明确则 \`other\` 或留空；不凭题号猜作文 |
| \`max_score\` | 正数或缺省 | 仅源题明确给出分值时写入 |

原有 \`module\`、\`category\`、\`topics\`、\`materials\` 保留。申论材料关联必须准确：明确关联的按来源对应材料编号映射；全卷适用的给定材料保持全卷可访问；无法核实时报告而不是编造引用。

### 4.3 原题与参考答案分离

当前 \`question_fields()\` 识别 \`Stem / Options / Answer / Explanation / Notes\`；\`Answer\` 当前按选择题字母解析。**本批新增 \`Reference Answer\` 区段**，只用于保存主观题原始参考答案/范文；\`Explanation\` 保留原始解析，\`Answer\` 对申论留空。

转换后的结构示意（不代表真实题目或分值）：

\`\`\`markdown
## Material M1

（给定材料 1 原文）

## Question 1

\`\`\`yaml
module: 申论
category: 概括归纳
answer_type: subjective
question_type: summary
max_score: 10
materials: [M1]
\`\`\`

### Stem

（完整题干和作答要求）

### Options

### Answer

### Reference Answer

（来源提供的主观题参考答案）

### Explanation

（来源提供的解析；没有则保留缺失状态）
\`\`\`

原来的 \`hyz-paper-id\`、\`hyz-question-id\`、\`hyz-material-id\` UUIDv7 注释机制不变。转换器应输出可被现有补写机制补齐身份标记的 Markdown；重复导入不得更换已存在的 ID。**\`question_fields()\` 必须先显式支持 \`Reference Answer\`，否则新章节可能被错误拼进上一字段。**

## 五、SQLite migration（独立 question-bank 数据库）

位置：\`apps/rust/things/migrations/question_bank/\`。新增**一个** migration，不修改任何已执行的 migration。以下是拟议 SQL，实际提交前需检查现有索引、metadata JSON 与 SQL 查询兼容性。

\`\`\`sql
ALTER TABLE papers
  ADD COLUMN subject TEXT NOT NULL DEFAULT 'xingce';

ALTER TABLE questions
  ADD COLUMN answer_type TEXT NOT NULL DEFAULT 'objective';

ALTER TABLE questions
  ADD COLUMN question_type TEXT;

ALTER TABLE questions
  ADD COLUMN max_score REAL;

ALTER TABLE questions
  ADD COLUMN reference_answer TEXT;

CREATE INDEX IF NOT EXISTS idx_bank_papers_subject
  ON papers(subject);
\`\`\`

- 旧 \`papers\` 行的 \`subject\` 自动为 \`xingce\`，旧 \`questions\` 行的 \`answer_type\` 自动为 \`objective\`。
- \`correct_answer\` 仍用于客观选项，\`reference_answer\` 是单独的主观题参考内容；不得向客观判分字段写入范文。
- 现有 \`gradable\` 保持“有可用选择题结构与答案”的含义。申论题不具备客观选项结构时为 \`0\`，不能由此推断题目无法保存/阅读。
- 同步层必须把 Markdown \`subject\` 写入 \`papers.subject\`，题级新增字段写入 \`questions\`；读接口/DTO 需保证一致性。当前 \`papers.metadata\` 仍保存部分索引信息，读写 SQL 必须检查，避免数据库字段与 JSON 快照产生不同值。
- 现有 \`practice_sessions\`、\`practice_items\`、\`practice_attempts\`、\`question_progress\`、\`favorites\` 保持原样；本批不新增作答或阅卷表。
- 迁移前备份 \`question-bank.db\`，验证旧数据库增量升级和新数据库初始化；不要通过删除数据库重建来“升级”。

## 六、转换器与同步适配

### 6.1 转换器放置

建议在现有题库导入工具位置 \`scripts/question_bank/\` 内增加申论转换入口（实际实施前确认与主分支工具结构一致），与设备端的 Rust 解析器分离。

处理步骤：

1. 固定上游 commit，并只扫描国考、湖南省考 2017—2026 年 \`申论-*.md\`。
2. 读取上游中文 frontmatter，核对年份、地区、科目、卷型；同名目标文件冲突拒绝静默覆盖。
3. 转换 \`## 给定材料 / ### 材料 N\` 为 \`## Material MN\`，保持材料编号、原文和次序。
4. 转换 \`## 题目 / ### 第 N 题\` 为 \`## Question N\`，保留题号、题干、字数要求和明确分值。
5. 将折叠答案块拆成 \`Reference Answer\` 与 \`Explanation\`；允许一题有多个来源参考答案，忠实保存，禁止 AI 改写。
6. 图片复制到现有 \`question_bank/assets/<sha256>.<ext>\`，重写正文相对路径；验证每一个实际引用图片存在且哈希匹配。
7. 做完整性审计：材料数量和编号、题目数量、缺题占位、答案缺失、图片引用、UTF-8 和解析器可读性。
8. 输出 dry-run 差异、冲突和缺项报告；经确认后执行增量导入。保留已存在 UUID，不覆盖人工编辑；再次执行应幂等。

源内容可能含不可信 Markdown/HTML，导入过程仅作为数据解析和渲染，不执行其中任何命令、脚本或外链操作。

### 6.2 现有代码的必要适配点

- \`apps/rust/things/src/application/question_bank/document.rs\`：Header/题级 metadata 扩展、\`Reference Answer\` 解析、兼容旧卷、唯一编号与 UUID 校验。
- \`apps/rust/things/src/domain/question_bank.rs\`：需要展示/传递的 \`subject\` 和题目属性 DTO。
- \`apps/rust/things/src/application/question_bank/mod.rs\`：\`StoredQuestion\` / \`PaperContent\` / \`IndexedPaper\` 元数据投影更新。
- \`apps/rust/things/src/adapters/outbound/question_bank_sqlite.rs\`：同步插入/更新、读取和 metadata 一致性，避免旧字段被覆盖。
- \`apps/rust/things/src/adapters/outbound/question_bank_source.rs\`：保持现有目录扫描、文件大小、图片哈希及 UUID 写回规则，不为第三方源增加运行时网络访问。

不修改现有训练页面与答题逻辑；如现有客观题查询会混入申论题，应在**现有查询范围**加 \`subject = 'xingce'\` 约束，以确保原行为不变，而不是增加新训练入口。

## 七、验证与交付判定

**必须通过：**

- [ ] 2017—2026 国考 25、湖南 23 份申论源文件全部逐份进入导入审计清单；每份标记成功/缺项/失败，不用总数掩盖缺损。
- [ ] 旧 \`hyz-paper/v1\` 行测解析、UUID 写回、内容指纹/同步和重复导入回归通过。
- [ ] 新 \`subject\` 默认值不影响旧行测数据；新迁移能升级旧数据库且不重建个人历史表。
- [ ] 申论材料序号与题号完整对应，题干要求、参考答案、解析不混淆、不丢失。
- [ ] 所有保留的图像引用经 SHA-256 文件与引用路径校验。
- [ ] 主观题 \`Answer\` 不被当作选择题选项解析；\`Reference Answer\` 不进入作答前题目 DTO。
- [ ] 现有 \`exam / drill\` 语义、行测客观判分和练习统计保持不变。
- [ ] 未确认再分发许可前不将第三方完整原文/图片加入公开仓库。

## 八、本 PR 的边界

**本 PR 只增加这份计划文档。** 不含 SQLite migration、Rust 修改、导入器实现、申论真题正文或任何部署操作。计划经审阅后，实施工作另行提交。
