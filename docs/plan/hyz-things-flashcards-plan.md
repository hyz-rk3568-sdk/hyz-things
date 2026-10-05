# hyz-things 卡片学习与间隔复习实现计划

## 状态

**计划已确认，待实施。**

创建日期：2026-10-05。

本文用于在现有 `apps/rust/things` 的 Study 页面中增加一套个人 Flashcard / Anki 风格学习能力。

本功能属于跨 native persistence、HTTP API、Yew Web 和 E2E 的较大功能。代码实施必须遵循仓库 `AGENTS.md`：

- 使用独立功能分支，例如 `feat/study-flashcards`；
- 通过 Pull Request 合入 `main`；
- 按 Red → Green → Refactor 推进；
- 最终候选通过 Portal CI；
- 不修改 `hyz-router` / `hyz-camera` 的职责边界。

## 1. 目标

核心原则：

1. 卡片内容由用户直接编辑 Markdown 文件，不做 Web 制卡器。
2. Markdown 和图片位于用户电脑硬盘，通过 **SMB over Tailscale** 挂载到 RK3568。
3. RK3568 固定从挂载目录读取卡片，例如 `/mnt/hyz-cards`。
4. Markdown 是卡片内容的 source of truth。
5. SQLite 保存卡片解析后的索引、Deck / Tag、FSRS 复习状态与 Review history。
6. 图片不复制、不入库，始终从挂载目录原地读取。
7. 新卡无需手写 ID；同步时自动生成 UUIDv7 并写回 Markdown。
8. Deck 和 Tag 可以同时筛选。
9. 点击“开始复习”后进入独立沉浸式界面，不显示门户侧栏、Header、Footer 等无关导航。
10. 复习使用三个操作：不会、模糊、会了；内部映射 FSRS 的 Again / Hard / Good。

## 2. 非目标

第一版不实现：

- Web 新建、修改或删除 Markdown 卡片；
- Anki `.apkg` 导入导出；
- Cloze；
- 多种 Card template；
- 多用户学习数据；
- 云端卡片同步服务；
- 图片复制到 `/userdata`；
- 将 SQLite 放到 SMB/Tailscale 网络目录；
- 文件系统实时 watcher；
- 自动管理 SMB mount；
- 全项目 `/userdata` 数据迁移到 SQLite；
- 修改管理员 credential 存储；
- 修改 apps registry 存储；
- 修改 router/camera wire protocol；
- 复杂学习统计和排行榜。

## 3. 最终数据流

~~~text
用户电脑
D:\hyz-cards
       │
       │ SMB over Tailscale
       ▼
RK3568
/mnt/hyz-cards
       │
       │ Flashcard Source Adapter
       │
       ├──── Markdown
       │
       └──── Images
               │
               ▼
       FlashcardApplication
               │
               ▼
        SQLite Repository
               │
               ▼
/userdata/hyz-things/flashcards.db
               │
               ▼
          HTTP API
               │
               ▼
          Yew Study UI
~~~

hyz-things 不负责 SMB 协议，也不负责连接用户电脑。

操作系统负责：

~~~text
电脑 Tailscale IP + SMB share
            ↓
       /mnt/hyz-cards
~~~

hyz-things 只把 `/mnt/hyz-cards` 当普通本地目录处理。

如果目标固件目前没有 CIFS/mount 支持，实施阶段先确认目标板能力；确实缺失时只增加完成 SMB mount 所需要的最小 Buildroot/kernel 支持，不把 SMB 用户名、密码等凭据写进仓库。

## 4. /userdata 策略

引入 SQLite **不代表统一迁移现有 /userdata 数据**。

保持：

~~~text
/userdata/hyz-router/admin/credential.json
~~~

现状不变。

保持：

~~~text
/userdata/hyz-things/apps/registry.json
~~~

现状不变。

新增：

~~~text
/userdata/hyz-things/flashcards.db
~~~

因此：

~~~text
credential
→ 继续 private JSON file

