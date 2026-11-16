# WPS PDF → 题库导入工具

把 WPS 文字导出的公考 PDF（题干册 + 答案汇总）转换成 hyz-things 题库 Markdown：

- 表格 → GFM 管道表格
- 公式 → `$...$` LaTeX
- 图片 → `assets/<sha256>.<ext>`，正文引用 `../../assets/...`
- 不写 `hyz-paper-id` / `hyz-material-id` / `hyz-question-id`；ID 由导入端生成
- 题目 YAML 的 `materials` 使用材料名（`M1`–`M4`）

## 依赖

```sh
pip3 install --user pymupdf
```

PDF 必须带完整文本层（WPS 文字输出通常满足）。扫描件需要先 OCR。

## 用法

```sh
python3 tools/wps-question-bank/import-wps-pdf.py \
  --profile tools/wps-question-bank/profiles/sihai-2026-haihaishua-ziliao.json \
  --report-only
```

确认报告无误后去掉 `--report-only`，用 `--out` 指定输出根目录：

```sh
python3 tools/wps-question-bank/import-wps-pdf.py \
  --profile tools/wps-question-bank/profiles/sihai-2026-haihaishua-ziliao.json \
  --out /tmp/question-bank-import
```

输出结构：

```
<out>/collections/<collection_id>/*.md
<out>/assets/<sha256>.<ext>
```

其他开关：

| 开关 | 作用 |
|---|---|
| `--book PDF` | 覆盖 profile 里的题干册路径 |
| `--answers PDF` | 覆盖 profile 里的答案册路径 |
| `--detect` | 按试卷标题行打印检测到的页码区间，与 profile 对照 |
| `--debug-page N` | 导出第 N 页 PNG 到 `--debug-out`，用于核对表格/选项图 |
| `--force` | 校验有问题时仍然写出 |

默认先完整提取并校验：题数、题号连续、四选项、空题干、空选项、答案覆盖。任何一项失败都会在写文件前退出。

## Profile

每本书一个 JSON，放在 `profiles/`。字段：

| 字段 | 说明 |
|---|---|
| `book` / `answers` | PDF 路径；相对路径按 profile 所在目录解析 |
| `collection_id` / `collection_title` | 题库集合标识与标题 |
| `year` / `region` / `variant` / `module` | 输出 frontmatter 与题级 YAML |
| `role` / `paper_kind` | 固定为 `primary` / `drill` |
| `option_labels` | 选项字母，默认 `ABCD` |
| `questions_per_material` | 材料与题号的对应粒度 |
| `expected` | 每种试卷的题数，用于校验 |
| `noise` | 页眉页脚裁切 `top_band` / `bottom_band`、噪声子串、整行正则 |
| `patterns` | 大题、子问题、题号、选项、`问题：` 前缀的正则 |
| `answer_key` | 答案册标题、标题映射、区间、连续字母的正则 |
| `papers` | 每份试卷的 `kind` / `num` / 起止页 |
| `overrides` | 提取缺陷的人工覆盖，按 `kind → num → 题号 → {stem/options}` |

`overrides` 是用来记录源头排版缺陷的补丁（例如分数被拆成上下两行）。新书优先修提取规则；必须覆盖时把内容写进 profile，并在提交说明里记录原因。

## 新书流程

1. 新建 profile，先填 `book`、`answers`、集合元数据与答案册正则。
2. 用 `--detect` 对照标题页，补齐 `papers`；标题缺失时手动填页段。
3. 用 `--debug-page` 抽查页眉页脚、表格、选项图。
4. 跑 `--report-only`，修 `patterns` / `noise`，直到题数、选项、答案覆盖通过。
5. 写出到临时目录，抽查 2–3 份试卷，再复制到 `question_bank/`。

## 测试

```sh
python3 tools/wps-question-bank/tests/test_wpsbank.py
```

测试只覆盖纯逻辑（profile 校验、文本规范化、答案解析、Markdown 组装），不依赖 PDF 或 PyMuPDF。

## 已知边界

- 题号在每个材料/试卷内从 1 连续递增；断号会报告，不会猜测。
- 默认每题四个选项；其它选项数通过 `option_labels` 配置。
- 单栏 A4 版式；页眉页脚阈值在 profile 的 `noise` 里调。
- 表格重建面向 WPS 的合并单元格与无横线数据带；复杂跨页表可能仍需人工核对。
- 图片广告条按“跨页重复哈希”排除；单张饼图按四选项切分的启发式只适用于宽幅图。
