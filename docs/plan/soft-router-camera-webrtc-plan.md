# RK3568 管理页面 WebRTC 摄像头直播实现计划

## 状态

**第一版已实现、构建并安装多轮 recovery-free OTA；时间戳水印（左上角、固定 20px）、编码前 `videoflip` 画面旋转、4K 直播稳定性与多人观看（同一编码流扇出，最多 4 个并发 viewer）均已在真实 RK3568 上验收。**

实现保持本文定义的独立 `hyz-camera`、HTTP SDP 信令、固定 LAN/Tailscale candidate、V4L2 + GStreamer + Rockchip MPP H.264、`str0m` 和多观看者边界（`MAX_VIEWERS=4`）。真实 Chromium 已分别通过 LAN 与 Tailscale origin，在桌面和移动端完成播放、H.264 解码、canvas 可见像素、全屏、停止与 session 清理验收；同一账号两个页面可同时观看同一路直播（`active_sessions=2`），关闭其一不影响另一个。最终 OTA 中的插件、Camera 和 Router 与构建产物逐字节一致；板端采集码流经 `ffprobe` 确认为 full-range `color_range=pc`。camera 纯测试（SDP 校验、控制协议 v2、水印与旋转 domain、FrameHub 扇出与多会话生命周期）已交叉编译并在板端全部通过。4K 预设间歇 HTTP 503 已定位为 4K 冷启动首帧超过 3 秒管线启动截止时间（实测 4.3-4.5s），截止时间上调至 10s 后 4K 稳定出流。

本文继续记录第一版管理页面摄像头直播的产品边界、进程架构、信令协议、WebRTC 媒体路径、Tailscale 集成、安全约束、测试顺序和验收矩阵。真实设备测试方法见 [`camera-hardware-e2e.md`](../camera-hardware-e2e.md)。

计划日期：2026-08-14。
完成日期：2026-08-14。

### 实施与验收摘要

- 独立程序：`/usr/bin/hyz-camera`；
- 固定媒体 profile：采集固定使用 RKISP 原生 `3840×2160` 全幅 NV12，输出分辨率与码率由固定枚举的 16:9 预设选择（默认 `720p · 2.5 Mbps`，可选 `1080p · 5 Mbps`、`1440p · 10 Mbps`、`4K · 20 Mbps`，均 `@ 30 FPS`、H.264 Baseline），非 4K 预设由 `videoscale` 缩放；
- 时间戳水印：`clockoverlay` 烧入日期+时间（`%Y-%m-%d %H:%M:%S`）、左上角、黑底，固定 `20px` 字号（所有预设一致）；依赖 gst1-plugins-base pango 插件与 DejaVu Sans 字体；
- 画面旋转：`videoflip` 编码前应用（0/90/180/270°，`BR2_PACKAGE_GST1_PLUGINS_GOOD_PLUGIN_VIDEOFILTER`），管理页「旋转画面」按 0 → 270 → 180 → 90 → 0 循环并自动重启直播，浏览器不再做 CSS 旋转；
- 固定媒体端口：UDP `40000-40015`；
- Router Playwright mock/harness 回归：10/10 通过；
- 真实设备 MJS/Playwright：LAN 与 Tailscale、`1280×800` 与 `360×800` 四组路径全部通过；
- 每组真实测试均确认 `RTCPeerConnection.connectionState=connected`、video inbound RTP bytes/packets/frames decoded 大于零、解码尺寸 1920×1080、canvas 存在非黑像素与可见细节、全屏覆盖 viewport、无横向溢出，停止后 pipeline/session 回零；
- GStreamer caps 固定 full-range BT.709 `1:3:5:1`；Rockchip MPP 插件将该范围映射到 `MPP_FRAME_RANGE_JPEG`、写入 `prep:range` 并强制提交配置，采集码流经 `ffprobe` 确认为 `color_range=pc`；
- recovery-free OTA 已审计为只包含 bootloader、U-Boot、misc、boot、rootfs 和 oem，不含 recovery/userdata；
- OTA 后 `/usr/bin/hyz-router`、`/usr/bin/hyz-camera` 与打包 rootfs 中对应 ELF 逐字节一致；
- 原管理员 credential 和 recovery 分区 SHA-256 在测试及 OTA 后恢复/保持一致。

本次最终 recovery-free OTA 审计值：

| 产物 | SHA-256 |
| --- | --- |
| `output/upgrade.fw`（460,284,490 bytes） | `2375bb757bede1ef710d908eaee6b54e6de1f49fade12db1e90e72c0a3be3010` |
| 打包 `boot.img` | `ecf7562037d8788b3601b796f7064a49e5075d6e8b4ccc4b9bd29ac43d075a08` |
| 打包 `rootfs.img` | `a9e7a5f11e6913085f14c57525d873c8adaa51d6e5bc8d66854bd3c28ecea729` |
| 打包 `oem.img` | `bc86066f63fc5b115b6bed66c85f22b3dcc97f498e6fe83d0f77e117e447fbf8` |
| OTA 中及板端 `/usr/bin/hyz-router` | `a9d966b35c0932651605d014890db2b9d3d24d7aef56148bf039724c407428d7` |
| OTA 中及板端 `/usr/bin/hyz-camera` | `8b819cb7fd287b695ae5d49f139b869d354cae392d1dcebedf3545a0a27f6ef2` |
| OTA 中及板端 `/usr/lib/gstreamer-1.0/libgstpango.so` | `c51115e14203ed515e33265ef27bc0405bfbb6fe4ba05a5bfee65e1163bf0451` |
| OTA 中及板端 `/usr/lib/gstreamer-1.0/libgstvideofilter.so` | `75a6f0097865e34ee1c33cc7ccc831e6723cf3770826fcea4d067124eb038b8b` |
| OTA 中及板端 `/usr/lib/gstreamer-1.0/libgstrockchipmpp.so` | `f6202995874bd3bde81a59fcb61ef0dde9f5a13e7b9176018e18dd82c18d3aad` |
| OTA 中及板端 `/usr/bin/rkaiq_3A_server` | `f13f6aaa404062d9e841f2a9e8b48b98d3c5c416a832dcee846ef54f3ea81441` |
| OTA 中及板端 `/etc/init.d/S81hyz-router` | `97db4519983d8212d4585c875c0abad4993271b7a3f95a32282dfc2c46a966a3` |
| OTA 中及板端 `/etc/init.d/S82hyz-camera` | `ff6f11f836ec9addd86964f4d64a196c567893918aff5f9e750fe07818ebc247` |
| 板端 recovery 分区（OTA 前后相同） | `9778353401cf31b6bdca54fd1e5dd59a7a0983d9b32488eb89e3f974f036e363` |

最终 OTA 经双层解包确认成员只有 bootloader、U-Boot、misc、boot、rootfs 和 oem，不含 recovery/userdata；打包关键文件与构建产物逐字节一致，且不含临时 `v4l2-ctl` 或 libv4l 诊断产物。安装后 boot ID 已变化，Router、Proxy、Tailscale 和 System 全部严格就绪，recovery 分区与 `/userdata/hyz-router` 聚合摘要均保持不变。

### 轮次 5 OTA（多人观看）审计值

