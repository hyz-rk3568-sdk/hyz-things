# hyz-things

`hyz-things` 是一个面向 ATK RK3568 的个人软路由、家庭设备管理与学习训练项目。板端将路由控制、HTTPS 管理门户和摄像头媒体拆成三个独立 Linux 进程；门户还集成 Flashcards 复习、速算、资料分析和逻辑判断训练。进程间通过类型化、root-only 的 Unix socket 协议协作。

项目不是通用 OpenWrt 后台，也不提供运行时插件 ABI 或任意 Linux 网络配置器。网络、进程、防火墙和固件操作都由产品代码中的固定用例和受限 adapter 驱动。

> **项目状态**：这是面向个人设备的持续开发项目，不是开箱即用的通用路由器发行版。主机测试、部分 RK3568 板端功能和管理页面已有实现；双上游完整切换、代理故障注入、长时间稳定性及部分后续产品范围仍需验收。阅读功能清单时请区分“源码已实现”和“真实硬件已验收”。

## 产品组成

```text
apps/rust/contract  共享版本化 wire 契约
apps/rust/router    无头路由核心，板端 /usr/bin/hyz-router，S81
apps/rust/things    管理门户，板端 /usr/bin/hyz-things，S83
apps/rust/camera    媒体进程，板端 /usr/bin/hyz-camera，S82
```

### `hyz-router`

无头路由控制核心，不提供 HTTP、Web UI、管理员认证或摄像头信令。它负责：

- 固定 LAN：`br-lan = eth1 + p2p0`，管理地址 `192.168.8.1/24`；
- LAN DHCP、DNS、IPv4 forwarding、NAT 和最小安全防火墙；
- `eth0` 有线 WAN DHCP，默认路由 metric `100`；
- `wlan0` Wi-Fi STA DHCP，默认路由 metric `600`，作为有线线路的热备用；
- Mihomo 显式代理、LAN TUN、普通 NAT fail-open 与受控核心故障恢复；
- Tailscale 生命周期、peer 状态和 runtime-owned 防火墙规则；
- recovery-free OTA、shutdown、DHCP hook 和状态聚合；
- root-only `/run/hyz-router/control.sock` 和管理面就绪标记 `/run/hyz-router/ready`。

`hyz-router` 只有在管理 LAN、AP、DNS 等资源经过严格 reconcile 后才写入 ready 标记。WAN 或代理不可用时，设备可以继续保持 management-only 管理路径。

### `hyz-things`

独立 HTTPS 管理门户（Axum + Yew/WASM SPA），不仅展示路由器状态，还负责浏览器会话、学习训练与设备管理：

- LAN 固定入口 `https://192.168.8.1:8080`（自签证书）；精确 Tailscale IPv4 监听及可选 IPv6-only HTTPS 监听；
- Argon2id 管理员认证、首次强制改密、会话与 CSRF；受保护的配置与控制 API；
- 八个导航页：**总览、学习、网络、代理、设备、Tailscale、摄像头、系统**；
- 学习中心：本地 SQLite 卡片及复习状态、`/mnt/hyz-cards` 文件源、专项训练记录；可选与 PC 上的 Codex App Server 协作出题；
- 系统健康、网络拓扑、设备活动、代理状态、LCD 背光/喇叭音量及当前热部署版本展示；
- 通过 `hyz-contract` 访问 `/run/hyz-router/control.sock`，通过 camera control socket 发起媒体控制和 WebRTC 信令。

门户等待 `/run/hyz-router/ready` 后才对外提供 HTTPS，不直接执行任意网络/防火墙命令。设备端学习数据位于 `/userdata/hyz-things/flashcards.db`；摄像头播放和受控操作要求管理员会话。

### `hyz-camera`

独立媒体进程，负责：

- V4L2 摄像头采集；
- GStreamer 与 Rockchip MPP H.264 编码；
- 固定分辨率、码率和旋转预设；
- WebRTC/str0m 会话；
- 固定 `40000-40015/udp` 媒体端口池；
- 音频采集、RNNoise/VAD、Opus 编码和对讲回放。

摄像头不执行 `ip`、`iptables` 或其他网络配置命令。浏览器和摄像头之间的媒体流不经过 Axum 或 router 转发。

## 门户功能与页面

