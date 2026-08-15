# 摄像头真实设备端到端测试

## 定位

`apps/rust/router/e2e/camera-hardware.mjs` 是面向真实 RK3568、真实摄像头和真实网络路径的 Playwright 测试。它与普通 `npm run test:e2e` 的职责不同：

- 普通 Playwright 套件启动宿主机 Axum harness，并用浏览器内 `RTCPeerConnection` mock 验证 UI、API 和清理状态；
- `camera-hardware.mjs` 不替换 WebRTC、HTTP、camera control 或媒体管线，浏览器直接访问目标板管理页面；
- 实际链路覆盖浏览器 → Axum 信令 → root-only camera Unix control → V4L2 → GStreamer → Rockchip MPP H.264 → str0m ICE/DTLS/SRTP/RTP → Chromium 解码。

因此该脚本属于真实硬件端到端验收，不能作为无设备的普通 host test 自动运行，也不能在 CI 中依赖固定设备地址、管理员凭据或实时摄像头。

## 前置条件

- 目标板已安装包含 `/usr/bin/hyz-router`、`/usr/bin/hyz-camera` 和 `S82hyz-camera` 的固件；
- Router、Camera、管理 LAN 和需要测试的 Tailscale exact listener 已 ready；
- 主机已安装仓库锁定的 Node.js 依赖和 Playwright Chromium；
- 主机能够路由到命令行传入的每一个 HTTP origin；
- 管理员密码只从 Git ignored、权限 `0600` 的工作区根目录 `.env` 或等价的进程环境加载。不得把真实密码写入脚本、Git、文档、命令输出或测试结果。

脚本不会使用默认设备地址。LAN 和 Tailscale origin 必须由操作者显式传入：

```sh
set -a
. ./.env
set +a
cd apps/rust/router
HYZ_CAMERA_SCREENSHOT_DIR=/tmp/hyz-camera-hardware \
npm run test:hardware-camera -- \
  http://LAN_ADDRESS:8080 \
  http://TAILSCALE_ADDRESS:8080
```

管理页面当前使用 HTTP。脚本只针对显式传入的 origin 启动 Chromium 的 `--unsafely-treat-insecure-origin-as-secure`，使本地受控测试可以使用真实 WebRTC API；它不会把任意 HTTP 地址设为安全上下文。

## 首次改密测试

如果目标板为了受控验收临时进入 bootstrap 管理员状态，可以让脚本在第一个 origin 登录后完成强制改密，后续 origin 自动使用新密码：

```sh
cd apps/rust/router
HYZ_ROUTER_ADMIN_PASSWORD="$BOOTSTRAP_PASSWORD" \
HYZ_ROUTER_BOOTSTRAP=1 \
HYZ_ROUTER_REPLACEMENT_PASSWORD="$TEMPORARY_PASSWORD" \
npm run test:hardware-camera -- \
  http://LAN_ADDRESS:8080 \
  http://TAILSCALE_ADDRESS:8080
```

测试前必须按 SHA-256 备份原管理员 credential；测试后应原子恢复原文件、核对原始 SHA-256、删除临时备份并重启 Router。测试日志只记录摘要，不读取或输出密码哈希之外的秘密内容。

## 每条路径的断言

脚本为每个 origin 建立独立浏览器 context，并在桌面 `1280×800` 和移动端 `360×800` 各运行一次完整播放/停止流程：

1. 登录真实管理页面并确认 camera capability 可用；
2. 读取真实 `/api/v1/camera/status`，要求 `pipeline=stopped`、`active_sessions=0`；
3. 点击“播放直播”，等待页面进入“直播中”；
4. 确认 `<video>` 持有 live `MediaStreamTrack`，且 `videoWidth`、`videoHeight` 非零；
5. 通过保留的真实 `RTCPeerConnection` 调用 `getStats()`；
6. 要求 `connectionState=connected`；
7. 要求 video `inbound-rtp` 的 `bytesReceived`、`packetsReceived` 和 `framesDecoded` 都大于零；
8. 默认 720p 预设解码尺寸为 `1280×720`；切换到 4K 预设后要求解码尺寸为 `3840×2160`，视频没有暂停；
9. 将真实 `<video>` 绘制到小尺寸 canvas，计算平均亮度、最大亮度、亮像素比例和亮度标准差，拒绝纯黑或没有可见细节的解码帧；
10. 点击“旋转画面”，要求 `<video>` 依次应用 `rotate-90/180/270/0` class，验证每次点击顺时针旋转 90°；
11. 要求页面 `scrollWidth <= clientWidth`，避免桌面或移动端横向溢出；
12. 点击“停止直播”，等待 UI 回到“未播放”；
13. 再次读取真实 camera 状态，要求 `pipeline=stopped`、`active_sessions=0`。

