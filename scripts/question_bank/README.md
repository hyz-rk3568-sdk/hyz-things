# 行测 Markdown question bank 导入

仅在电脑上转换题库；不修改 Rust、设备、Samba 或 Flashcards。默认导入上游现存全部年份，不把当前年份或 2016–2026 写成筛选边界。

## 安装与运行

需要 Python 3.10+、Git；依赖固定在 requirements.txt。下面使用 uv，也可使用标准 venv + pip 安装相同文件。

```sh
uv venv /home/hyz/.cache/question-bank/venv
uv pip install --python /home/hyz/.cache/question-bank/venv/bin/python -r scripts/question_bank/requirements-dev.txt

git clone --depth 1 https://github.com/ProfessorZhi/gwy-zhenti.git /home/hyz/.cache/question-bank/gwy-zhenti
git -C /home/hyz/.cache/question-bank/gwy-zhenti rev-parse HEAD

/home/hyz/.cache/question-bank/venv/bin/python scripts/question_bank/import_bank.py \
  --source /home/hyz/.cache/question-bank/gwy-zhenti \
  --output /home/hyz/Code/hyz_things/question_bank --dry-run

/home/hyz/.cache/question-bank/venv/bin/python scripts/question_bank/import_bank.py \
  --source /home/hyz/.cache/question-bank/gwy-zhenti \
  --output /home/hyz/Code/hyz_things/question_bank
```

题库位于主项目的 `question_bank/`。公开仓库只跟踪设备运行需要的 `papers/` 与 `assets/`；`manifest.json`、审计报告和备份是本机导入状态，不公开提交。工具不会 clone/fetch、初始化 Git、提交或 push；源库版本由操作者固定，实际 commit、源文件 SHA-256 和转换器指纹写入本地清单与报告。原始源库留在缓存目录。

退出码：0 表示结构转换/校验成功；1 表示有冲突、解析失败或图片异常；2 表示输入、已有题库或文件操作不能安全处理。缺题占位等上游内容问题写入逐题 issues，不编造内容，不因此丢弃整卷。

## 内容与本地报告

- `papers/`：一卷一 Markdown，`hyz-paper/v1`；题目保留来源题号，三种 UUID 由未来设备首次同步分配，导入器不生成或替换已有 UUID。
- `assets/`：只复制正文、选项、解析、材料及说明实际引用的图片；按内容 SHA-256 去重，无有损压缩，同时处理 Markdown 和 HTML img。
- `manifest.json`：本地来源文件与正式文件的映射、源/生成指纹、已观察 UUID 和计数；不存整题正文，也不公开提交。
- `reports/inventory.{md,json}`：逐文件纳入/排除及年份分布。
- `reports/quality.md`、`reports/import.json`：覆盖率、已知缺损、多标签统计、图片、冲突及解析错误。
- `reports/issues.jsonl`：逐题问题标签；同一题可有多种问题，标签计数不可直接相加当作题数。
- `reports/backups/`：替换正式 Markdown 和清单前保留的旧版本（按内容哈希命名）。
- `reports/provenance.jsonl`、`reports/sources/`：来源元数据、源哈希、原始答案标签、逐题来源、材料关联依据及各版本审计记录。

首次从公开仓库检出后，题目和图片可直接供设备同步。导入器的 `--update`/`--verify` 还需要本地 `manifest.json`；若要在新工作区维护上游转换，先从固定源重新构建并人工核对，不能对缺少本地清单的既有题库直接执行更新。

模块优先取卷首显式题号范围，类别只读取明确标题值；classification.yaml 仅维护确定的类别→模块关系。原始考点按顿号拆列表，未知题型不猜测。没有 AI 补题或自动标注。上游明确标记的从属佐证副本保留为 `role: evidence`，普通卷为 `role: primary`；佐证版不进入正常练习池。`Paper Notes` 保留来源声明与作答说明，`Notes` 保留答案状态/性质等额外原文；无原始解析则省略 Explanation。

`ready` 要求无已知问题；`structurally_answerable` 仅指未发现影响题面/判分的已知缺损，仍可能题序待核。两者都不代表官方原卷认证。标题与逐题来源冲突、答案映射待核、公式待核等应查看报告。目录中保留这些候选内容不意味着已通过人工审核。

## 更新与校验

```sh
/home/hyz/.cache/question-bank/venv/bin/python scripts/question_bank/import_bank.py \
  --source /home/hyz/.cache/question-bank/gwy-zhenti \
  --output /home/hyz/Code/hyz_things/question_bank --update --dry-run

/home/hyz/.cache/question-bank/venv/bin/python scripts/question_bank/import_bank.py \
  --source /home/hyz/.cache/question-bank/gwy-zhenti \
  --output /home/hyz/Code/hyz_things/question_bank --update

/home/hyz/.cache/question-bank/venv/bin/python scripts/question_bank/import_bank.py \
  --output /home/hyz/Code/hyz_things/question_bank --verify
```

`--dry-run`、`--verify` 都只读。更新前先查看本地 Git diff 并保存人工编辑。源库不变时保留人工版本；源库和人工版本同时变化时报告冲突，保留旧文件。UUID 注释不算正文编辑；仅当题面/选项指纹能唯一匹配时继承题目 UUID，材料按内容唯一匹配。身份不能确定、已记录 UUID 丢失、目标文件被删除或原文件改名有歧义时拒绝自动覆盖。题号不参与身份匹配。源文件仅改路径且哈希唯一匹配时沿用旧正式文件路径；源删除保留旧文件并记入 archived_sources。

这是保守的三方比较，不尝试自动合并两侧正文编辑。工具在替换前再次检测文件变化并备份，但未提供跨编辑器的原子比较后交换；更新期间应暂停人工保存和设备写回。首次观察 UUID 之前工具无法判断某个无 ID 题目是否曾在设备上有过 ID；设备同步与电脑编辑的并发协调属于后续接入工作。保留 manifest 与 reports，不能仅靠 Git 历史替代关联记录。工具不删除旧图片或旧来源报告。

## 验证工具自身

先静态检查，再运行纯主机单元测试；不构建 SDK/Rust 或执行设备命令。

```sh
/home/hyz/.cache/question-bank/venv/bin/ruff check scripts/question_bank
/home/hyz/.cache/question-bank/venv/bin/ruff format --check scripts/question_bank
/home/hyz/.cache/question-bank/venv/bin/python -m unittest discover -s scripts/question_bank -v
git diff --check
```

测试覆盖多选、两类题块、共享材料跨模块截断、原始答案性质保留、缺题、题序待核、图片闭包、缺图、人工编辑冲突、UUID 保留、源重命名、重复 UUID、已识别文件丢失及 dry-run。