| 产物 | SHA-256 |
| --- | --- |
| `output/upgrade.fw`（460,284,490 bytes） | `5eda8d1079d5435b6e51d2941b08700808c5fbbc99139c0acda0ec5fa1b654e1` |
| 打包 `boot.img` | `06276f751309d07a484925102cc0ddb5fb31afbabe0882c7881f3e7b2c81e59a` |
| 打包 `rootfs.img` | `9a32d7a91af473fbb654b36eddbc9775a67f988d3efe06256c3ba8b3ddd0fbd6` |
| 打包 `oem.img` | `ac2fc8b4e426d7474afa318af3ebdca00224111e20d0a297572bff419051fd32` |
| OTA 中及板端 `/usr/bin/hyz-router` | `61ca68ef97c310f8f685a98785bd501fc2970eacaa14f661618dfc1b85e7ad10` |
| OTA 中及板端 `/usr/bin/hyz-camera` | `a082bcaf9e7decb3e2dad85db8355eeefe04dee07ef3b0ba321bf04d440c2797` |
| 板端 recovery 分区（OTA 前后相同） | `9778353401cf31b6bdca54fd1e5dd59a7a0983d9b32488eb89e3f974f036e363` |

本轮 Buildroot 与媒体插件（`libgstpango.so`、`libgstvideofilter.so`、`libgstrockchipmpp.so`、DejaVu 字体等）未变化，沿用轮次 4 哈希。控制协议保持 v2：新增语义为超过 `MAX_VIEWERS=4` 返回 `ResourceExhausted`（HTTP 503 `camera_resource_exhausted`），Router 与 Camera 在同一 OTA 中升级。板端双页面并发观看（同一管理员 cookie、同一 owner）验收通过：`active_sessions=2`、两路均真实解码 720p、关闭其一不影响另一个、全部关闭后 pipeline/session 归零。

## 目标

第一版允许已认证管理员在现有管理页面点击“播放”，由浏览器与设备建立一条仅视频的 WebRTC 会话，观看 RK3568 本机摄像头实时画面。

```text
浏览器管理页面
    │
    │ 同源 HTTP：创建/关闭会话，交换 SDP
    ▼
hyz-router
    │
    │ root-only typed Unix socket
    ▼
hyz-camera
    ├── GStreamer：V4L2 → clockoverlay 时间戳水印 → Rockchip MPP H.264 → appsink
    └── str0m：ICE → DTLS → SRTP → RTP/RTCP
             │
             └──────── WebRTC UDP 视频 ────────► 浏览器 <video>
```

第一版同时支持两种管理入口（均为 HTTPS，自签证书见 `soft-router-camera-audio-intercom-plan.md` 的 HTTPS 一节）：

- 固定 LAN listener：`https://192.168.8.1:<port>`；
- 已严格确认的 Tailscale IPv4 exact listener：`https://100.x.y.z:<port>`。

用户通过哪个管理入口创建会话，服务端就只向该会话公布对应入口的 WebRTC host candidate。视频媒体不经过 Axum HTTP body、WebSocket 或 `hyz-router` 数据转发。

## 第一版固定范围

### 包含

- 一个独立板端 Rust 进程 `/usr/bin/hyz-camera`；
- 一个固定摄像头设备，由产品配置确定，不接受浏览器路径；
- V4L2 视频采集；
- GStreamer pipeline；
- 画面旋转：`videoflip` 在编码前应用（0/90/180/270°），由管理页「旋转画面」驱动，浏览器端不做 CSS 旋转；
- `clockoverlay` 时间戳水印：日期+时间、左上角、黑底，字号随旋转后显示高度缩放，烧入编码码流；
- Rockchip MPP H.264 硬件编码；
- H.264 Baseline、Annex-B、access-unit 对齐；
- `str0m` WebRTC 会话；
- 浏览器创建 recvonly SDP Offer，设备返回 SDP Answer；
- 第一版非 trickle ICE，通过完整 SDP 一次性交换 candidate；
- LAN 和 Tailscale RouterOnly 入口；
- 单路编码器；
- 同一编码流扇出到最多 4 个并发 viewer（每个 viewer 独立 UDP 端口、DTLS/SRTP、str0m 会话与帧队列）；
- 管理员认证、强制改密、exact Origin、CSRF 和小尺寸 typed API；
- 固定且有界的 UDP 媒体端口池；
- 页面播放、停止、错误和不可用状态；
- camera 进程、pipeline、浏览器断开和网络变化后的有界清理。

### 不包含

- 麦克风或 Opus 音频；
- 浏览器向设备回传音频；
- 录像、回放、截图、事件剪辑或云上传；
- 云信令服务；
- 普通公网直接访问；
- STUN/TURN 服务；
- 任意摄像头路径、分辨率、码率、pipeline 或 UDP 端口配置页面；
- DataChannel；
- Matter Camera cluster 集成；
- 动态多档编码或 simulcast；
- H.265；
- 未登录匿名观看；
- 从 WAN listener 暴露 HTTP 或媒体端口；
- 由 `hyz-camera` 自行执行 `iptables`、`ip` 或其他网络配置命令。

## 产品决策

### 独立媒体进程

新增独立应用：

```text
apps/rust/camera/
```

最终板端程序：

```text
/usr/bin/hyz-camera
```

`hyz-camera` 是独立媒体服务，不并入 `hyz-router` 的进程，也不作为 router outbound adapter 加载到同一地址空间。这样可以确保：

- V4L2、GStreamer、MPP 或 WebRTC 故障不终止路由和管理控制面；
- camera 重启不重建管理 LAN、AP、DHCP、NAT、Mihomo 或 Tailscale；
- router 重启可以保守关闭旧 camera session，而不是继承未知媒体状态；
- 摄像头设备、编码器、UDP socket 和 session ownership 有单独生命周期；
- GStreamer/GLib 线程模型不进入 `hyz-router` 核心；
- camera 依赖不会进入 router 的 WASM 前端构建目标。

`apps/rust/router` 仍遵守现有六边形架构和唯一 native composition root 规则。camera 应用自身采用同样的向内依赖原则，但它是另一个产品进程和 Cargo 包。

### HTTP 只承载信令

现有 `hyz-router` Axum server 承载：

- camera 状态读取；
- 创建 WebRTC session；
- 关闭 WebRTC session；
- SDP Offer/Answer 交换。

视频媒体不经过 HTTP server。会话建立后：

```text
hyz-camera UDP socket ←→ browser RTCPeerConnection
```

### Tailscale RouterOnly 即可观看

摄像头是路由器本机服务，因此远程观看只要求 Tailscale `RouterOnly` ready，不要求 `LanSubnetAccess`。

```text
远程浏览器 → Tailscale → 路由器本机 hyz-router/hyz-camera
```

`LanSubnetAccess` 是否启用不参与 camera readiness。camera session 创建也不会停止、重启或注销 Tailscale，因此可以从 Tailscale exact listener 发起。

## 参考实现与复用边界

### RK3568 GStreamer 参考

参考：

```text
/home/hyz/Code/connectedhomeip-1.5.0.0/examples/camera-app/rk3568-buildroot-linux/
```

可复用的设计信息：

- `v4l2src` 采集；
- NV12 输入；
- `mpph264enc` 硬件编码；
- `h264parse config-interval=-1`；
- `appsink` 回调；
- pipeline 创建、启动、停止和 bus error 处理；
- 摄像头能力与 V4L2 control 的探测方法。

不得直接复制以下含糊路径：

```text
mpph264enc → h264parse → rtph264pay → appsink
                                  ↓
                       WebRTC H264 packetizer
```