apps registry
→ 继续 JSON contract

flashcard Markdown / images
→ /mnt/hyz-cards

flashcard runtime state
→ SQLite
~~~

SQLite 只属于 Flashcards feature，不做全局 persistence 重构。

## 5. Markdown 格式

一个 Markdown 文件允许包含多张卡。

目录决定 Deck。

例如：

~~~text
/mnt/hyz-cards/
├── 判断推理/
│   └── 逻辑判断/
│       ├── 由果推因.md
│       └── images/
│           └── q001.png
└── 资料分析/
    └── 基期量.md
~~~

那么：

~~~text
判断推理/逻辑判断/由果推因.md
~~~

中的卡片自动属于：

~~~text
Deck = 判断推理/逻辑判断
~~~

Markdown 示例：

~~~markdown
---
tags:
  - 由果推因
---

## Card
<!-- hyz-card-id: 019a31b7-8c42-7c16-a6d1-0c1f0f34c821 -->
<!-- tags: 错题, 高频 -->

### Front

![题目](./images/q001.png)

为什么这个论证属于因果倒置？

### Back

题干认为：

$$
A \rightarrow B
$$

但实际可能是：

$$
B \rightarrow A
$$

---

## Card

### Front

“另有他因”为什么能够削弱？

### Back

因为结果可能由第三因素产生。
~~~

文件级 `tags` 对文件里的全部卡片生效。

单张卡还可以增加自己的：

~~~text
<!-- tags: 错题, 高频 -->
~~~

最终 Tag = 文件 Tag ∪ Card Tag。

不区分文字卡、图片卡、公式卡、错题卡，统一都是 `Flashcard`。

## 6. Card ID

用户新增卡片时不需要写 ID：

~~~markdown
## Card

### Front
问题

### Back
答案
~~~

执行同步后：

~~~text
发现无 ID
    ↓
生成 UUIDv7
    ↓
写回 Markdown
~~~

变为：

~~~markdown
## Card
<!-- hyz-card-id: 019a31b7-8c42-7c16-a6d1-0c1f0f34c821 -->

### Front
问题

### Back
答案
~~~

ID 规则：

- UUIDv7 是 Card 永久身份；
- 文件名不是 ID；
- Deck 不是 ID；
- 卡片在文件中的顺序不是 ID；
- front/back hash 不是 ID。

因此文件移动时只更新 source path 与 Deck，不创建新卡，复习历史不变。

### 6.1 重复 ID

同步开始时首先扫描已有 ID。

如果发现同一个 UUID 出现在两张 Card，则明确返回 `duplicate_card_id`。

不会自动覆盖、自动重新编号或猜测哪张是原卡。

SQLite 同时以：

~~~sql
id TEXT PRIMARY KEY
~~~

作为最终唯一性保护。

### 6.2 自动写 ID 的安全行为

给 Markdown 写回 ID 时：

~~~text
读取完整文件
→ 修改内存内容
→ 写同目录临时文件
→ flush/fsync
→ rename
~~~

如果 ID 无法成功写回源文件，则这张新卡本轮不写入 SQLite，避免产生“DB 有 ID，但 Markdown 没 ID”的身份分叉。

## 7. 同步语义

第一版不使用 file watcher。

同步由 Study 页面上的“同步卡片”按钮明确触发。

流程：

~~~text
扫描 /mnt/hyz-cards
        ↓
读取所有 .md
        ↓
解析 cards / tags / deck
        ↓
验证已有 ID
        ↓
给新 Card 分配 UUIDv7
        ↓
写回缺失 ID
        ↓
重新得到稳定 source snapshot
        ↓
与 SQLite 对比
        ↓
事务更新数据库
~~~

每张卡对应以下状态：

| Source 状态 | SQLite 行为 |
| --- | --- |
| 新 UUID | INSERT |
| UUID 已存在、内容变化 | UPDATE |
| UUID 已存在、文件移动 | 更新 source/deck |
| UUID 已存在、无变化 | SKIP |
| 已删除 | `active = false` |
| inactive Card 重新出现 | `active = true` |
| Tag 改变 | 更新 Tag relation |

