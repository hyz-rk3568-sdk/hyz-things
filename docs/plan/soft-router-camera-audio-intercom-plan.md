# RK3568 摄像头全双工语音对讲实现计划

## 状态

**已决策并进入实现。** 摄像头直播第一版为纯视频（见
[`soft-router-camera-webrtc-plan.md`](soft-router-camera-webrtc-plan.md)），本文定义在
同一 `hyz-camera` 进程、同一 WebRTC 会话上叠加的全双工语音对讲：设备板载麦克风 →
浏览器（听），浏览器麦克风 → 设备喇叭（说），双向同时。

产品决策（2026-08-16 确认）：

| 决策点 | 结论 | 影响 |
| --- | --- | --- |
| 喊话权限 | 匿名 viewer 也可喊话（与观看同模型，15 分钟 viewer 令牌） | 控制协议保持 v2，SDP 驱动协商；安全边界 = 现有 viewer token 边界 + `MAX_VIEWERS=4` |
| 对讲形态 | 全双工 | 设备端 GStreamer `webrtcdsp` + `webrtcechoprobe` 回声消除；浏览器端 `getUserMedia({ echoCancellation: true })` |

## HTTPS / secure context 约束（板端验收发现，已实现）

浏览器只在 secure context（HTTPS 或 localhost）暴露 `navigator.mediaDevices`；
`http://192.168.8.1:8080` 不是 secure context，`getUserMedia` 恒为
`undefined`（实测 Chromium 报
`TypeError: undefined is not an object (evaluating 'getObject(arg0).getUserMedia')`），
因此「说」这一方向在纯 HTTP 下**对任何真实浏览器都不可用**，不只是测试环境问题。

结论：hyz-things 门户只提供 HTTPS：

- 首次启动时用 `rcgen` 生成自签 ECDSA 证书并持久化到
  `/userdata/hyz-things/tls/`（私钥 `0600` root-only），SAN 含
  `192.168.8.1` 与 `hyz-things.local`，有效期 10 年；损坏则重新生成。
- LAN 固定 listener 与 Tailscale exact listener 都用
  `axum::serve(TlsListener, app)` 终止 TLS（ALPN 仅 `http/1.1`）；
  管理员会话 cookie 在 TLS 下带 `Secure`，`allowed_origin` 使用 `https` scheme。
- 真实用户首次访问 `https://192.168.8.1:8080` 需要在浏览器手动通过一次自签证书
  警告（与主流 IPC/路由器一致）。设备上没有可校验自签证书的 TLS wget/curl，
  `S83hyz-things` 就绪探测从 HTTP `wget` 改为守护进程写入的
  `/run/hyz-things/ready` 标记。
- 宿主 Playwright 套件（`http://127.0.0.1:*`）与 http unit 测试不受影响
  （localhost 即 secure context，且测试 harness 保持明文 loopback）。

## 硬件与内核核实结论（已完成，作为事实基础）

| 层 | 结论 |
| --- | --- |
| 硬件（ATK-DLRK3568 底板） | 支持。板载咪头接 RK809 codec `MIC1_INP/MIC1_INN`（差分）；3.5mm 耳机座（插拔检测）；底板背面 8Ω 2W 喇叭接 RK809 内置 Class-D 功放 `SPKN_OUT/SPKP_OUT` |
| 内核（`rockchip_linux_defconfig`，产品默认） | 支持，零改动。`CONFIG_SND/SND_SOC=y`、`SND_SOC_RK817=y`（驱动含 RK809）、I2S TDM、MULTICODECS、`SND_USB_AUDIO=y` |
| DTS（`rk3568-atk-evb1-mipi-dsi-1080p`） | 支持，零改动。`i2s1_8ch`、`rk809_codec`（`mic-in-differential`、`hp-volume`、`spk-volume`）、`rk809_sound`（`hp-det-gpio`）全部 `status="okay"` |
| Buildroot rootfs | 原本无任何音频组件；本计划新增（见下） |
| hyz-camera / 前端 | 原本 SDP 硬拒 `m=audio`、pipeline 纯视频、SPA 无麦克风；本计划实现 |