当前导航以 [`apps/rust/things/src/web/navigation.rs`](apps/rust/things/src/web/navigation.rs) 中的 `AppPage` 为准；**不存在独立的“应用”导航页**，`#/apps` 也不是有效的当前页面路由。

| 页面 | 当前功能 |
| --- | --- |
| 总览 `#/overview` | 核心健康、网络拓扑、系统/代理状态、故障与只读信息 |
| 学习 `#/study` | 学习概览、Flashcards 卡片库与间隔复习、专项训练 |
| 网络 `#/network` | Wi-Fi STA 扫描和配置、AP 设置及受控确认/回滚 |
| 代理 `#/proxy` | Mihomo 订阅、节点/延迟、LAN 透明代理、本机系统显式代理 |
| 设备 `#/devices` | LAN 设备列表、搜索、活动流量/连接信息、按设备管理策略 |
| Tailscale `#/tailscale` | 远程访问模式、登录/启停、peer 与 direct/relay 状态 |
| 摄像头 `#/camera` | WebRTC 直播、画面预设/旋转、暂停/停止、画中画、双向语音对讲 |
| 系统 `#/system` | 状态与资源详情、LCD 亮度、喇叭音量、**热部署记录** |

网络、代理、设备、Tailscale、摄像头等管理操作由管理员会话保护；学习内容的阅读/复习与受控同步具有不同的权限边界。热部署记录读取的是当前 registry 快照，**仅记录各组件现有热推送版本，不保存完整的历次部署日志**；固件内置组件可能没有记录。

### 学习中心

学习模块属于门户的一部分，而不是单独的板端服务；相关页面见 `apps/rust/things/src/web/pages/`，持久数据由门户的 SQLite repository 管理。

- **Flashcards**：从板端 `/mnt/hyz-cards` 卡片目录同步 Markdown，按 Deck/Tag 查看、筛选和复习；复习进度存入 `/userdata/hyz-things/flashcards.db`。主机可通过 `scripts/share-flashcards.sh` 提供受控 Samba 共享，需另外完成目标板挂载。
- **速算训练**：在固定全屏答题区练习计算，支持草稿操作。
- **错题复练**：利用已同步的 Flashcards 进行专项复练，和正式的间隔复习记录分离。
- **资料分析综合训练**：每卷 **4 份模拟材料 × 每份 5 题、共 20 题 / 20 分钟**；支持暂停、跨设备续答、交卷解析、历史查看与原题重做，模拟材料不应当作官方真题或真实统计数据。
- **逻辑判断**：基于已同步的形式推理方法卡出题，每卷 **10 道独立题 / 10 分钟**；支持暂停/恢复、解析、历史记录与原题重做。
- **出题依赖**：资料分析与逻辑判断的 AI 生成需要另外配置 PC 上的 Codex App Server、能力令牌与有效的 Flashcard/Skill 工作目录；未配置时不能假定出题可用。参见 `apps/rust/things/.env.example` 和相关训练方案文档。

iPad 训练界面使用分区滚动与 Apple Pencil/触控草稿适配。浏览器 WebKit 设备模拟仅用于主机回归，不替代真实 iPad 或板端体验验证。

## 运行时边界

```text
浏览器 -- HTTPS / 同源 API --> hyz-things (Axum + Yew)
                               ├── root-only versioned UDS --> hyz-router --> LAN/WAN/Mihomo/Tailscale/OTA
                               ├── root-only camera UDS  --> hyz-camera --> WebRTC 媒体直连浏览器
                               ├── SQLite --> /userdata/hyz-things/flashcards.db
                               └── 可选：/mnt/hyz-cards / PC Codex App Server
```

三进程各自拥有独立的 composition root 和板端 ELF。共享协议位于 `apps/rust/contract`；router/camera 的系统权限及网络变更不授予浏览器或 Web 层。媒体码流不经门户/路由进程转发。

## 当前实现状态

**源码已实现或已有主机/部分板端验证：**

