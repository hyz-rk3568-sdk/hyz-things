# hyz-things

`hyz-things` 是一个面向 ATK RK3568 的个人软路由与家庭设备项目。它把设备网络、管理门户、Tailscale、Mihomo 代理和本地摄像头拆分为三个独立的 Linux 服务，并通过类型化的 root-only Unix socket 协议协作。

项目不是通用 OpenWrt 后台，也不提供运行时插件 ABI 或任意 Linux 网络配置器。网络、进程、防火墙和固件操作都由产品代码中的固定用例和受限 adapter 驱动。

> 当前项目处于持续的 RK3568 板端 bring-up 和验证阶段。源码结构、主机测试和部分板端能力已经完成；完整双上游切换、代理故障注入和长期稳定性验证仍在进行。

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
- Mihomo 显式代理、LAN TUN、Direct fallback 和恢复；
- Tailscale 生命周期、peer 状态和 runtime-owned 防火墙规则；
- recovery-free OTA、shutdown、DHCP hook 和状态聚合；
- root-only `/run/hyz-router/control.sock` 和管理面就绪标记 `/run/hyz-router/ready`。

`hyz-router` 只有在管理 LAN、AP、DNS 等资源经过严格 reconcile 后才写入 ready 标记。WAN 或代理不可用时，设备可以继续保持 management-only 管理路径。

### `hyz-things`

管理门户和浏览器侧 driving adapter，负责：

- HTTPS 管理页面：固定 LAN 地址 `https://192.168.8.1:8080`；
- 精确绑定的 Tailscale IPv4 管理 listener；
- Argon2id 管理员凭据、首次改密、会话和 CSRF；
- 嵌入式 Yew/WASM SPA 与 Axum API；
- router、WAN、代理、Tailscale、系统和应用状态聚合；
- 摄像头状态、会话和受限 SDP 信令；
- 通过注册表展示 `hyz-things`/`hyz-camera` 应用的热推送状态。

门户会等待 `/run/hyz-router/ready` 后才绑定管理地址。它不直接执行网络命令，而是通过 `hyz-contract` 客户端访问 router control socket，通过 `/run/hyz-camera/control.sock` 访问 camera。

### `hyz-camera`

独立媒体进程，负责：

- V4L2 摄像头采集；
- GStreamer 与 Rockchip MPP H.264 编码；
- 固定分辨率、码率和旋转预设；
- WebRTC/str0m 会话；
- 固定 `40000-40015/udp` 媒体端口池；
- 音频采集、RNNoise/VAD、Opus 编码和对讲回放。

摄像头不执行 `ip`、`iptables` 或其他网络配置命令。浏览器和摄像头之间的媒体流不经过 Axum 或 router 转发。

## 运行时边界

```text
浏览器
   │ HTTPS / typed HTTP API
   ▼
hyz-things ── versioned JSON frame ──> hyz-router
     │                                  │
     └──── versioned camera frame ────> hyz-camera

hyz-router 是网络和防火墙的唯一 authority；
hyz-things 是管理门户；hyz-camera 是媒体进程。
```

三个进程各自拥有独立的 composition root 和板端 ELF。共享协议位于 `apps/rust/contract`，服务端接受当前和前一协议版本，客户端要求精确匹配。

## 当前实现状态

已完成或已进入主机验证的主要内容：

- 三进程拆分：无头 router、things 门户和 camera 媒体进程；
- router ready 门控和 management-only 降级边界；
- typed Ethernet/Wi-Fi 双上游模型、有线优先 route metric 和 uplink ownership 隔离；
- Tailscale 本机/peer 状态、online/offline、active、direct/relay、last seen 和收发流量；
- UDP `41641` 防火墙规则同时覆盖 `eth0` 和 `wlan0`；
- Mihomo 显式代理 CONNECT 探测，连续 3 次确认失败后运行时回退 Direct；
- Direct 冷却 30 秒后自动恢复 `MihomoExplicit`，不修改用户持久化开关；
- things/camera 热推送只重启对应应用，router 保持运行；
- router 开发热替换规则：USB ADB 使用 `stop → 原子替换 → start`，网络 ADB 使用 `原子替换 → reboot`；
- 摄像头 WebRTC、固定媒体端口池和匿名短时 viewer 会话边界。

仍需要板端或更长时间验证的内容包括：

- Ethernet、`eth1` 和双上游完整切换矩阵；
- SR-09 反复切换和长时间稳定性；
- 板端显式代理不可用 → Direct → `MihomoExplicit` 恢复故障注入；
- DNS 接管、所有代理节点失效和 Mihomo live-hang 回退；
- PPPoE、DNS 中心和黑匣子等后续产品范围。

详细验收状态见 [`docs/soft-router-user-stories.md`](docs/soft-router-user-stories.md)。

## 开发环境

当前产品构建面向 Linux x86-64 主机和 ATK RK3568 目标板。

基本依赖：

- Git、curl、Python 3、GNU Make；
- Rustup 和 Rust `1.85` 或更新版本；
- Node.js `20` 或更新版本、npm；
- Buildroot 所需的常规 Linux 主机工具；
- 仓库本地 Trunk：`.tools/trunk/bin/trunk`；
- Android `repo` 工具会在首次使用 SDK 目标时自动下载到 `.tools/repo`。

Rockchip vendor source、Buildroot 和内核位于 `sdk/`，由 Android `repo` manifest 固定具体组件版本；产品应用位于 `apps/`。

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
make things-e2e      # 运行 loopback Axum/Playwright 浏览器测试
make router-app      # 构建 hyz-router AArch64 ELF
make things-app      # 构建 hyz-things AArch64 ELF
make camera-app      # 构建 hyz-camera AArch64 ELF
make apps            # 构建三个产品 ELF
make overlay         # 将三个 ELF 和产品元数据放入 rootfs overlay
```

`make check` 不会构建 Buildroot 固件；完整 firmware、kernel、rootfs 和 loader 构建由 `make upgrade` 触发。

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

部署工具会先检查 wire protocol 兼容性和 ELF 校验和，只停止并重启对应的 `S83hyz-things` 或 `S82hyz-camera`，不会重启 router。回滚使用：

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
apps/rust/things/    HTTPS 管理门户、Yew SPA 和部署工具
apps/rust/camera/    V4L2/GStreamer/MPP/WebRTC 媒体进程
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

- [`apps/rust/things/README.md`](apps/rust/things/README.md)：门户进程、HTTP 安全边界和前端测试；
- [`apps/rust/router/README.md`](apps/rust/router/README.md)：router 用例、CLI、持久化配置和 OTA；
- [`docs/architecture.md`](docs/architecture.md)：完整架构和启动时序；
- [`docs/soft-router-user-stories.md`](docs/soft-router-user-stories.md)：当前产品状态和未完成验收；
- [`docs/router-app-debug.md`](docs/router-app-debug.md)：router 开发热替换；
- [`docs/network-adb.md`](docs/network-adb.md)：物理 LAN/Tailscale 网络 ADB；
- [`docs/ota-deployment.md`](docs/ota-deployment.md)：OTA 部署和回滚边界。

## 安全和公开发布注意事项

公开仓库前必须单独审计：

- `.env`、设备密码、测试凭据和签名材料；
- Tailscale 地址、内部 IP、ADB 地址和设备路径；
- Buildroot、Rockchip SDK、内核和第三方组件的许可证；
- 生成产物、交叉工具链、`target/`、`output/` 和本地部署记录。

本仓库的产品代码与 vendor SDK 不是同一个发布单元。vendor source 的公开范围和许可证必须以各自组件仓库为准。