如果 appsink 位于 `rtph264pay` 后面，appsink 获得的是 RTP packet；再次交给 H.264 WebRTC packetizer 会造成接口语义错误。第一版必须使用 frame-level 边界：

```text
mpph264enc → h264parse → H.264 Annex-B AU → appsink → str0m Writer::write
```

因此第一版 pipeline 不包含 `rtph264pay`。

### Rust WebRTC 参考

参考：

```text
/home/hyz/Code/matter_bridge_device_protocol/rs-matter/examples/src/bin/webrtc_camera.rs
/home/hyz/Code/matter_bridge_device_protocol/rs-matter/examples/Cargo.toml
```

可复用的设计信息：

- `str0m::Rtc` 每会话状态；
- SDP Offer 解析与 Answer 生成；
- local host candidate；
- UDP receive/transmit 驱动；
- `poll_output`/`handle_input` 调度约束；
- ICE、DTLS 和 RTP/RTCP session loop；
- H.264 Annex-B access unit 输入；
- 90 kHz 视频时钟；
- session shutdown 和超时清理。

不复用：

- Matter cluster 和 TLV 信令；
- Matter session ID；
- 文件循环媒体源；
- 固定 FPS 累加作为真实摄像头时间戳来源；
- camera、zone 或 Matter commissioning 逻辑。

## 进程架构

### `hyz-router` 职责

- 继续提供 LAN 和 Tailscale exact HTTP listener；
- 管理员认证、session、Origin 和 CSRF；
- 暴露固定 camera HTTP facade；
- 根据请求实际进入的 listener 派生 `Lan` 或 `Tailscale` access scope；
- 通过 root-only Unix socket 调用 camera application；
- 管理 camera 所需固定防火墙 hook 和规则；
- 聚合 camera 状态，但 camera 状态失败不能拖垮 router/system/status 其他部分；
- 在 camera 未严格 ready 时拒绝创建会话；
- 不加载 GStreamer，不打开 V4L2，不处理视频帧，不驱动 `str0m`。

### `hyz-camera` 职责

- 独占并验证固定 V4L2 设备；
- 构造固定 GStreamer pipeline；
- 驱动 MPP H.264 编码；
- 从 appsink 提取有界编码帧；
- 创建和关闭 `str0m` session；
- 绑定固定 UDP 端口池中的端口；
- 根据服务端提供的 typed access scope 公布一个精确 host candidate；
- 维护 session 和 pipeline 状态；
- 处理 pipeline ERROR/EOS、ICE failure、DTLS failure、浏览器断开和超时；
- 通过 Unix socket 返回窄化状态和 SDP Answer；
- 不修改 firewall、route、interface、Tailscale 或 router 状态；
- 不提供 LAN/Tailscale HTTP listener。

### 运行目录

建议：

```text
/run/hyz-camera/
├── control.sock
├── daemon.owner
├── camera.owner
└── camera.log             # 可选，有界、root-only
```

持久目录第一版不需要。分辨率、码率和设备路径使用编译期产品常量或 rootfs 中无秘密、只读、严格版本化的产品配置；不得由 Web 写入 `/userdata`。

权限：

- `/run/hyz-camera`：`0700`；
- `control.sock`：`0600`；
- owner 文件：root-owned regular file，禁止 symlink；
- socket server 使用 Linux peer credentials，只允许预期 UID；
- 第一版可由 root 运行，后续在设备和 MPP 权限验证后再评估专用用户和最小 capabilities。

## camera 包内架构

建议目录：

```text
apps/rust/camera/
├── Cargo.toml
├── Cargo.lock
├── src/
│   ├── domain/
│   │   ├── mod.rs
│   │   ├── session.rs
│   │   ├── stream.rs
│   │   └── status.rs
│   ├── application/
│   │   ├── mod.rs
│   │   ├── ports.rs
│   │   ├── lifecycle.rs
│   │   └── session.rs
│   ├── adapters/
│   │   ├── mod.rs
│   │   ├── inbound/
│   │   │   └── unix_control.rs
│   │   └── outbound/
│   │       ├── gstreamer.rs
│   │       ├── v4l2.rs
│   │       └── webrtc.rs
│   ├── lib.rs
│   └── main.rs
└── tests/
    ├── lifecycle.rs
    ├── session.rs
    └── unix_control.rs
```

依赖方向：

- `domain`：纯值对象、状态和不变量，不依赖 GStreamer、V4L2、socket 路径或 Linux 命令；
- `application`：会话与 pipeline 用例，只依赖 domain 和 application ports；
- inbound Unix control：解析固定版本协议并调用 application；
- outbound GStreamer/WebRTC：实现 application ports；
- `main.rs`：camera 唯一 production composition root。

## Domain 模型

第一版至少包含：

```rust
enum CameraAccessScope {
    Lan { address: Ipv4Addr },
    Tailscale { address: Ipv4Addr },
}

enum CameraPipelineState {
    Stopped,
    Starting,
    Streaming,
    Stopping,
    Failed,
    Unknown,
}

enum CameraSessionState {
    Negotiating,
    Connecting,
    Connected,
    Closing,
    Closed,
    Failed,
}

struct CameraStreamProfile {
    width: u16,
    height: u16,
    fps: u8,
    bitrate_bps: u32,
    codec: CameraVideoCodec,
}

enum CameraVideoCodec {
    H264Baseline,
}
```

不变量：

- 最多 4 个并发 active/negotiating session（`MAX_VIEWERS=4`，同一编码流扇出）；
- session 必须绑定一个服务端派生的 access scope；
- `Lan` address 必须严格等于固定管理 LAN 地址；
- `Tailscale` address 必须是 router 严格观测的单个 Tailscale CGNAT IPv4；
- session 不接受调用方指定 UDP 端口；
- pipeline 只有在 session 存在时才需要运行；
- pipeline `Unknown`、设备 foreign/busy 或编码器状态不明时不允许创建 session；
- session close 必须幂等；
- pipeline 和 UDP 资源只清理 runtime-owned 对象；
- camera failure 不改变 router 或 Tailscale desired state。

## Application ports

建议拆分：

### `CameraMediaPort`

负责固定媒体 pipeline：

- probe fixed camera device；
- start fixed stream profile；
- stop exact owned pipeline；
- read encoded frame；
- request keyframe；
- observe pipeline/device ownership 和状态。

### `WebRtcSessionPort`

负责 WebRTC session：

- validate bounded SDP Offer；
- create Answer with exact host candidate；
- drive ICE/DTLS/SRTP；
- write encoded H.264 access unit；
- observe connection state；
- close exact session；
- report RTCP keyframe request。

如果 `str0m::Rtc` 与 socket 驱动无法形成适合 trait 的对象边界，可以让 application 定义 session driver factory/handle port，而不是把 `str0m` 类型泄漏到 application。

### `ClockPort`

用于：

- negotiation deadline；
- ICE/DTLS deadline；
- idle session timeout；
- last-viewer pipeline stop 计数（引用计数 terminator，无固定 grace 定时器）；
- 测试中的确定性时间推进。

## GStreamer 媒体路径

### 固定 profile、旋转与时间戳水印