- 三进程拆分、router ready 门控、管理网络失联降级，以及 typed Ethernet/Wi-Fi 双上游状态与路由优先级；
- Mihomo 订阅和节点管理、`127.0.0.1:7890` 本机显式代理、LAN TUN 与普通 NAT fail-open；
- Tailscale RouterOnly / 固定 LAN subnet access、peer 状态、direct/relay/DERP 观测及独立管理入口；
- HTTPS 管理门户、管理员会话、网络/AP 设置、设备策略与活动视图、系统和显示/喇叭控制；
- WebRTC 摄像头直播、H.264 画面预设、旋转、画中画及音频对讲；板端媒体链路另有真实硬件验收脚本；
- 学习中心的 Flashcards、资料分析综合训练、速算、错题复练、逻辑判断和持久化训练历史；
- things/camera 热推送只重启对应进程，router 继续运行；router 开发热替换与正式 OTA 分离；
- 前端确定性 bundle、主机单元/静态检查，以及默认 WebKit iPad 横屏的 Playwright E2E。

**尚不能视作全部验收完成：**

- Ethernet/`eth1`/双上游的完整切换矩阵、长时间稳定性及 SR-09 反复切换；
- DNS 接管、所有代理节点失效、Mihomo live-hang 及更全面的故障注入；
- PPPoE、DNS 中心、网络黑匣子等规划功能；AI 出题还受外部服务和实际卡片数据可用性约束。

实际产品验收仍以 [`docs/soft-router-user-stories.md`](docs/soft-router-user-stories.md)、板端测试与 CI 结果为准；`docs/plan/` 中的历史方案不必然代表当前代码行为。

## 开发环境

当前产品构建面向 Linux x86-64 主机和 ATK RK3568 目标板。

基本依赖：

- Git、curl、Python 3、GNU Make；
- Rustup 和 Rust `1.85` 或更新版本；
- Node.js `20` 或更新版本、npm；
- Buildroot 所需的常规 Linux 主机工具；
- 仓库本地 Trunk：`.tools/trunk/bin/trunk`；
- Android `repo` 工具会在首次使用 SDK 目标时自动下载到 `.tools/repo`。

Rockchip vendor source、Buildroot 和内核位于 `sdk/`，由 Android `repo` manifest 固定具体组件版本；产品应用位于 `apps/`。门户 UI 的卡片源、Codex Agent 和 Samba 共享属于额外的个人工作流，不能仅靠编译固件自动启用。

## 构建和检查

首次同步 SDK：

```sh
make sdk
```

完整固件构建使用默认目标 `upgrade`：

```sh
make
# 或
make upgrade
```

常用开发目标：

```sh
make check-static    # shell、Python、manifest 和源码引用检查，不编译固件
make check           # fmt、host tests、Clippy 和部署工具测试
make things-frontend # 构建确定性的 Yew/Tailwind 前端 bundle
make things-e2e      # 在 loopback Axum harness 上运行默认的 iPad WebKit E2E 测试
make router-app      # 构建 hyz-router AArch64 ELF
make things-app      # 构建 hyz-things AArch64 ELF
make camera-app      # 构建 hyz-camera AArch64 ELF
make apps            # 构建三个产品 ELF
make overlay         # 将三个 ELF 和产品元数据放入 rootfs overlay
```

`make check` 不会构建 Buildroot 固件；完整 firmware、kernel、rootfs 和 loader 构建由 `make upgrade` 触发。

### 门户浏览器 E2E

`make things-e2e` 默认使用 Playwright 的 **WebKit + `iPad Pro 11 landscape`** 设备预设，验证 iPad 横屏布局、触摸交互及门户功能。此配置是在 Linux/开发机上的浏览器与设备模拟，**不等于真实 iPad Safari 或 RK3568 板端验收**。首次运行需先在 `apps/rust/things` 执行 `npm ci` 并安装 WebKit：

```sh
cd apps/rust/things
npm ci
npx playwright install --with-deps webkit
npm run test:e2e
```

`npm run test:e2e` 会先执行启动与就绪信号回归测试，再运行 Playwright 套件；CI 也默认安装并运行 WebKit。需要切换到桌面 Chromium 时，另外安装浏览器并执行 `npm run test:e2e:chromium`。本地 `make things-e2e` 还要求仓库本地 Trunk 已配置。具体用法见 [`apps/rust/things/README.md`](apps/rust/things/README.md)，真实摄像头硬件测试见 [`docs/camera-hardware-e2e.md`](docs/camera-hardware-e2e.md)。

构建产物主要位于：

```text
output/upgrade.fw
output/upgrade.fw.sha256
output/upgrade-recovery.fw
output/recovery.img
```