删除 Card 时不物理删除 Review history。

~~~text
Card 从 Markdown 删除
       ↓
active=false
       ↓
不再进入复习
       ↓
review state/log 保留
~~~

以后同 UUID 再出现时恢复 `active=true`，原复习状态继续使用。

### 7.1 同步错误保护

必须避免：

~~~text
SMB 暂时读失败
        ↓
扫描到 0 张
        ↓
错误地把所有卡 inactive
~~~

因此：

- source root 无法读取：整个同步失败，不修改 Card active 状态；
- 某 Markdown 解析失败：保留该文件上一次成功同步的数据；
- 只有能够确认“文件确实已经不存在”时才做 inactive；
- 一张文件失败不阻止其他正常文件同步。

同步结果返回：

- 新增；
- 更新；
- 未变化；
- 重新激活；
- 停用；
- 自动生成 ID；
- 错误。

错误应显示具体文件与行号。

## 8. SQLite + SQLx

`apps/rust/things` 当前没有 SQLite/SQLx。

本功能新增：

- `sqlx`
- `uuid`

SQLx 只在 native feature 使用。

目标能力：

~~~text
sqlx
features:
- sqlite
- runtime-tokio
- migrate
~~~

不使用需要 build-time database 的 `query!()` 宏。

优先使用：

~~~rust
sqlx::query(...)
sqlx::query_as(...)
~~~

以保持 host CI 和 cross build 简单。

新增 migration 目录：

~~~text
apps/rust/things/
└── migrations/
    └── 0001_flashcards.sql
~~~

启动时执行：

~~~rust
sqlx::migrate!("./migrations")
    .run(&pool)
    .await?;
~~~

数据库位置固定：

~~~text
/userdata/hyz-things/flashcards.db
~~~

数据库目录与文件保持 root-only 私有权限。

SQLite 配置：

~~~text
foreign_keys = ON
journal_mode = WAL
busy_timeout
small SqlitePool
~~~

SQLite 永远不放到 `/mnt/hyz-cards`。

## 9. 第一版 Schema

### 9.1 flashcards

~~~text
id                  TEXT PRIMARY KEY
source_file         TEXT NOT NULL
source_order        INTEGER NOT NULL
deck_path           TEXT NOT NULL
front_markdown      TEXT NOT NULL
back_markdown       TEXT NOT NULL
source_hash         TEXT NOT NULL
active              INTEGER NOT NULL
created_at_ms       INTEGER NOT NULL
updated_at_ms       INTEGER NOT NULL
~~~

`source_file` 保存相对于 `/mnt/hyz-cards` 的路径，不把绝对系统路径返回浏览器。

### 9.2 flashcard_card_tags

~~~text
card_id             TEXT NOT NULL
tag                 TEXT NOT NULL

PRIMARY KEY(card_id, tag)
FOREIGN KEY(card_id)
    REFERENCES flashcards(id)
    ON DELETE CASCADE
~~~

第一版不单独建立 `decks` 表。

Deck 本身就是 `flashcards.deck_path`，Deck 列表使用 `SELECT DISTINCT deck_path` 即可。

### 9.3 flashcard_review_state

至少保存：

~~~text
card_id
due_at_ms
last_reviewed_at_ms
stability
difficulty
state
review_count
lapse_count
~~~

### 9.4 flashcard_review_logs

保存：

~~~text
id
card_id
reviewed_at_ms
rating
scheduler state/result
~~~

Review log 只追加，不因为 Markdown 更新删除。

### 9.5 Index

至少：

~~~text
flashcards(active, deck_path)
flashcard_card_tags(tag)
flashcard_review_state(due_at_ms)
flashcard_review_logs(card_id, reviewed_at_ms)
~~~

## 10. Deck / Tag 筛选