最终值固化在 `apps/rust/camera/src/domain/`，不是 Web 输入。采集固定使用 RKISP 原生 `3840×2160` 全幅 NV12，输出分辨率与码率由固定枚举的 16:9 预设选择（默认 `Hd720p25m`：`1280×720`、`2.5 Mbps`；可选 `Fhd1080p5m`、`Qhd1440p10m`、`Uhd4k20m`，均 30 FPS、H.264 Baseline）。非 4K 预设由 GStreamer `videoscale` 从 4K 全幅缩放。

画面旋转是服务端媒体管线属性：`videoflip` 在缩放之后、水印之前应用，90/270 时输出宽高互换，水印始终叠加在最终方向画面的左上角。管理页「旋转画面」按 0 → 270 → 180 → 90 → 0 循环，切换会自动停止并重新打开直播。

时间戳水印固定值：

| 项目 | 固定值 |
| --- | --- |
| 时间格式 | `%Y-%m-%d %H:%M:%S` |
| 位置 | 左上角，`xpad=ypad=8` |
| 字体 | DejaVu Sans，固定 `20px`（显式像素，所有预设一致） |
| 背景 | `shaded-background=true`（黑底保证亮场景可读） |
| 时间来源 | pipeline clock，v4l2src 直播时为系统实时钟 |
| 音频 | 无 |
| 最大编码器数量 | 1 |
| 最大并发 viewer | 4（`MAX_VIEWERS`，同一编码流扇出） |

`clockoverlay` 属于 gst1-plugins-base 的 pango 插件（`libgstpango.so`，`BR2_PACKAGE_GST1_PLUGINS_BASE_PLUGIN_PANGO`），文本渲染依赖 pango/cairo/fontconfig/freetype/harfbuzz，并安装 DejaVu Sans 字体（`BR2_PACKAGE_DEJAVU` + `BR2_PACKAGE_DEJAVU_SANS`）。`videoflip` 属于 gst-plugins-good 的 videofilter 插件（`BR2_PACKAGE_GST1_PLUGINS_GOOD_PLUGIN_VIDEOFILTER`）。1.22.2 的 `GstBaseTextOverlay` 原生支持 NV12，不会自动插入 videoconvert 造成额外 4K 全帧转换；混合只写文字包围盒区域，时间文本每秒才更新一次，CPU 开销可控。

### pipeline

目标语义：

```text
v4l2src fixed-device
  → video/x-raw,format=NV12,width=3840,height=2160,colorimetry=1:3:5:1,framerate=30/1
  → queue, bounded and downstream-leaky
  → videoscale → video/x-raw,format=NV12,width=<输出宽>,height=<输出高>  （非 4K 预设）
  → videoflip method=<none|clockwise|rotate-180|counterclockwise>  （0/90/180/270°）
  → clockoverlay time-format=%Y-%m-%d %H:%M:%S font-desc="DejaVu Sans 20px"
                halignment=left valignment=top xpad=8 ypad=8 shaded-background=true
  → mpph264enc profile=baseline level=<4|5.1> gop=30 bps=<预设码率>
  → h264parse config-interval=-1
  → video/x-h264,stream-format=byte-stream,alignment=au
  → appsink max-buffers=2 drop=true sync=false
```

H.264 level 按帧大小选择：Level 4.0 的 MaxFS 为 2,097,152 像素，720p/1080p 使用 `4`，1440p/4K 使用 `5.1`（否则 SPS level 与帧大小不符）。管线启动首帧截止时间为 10 秒（4K 冷启动含 ISP 初始化与 MPP 编码器启动，3 秒过于紧张，曾导致间歇 `media_pipeline_failed` → HTTP 503）。

实现必须使用 element API 或固定内部 pipeline construction；不得接受 caller-provided `gst_parse_launch` 字符串。若使用 `gst_parse_launch` 进行早期板端诊断，只能是开发命令，不进入生产 Web/API 路径。

### appsink 规则

- callback 不执行网络 I/O；
- callback 不等待 peer；
- 每个 sample 验证 caps 仍为预期 H.264 byte-stream/AU；
- 使用 `GST_BUFFER_PTS` 转换到 90 kHz media time；
- PTS 缺失、倒退或异常跳变时返回 typed pipeline error，不猜测长期 ready；
- 从 buffer flags 判断 delta/key frame；
- 映射后复制到有界 `Arc<[u8]>`；后续可评估 zero-copy，但不作为第一版前置条件；
- frame channel 有界，满时丢弃旧 delta frame，不能阻塞采集线程；
- 新 session 连接后等待下一个 IDR；
- 收到 RTCP PLI/FIR 时通过 GStreamer force-key-unit event 请求 IDR；
- `h264parse config-interval=-1` 确保 IDR 携带 SPS/PPS；
- pipeline ERROR/EOS 立即终止 session 并清理 owned pipeline。

## WebRTC 会话设计

### 协商方向

第一版固定为浏览器 offerer：

1. 浏览器创建 `RTCPeerConnection`；
2. 添加一个 `video`、`recvonly` transceiver；
3. 浏览器创建 SDP Offer；
4. 等待 ICE gathering complete；
5. 浏览器向 router 提交完整 Offer；
6. router 通过 Unix socket 请求 camera 创建 session；
7. camera 创建 `str0m::Rtc`、绑定固定 UDP 端口、添加 exact host candidate 并接受 Offer；
8. camera 返回完整 SDP Answer；
9. 浏览器设置 remote description；
10. ICE/DTLS 成功后 camera 发送 H.264；
11. 浏览器停止、超时或连接失败时关闭 session。

第一版不实现 trickle ICE，避免 WebSocket、SSE 或额外 candidate mutation。若实测建立时间不可接受，再以新的 typed API 增加 bounded trickle ICE。

### SDP allowlist

camera 必须解析 SDP，不得只把字符串交给底层后默认接受所有内容。至少要求：

- UTF-8；
- 最大 32 KiB；
- 一个 video m-line；
- video direction 与 recvonly/sendonly 关系符合设备发送视频；
- H.264 codec；
- 允许的 payload type 数量有界；
- 不允许 audio；
- 不允许 application/DataChannel；
- ICE username/password 长度有界；
- fingerprint 算法在 allowlist；
- candidate 数量和单项长度有界；
- 不接受无法解析、重复或歧义的关键字段；
- 不记录完整 SDP。

第一版根据实际 Chrome/Firefox SDP fixture 编写 narrow compatibility tests。字段扩展可以忽略，但影响 codec、direction、ICE、DTLS 或地址选择的未知状态必须拒绝，不能猜测。

### host candidate

`hyz-router` 根据实际 listener 派生：

```rust
enum CameraAccessScopeRequest {
    Lan,
    Tailscale,
}
```

通过 Unix socket 传递 scope 和已经严格验证的地址。`hyz-camera` 再次检查：

- LAN 必须是固定 `192.168.8.1`；
- Tailscale 必须位于 `100.64.0.0/10`；
- 不能是 unspecified、loopback、multicast 或 broadcast；
- 不能由浏览器 body 指定；
- SDP Answer 只公布该地址和本 session 分配的固定池端口。

不公布 `wlan0`、普通 WAN、loopback、其他 interface 或全部本机地址。

### UDP 端口池

初始候选：

```text
40000-40015/udp
```

最终常量应放在 domain/product config 中，并由 static checks、camera 和 router firewall 共同引用，避免三处漂移。

最多 4 个并发 session，每个 session 从端口池独占一个端口；小型固定池以便：

