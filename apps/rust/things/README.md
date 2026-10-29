# hyz-things

`hyz-things` 是 RK3568 产品上的**门户与管理进程**，按用户需求以“hyz things”个人网站形式承载设备管理面。它不参与路由/防火墙/NAT，而是通过 root-only Unix socket 驱动独立无头核心 `hyz-router`，并通过 `/run/hyz-camera/control.sock` 驱动独立媒体进程 `hyz-camera`。

- 板端程序：`/usr/bin/hyz-things`（init：`/etc/init.d/S83hyz-things`）
- 管理地址：`https://192.168.8.1:8080`（LAN 固定 HTTPS 监听，设备使用自签证书；Tailscale 认证后精确绑定单个 Tailscale IPv4）
- 启动前置：等待 `/run/hyz-router/ready` 标记，之后才绑定 HTTPS
- 共享 wire 契约：`apps/rust/contract`（`hyz-contract`）
- 详细设计、架构图和板端验证记录：[docs/architecture.md](../../../docs/architecture.md)

## 当前功能与页面

当前页面集合以 [`src/web/navigation.rs`](src/web/navigation.rs) 的 `AppPage::ALL` 为准，导航顺序为 **总览、学习、网络、代理、设备、Tailscale、摄像头、系统**。旧“应用”页面已经取消：热推送组件的当前版本信息归入“系统 → 热部署记录”，不保留 `#/apps` 页面。

| 页面 / 路由 | 实现模块 | 功能范围 |
| --- | --- | --- |
| `#/overview` | `web/pages/overview.rs` | 健康状态、网络拓扑、router/代理/系统状态、异常 |
| `#/study` | `web/pages/study.rs` | 学习概览、Deck/Tag 卡片库、Flashcards 复习与专项训练入口 |
| `#/network` | `web/pages/network.rs` | Wi-Fi STA 扫描与配置、AP 修改及确认/回滚 |
| `#/proxy` | `web/pages/proxy.rs` | Mihomo 订阅、节点选择与延迟、LAN TUN、本机系统代理 |
| `#/devices` | `web/pages/devices.rs` | LAN 设备、活动/流量、单设备策略与详情 |
| `#/tailscale` | `web/pages/tailscale.rs` | 远程 LAN 管理、Tailscale peers、direct/relay 状态 |
| `#/camera` | `web/pages/camera.rs` | WebRTC 直播、画中画、音视频对讲、画面预设与旋转 |
| `#/system` | `web/pages/system.rs`、`deployments.rs` | 系统状态、LCD 背光/亮度、喇叭音量、热部署当前版本 |

当前摄像头观看与设置均复用管理员会话和 CSRF，不是免登录匿名直播。门户的固定 LAN 入口使用 **HTTPS + 自签证书**，可选绑定 IPv6-only 监听，并对精确的 Tailscale IPv4 提供管理 listener；不会回退到 `0.0.0.0` 广泛监听。首次登录必须更改默认管理密码；管理员凭据位于 `/userdata/hyz-router/admin/credential.json`（Argon2id）。外部服务异常时，状态聚合返回明确的 degraded 快照，健康检查不伪装成依赖就绪。

### 学习与出题

`src/web/pages/training.rs` 当前包含 **速算训练、错题复练、资料分析综合训练、逻辑判断** 四种模式；学习中心首页仍可能保留较旧的“三种模式”提示文案，不能据此忽略已接入的逻辑判断页面。

- 卡片源：设备端 `/mnt/hyz-cards`，经 `FilesystemFlashcardSource` 读取 Markdown；主机可运行仓库根目录的 `scripts/share-flashcards.sh` 配置 Samba 共享（默认 TCP 1445），还需独立处理板端挂载。
- 持久化：`/userdata/hyz-things/flashcards.db`（SQLite），承载卡片索引、间隔复习及资料分析/逻辑判断训练记录。复习与“错题复练”的本轮自评并非同一写入路径。
- 资料分析：4 份模拟统计材料 × 各 5 题，20 分钟；支持生成、暂停、跨设备续答、历史解析、原题重做和卡片统计。
- 逻辑判断：形式推理相关卡片生成 10 道独立题，10 分钟；支持答题、暂停/恢复、成绩、解析和重做。
- 出题配置：需要 PC 侧 Codex App Server、卡片与 Skill 所在工作目录；参见 [`.env.example`](.env.example) 中的 `HYZ_THINGS_CODEX_WS_TOKEN` 和 `HYZ_THINGS_CODEX_CWD`。这些配置不是源码里可公开复用的令牌。
- 画布与触控：训练阅读区域、滚动控制和 Apple Pencil 草稿层位于 `src/web/pages/`，并通过 Playwright WebKit iPad 预设做主机回归。

### 运行边界