## 目标

已认证管理员或 15 分钟 viewer 令牌持有者，在管理页面播放直播后：

- **听**：设备板载麦克风 → Opus → WebRTC → 浏览器独立 `<audio>` 元素播放；
- **说**：浏览器 `getUserMedia` 麦克风 → Opus → WebRTC → 设备喇叭播放；
- 双向同时（全双工），两端回声消除。

```text
浏览器 (things SPA)
  ├─ getUserMedia(mic, echoCancellation) ──► sendrecv audio ──► hyz-camera 喇叭（alsasink）
  └─ <video muted> + <audio> ◄── device mic（alsasrc → AEC → opusenc）── 同一 RTC session
hyz-camera 音频管线（新，独立于视频管线，同一进程）
  capture : alsasrc(hw:0) → audioconvert → audioresample → webrtcdsp(AEC) → opusenc → appsink → str0m writer
  playback: str0m reader → appsrc → opusdec → audioconvert → audioresample → audiomixer → webrtcechoprobe → alsasink(hw:0)
```

采集与回放必须在**同一条 GStreamer pipeline**：`webrtcdsp` 的回声参考来自
`webrtcechoprobe` 对播放信号的采样，二者同管线才能耦合。

## 固定媒体参数

| 项目 | 固定值 |
| --- | --- |
| 采样率 / 声道 | 48 kHz / 单声道（mic 与喇叭均为单声道） |
| 帧长 / 码率 | 20 ms / Opus 32 kbps |
| ALSA 设备 | 固定 `hw:0`；probe 读 `/proc/asound/cards`，card 0 必须精确为 `rockchiprk809`（foreign/未知不视为 owned） |
| 编解码路径（板测发现，必须设置） | RK809 的 `Playback Path` 与 `Capture MIC Path` 驱动复位值都是 OFF（喇叭/麦克风完全静音，且无 volume/gain 用户控件；音量由 DTS `spk-volume=<3>`、`hp-volume=<20>` 固定）。`S82hyz-camera` 每次启动先 `amixer` 设 `Playback Path=SPK`、`Capture MIC Path=Main Mic`，失败只降级音频 |
| 浏览器采集 | `getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true } })`，**仅在用户显式点击「开启对讲」后**采集；页面必须 HTTPS（secure context，见上节） |
| 协商 | 浏览器 offer 带 1 个 `m=audio`（Opus，`sendrecv`）；camera 校验并应答；不重协商 |
| 回放混音 | GStreamer `audiomixer`：每个会话一个动态 request pad（上限 `MAX_VIEWERS`），多说话者同时发声混音到喇叭 |
| 能力信号 | `CameraStatus.audio.supported`（additive，缺省 false；旧 camera 响应视为不支持，前端不显示对讲） |

## Buildroot 变更（`sdk/buildroot/configs/rockchip/hyz_things.config`）

| 配置项 | 说明 |
| --- | --- |
| `BR2_PACKAGE_GST1_PLUGINS_BASE_PLUGIN_ALSA=y` | `alsasrc`/`alsasink`（自动 `select BR2_PACKAGE_ALSA_LIB`） |
| `BR2_PACKAGE_GST1_PLUGINS_BASE_PLUGIN_AUDIOCONVERT=y` / `AUDIORESAMPLE=y` / `AUDIOMIXER=y` | PCM 转换与混音 |
| `BR2_PACKAGE_GST1_PLUGINS_BASE_PLUGIN_OPUS=y` | `opusenc`/`opusdec`（自动 `select BR2_PACKAGE_OPUS`） |
| `BR2_PACKAGE_GST1_PLUGINS_BAD_PLUGIN_WEBRTCDSP=y` | `webrtcdsp` + `webrtcechoprobe`（自动 `select BR2_PACKAGE_WEBRTC_AUDIO_PROCESSING`） |
| `BR2_PACKAGE_ALSA_UTILS=y` + `APLAY` + `AMIXER` | 板端验证工具（`aplay`/`arecord` 同源） |
| `BR2_PACKAGE_ROCKCHIP_ALSA_CONFIG=y` | 安装 `external/alsa-config` 的 `/usr/share/alsa`（rk809 card init/UCM，SPK/HP/Mic 路由生效） |