- 旧 socket TIME_WAIT 不适用于 UDP，但短暂旧 session/重启状态可被精确隔离；
- 支持少量多 peer（16 个端口满足 `MAX_VIEWERS=4`）；
- 避免任意 ephemeral port 导致 firewall 无界开放。

端口分配必须：

- 仅从固定池选择；
- bind 成功后才生成 Answer；
- session close 时释放；
- bind 冲突时尝试池中下一端口；
- 全部冲突时返回 ResourceExhausted；
- 不终止或接管 foreign socket。

## Router-camera Unix 协议

协议使用长度有界、版本化 JSON frame；可复用 router control 的 framing 思路，但 camera 协议独立版本化。

请求示例：

```json
{
  "version": 1,
  "operation": {
    "create_session": {
      "scope": "lan",
      "address": "192.168.8.1",
      "offer_sdp": "v=0\r\n..."
    }
  }
}
```

操作 allowlist：

```text
status
create_session
close_session
shutdown
```

其中 `shutdown` 只允许 camera init/lifecycle authority 使用，不通过 Web 暴露。

明确禁止：

```text
execute
pipeline
command
set_device_path
set_address
set_port_range
set_encoder
set_property
set_timeout
```

协议要求：

- `deny_unknown_fields`；
- frame 最大值，例如 64 KiB；
- response 最大值，例如 64 KiB；
- 单连接 request deadline；
- peer credentials；
- 路径为固定常量；
- 不跟随 symlink；
- 未知 version/operation 返回 typed error；
- session ID 是随机、不可猜测、有界字符串；
- router 只能关闭自己创建且仍有效的 session；
- daemon restart 后旧 generation 的 session ID 必须失效。

## HTTP API

### 状态

```text
GET /api/v1/camera/status
```

要求管理员 session。第一版不向匿名 LAN 页面公开摄像头存在、active session 数量或错误细节。

响应示例：

```json
{
  "camera": {
    "available": true,
    "pipeline": "stopped",
    "active_sessions": 0,
    "profile": {
      "codec": "h264",
      "width": 1280,
      "height": 720,
      "fps": 30
    },
    "access": "lan"
  }
}
```

不得返回：

- `/dev/video*` 路径；
- UDP 端口；
- SDP；
- ICE credentials；
- DTLS private key；
- 完整 candidate；
- GStreamer pipeline 字符串；
- MPP/V4L2 原始错误文本；
- Tailscale peer 或 endpoint 信息。

### 创建 session

```text
POST /api/v1/control/camera/session/create
```

请求：

```json
{
  "offer_sdp": "v=0\r\n..."
}
```

响应：

```json
{
  "session_id": "opaque-random-id",
  "answer_sdp": "v=0\r\n...",
  "negotiation_timeout_seconds": 30
}
```

要求：

- 管理员 session；
- 已完成强制首次改密；
- exact Origin；
- CSRF；
- JSON body limit；
- `deny_unknown_fields`；
- 当前请求来自 LAN exact listener 或严格 ready 的 Tailscale exact listener；
- camera process、device、pipeline 和对应 firewall scope 可用；
- 多 session 上限（`MAX_VIEWERS=4`，超限返回 503 `camera_resource_exhausted`）；
- camera error 映射为窄化 HTTP error，不返回底层原文。

### 关闭 session

```text
POST /api/v1/control/camera/session/close
```

请求：

```json
{
  "session_id": "opaque-random-id"
}
```

要求与创建相同。关闭幂等：已自然结束的本 generation session 可以返回成功或明确 Gone，但不得关闭其他管理员、其他 router session 或旧 daemon generation 的 camera session。`hyz-router` 应在内存中把 camera session ID 绑定到创建它的管理员 session；logout、密码变化导致 session 撤销或管理员 session 到期时主动关闭对应 camera session。

### 错误映射

建议：

| 情况 | HTTP |
| --- | ---: |
| 未登录 | 401 |
| 未完成首次改密 | 403 |
| Origin/CSRF 失败 | 403 |
| camera 尚未 ready | 409 |
| 已有 active session | 409 |
| SDP 不支持 | 422 |
| body/SDP 超限 | 413 |
| UDP 端口耗尽 | 503 |
| camera control 不可达 | 503 |
| 未知 session | 404 |
| camera 内部故障 | 503，窄化错误类别 |

未知 `/api/*` 继续 JSON 404，不进入 SPA fallback。

## Web UI

### 页面位置

第一版在管理员登录后的“总览”加入摄像头卡片，避免新增复杂路由。卡片只有在 camera capability 存在时显示。

状态：

- 可播放；
- 正在连接；
- 正在播放；
- 已停止；
- 摄像头忙；
- 摄像头不可用；
- 网络路径变化，请重新连接；
- 浏览器不支持协商到的 H.264 profile。

### 用户流程

1. 管理员点击“播放”；
2. 按钮禁用并显示正在连接；
3. 浏览器创建 recvonly Offer；
4. 等待 ICE gathering complete，设置有界前端 deadline；
5. 调用同源 `POST /api/v1/control/camera/session/create`；
6. 设置 Answer；
7. `ontrack` 后设置 `<video autoplay playsinline muted>`；
8. 首帧或 connection state connected 后显示正在播放；
9. 点击“停止”关闭 peer 并调用 `POST /api/v1/control/camera/session/close`；
10. 页面组件卸载、tab 切换离开总览、logout 或 session 失效时关闭；
11. camera/ICE failure 时保留页面其他状态，显示可重试错误。

第一版不自动播放，不在页面加载时打开摄像头。必须由管理员显式点击。

### 浏览器资源和 CSP

扩展 Yew WASM 使用的 `web-sys` features，包括实际需要的：

- `RtcPeerConnection`；
- `RtcConfiguration`；
- `RtcSessionDescriptionInit`；
- `RtcRtpTransceiver`/direction；
- `RtcTrackEvent`；
- `MediaStream`；
- `HtmlVideoElement`；
- ICE gathering/connection state events。

不得加入外部 JavaScript CDN。现有 CSP 继续只允许同源 script/WASM。若浏览器 API 需要新的 Permissions Policy 或 CSP directive，必须通过 HTTP tests 和浏览器验证精确增加，不使用宽泛通配。

## Tailscale 集成

### 信令

复用现有 Tailscale exact HTTP listener 和 exact Origin：

```text
http://<strict-tailscale-ip>:<port>/api/v1/control/camera/session/create
```

前端始终使用相对 URL，因此同一 bundle 无需判断 LAN/Tailscale host。

### 媒体

Tailscale session 的 Answer 只公布：

```text
<strict-tailscale-ip>:<fixed-camera-udp-port>
```

底层 Tailscale 可以 direct、peer relay 或 DERP。camera readiness 不依赖具体连接类型；但 DERP/relay 下直播吞吐和延迟必须单独验收，不能由本地 ready 推断播放质量。

### 地址变化

如果严格观测的 Tailscale IPv4 变化：

1. 停止旧 Tailscale HTTP listener；
2. 标记绑定旧 address 的 camera sessions 失效；
3. 关闭对应 WebRTC session；
4. 绑定新 exact listener；
5. 新 session 使用新 address。

不得继续把旧 candidate 视为可恢复 session。

## 防火墙设计

`hyz-router` 继续作为唯一网络规则 authority。`hyz-camera` 只 bind socket，不执行 `iptables`。

建议新增 camera-owned INPUT chain，例如：