两个维度可以一起使用。

例如：

~~~text
Deck = 判断推理/逻辑判断
Tag = 错题
~~~

得到“逻辑判断中的错题”。

选择父 Deck：

~~~text
判断推理
~~~

默认包含：

~~~text
判断推理
判断推理/逻辑判断
判断推理/图形推理
...
~~~

即 Deck 使用目录 subtree 语义。

多个 Tag 第一版使用 AND：

~~~text
错题 + 高频
~~~

表示同时拥有两个 Tag。

第一版不要求全文搜索；先保证 Deck + Tag 筛选与复习工作流完整。

## 11. hyz-things 后端架构

遵守当前：

~~~text
domain
application
adapters/inbound
adapters/outbound
composition root
~~~

边界。

### 11.1 Domain

新增：

~~~text
apps/rust/things/src/domain/flashcards.rs
~~~

负责纯值对象：

~~~text
FlashcardId
Flashcard
FlashcardSummary
FlashcardFilter
ReviewRating
ReviewState
SyncSummary
~~~

不依赖 Axum、SQLx、filesystem、Yew。

### 11.2 Application

新增：

~~~text
apps/rust/things/src/application/flashcards.rs
~~~

负责：

- sync use case；
- 卡片查询；
- Deck/Tag filter；
- due queue；
- review；
- FSRS 调度；
- inactive/reactivate 规则。

新增 application ports：

~~~text
FlashcardSourcePort
FlashcardRepositoryPort
~~~

测试用 fake port，不直接测试具体 SQLx 实现内部。

### 11.3 Outbound adapters

新增：

~~~text
apps/rust/things/src/adapters/outbound/flashcard_source.rs
apps/rust/things/src/adapters/outbound/flashcard_sqlite.rs
~~~

`flashcard_source.rs`：

~~~text
/mnt/hyz-cards
→ scan
→ parse Markdown
→ assign/write UUID
→ resolve assets
~~~

`flashcard_sqlite.rs`：

~~~text
SqlitePool
migrations
queries
transactions
~~~

### 11.4 Composition root

仍然只由：

~~~text
apps/rust/things/src/main.rs
~~~

创建具体：

~~~text
FilesystemFlashcardSource
SqliteFlashcardRepository
FlashcardApplication
~~~

LAN listener 与 Tailscale listener 共用同一个 FlashcardApplication / SqlitePool。

Flashcard DB 初始化失败不得导致整个管理门户无法启动，而是让 Study 卡片模块显示 unavailable，网络/代理/Camera 等门户继续正常。

## 12. HTTP API

Flashcard API 属于 `hyz-things` 自己的 HTTP contract，不加入 `hyz-contract`。

新增独立模块：

~~~text
apps/rust/things/src/adapters/inbound/http/flashcards.rs
~~~

避免继续膨胀当前较大的 `http/mod.rs`。

初始 API：

~~~text
GET  /api/v1/study/cards
GET  /api/v1/study/decks
GET  /api/v1/study/tags
GET  /api/v1/study/review
GET  /api/v1/study/summary

POST /api/v1/study/review
POST /api/v1/control/study/sync

GET  /api/v1/study/assets/{*path}
~~~

Cards 支持：

~~~text
deck
tag
limit
offset
~~~

Review 支持相同 Deck / Tag 范围。

### 12.1 权限

卡片阅读和普通复习保持 Study 页当前偏个人工具的体验：

~~~text
GET cards/decks/tags/review/assets
→ 不要求管理员登录

POST review
→ same-origin + CSRF
→ 不要求管理员登录
~~~

而：

~~~text
POST /api/v1/control/study/sync
~~~

会扫描文件、修改 SQLite、给 Markdown 写回 ID，因此要求：

~~~text
管理员 session
+
same-origin
+
CSRF
~~~

### 12.2 Asset 安全

浏览器不能直接访问 `/mnt/hyz-cards`。

图片通过：

~~~text
/api/v1/study/assets/...
~~~