这组断言不仅证明 SDP API 返回成功，还证明 ICE、DTLS、SRTP、RTP、H.264 解码、**实际可见像素内容**、全屏交互、页面媒体绑定和服务端 session 清理真实完成。仅有 `framesDecoded > 0` 不足以通过验收，因为 ISP 未运行或编码色彩范围错误时也可能持续编码和解码纯黑帧。

当前采集固定使用 RKISP 原生 `3840×2160` 全幅（不再裁剪），输出分辨率与码率通过固定枚举的 16:9 预设选择：默认 `720p · 2.5 Mbps`，可选 `1080p · 5 Mbps`、`1440p · 10 Mbps`、`4K · 20 Mbps`（均 `@ 30 FPS`、H.264 baseline）。非 4K 预设由 GStreamer `videoscale` 从 4K 全幅缩放，保留完整视野。预设只能在无活跃会话时通过管理页「画面分辨率 / 码率」下拉切换，切换会自动停止并重新打开直播。

时间戳水印在旋转、缩放之后、编码之前由 GStreamer `clockoverlay` 烧入码流：左上角、黑底、`%Y-%m-%d %H:%M:%S`，字号随旋转后显示高度缩放（`DejaVu Sans`，720p→18 … 4K→54）。水印是编码视频的一部分，不是浏览器叠加层；截图/录屏均包含它。验收截图应能在左上角看到与板端系统时间一致的日期时间文本。

画面旋转由「旋转画面」按钮驱动，是服务端媒体管线属性：`videoflip` 在编码前应用（0/90/180/270°），按钮按 0 → 270 → 180 → 90 → 0 循环，切换会自动停止并重新打开直播，浏览器端不再应用 CSS 旋转 class。90/270 时解码尺寸宽高互换（如 4K 预设变为 `2160×3840`），时间戳水印始终在最终方向画面的左上角。

GStreamer caps 显式声明 full-range BT.709 `colorimetry=1:3:5:1`；Rockchip MPP 插件必须把 `GST_VIDEO_COLOR_RANGE_0_255` 映射为 `MPP_FRAME_RANGE_JPEG`，设置 `prep:range` 后将 encoder 配置标记为待提交。可使用受控的一次性板端采集程序取得 Annex-B H.264，再在宿主机运行 `ffprobe`；正确 SPS/VUI 应报告 `color_range=pc`，通常同时显示 `pix_fmt=yuvj420p`。

弱光会降低平均亮度，但不应使最大亮度、亮像素比例和标准差同时归零。曾出现原始 NV12 有细节、Chromium canvas 全黑的情况：原因是 full-range 输入未进入 MPP encoder 配置，低于 16 的 Y 分量被浏览器按 limited-range 裁剪。最终 OTA 的四组真实 Chromium 样本平均亮度约 `17.0-21.9`、最大亮度约 `121-123`、亮像素比例约 `38.9%-49.5%`，不再依赖白天环境才能通过。

当前 IMX415 IQ 文件启用了 `AECV2_ANTIFLICKER_AUTO_MODE`，频率配置为 50 Hz。凌晨或极暗场景中，自动曝光提高 sensor/ISP gain 后可能显现横向行噪声、彩色热点；如果现场 LED 使用 60 Hz 或非标准 PWM 调光，还可能出现移动的 rolling-shutter banding。开灯后消失通常表示弱光高增益噪声，持续移动通常表示光源频闪，固定在相同行位置则应继续检查 sensor 行噪声、黑电平和坏点校准。这类条纹来自采集/ISP 条件，不是 WebRTC、H.264 transport 或全屏布局生成。

## 结果与诊断

成功时脚本向 stdout 输出结构化 JSON，包括每个 origin 和 viewport 的：

- 解码宽高；
- `readyState`；
- paused 状态；
- 平均/最大亮度、亮度标准差和亮像素比例；
- 全屏元素与 viewport 尺寸；
- 页面 client/scroll width。

设置 `HYZ_CAMERA_SCREENSHOT_DIR` 时会为每个 origin 和 viewport 分别保存普通页面和全屏画面截图。截图和 JSON 属于本地验收产物，不提交到 Git。

如果测试失败，应依次检查：

- 浏览器 console/page error；
- session create 的 Offer/Answer；
- `iceGatheringState`、`iceConnectionState`、`connectionState`；
- selected candidate pair、transport 和 inbound RTP stats；
- `/run/hyz-camera/control.sock` 与 camera status；
- UDP `40000-40015` socket 和 router-owned INPUT rules；
- GStreamer pipeline、V4L2 设备和 MPP encoder 状态。

不要因为页面显示“正在连接”就修改媒体管线；应先通过 `getStats()` 区分 UI 状态顺序、ICE/DTLS 连接问题和实际 RTP/解码问题。
