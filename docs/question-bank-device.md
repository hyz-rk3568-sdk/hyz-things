# 题库共享、同步与整卷加载

当前实现提供电脑端独立共享脚本、设备端挂载脚本，以及学习中心的「同步题库」和整卷题面/材料加载。~~作答判分、答案解析、专项练习、收藏错题和套装尚未接入。~~ 现已提供套卷/模块/专项练习、交卷判分、原题解析、错题及收藏练习、历史和恢复；运行时题库正文采用 SQLite。参见 [题库 SQLite 读取与性能边界](question-bank-sqlite.md)。

## 电脑端独立共享

设备题库在本仓库 `question_bank/papers/`、`question_bank/collections/` 和 `question_bank/assets/`，作为普通文件随公开主仓库版本管理；导入器清单、审计报告及备份保留在本机，不纳入公开提交。`collections/` 存放 `kind: drill` 的专项合集，与真题共用练习引擎，但不进入真题试卷列表和真题预测。维护方式见 [导入工具](../scripts/question_bank/README.md)。

先保留现有 `hyz-cards` 共享，然后运行：

```sh
sudo scripts/share-question-bank.sh --configure
```

脚本复用既有 Samba 服务、端口和账号，新增 `[hyz-question-bank]`，不改卡片共享块或全局端口。默认参数沿用卡片脚本的 `HYZ_CARDS_SMB_USER` / `HYZ_CARDS_SMB_PORT`；如果现有卡片共享用了非默认账号/端口，需传入相同值。目录可用 `HYZ_QUESTION_BANK_SHARE_DIR` 显式指定。配置替换前备份并通过 `testparm` 检查；服务重载会影响同一 smbd 上的连接，应在没有编辑/同步时执行。

共享隐藏 `.git` 和 `reports`。网络 ADB 板测时确认主机已存在对应共享，直接复用，没有改写主机 Samba 配置。

## 设备端挂载

Buildroot overlay 新增 `/etc/init.d/S83hyz-question-bank`，固定挂载：

```text
//<已配置服务器>/hyz-question-bank → /mnt/hyz-question-bank
```

默认复用：

- `/userdata/hyz-things/flashcards/mount.conf` 中的服务器和端口；
- `/userdata/hyz-things/flashcards/credentials` 中的既有 Samba 账号凭据。

可选独立服务器/端口配置为 `/userdata/hyz-things/question-bank/mount.conf`，格式参见 overlay 内 `question-bank-mount.conf.example`。凭据仍复用已有文件，不在 Web API 传递或显示。挂载脚本只卸载与配置精确匹配的 CIFS source，不清理 foreign mount。

开机挂载脚本仍需随 Buildroot overlay 安装。本次已在授权板端手动挂载题库，并通过网络 ADB 热更新 things；没有构建或安装固件，也没有安装新的开机挂载脚本。

## 学习中心

登录已有管理员 Session 并完成首次改密，在「学习」页：

1. 点击「同步题库」。
2. 等待完整目录扫描、格式/身份/图片引用预检。
3. 对缺失的 paper/question/material UUIDv7，仅插入必要 HTML 注释并写回 Markdown。
4. 写回前按原内容哈希备份到共享根 `.hyz-sync-backups/`；再次核对文件版本，经同目录临时文件原子替换。
5. 全批复核成功后，事务性更新 `/userdata/hyz-things/question-bank.db` 的索引。
6. 点击试卷，按来源题号加载整卷题面、选项、共享材料与图片。材料在首次引用处展示一次。

普通列表排除 `role: evidence` 的同卷佐证副本。~~题库数据库只存索引与稳定身份，不存整题正文，也不接触 `flashcards.db`。源文件变化后旧索引拒绝加载该卷并提示重新同步；源不可用或扫描失败时保留上次索引，不把断连视作删除。~~ 当前 `question-bank.db` 保存已同步的题目正文、选项、材料、索引及独立练习状态；运行时从 SQLite 读取内容，源 Markdown 通过显式同步导入，不混入 `flashcards.db`。归档身份仍保留，以检测旧文件恢复后 UUID 丢失。详见 [SQLite 设计](question-bank-sqlite.md)。

同步在同一个 application 实例内串行，LAN 与 Tailscale 共用它。CIFS 不提供跨编辑器的原子比较后交换；同步期间应暂停电脑端保存/导入，也不要让多个设备同时补写同一题库的 UUID。部分文件写回成功但整批失败时，索引不提交；重试会复用已写回 ID。之后在电脑端审查 Git diff 并保存 ID 变更。

当前设备读取的图片是导入器生成的 `assets/<SHA-256>.<png|jpg|jpeg|webp|gif|bmp>`；手工添加图片也需按这个规则存放和引用。文件路径经过逐级禁止符号链接的目录句柄解析；API 只接收 UUID 或图片哈希名，不接收服务器、路径、命令或 URL。

## 接口与验证

所有题库读取和图片接口都要求现有管理员 Session。同步额外要求精确 Origin、CSRF 和大小受限的空 JSON `{}`。

- `POST /api/v1/control/question-bank/sync`
- `GET /api/v1/question-bank/papers`
- `GET /api/v1/question-bank/papers/{UUIDv7}`
- `GET /api/v1/question-bank/collections`
- `GET /api/v1/question-bank/assets/{SHA-256}.{extension}`

作答前的题目 DTO 不包含 Answer、Explanation 或可能附有答案校核内容的 Notes；~~当前也没有答案读取接口。~~ 已交卷后可通过受授权保护的复盘接口读取标准答案与解析。共享 Markdown 中原始答案与解析仍完整保留。