- `hyz-things` 不拥有 LAN/WAN/NAT/firewall：通过 `hyz-contract` 的 `RouterControlClient` 请求 `hyz-router` 的 root-only socket。
- 摄像头信令使用 `CameraUnixAdapter`，实际 H.264/Opus 媒体通过浏览器和 `hyz-camera` 的 WebRTC UDP 路径传送。
- `tools/deploy-app.sh` 仅针对 `hyz-things` / `hyz-camera` 推送 ELF 并检查协议兼容性；不会重启 router。注册表 `/userdata/hyz-things/apps/registry.json` 只保存**各组件现用热部署版本**，不保存历次日志；固件内置版本可能没有记录。
- Router 的 USB ADB、网络 ADB 开发热替换和正式 OTA 有独立的生命周期，不应混用。

## 代码结构

```text
src/domain/                 纯类型与不变量，含 flashcards、资料分析、设备策略
src/application/            admin、camera、Flashcards、逻辑判断、status 和 ports
src/adapters/inbound/http/  Axum HTTPS/API，包含学习与控制端点
src/adapters/outbound/      Router/Camera UDS、管理员、SQLite、卡片文件源、Codex、注册表
src/web/navigation.rs       当前八页导航和 hash 路由契约
src/web/pages/             总览、学习、训练、网络、代理、设备、Tailscale、摄像头、系统
src/e2e/                   host-only loopback harness（e2e feature）
e2e/                       Playwright E2E 套件与 fixtures
migrations/                SQLite schema / 训练和复习状态迁移
frontend/                  Trunk + Tailwind CSS 4 / daisyUI 5 前端入口
tools/                     前端 bundle、E2E 启动、热部署及测试脚本
tests/                     HTTP 安全与服务端集成测试
```

各层保持 `adapters -> application -> domain` 调用方向。Yew 浏览器只通过受限 HTTPS API 请求 native 端能力，不直接访问 UDS 或执行 Linux 管理命令。运行时采用独立的 router、things、camera 进程。

## 构建与检查

最低 Rust 版本为 `1.85`，前端工具要求 Node.js `20` 或更新版本。native 实现只支持 Linux。

```sh
cargo test --locked --features native
cargo clippy --locked --all-targets --features e2e -- -D warnings
```

前端 deterministic bundle 与 Playwright 浏览器测试从仓库根目录执行：

```sh
make things-frontend
make things-e2e
make things-app
```

`things-frontend` 先运行 `npm run build:css`（Tailwind/daisyUI 静态 CSS）再运行 Trunk release WASM 构建，产物为确定性的 `hyz-things-frontend.tar`；`things-e2e` 使用 host-only loopback Axum harness 运行 Playwright，不执行产品网络命令；`things-app` 将 bundle 嵌入 ELF 并完成 AArch64 native 构建。

### Playwright E2E：默认 iPad WebKit

`playwright.config.ts` 默认使用 `webkit` 浏览器与 `iPad Pro 11 landscape` 设备预设（移动端与触控模拟）；它覆盖 iPad 横屏交互，但**不是在真实 iPad 上运行的 Safari 测试**。从仓库根目录执行 `make things-e2e`，或在项目目录直接运行：

```sh
cd apps/rust/things
npm ci
npx playwright install --with-deps webkit
npm run test:e2e
```

`npm run test:e2e` 会先执行 `npm run test:e2e:startup`（`tools/test-e2e-startup.py`），再执行 Playwright 用例。启动测试覆盖默认 WebKit、可选 Chromium、非法浏览器配置、服务未就绪和端口占用等情况；测试 harness 通过输出包含 `web_origin` 的就绪记录通知 Playwright，而不是等待 loopback HTTP 健康探针。默认使用独立端口，不复用已运行的服务器。

如需回归桌面 Chromium，请安装相应浏览器后显式选择：

```sh
npx playwright install --with-deps chromium
npm run test:e2e:chromium
# 等价于：HYZ_THINGS_E2E_BROWSER=chromium npm run test:e2e
```

CI 的 `Portal CI` 工作流默认安装 WebKit 并执行 `npm run test:e2e`，失败时上传 Playwright 截图、trace、视频等可用产物。它复用事先校验的前端 bundle；本地测试仍会按需准备 bundle。真实 RK3568 摄像头和音视频路径不由这些 host-only mock 套件验收，另见 [`docs/camera-hardware-e2e.md`](../../../docs/camera-hardware-e2e.md)。

## 运行

`hyz-things daemon` 只能在具备完整产品运行时的目标板上以 root 启动，且必须由 `S83hyz-things` 在 router 就绪后拉起。普通开发机上可用 `hyz-things-e2e`（非默认 `e2e` feature）预览页面，它只绑定 loopback 并使用 fake ports。