读取。

服务端必须：

- 只接受相对路径；
- 拒绝 `..`；
- 拒绝绝对路径；
- canonicalize 后确认仍位于 `/mnt/hyz-cards`；
- symlink 不得跳出 source root；
- 第一版只返回明确支持的图片格式；
- 使用合理文件大小上限。

不提供任意文件读取接口。

## 13. Markdown 与图片渲染

浏览器收到：

~~~text
front_markdown
back_markdown
source_file
~~~

后进行 Markdown 渲染。

新增 `pulldown-cmark` 作为 web feature dependency。

Markdown raw HTML 默认不执行，不能通过卡片 Markdown 注入门户。

相对图片：

~~~markdown
![题目](./images/q001.png)
~~~

结合：

~~~text
source_file =
判断推理/逻辑判断/由果推因.md
~~~

解析为：

~~~text
判断推理/逻辑判断/images/q001.png
~~~

然后请求：

~~~text
/api/v1/study/assets/...
~~~

图片不 copy、不 encode、不存 SQLite blob、不存 `/userdata/assets`。

## 14. LaTeX

前端加入 self-hosted KaTeX，不使用 CDN。

支持至少：

~~~text
$ ... $
$$ ... $$
~~~

以及常见 display math。

例如：

~~~markdown
$$
基期量 = \frac{现期量}{1+r}
$$
~~~

KaTeX JS/CSS/font 都进入现有 deterministic frontend bundle。

继续满足 CSP：

~~~text
script-src 'self'
style-src 'self'
font-src 'self'
~~~

不为了数学公式开放第三方域名。

## 15. FSRS

复习调度使用成熟 FSRS 实现，不自行设计间隔算法。

UI 暴露三个评分：

~~~text
不会 → Again
模糊 → Hard
会了 → Good
~~~

Easy 第一版不暴露。

新 Card：

~~~text
没有 review_state
→ 视为待学习
~~~

已学习 Card：

~~~text
due_at <= now
→ 今日待复习
~~~

时间通过现有 `ClockPort` 注入，不在 application 测试中依赖真实墙钟。

Markdown 内容更新：

~~~text
Card UUID 不变
→ review state 保留
~~~

第一版不因为用户修改答案自动重置 FSRS。

## 16. Web 信息架构

现有一级“学习”继续存在，不新增“卡片”顶级门户入口。

Study 页调整为：

~~~text
学习
├── Flashcards
│   ├── 今日待复习
│   ├── 新卡
│   ├── 开始复习
│   ├── 卡片库
│   └── 同步卡片
│
├── 当前倒计时
└── 考试倒计时
~~~

现有 countdown/PiP 功能继续保留。

路由扩展：

~~~text
#/study
#/study/cards
#/study/review
~~~

Deck / Tag 筛选进入 query：

~~~text
#/study/cards?deck=判断推理&tag=错题
~~~

复习也允许限定范围：

~~~text
#/study/review?deck=判断推理&tag=错题
~~~

## 17. 独立复习界面

`#/study/review` 不使用现有 `AppShell`。

当前：

~~~text
AppShell
├── Header
├── Portal navigation
├── Content
└── Footer
~~~

复习路由改为：

~~~text
ReviewLayout
├── Back / Exit
├── Deck + Tag
├── Progress
├── Card
└── Review Controls
~~~

界面只保留：

~~~text
← 退出复习

判断推理 · 错题
12 / 38

┌────────────────────────┐
│                        │
│       Card Front       │
│                        │
│       显示答案          │
│                        │
└────────────────────────┘
~~~

翻面：

~~~text
┌────────────────────────┐
│ Front                  │
│                        │
│ Back                   │
│                        │
│ Markdown / Image / TeX │
└────────────────────────┘

[ 不会 ]   [ 模糊 ]   [ 会了 ]
~~~

不会出现总览、网络、代理、设备、Tailscale、Camera、Apps、System、Portal footer。

