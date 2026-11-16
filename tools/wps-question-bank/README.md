# WPS PDF → 题库导入工具

把 WPS 文字导出的公考 PDF（题干册 + 答案汇总）转换成 hyz-things 题库 Markdown：

- 表格 → GFM 管道表格
- 公式 → `$...$` LaTeX
- 图片 → `assets/<sha256>.<ext>`，正文引用 `../../assets/...`
- 不写 `hyz-paper-id` / `hyz-material-id` / `hyz-question-id`；ID 由导入端生成
- 题目 YAML 的 `materials` 使用材料名（`M1`–`M4`）

## 依赖

```sh
pip3 install --user pymupdf pillow
```

文本层 PDF（WPS 文字输出）直接提取。扫描件或矢量轮廓页（文字被转成曲线，`get_text()` 为空）走本地 PaddleOCR：工具先检测横线并涂白，让 OCR 在填空处断开，再把横线还原成 `______`。

OCR 服务（每个实例约 1.2 GiB 内存，单页约 35–60 秒）：

```sh
PADDLE_PDX_ENABLE_MKLDNN_BYDEFAULT=False PADDLE_PDX_MODEL_SOURCE=modelscope \
PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK=True \
paddlex --serve --pipeline OCR --device cpu --host 127.0.0.1 --port 8080
```

`PADDLE_PDX_ENABLE_MKLDNN_BYDEFAULT=False` 用于绕开 Paddle oneDNN 在部分扫描页上的 `ConvertPirAttribute2RuntimeAttribute` 崩溃。扫描件推荐 `ocr.dpi: 150`；同页 150 与 220 dpi 的文字结果基本一致，150 dpi 单页约快 40%。

一个 OCR 请求本身就会占用约 8 个核（150 dpi 单页 35–60 秒），而单个实例一次只处理一个请求。16 核实测：两个实例并发已占 14.6/16 核；四个实例会过订阅，每页慢到 130–160 秒，总吞吐与两个实例相同。`OMP_NUM_THREADS` 不会限制 Paddle 的推理线程数，并发度由实例数决定；要更均匀地吃满机器，可用 `taskset` 给每个实例固定核数。客户端为每个 `--ocr-url` 开一个 worker，通常给两个端点即可。

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
| `--ocr` | 强制走 OCR（profile 里的 `ocr.enabled` 也可开启） |
| `--ocr-url URL` | OCR 端点，可重复；每个端点一个 worker |
| `--ocr-dpi N` | 覆盖 profile 的 `ocr.dpi` |
| `--ocr-cache DIR` | OCR 结果缓存，默认 `~/.cache/hyz-wpsbank/ocr` |
| `--ocr-report` | 打印每页块数、token 数、平均置信度与低置信度块数 |
| `--detect-ocr` | OCR 全书，按页眉“练习题N套”分组打印页段 |
| `--explanations` | OCR 解析页，把解析与解析中的答案合并进题目 |
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
| `schema` / `subject` | 输出格式；新书用 `hyz-paper/v2` + `subject: xingce`，v1 只用于既有书目保持字节一致 |
| `noise` | 页眉页脚裁切 `top_band` / `bottom_band`、噪声子串、整行正则 |
| `patterns` | 大题、子问题、题号、选项、`问题：` 前缀的正则 |
| `answer_key` | 答案册标题、标题映射、区间、连续字母的正则 |
| `papers` | 每份试卷的 `kind` / `num` / 起止页 |
| `answer_pages` | 扫描版答案封面：`kind` / `num` / `pdf` / `page`；可选 `explain_start`、`explain_end` 覆盖解析页段，`section` 指定答案页上的小节标题 |
| `answers_required` | 答案缺失时是否判为校验失败，默认 `true` |
| `overrides` | 提取缺陷的人工覆盖，按 `kind → num → 题号 → {stem/options}` |
| `question_starts` | 页码 → 题号；扫描件丢失题号时，在该页强制开始这一题 |
| `ocr` | 扫描件配置：`enabled`、`dpi`、`badge`、`replacements`（OCR 错字、水印与横线残端的映射） |

`overrides` 是用来记录源头排版缺陷的补丁（例如分数被拆成上下两行）。新书优先修提取规则；必须覆盖时把内容写进 profile，并在提交说明里记录原因。

扫描页的句末标点容易丢：横线涂白会带走紧跟其后的句号，句号单独占一行时 OCR 也常漏检。OCR 模式会在题干末尾补 `。`，把行末孤立的 `o`/`0` 还原成 `。`，并去掉 `______ ，` 中多余的空格；无法自动判断的残端用 `overrides` 修正。

扫描版答案册按 `answer_pages` 定位每套答案页：该页同时给出参考答案和该套第一题的解析，解析页段默认取“本套答案页到下一套答案页前一页”，跨分册时最后一页取该 PDF 末尾。解析以题号 `N.(来源)` 分块，去掉重复的题干和选项，保留“第一步/第二步/综上”等正文；`【参考答案】X` 与 `综上，这道题的正确答案为X选项` 同时用于补齐或交叉校验答案。

语句排序题的选项是 ①…⑥ 的排列，可作为题干标记的校验集：OCR 常把 ⑤ 读成 `5`、把 ① 读成 `(1`，工具会根据选项里出现而题干缺失的标记回填，跳过 `5分钟` 这类数量词，并去掉标记前多余的括号。只在选项全为标记排列的题目上生效，普通题目里的数字不受影响。

答案页可能把多个小节排在同一页（例如每个题组、每个小题型各一节），而每节题号都从 1 重新开始。此时给每套题填 `section`，解析只取该标题到下一个已声明标题之间的行，避免后一节覆盖前一节的答案。

## 新书流程

1. 新建 profile，先填 `book`、`answers`、集合元数据与答案册正则。
2. 用 `--detect` 对照标题页，补齐 `papers`；标题缺失时手动填页段。
3. 用 `--debug-page` 抽查页眉页脚、表格、选项图。
4. 跑 `--report-only`，修 `patterns` / `noise`，直到题数、选项、答案覆盖通过。
5. 写出到临时目录，抽查 2–3 份试卷，再复制到 `question_bank/`。

### 扫描件先跑 8 页原型

```sh
python3 tools/wps-question-bank/ocr-preview.py \
  --job "book=/path/to/题本.pdf:10,61,131" \
  --job "answers=/path/to/解析上.pdf:3,21" \
  --ocr-url http://127.0.0.1:8080/ocr \
  --ocr-url http://127.0.0.1:8081/ocr \
  --cache /tmp/ocr-cache --fix "如火如茶=如火如荼"
```

逐页打印重建后的行，用来核对换行、选项分栏、横线位置和侧栏顺序。确认后再写 profile、跑完整导入。

扫描件答案与解析放在 `answer_pages` 后，用同一条命令合并：

```sh
python3 tools/wps-question-bank/import-wps-pdf.py \
  --profile tools/wps-question-bank/profiles/huasheng-2026-luojitiankong-600.json \
  --explanations --out /tmp/question-bank-import \
  --ocr-url http://127.0.0.1:8080/ocr --ocr-url http://127.0.0.1:8081/ocr \
  --ocr-url http://127.0.0.1:8082/ocr --ocr-url http://127.0.0.1:8083/ocr \
  --ocr-cache /tmp/ocr-cache
```

`--explanations` 会把每套答案页到下一套答案页前一页之间的正文 OCR 出来，按题号块去掉重复题干/选项，写成 `### Explanation`；侧栏（花生批注）不并入正文。

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