```text
HYZ_CAM_INPUT
```

规则语义：

### LAN

- 只允许 `-i br-lan`；
- destination 必须严格为 `192.168.8.1`；
- protocol UDP；
- destination port 为固定 camera 端口池；
- 其他接口到该端口池拒绝。

### Tailscale

- 只允许 `-i tailscale0`；
- destination 必须严格为当前 observed Tailscale IPv4；
- source 必须位于 `100.64.0.0/10`；
- protocol UDP；
- destination port 为固定 camera 端口池；
- Tailscale RouterOnly ready 即可安装；
- Tailscale disabled、NeedsLogin、Unknown 或地址未知时不得安装 Tailscale camera allow 规则。

### WAN

`wlan0`、`eth0`、`ppp0` 和其他非管理入口始终不能访问 camera UDP 池。

规则必须：

- fixed、typed、exact；
- comment-token ownership；
- exact hook 顺序；
- unknown/foreign 不接管；
- shutdown 只删除 runtime-owned 规则；
- camera rule failure 不应暴露 HTTP readiness 为可播放；
- 不改变 ordinary forwarding、NAT、Mihomo 或 Tailscale LAN forwarding。

第一版可以在 camera service ready 时常驻安装 LAN allow 规则，在 Tailscale RouterOnly ready 时常驻安装 Tailscale allow 规则。实际媒体仍需 DTLS/SRTP session 才能建立。若后续安全审计要求按 session 动态开关，再增加会话引用计数，不在第一版引入额外 firewall churn。

## 生命周期与 readiness

### 启动

建议顺序：

1. `hyz-router` 按现有流程完成严格 management LAN readiness；
2. `hyz-camera` 独立启动，获取 daemon ownership 并绑定 root-only control socket；
3. camera 静态 probe 固定 V4L2 节点和 GStreamer element capability，但不启动持续采集；
4. router camera status probe 确认 control socket、process identity 和 capability；
5. router 安装并复核 LAN camera firewall；
6. Tailscale RouterOnly ready 时安装并复核 Tailscale camera firewall；
7. 只有对应 HTTP listener、camera control、地址和 firewall 都严格 ready 时，该入口才允许 create-session。

camera 不应成为现有管理 HTTP 启动的关键路径。camera 失败只让 camera card degraded，不阻止路由管理页面可见。

### 首个 session

1. 验证 scope/address；
2. 分配 UDP 端口并 bind；
3. 启动或复用 exact owned pipeline；
4. 等待 pipeline 达到 PLAYING 并收到有效 caps/首个编码 access unit；
5. 创建 `str0m::Rtc`；
6. 接受 Offer 并生成 Answer；
7. 返回 session；
8. 有界等待 ICE/DTLS connected；
9. 等待 IDR 后开始发送；
10. 连接 deadline 到期则关闭 session，最后一个 session 时停止 pipeline。

需要在实现时验证第 3～6 步的顺序：如果 `str0m` Answer 可以先生成，pipeline 启动失败也必须在 HTTP deadline 内返回失败并清理；不能返回成功 Answer 后留下永久 Connecting。

### 最后一个 session 关闭

1. 停止向该 peer 投递帧并注销其订阅队列；
2. 关闭 `Rtc` 和 UDP socket；
3. 从 active session 列表删除；
4. 引用计数 terminator 递减：最后一个 viewer 线程退出时立即停止 pipeline（应用层在 session 列表清空时幂等兜底）；
5. 复核 V4L2/MPP/GStreamer owned 资源已释放。

### shutdown

`hyz-camera`：

1. 停止接受新 control request；
2. 关闭所有 session；
3. 停止 pipeline；
4. 释放固定摄像头设备；
5. 删除 owned socket/owner 状态；
6. 退出。

`hyz-router` shutdown 不应等待 camera 无限完成；使用有界 typed shutdown，超时后只清理 router-owned camera firewall 和 client handle，不杀死或接管身份未知的进程。init script 可以在精确 PID/start/exe/argv 身份确认后执行后续终止策略。

## 状态模型

Router `StatusSnapshot` 不需要把 camera 合并进路由核心 readiness。建议增加独立可选组件：

```rust
camera: Component<CameraStatus>
```

只返回：

- capability present；
- daemon/process ready；
- pipeline state；
- active session count；
- fixed profile public summary；
- LAN media path ready；
- Tailscale media path ready；
- typed error category；
- snapshot freshness。

错误类别示例：

```text
camera_not_found
camera_busy
media_pipeline_failed
encoder_unavailable
control_unavailable
firewall_not_ready
webrtc_negotiation_failed
webrtc_transport_failed
resource_exhausted
unknown
```

不得返回底层路径、命令、完整日志或协商内容。

## 测试策略

测试继续遵守“测试即文档”和 application seam 优先原则。普通 host tests 不打开真实 `/dev/video*`、不调用 MPP、不修改 firewall、不依赖真实浏览器网络。

### camera domain/application tests

使用 fake media/WebRTC/clock ports，覆盖：

- stopped → first session → pipeline start → Answer；
- session close → last-viewer → pipeline stop；
- 并发多 session、`MAX_VIEWERS` 上限；
- LAN/Tailscale scope address validation；
- unknown/busy/foreign device 不 ready；
- UDP port pool exhausted；
- pipeline start failure rollback；
- Answer generation failure rollback；
- ICE/DTLS timeout cleanup；
- browser disconnect cleanup；
- pipeline ERROR/EOS 终止 session；
- close 幂等；
- old daemon generation session ID 拒绝；
- keyframe request forwarding；
- bounded frame queue 丢弃策略；
- PTS 转换和倒退拒绝；
- shutdown action order。

### Unix control tests

覆盖：

- exact operations；
- protocol version；
- frame/body limits；
- `deny_unknown_fields`；
- peer credential 拒绝；
- SDP 不进入普通日志；
- typed error mapping；
- malformed JSON；
- session ID ownership；
- control socket fallback/cleanup。

### Router HTTP tests

通过 fake camera application/control seam，不构造真实 GStreamer adapter，覆盖：

- 精确 route/method；
- 匿名拒绝；
- 强制改密前拒绝；
- Origin/CSRF；
- body limit；
- SDP size limit；
- unknown fields；
- LAN listener 派生 LAN scope；
- Tailscale listener 派生 Tailscale scope；
- Tailscale self-stop 限制不错误应用到 camera create/close；
- camera error 的窄化 HTTP 映射；
- secret/SDP/candidate exclusion；
- security headers；
- SPA/API fallback；
- status partial failure 不拖垮其他组件。

### Router network tests

通过现有 application/network seam 和 fake ports，覆盖：

- LAN camera INPUT allow；
- Tailscale camera INPUT allow；
- WAN camera INPUT reject；
- Tailscale unknown/address missing 不安装；
- exact destination IP 和 UDP range；
- hook ordering；
- install/remove 幂等；
- foreign/duplicate rule 拒绝；
- shutdown 只清理 owned rule；
- camera rule 与 Mihomo/Tailscale ordinary rules 共存；
- camera firewall 失败只影响 camera path，不改变 ordinary router readiness。

### 浏览器 E2E

扩展现有 loopback Axum harness，并增加 fake signaling/media seam。至少覆盖：