明确不启用：`RTP`/`RTPMANAGER`/`UDP`（传输由 str0m 承担）、`PULSEAUDIO`（直接
ALSA）。内核与 DTS 零改动。音频包只能随完整 rootfs 交付（recovery-free OTA）。

## 协议与 HTTP（最小面）

- **控制协议保持 v2**：`CreateSessionRequest` 形状不变；浏览器 offer 是否带
  `m=audio` 即表达音频意愿，camera 校验并应答。匿名 viewer 与管理员同权（决策）。
- `CameraStatus` 增加可选 `audio: { supported: bool }`（additive、serde default、
  不 bump 版本）。
- HTTP 头 `permissions-policy`：`microphone=()` → `microphone=(self)`（其余保持拒绝）。
- 无新 route、无新 body 字段、viewer-token 模型不变。

## hyz-camera 变更

### domain

- 新增 `domain/sdp.rs`：SDP 校验迁移为纯函数；video m-line 仍恰 1 个，audio 允许
  0 或 1 个且必须有 Opus；audio 方向只允许 `recvonly`/`sendrecv`（浏览器视角，
  `sendonly` 纯喊话不在产品模型内）；`offer_negotiates_audio` 供 application 决策。
- 新增 `domain/audio.rs`：固定参数常量、`AudioFrame`、`BoundedAudioQueue`、
  `AudioHub`（扇出，复用 `BoundedFrameQueue` 的 Mutex+Condvar 模式）、
  `pts_ns_to_48khz`。

### application ports

- `CameraAudioPort`（`probe`/`start`）+ `RunningAudioMedia`（`subscribe_capture`/
  `unsubscribe_capture`/`terminator`/`playback`/`state`/`stop`）+ `AudioSink`
  （`register`/`unregister`/`push_opus`）。
- `WebRtcSessionPort::create` 增加 `audio: Option<SessionAudio>`。
- `MediaError` 增加 `AudioDeviceNotFound`/`AudioDeviceBusy`/`AudioPipelineFailed`。

### 生命周期

- 音频采集+回放共享一条管线，引用计数随音频会话首建末停；音频失败只降级
  `audio.supported=false`（video-only），不影响 `available` 与视频会话。
- 会话关闭/线程退出（guard）/shutdown 均退订采集队列、注销混音输入、归还引用计数。

### outbound

- `gstreamer.rs`：`GStreamerAudioAdapter` 构建采集+回放单管线（元素 API，无
  `gst_parse_launch`）；`GStreamerAudioPlaybackSink` 按会话动态建立
  `appsrc → opusdec → audioconvert → audioresample → audiomixer(sink pad)` 支路。
- `webrtc.rs`：`MediaAdded(Audio)` 记录 audio mid 与 Opus payload；发送从音频队列
  取 Opus AU 写 `writer`（48 kHz `MediaTime`）；接收 `Event::MediaData` 解出 Opus
  推入对应会话的播放支路。

## hyz-things 变更