桌面端限制阅读宽度；iPad/手机充分利用屏幕，但不让正文无限拉宽。

第一版按钮优先，不加入左右滑评分，避免触摸误操作。

可以增加简单键盘快捷键：

~~~text
Space → 显示答案
1     → 不会
2     → 模糊
3     → 会了
~~~

## 18. Web 代码结构

现有：

~~~text
src/web/pages/study.rs
~~~

由当前的简单倒计时页扩展。

如果实现过程中体积明显增大，则拆成：

~~~text
src/web/pages/study/
├── mod.rs
├── dashboard.rs
├── cards.rs
├── review.rs
└── markdown.rs
~~~

API 新增：

~~~text
src/web/api/flashcards.rs
~~~

Route 继续由：

~~~text
src/web/navigation.rs
~~~

统一管理。

不引入 React/Vue/Svelte，继续使用 Yew、Tailwind CSS 4、daisyUI 5。

## 19. 并发与事务

同一时间只允许一个 Sync。

重复点击同步返回 busy，而不是启动多个目录扫描。

Sync：

~~~text
文件扫描/ID 写回
        ↓
SQLite transaction
        ↓
commit
~~~

数据库更新失败：

~~~text
SQLite transaction rollback
~~~

已经成功写入 Markdown 的 UUID 可以保留。下一次 sync 会正常识别，不需要回滚 Markdown ID。

Review 与普通查询允许在 Sync 过程中继续工作。

SQLite WAL 用于降低 review write 与 cards read 之间的阻塞。

## 20. TDD

按 `AGENTS.md` 对每个行为执行：

~~~text
Red
↓
Green
↓
Refactor
~~~

### 20.1 Parser / Source

覆盖：

- 一个文件解析多张 Card；
- file tags；
- card tags；
- Markdown 图片；
- LaTeX 原文不被破坏；
- 无 ID 自动分配 UUIDv7；
- ID 写回后重新解析仍得到同一 Card；
- duplicate ID 被拒绝；
- source directory 决定 Deck；
- 文件移动但 UUID 不变；
- `..` / symlink asset escape 被拒绝。

### 20.2 Sync Application

覆盖：

- 新 Card → insert；
- 内容变化 → update；
- unchanged → skip；
- 删除 → inactive；
- 重新出现 → reactivate；
- 文件移动 → preserve review；
- parse error 不错误停用旧 Card；
- source root unavailable 不批量 inactive；
- 同时只运行一个 sync。

### 20.3 SQLite

使用临时数据库验证：

- migrations；
- Card upsert；
- transaction rollback；
- Deck filter；
- Tag filter；
- Deck + Tag filter；
- due query；
- review state；
- review log；
- foreign key。

### 20.4 HTTP

验证：

- 精确 route/method；
- body limit；
- CSRF；
- Sync 要求管理员；
- Review 不要求管理员；
- typed JSON；
- stable error code；
- asset traversal 拒绝；
- asset root escape 拒绝；
- source absolute path 不泄漏。

### 20.5 Web unit

验证：

- Study route；
- cards route；
- review route；
- Deck/Tag query；
- relative image resolution；
- Markdown raw HTML 不执行；
- rating 映射。

### 20.6 Playwright

新增：

~~~text
flashcards.spec.ts
~~~

覆盖：

- Study 显示 Flashcards 区；
- Deck filter；
- Tag filter；
- Deck + Tag filter；
- 开始复习；
- 显示答案；
- 三个 rating；
- review 页面不显示 Portal navigation；
- review 完成状态；
- reload/Back 行为；
- sync success/error 展示；
- countdown 原有行为不回归。

## 21. 实施阶段

### 阶段 A：Storage foundation

完成：

~~~text
SQLx + SQLite
migrations
FlashcardRepositoryPort
SQLite adapter
composition root
~~~

只解决数据库和 persistence，不开始做完整 UI。

### 阶段 B：Markdown source + Sync

完成：