- 未登录不显示或不能操作直播；
- 登录后摄像头卡片；
- 点击播放实际调用 `RTCPeerConnection` 流程；
- Offer 创建和 API 提交；
- Answer 设置；
- fake track 到 `<video>`；
- 点击停止；
- route/tab 离开清理；
- logout 清理；
- camera unavailable；
- SDP/API error；
- negotiation timeout；
- 重试；
- 页面其他 status polling 不重复创建 session；
- 桌面 viewport；
- 360px 移动 viewport；
- 无横向溢出；
- video aspect ratio 和空状态布局；
- LAN 和 Tailscale exact-origin variants。

如果 Playwright loopback 环境无法完成真实 H.264 WebRTC track，至少使用浏览器内受控 fake `RTCPeerConnection` seam 验证 UI 生命周期，同时另设 host integration test 完成真实 `str0m ↔ Chromium` 媒体协商。最终仍必须在真实浏览器和板端验证。

### 主机 WebRTC integration

新增不依赖摄像头的 host-only integration：

- 使用固定短 H.264 Annex-B fixture；
- Rust `str0m` device peer；
- Chromium browser peer；
- loopback HTTP signaling；
- 实际 ICE/DTLS/SRTP/H.264 首帧；
- stop/reconnect；
- fixture 有界、确定、不依赖网络下载。

### 板端测试

板端能力和真实媒体测试不能作为普通 host tests 运行。至少覆盖：

1. 识别固定摄像头和格式；
2. GStreamer element/caps；
3. NV12 → MPP H.264；
4. H.264 profile、level、SPS/PPS 和 AU 边界；
5. 1080p/30 FPS/4 Mbps；
6. LAN Chrome/Firefox/手机浏览器播放；
7. Tailscale direct 播放；
8. 可用时 Tailscale DERP/relay 播放并记录性能；
9. 连续 50 次播放/停止；
10. 浏览器直接关闭或切网；
11. camera 进程 `SIGKILL`；
12. router restart；
13. Tailscale disable/address change；
14. pipeline ERROR/EOS 注入；
15. 摄像头 busy/拔插；
16. 8 小时直播；
17. CPU、RSS、温度、MPP/V4L2 error、丢帧、码率、RTT、丢包；
18. camera 故障期间管理 HTTP、AP、STA、NAT、Mihomo 和 Tailscale 状态不回归；
19. WAN 无法访问 camera HTTP/UDP；
20. shutdown/restart 后无 socket、进程、设备或 firewall ownership 残留。

## Buildroot 与 SDK 集成

实施此阶段前按 SDK 规则检查 `repo status`、owning repository、分支和 manifest；不得修改 `sdk-main`，不得在未授权时启动 Buildroot、Rust、rootfs 或固件构建。

### 应用源码

产品 Rust 源码：

```text
apps/rust/camera
```

需要决定顶层仓库是否继续直接拥有该目录，或后续将 camera 作为独立 manifest project。第一版优先最小变更，不为了原型提前拆 repository。

### Buildroot package

在 `sdk/buildroot` owning repository 增加或扩展产品 Rust package，使最终 rootfs 安装：

```text
/usr/bin/hyz-camera
/etc/init.d/S82hyz-camera    # 名称和顺序实施时确认
```

camera 是独立进程，因此允许独立 init entry；它不得替代或控制 `S81hyz-router`。init 顺序应保证 router management plane 不依赖 camera 成功，同时 camera 可以在 router 前后独立重启。

需要固定启用或确认：

- GStreamer `1.22.2` headers/runtime；
- gst-plugins-base app；
- gst-plugins-good V4L2；
- gst-plugins-bad videoparsers；
- `gstreamer1-rockchip`；
- Rockchip MPP；
- 当前 pipeline 实际需要的最小插件；
- 目标 rootfs 第一版不依赖或安装 `ffmpeg`、`ffprobe`、`ffplay`；采集、硬编码和帧输出全部由 GStreamer/V4L2/MPP 完成；
- 开发主机可以使用 `ffmpeg` 生成固定 H.264 测试 fixture，使用 `ffprobe`/`ffmpeg` 辅助检查录制样本，但它们不是板端运行时依赖；
- 板端能力调查优先使用 `v4l2-ctl`、`gst-inspect-1.0` 和 `gst-launch-1.0`；是否在最终生产 rootfs 保留这些诊断 CLI 由 rootfs 审计单独决定；
- 不为 `str0m` 路径启用 GStreamer `webrtcbin`、libnice 或 `webrtcsink`；
- Rust GStreamer bindings 与目标 GStreamer `1.22.2` ABI 相容；
- 固定 Cargo.lock 和 licenses。

当前 SDK 已存在上述大部分媒体基础，但必须根据产品 source config 而不是忽略的 Buildroot output `.config` 确认最终可复现配置。

### 顶层构建入口

后续建议增加：

```text
make camera-check
make camera-app
make camera-host-webrtc
```

语义：

- `camera-check`：format、Clippy、host tests、source/static checks，不构建 SDK/rootfs；
- `camera-host-webrtc`：host-only loopback WebRTC integration；
- `camera-app`：显式授权后执行 AArch64 camera build；
- `make apps` 是否包含 camera 由集成阶段决定，但必须保持 deterministic 和 locked。

静态检查至少断言：

- 禁止 `sh -c`；
- 禁止 browser-provided pipeline/path/port/address；
- 固定 V4L2 路径和 UDP range；
- 不存在 camera WAN listener；
- `hyz-camera` 不执行 iptables/ip；
- GStreamer pipeline 不含 `rtph264pay`；
- H.264 caps 为 byte-stream/AU；
- Buildroot 必需插件启用；
- rootfs 安装路径和 init entry 唯一；
- OTA 继续排除 userdata；
- camera secret/session/SDP 不进入 source、fixtures 或发布文档。

## 分阶段实施

### 0. 固化需求与板端能力调查

修改：

- 本文；
- 实施开始后更新 `docs/soft-router-user-stories.md`；
- 架构落定后更新 `docs/architecture.md`。

只做静态和板端只读探测，不开始完整应用构建：

- 摄像头 `/dev/video*` topology；
- supported formats/resolutions/FPS；
- NV12；
- MPP encoder properties；
- pipeline 所需插件；
- camera 电源、驱动和启动时序；
- 目标 Chrome/Firefox/手机浏览器；
- LAN/Tailscale UDP route。

输出最终固定 camera device、profile 和端口池决策。

### 1. Host-only `str0m` WebRTC spike

新增最小实验或直接在 camera package tests 中实现：

- H.264 fixture；
- browser Offer/device Answer；
- exact host candidate；
- 实际 Chromium 首帧；
- session close/reconnect。

本阶段不接 GStreamer，不改 Buildroot，不改 router firewall。通过后固定 `str0m` 版本和 SDP fixture。

### 2. Camera domain/application 与 fake tests

创建 `apps/rust/camera`：

- domain types/invariants；
- application ports；
- session lifecycle；
- fake media/WebRTC/clock；
- Unix protocol DTO；
- action order、rollback、timeout 和 ownership tests。

本阶段不依赖生产 GStreamer adapter也应能完成绝大部分行为测试。

### 3. GStreamer/V4L2/MPP outbound adapter

实现：

- fixed pipeline construction；
- appsink frame extraction；
- PTS/keyframe/caps；
- bounded channel；
- bus ERROR/EOS；
- start/stop ownership；
- force-key-unit。