- outbound camera client：`CameraStatusWire` 增加 `audio` 字段并映射到 domain DTO。
- HTTP：`permissions-policy` 头、status 透传（自动）。
- SPA（`src/web/main.rs`）：
  - 播放时在 `audio.supported` 时创建 audio transceiver（`sendrecv`）；
  - `ontrack` 按 `track.kind()` 分流：video → `<video muted>`，audio → 独立
    `<audio autoplay playsinline>`；
  - 「开启对讲」按钮（仅播放中且 `audio.supported`）：点击后
    `getUserMedia` → `sender.replaceTrack(track)`；再点/停止 → `replaceTrack(null)`
    + `track.stop()`；清理路径（pagehide/visibilitychange/停止/退出/超时/断连）
    一律停止麦克风；
  - 帮助文本与提示更新；web-sys 新增 features：
    `MediaDevices`/`MediaStreamConstraints`/`MediaTrackConstraints`/`Navigator`/
    `RtcRtpSender`。

## 安全边界

沿用既有 viewer token 模型（同源领取、15 分钟、内存驻留、数量有界、仅能关闭
自己名下会话）、`MAX_VIEWERS=4`、exact Origin/CSRF、body 上限不变、SDP 仍由
camera 严格校验。对讲对匿名 viewer 开放是明确产品决策；`Permissions-Policy`
仅放开 `microphone=(self)`，CSP 不变，无新 route。

## 测试策略

- **camera domain/纯测试**：SDP audio 校验（合法 opus offer、缺 opus、双 audio、
  sendonly、video-only 兼容、payload 上限）、音频 profile 常量、队列/hub 扇出、
  PTS 转换。交叉编译后在板端执行。
- **camera application**（fake ports）：音频管线首建末停引用计数、video-only 不
  启音频、音频失败降级不影响视频、webrtc.create 失败回滚音频注册。
- **HTTP**：`permissions-policy: microphone=(self)` 精确断言；status audio 透传。
- **前端 Playwright（mock RTCPeerConnection + 真实 getUserMedia fake device）**：
  video+audio 双 transceiver、对讲按钮仅 `audio.supported` 时出现、开启挂载真实
  麦克风轨道、关闭摘下并停止、停止直播后 session 归零。
- **硬件 e2e（`camera-hardware.mjs` 扩展）**：设备麦克风 → 浏览器 `<audio>` 持有
  真实音频轨道且 `inbound-rtp` audio stats 递增；浏览器 `getUserMedia` → 设备喇叭
  `outbound-rtp` 递增，关闭对讲后 outbound 停止增长；并发多说话者混音；清理归零。
  **不做**「浏览器音调 → 设备喇叭 → 板载 mic → 回传浏览器」的环回音调检测：
  正确工作的浏览器端 AEC 会消除这个自听信号（这正是全双工应有的行为）；喇叭 →
  mic 声学通路由板端 `arecord`/`aplay` 冒烟覆盖。

## 验收矩阵

1. 板载 mic → 浏览器 `<audio>` 出声，`inbound-rtp` audio stats 递增且 `audioLevel>0`；
2. 浏览器 `getUserMedia` → 设备喇叭出声，`outbound-rtp` 递增，关闭对讲后停止增长；
   双向同时性由 inbound/outbound 双向 RTP stats 与板端 `arecord`/`aplay` 冒烟共同证明
   （浏览器端 AEC 会消除「自听」环回音调，不做环回音调检测）；
3. 匿名 viewer 令牌可听可说，过期失效；`MAX_VIEWERS=4` 并发多说话者混音正常；
4. 说话时无自听回环（浏览器端 AEC + 设备端 webrtcdsp）；
5. 设备端仅使用固定 `hw:0`（probe 校验 card id）；缺包/非 rk809 卡 →
   `audio.supported=false`，视频不受影响；
6. `permissions-policy` 精确为 `microphone=(self)`，CSP 不变，无新 route，协议 v2 形状不变；
7. 停止/离开/logout/超时后：mic、audio 元素、pipeline、mixer pad、`active_sessions` 全归零；
8. 既有四组真实路径（LAN/Tailscale × 桌面/移动）回归通过；
9. OTA 后 `/usr/bin/hyz-camera` 与打包 rootfs 逐字节一致，recovery/userdata 保留。