普通 `upgrade.fw` 是 recovery-free OTA，不更新 recovery 和 userdata。只有 recovery 本身或固件兼容性发生变化时，才显式运行：

```sh
make upgrade-recovery
```

## 板端部署

### Things 和 camera 热推送

只热推送非 router 应用：

```sh
ADB_SERIAL=<serial> make deploy-things
ADB_SERIAL=<serial> make deploy-camera
```

部署工具会先检查 wire protocol 兼容性和 ELF 校验和，只停止并重启对应的 `S83hyz-things` 或 `S82hyz-camera`，不会重启 router。门户“系统 → 热部署记录”展示的是 `/userdata/hyz-things/apps/registry.json` 的当前版本快照，不是完整部署流水账。回滚使用：

```sh
ADB_SERIAL=<serial> make revert-things
ADB_SERIAL=<serial> make revert-camera
```

### Router 开发热替换

router 不使用 `deploy-app.sh`：

- USB ADB：上传、校验、备份后执行 `stop → 原子替换 → start`；
- 网络 ADB：旧进程仍运行时完成校验、备份和原子替换，然后 `adb reboot`；
- 网络 ADB 可以使用物理 LAN 或明确可达的 Tailscale TCP `5555`；
- 正式发布使用 OTA，不把开发热替换当作正式发布流程。

完整步骤见 [`docs/router-app-debug.md`](docs/router-app-debug.md) 和 [`docs/network-adb.md`](docs/network-adb.md)。

### OTA

OTA 构建、传输、SHA-256 校验、安装和重启流程见 [`docs/ota-deployment.md`](docs/ota-deployment.md)。当前 OTA 的 SHA-256 只提供传输完整性校验，还没有发布者签名、anti-rollback 或 A/B 回滚机制，因此不应直接视为最终生产发布系统。

## 仓库结构

```text
apps/rust/contract/  共享 serde wire 契约
apps/rust/router/    无头路由核心
apps/rust/things/    HTTPS 管理门户、学习业务、Yew SPA、E2E 与部署工具
apps/rust/camera/    V4L2/GStreamer/MPP/WebRTC 媒体进程
flashcards/          主机侧 Markdown 学习卡片与错题材料
.agents/skills/      供本地 Agent / Codex 出题使用的技能与参考
scripts/             主机卡片共享等辅助脚本
product/             Buildroot rootfs overlay 和 init 脚本
sdk/                 Android repo 管理的 Rockchip/Buildroot/kernel 组件
output/              本地构建产物，不应提交
.tools/              本地构建工具，不应提交

docs/architecture.md
  三进程架构、composition root 和启动边界

docs/soft-router-user-stories.md
  产品范围、用户故事和验收状态

docs/camera-hardware-e2e.md
  摄像头真实硬件验收
```

更多文档入口：

- [`apps/rust/things/README.md`](apps/rust/things/README.md)：门户进程、页面模块、学习数据、HTTPS 安全边界及前端测试；
- [`apps/rust/router/README.md`](apps/rust/router/README.md)：router 用例、CLI、持久化配置和 OTA；
- [`docs/architecture.md`](docs/architecture.md)：完整架构和启动时序；
- [`docs/soft-router-user-stories.md`](docs/soft-router-user-stories.md)：当前产品状态和未完成验收；
- [`docs/router-app-debug.md`](docs/router-app-debug.md)：router 开发热替换；
- [`docs/network-adb.md`](docs/network-adb.md)：物理 LAN/Tailscale 网络 ADB；
- [`docs/ota-deployment.md`](docs/ota-deployment.md)：OTA 部署和回滚边界；
- [`docs/camera-hardware-e2e.md`](docs/camera-hardware-e2e.md)：真实摄像头与音视频链路验收；
- [`docs/frontend-bundle-cache.md`](docs/frontend-bundle-cache.md)：前端确定性构建缓存与复用。

## 安全和公开发布注意事项

公开仓库前必须单独审计：

- `.env`、设备密码、测试凭据和签名材料；
- Tailscale 地址、内部 IP、ADB 地址和设备路径；
- Buildroot、Rockchip SDK、内核和第三方组件的许可证；
- 生成产物、交叉工具链、`target/`、`output/` 和本地部署记录。

本仓库的产品代码与 vendor SDK 不是同一个发布单元。vendor source 的公开范围和许可证必须以各自组件仓库为准。