主机验证入口：

```sh
bash scripts/test-share-question-bank.sh
bash scripts/test-question-bank-mount.sh
cargo test --locked --manifest-path apps/rust/things/Cargo.toml --test question_bank
cargo test --locked --manifest-path apps/rust/things/Cargo.toml --no-default-features --features web --bin hyz-things-web
```

仅运行题库相关浏览器用例：

```sh
cd apps/rust/things
HYZ_THINGS_E2E_BROWSER=chromium npm run test:e2e -- e2e/flashcards.spec.ts --grep 'question bank'
```

浏览器用例以固定 API fixture 验证同步按钮、试卷加载、来源题号和材料只展示一次；HTTP/application 测试通过 fake ports 验证真实服务器安全边界。真实 SMB 断连、写回恢复、设备挂载、LAN/Tailscale 和设备页面仍需部署后板端验收，不能用主机测试替代。

## 本次验收记录（2026-11-02）

> 以下是 2026-11-02 当天的历史现场记录，不应被当作当前功能缺失清单或最新设备验收状态。

- 题库专项 Rust 测试 10 项、前端主机测试 42 项、电脑端导入测试 21 项通过；既有原生应用测试也已通过。
- 使用当前设备端纯解析器对实际题库做只读验收：739 份文件、86,545 题位、16,449 处图片引用及 90,314 个 UUID 的内存写回计划通过；没有改写电脑题库 Markdown。记录保存在题库内 `reports/device-format-audit.json`。
- 新增共享的配置渲染/幂等测试、原卡片共享回归和挂载 ownership fake-command 测试通过。
- 原生 Clippy（包含 e2e feature）通过。额外严格 Web Clippy 被未修改的 `protected_control.rs` / `resources.rs` / `scratchpad.rs` 既有告警阻塞，不计作通过；Web 主机测试与前端测试包另行验证。
- 浏览器验证仅选择 `flashcards.spec.ts` 中题库同步与整卷加载这一条 Chromium 用例，**1/1 通过**，未执行全套 E2E。
- 后续网络 ADB 热更新验证见下文。实际全量同步/UUID 写回、登录后的整卷加载、Tailscale 访问及断连恢复仍待管理员操作和板端验收。

## 网络 ADB 临时热更新记录（2026-11-02）

- 确认板端内核已支持 CIFS，`/usr/sbin/mount.cifs` 可用，原 `hyz-cards` 挂载正常。
- 复用已有 Samba 账号、凭据文件、端口与 `hyz-question-bank` 共享，将题库手动挂载到 `/mnt/hyz-question-bank`；读写探针、主机/板端样卷 SHA-256 一致，`.git` 与 `reports` 被共享隐藏。
- 使用现有 Buildroot AArch64 工具链和已通过题库 E2E 的前端包构建 things，仅执行应用构建，没有重新配置/构建 SDK、rootfs、kernel 或 firmware。
- 使用 `deploy-app.sh` 经网络 ADB 热更 things。协议兼容检查在停止服务前通过；新 ELF SHA-256 为 `f09936ffa50d4735e43badfd333a5aae68a5947d5b6f1b153142cb3c1a693d88`，原 ELF `6b4b06cdba978396b7c9afe1adabd5da38329b2847a55abef6d0fc8181eafcbd` 已由部署工具保留为回滚版本。
- router PID/start time、router/camera ELF、router ready 哈希在热更前后保持一致；公共状态接口 HTTP 200，组件状态与更新前一致，既有 Tailscale degraded 状态未变化。
- 板端实际提供的 index 与测试前端包一致；题库接口从旧版本的 404 变为需要管理员认证的 401；独立 `question-bank.db` 已创建且由 things 打开。
- 此次是临时手动挂载，重启后需要重新挂载。热更重启了 things，管理员需刷新页面重新登录，再在学习中心执行首次同步；未读取或重置管理员凭据，也未绕过认证发起同步。

构建与部署日志保存在工作区忽略目录 `output/question-bank-hot-update-20261102/`，包括更新前后 router/camera 校验与状态对照。

## 实机首次同步诊断（2026-11-02，待修复）

用户触发的同步在日志中于北京时间 21:55:04 返回 `question_bank_conflict`（HTTP 409），该请求耗时 355,477 ms（5 分 55 秒）。现场曾同时观察到两个读取题库图片的 blocking worker，其中较早线程于 21:48:20 创建；现有实现缺少同步开始/阶段状态，线程年龄不能替代请求精确耗时。随后扫描线程退出。

核验 739 个 Markdown 文件：90,314 个 UUID 格式合法且跨文件唯一，去除 ID 注释后的正文哈希全部与导入基线相同，没有正文丢失；设备端索引仍为 0 卷、0 条身份记录。当前状态是 UUID 已写回、索引未提交，不能宣称实机同步成功。源码中的请求级 semaphore 随调用 future 释放，而 `spawn_blocking` 扫描可能继续执行，存在请求取消后重复扫描的风险；该风险与现场现象一致，具体冲突触发点仍需确定性复现和修复。

同次排查发现 Tailnet 设备列表 503：router 与 Tailscale 后端都仍运行，后端 `status --json` 能返回 5 个 peer，Health 为空；但运行中的 tailscaled 为 1.102.2，其 `/proc/<pid>/exe` 已标记 `(deleted)`，磁盘客户端为 1.104.4。router 的 exact executable ownership 检查拒绝此进程，导致 `tailscale_peers_probe_failed`。此为需要受控维护恢复的运行时版本/身份不一致，诊断没有重启服务或改写 ownership 记录。