~~~text
/mnt/hyz-cards scanner
Markdown parser
Deck/Tag
UUIDv7
ID writeback
duplicate handling
inactive/reactivate
sync API
~~~

到这一阶段已经可以：

~~~text
编辑 Markdown
→ Sync
→ SQLite
~~~

### 阶段 C：查询 + FSRS

完成：

~~~text
cards/decks/tags API
due queue
review mutation
FSRS
review log
~~~

### 阶段 D：Study UI

完成：

~~~text
Study dashboard
卡片库
Deck/Tag filter
Markdown renderer
Image handler
KaTeX
~~~

### 阶段 E：沉浸式 Review

完成：

~~~text
#/study/review
独立 ReviewLayout
显示答案
不会 / 模糊 / 会了
进度
键盘快捷键
~~~

### 阶段 F：Regression / CI / target validation

完成 host：

~~~text
fmt
unit
clippy
frontend
Playwright
Portal CI
~~~

然后给出 RK3568 验收清单。

## 22. RK3568 验收

目标板实际验证：

- [ ] 电脑 SMB share 能经 Tailscale IP 挂载到 `/mnt/hyz-cards`。
- [ ] hyz-things 可以读取 `.md`。
- [ ] Sync 可以向 SMB share 原文件写回 UUID。
- [ ] 新增 Card 正确进入 SQLite。
- [ ] 修改 Card 后复习记录不丢。
- [ ] 移动 Markdown 后 UUID / Review history 不变。
- [ ] Deck 随目录变化。
- [ ] Tag 正确同步。
- [ ] 图片直接从 `/mnt/hyz-cards` 加载，没有本地 copy。
- [ ] LaTeX 正常显示。
- [ ] Deck + Tag 筛选正确。
- [ ] Review page 无 Portal navigation 干扰。
- [ ] 不会 / 模糊 / 会了正确改变下次 due。
- [ ] Tailscale 地址访问 Study/Review 正常。
- [ ] 原 Network/Proxy/Devices/Tailscale/Camera 功能无回归。

## 23. 完成定义

本功能完成需要同时满足：

~~~text
电脑 Markdown
      ↓
Tailscale + SMB mount
      ↓
/mnt/hyz-cards
      ↓
自动 UUID
      ↓
SQLite
      ↓
Deck / Tag
      ↓
FSRS
      ↓
沉浸式 Review
~~~

并且：

- Markdown 是内容真源；
- 图片永远原地读取；
- SQLite 永远在 RK3568 `/userdata`；
- Card ID 自动生成；
- 修改、移动 Card 不丢 Review history；
- 删除 Card 不物理删除历史；
- Deck/Tag 都可筛并可组合；
- Study 继续保留现有倒计时；
- Review 使用独立 Layout；
- 不迁移现有 credential/registry；
- 不修改 router/camera process ownership；
- 最终 Portal CI 全绿；
- 真实 RK3568 验收与 CI 状态分开记录。

## 24. 预计主要改动范围

~~~text
apps/rust/things/Cargo.toml
apps/rust/things/Cargo.lock
apps/rust/things/migrations/

apps/rust/things/src/domain/flashcards.rs
apps/rust/things/src/application/flashcards.rs
apps/rust/things/src/application/ports.rs

apps/rust/things/src/adapters/outbound/flashcard_source.rs
apps/rust/things/src/adapters/outbound/flashcard_sqlite.rs
apps/rust/things/src/adapters/inbound/http/flashcards.rs

apps/rust/things/src/main.rs

apps/rust/things/src/web/navigation.rs
apps/rust/things/src/web/api/flashcards.rs
apps/rust/things/src/web/pages/study*
apps/rust/things/src/web/components/*

apps/rust/things/frontend/
apps/rust/things/package.json
apps/rust/things/package-lock.json

apps/rust/things/e2e/flashcards.spec.ts
apps/rust/things/src/e2e/main.rs
~~~

`hyz-contract`、`hyz-router`、`hyz-camera` 原则上不改。