先做 host videotestsrc/x264 或 fixture adapter 验证，再在明确授权后做 AArch64/板端 MPP 验证。生产配置必须固定使用 MPP，不因插件缺失静默回退软件编码。

### 4. `str0m` outbound adapter 与 Unix daemon

实现：

- UDP port pool；
- SDP validation；
- exact candidate；
- session loop；
- frame writer；
- PLI/FIR；
- ICE/DTLS timeout；
- root-only control socket；
- process ownership 和 graceful shutdown。

完成 host integration 后再进入 router。

### 5. Router application seam 与 firewall

修改：

- router domain/status 中的 camera public state；
- application camera client/status ports；
- outbound Unix camera client；
- network camera firewall action/probe；
- composition root；
- shutdown/status aggregation。

camera 不进入 router core network readiness，camera firewall readiness 只控制 camera session API。

### 6. HTTP API

修改：

- `apps/rust/router/src/adapters/inbound/http/mod.rs`；
- HTTP DTO/tests；
- e2e fake application seam。

完成 auth、Origin、CSRF、body limit、listener-derived scope 和 error mapping。

### 7. Yew UI

修改：

- `apps/rust/router/src/web/main.rs`；
- `apps/rust/router/src/web/ui.rs`；
- `web-sys` features；
- CSS 和 Playwright tests。

实现显式播放、停止、cleanup、error、desktop/mobile。

所有 UI 变更必须按项目规则使用浏览器完成真实交互验证，不能只依赖截图或编译。

### 8. Buildroot、rootfs 与 init 集成

在明确构建授权后：

- Buildroot package/config；
- camera init；
- licenses；
- AArch64 ELF；
- rootfs audit；
- deterministic build；
- OTA 内容审计。

应用、Buildroot 和必要的设备/内核变更保持 repository-local cohesive commits，并更新 pinned manifest。

### 9. 板端与 Tailscale 验收

依次执行：

1. LAN 双页面并发观看；
2. 重复 start/stop；
3. 进程和 pipeline 故障；
4. Tailscale RouterOnly direct；
5. relay/DERP 可用场景；
6. 长时间稳定性；
7. 安全边界和 WAN 拒绝；
8. recovery-free OTA 后恢复。

在所有板端验收完成前，本文状态不得标记为完成。

## 建议提交顺序

1. **`docs: define first camera WebRTC live-view contract`**
   固定本文、产品范围、安全边界和验收矩阵。
2. **`feat(camera): add typed media session domain and lifecycle`**
   camera domain/application/ports/fake tests。
3. **`feat(camera): add bounded Unix control daemon`**
   control protocol、ownership、shutdown。
4. **`feat(camera): add GStreamer MPP H264 media adapter`**
   V4L2、pipeline、appsink、PTS、keyframe、bus lifecycle。
5. **`feat(camera): add str0m WebRTC session adapter`**
   SDP、ICE/DTLS/SRTP、UDP pool、session driver。
6. **`feat(router): add camera status and protected signaling facade`**
   application seam、Unix client、HTTP API、安全测试。
7. **`feat(router): add owned camera media firewall`**
   LAN/Tailscale INPUT、ownership、network tests。
8. **`feat(router-web): add authenticated camera live view`**
   Yew/WebRTC、Playwright desktop/mobile。
9. **`buildroot: install hyz-camera and required media runtime`**
   package/config/init/licenses。
10. **`docs: record camera LAN and Tailscale validation`**
    OTA、板端矩阵和已知限制。

实际提交可以按 owning repository 调整，但不得把应用、Buildroot、kernel 和生成产物混入同一 repository commit。

## 第一版验收标准

### 功能

- 管理员从 LAN 页面点击播放后能看到实时视频；
- 管理员从 Tailscale RouterOnly 页面点击播放后能看到实时视频；
- 同一前端 bundle 使用相对 URL，无 LAN/Tailscale 手工配置；
- 浏览器停止和离开页面会回收 session；
- 最后一个 session 结束后 pipeline 自动停止；
- camera 不可用时管理页面其他功能正常。

### 媒体

- 实际使用 Rockchip MPP，不静默回退软件编码；
- 输出 H.264 Baseline、Annex-B、AU-aligned；
- SPS/PPS 随 IDR 可供新 peer 解码；
- 目标分辨率、FPS 和码率达到最终固定 profile；
- 无持续无界队列或内存增长；
- PLI/FIR 或新连接能够获得 IDR；
- 8 小时直播无不可恢复 V4L2/MPP/GStreamer failure。

### 安全

- 匿名用户不能读取 camera status 或创建 session；
- 强制改密前不能创建 session；
- Origin、CSRF、body limit 和 typed DTO 生效；
- 浏览器不能指定设备路径、pipeline、地址、端口、codec、码率或 timeout；
- LAN session 只公布 LAN candidate；
- Tailscale session 只公布 Tailscale candidate；
- WAN 不能访问 camera HTTP 或 UDP；
- SDP、ICE credentials、DTLS key 和完整 candidate 不进入普通日志/status；
- unknown/foreign resource 不当作 owned/ready。

### 故障隔离

- `hyz-camera` `SIGKILL` 不影响 management LAN、HTTP、AP、STA、NAT、Mihomo 和 Tailscale；
- pipeline ERROR/EOS 只关闭 camera session；
- browser crash/断网能在 deadline 内回收；
- router restart 不接管旧未知 session；
- Tailscale address 变化关闭旧 session并要求重连；
- repeated start/stop 不累积 socket、process、device handle、GStreamer pipeline 或 firewall rule。

### 可复现性

- Cargo.lock 固定；
- Buildroot source config 固定所需插件；
- license 和版本可审计；
- host tests 不依赖真实摄像头或网络下载；
- target-device tests 与普通 host tests 分离；
- rootfs 只安装预期 camera ELF/init/media runtime；
- recovery-free OTA 不包含 userdata/recovery，除非另有明确发布决策。

## 待实施阶段确认的有限问题

以下问题不阻塞本文作为第一版计划，但必须在对应阶段通过探测或测试固定：

1. 最终固定 `/dev/video*` 节点及其跨冷启动稳定身份；
2. 摄像头已确认原生输出 NV12，支持 `3840×2160` crop bounds，并稳定输出 1920×1080 中央裁剪 30 FPS；
3. `mpph264enc` 在目标板上的实际 property 名、Baseline SPS 和浏览器兼容性；
4. Rust GStreamer bindings 与 SDK GStreamer `1.22.2`、Rust `1.85` 的精确版本组合；
5. `str0m` 最终固定版本及其与目标浏览器 SDP 的兼容 fixture；
6. Chromium/Firefox 在当前 HTTP 管理 origin 下的 recvonly `RTCPeerConnection` 行为；
7. Tailscale direct、peer relay 和 DERP 下的实际视频吞吐与延迟；
8. camera init 顺序、用户身份和 `/dev/video*`/MPP 权限；
9. ~~固定 UDP 端口池是否需要根据并发目标从 16 个进一步缩小~~ —— 已确认 16 端口满足 `MAX_VIEWERS=4`；
10. ~~第一版是否只支持一个管理员 session，还是在不增加编码器的前提下直接允许两个 peer~~ —— 已确认同一编码流扇出、最多 4 个 peer、无额外编码器；同一管理员账号可同时持有多个 session。

任何确认结果都应更新本文和对应测试，不通过在 adapter 中添加宽松 fallback 规避。
