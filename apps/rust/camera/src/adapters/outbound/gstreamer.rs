use super::v4l2::probe_fixed_camera_device;
use crate::{
    application::ports::{
        AudioSink, CameraAudioPort, CameraMediaPort, KeyframeRequester, MediaError,
        MediaTerminator, RunningAudioMedia, RunningMedia,
    },
    domain::{
        pts_ns_to_48khz, pts_ns_to_90khz, AudioFrame, AudioHub, BoundedAudioQueue,
        BoundedFrameQueue, CameraPipelineState, CameraRotation, CameraStreamProfile, EncodedFrame,
        FrameHub, TimestampWatermark, AUDIO_BITRATE_BPS, AUDIO_CHANNELS, AUDIO_FRAME_MS,
        AUDIO_SAMPLE_RATE_HZ, EXPECTED_ALSA_CARD_ID, FIXED_ALSA_DEVICE, FIXED_CAMERA_DEVICE,
        FIXED_CAPTURE_HEIGHT, FIXED_CAPTURE_WIDTH, MAX_AUDIO_FRAME_BYTES, MAX_ENCODED_FRAME_BYTES,
        WATERMARK_FONT_FAMILY,
    },
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const PIPELINE_START_DEADLINE: Duration = Duration::from_secs(10);
/// 管线停止上限：set_state(Null) 在设备/元素卡住时会永久阻塞（板端实测 RK809
/// 采集 XRUN 后任务不退出）。它若跑在会话线程上，close_session 的 join 会连带把
/// control socket 主线程卡死，整个 daemon 失去响应。超时则放弃停止、记录元素
/// 状态；残留管线由进程生命周期兜底，后续 start 会因设备占用而失败降级，而不是
/// 拖垮整个守护进程。
const PIPELINE_STOP_DEADLINE: Duration = Duration::from_secs(3);
const SYSTEM_PLUGIN_DIRECTORY: &str = "/usr/lib/gstreamer-1.0";
const FULL_RANGE_BT709_COLORIMETRY: &str = "1:3:5:1";
// Keep most of the output gain after RNNoise so the denoiser does not receive
// an unnecessarily amplified microphone noise floor.
const RNNOISE_POST_GAIN_DB: f32 = 3.0;

pub struct GStreamerMediaAdapter;

impl GStreamerMediaAdapter {
    pub fn new() -> Result<Self, MediaError> {
        gst::init().map_err(|_| MediaError::PipelineFailed)?;
        let registry = gst::Registry::get();
        registry.scan_path(SYSTEM_PLUGIN_DIRECTORY);
        if let Some(paths) =
            std::env::var_os("GST_PLUGIN_PATH_1_0").or_else(|| std::env::var_os("GST_PLUGIN_PATH"))
        {
            for path in std::env::split_paths(&paths).filter(|path| path.is_absolute()) {
                registry.scan_path(path);
            }
        }
        Ok(Self)
    }
}

impl CameraMediaPort for GStreamerMediaAdapter {
    fn probe(&self) -> Result<(), MediaError> {
        probe_fixed_camera_device()?;
        for factory in [
            "v4l2src",
            "queue",
            "videoscale",
            "videoflip",
            "clockoverlay",
            "mpph264enc",
            "h264parse",
            "capsfilter",
            "appsink",
        ] {
            if gst::ElementFactory::find(factory).is_none() {
                return Err(if factory == "mpph264enc" {
                    MediaError::EncoderUnavailable
                } else {
                    MediaError::PipelineFailed
                });
            }
        }
        Ok(())
    }

    fn start(&self, profile: CameraStreamProfile) -> Result<Box<dyn RunningMedia>, MediaError> {
        self.probe().map_err(|error| {
            eprintln!("video probe failed before start: {error:?}");
            error
        })?;
        let pipeline = gst::Pipeline::with_name("hyz-camera-pipeline");
        let source = make("v4l2src", "camera-source")?;
        let capture_caps = make("capsfilter", "capture-caps")?;
        let queue = make("queue", "capture-queue")?;
        let scaling =
            profile.width != FIXED_CAPTURE_WIDTH || profile.height != FIXED_CAPTURE_HEIGHT;
        let videoscale = make("videoscale", "video-scaler")?;
        let scale_caps = make("capsfilter", "scale-caps")?;
        let flip = make("videoflip", "orientation-flip")?;
        let overlay = make("clockoverlay", "timestamp-overlay")?;
        let encoder = make("mpph264enc", "h264-encoder")?;
        let parser = make("h264parse", "h264-parser")?;
        let h264_caps = make("capsfilter", "h264-caps")?;
        let sink_element = make("appsink", "encoded-frames")?;

        source.set_property("device", FIXED_CAMERA_DEVICE);
        capture_caps.set_property(
            "caps",
            gst::Caps::builder("video/x-raw")
                .field("format", "NV12")
                .field("colorimetry", FULL_RANGE_BT709_COLORIMETRY)
                .field("width", i32::from(FIXED_CAPTURE_WIDTH))
                .field("height", i32::from(FIXED_CAPTURE_HEIGHT))
                .field("framerate", gst::Fraction::new(i32::from(profile.fps), 1))
                .build(),
        );
        queue.set_property("max-size-buffers", 2u32);
        queue.set_property("max-size-bytes", 0u32);
        queue.set_property("max-size-time", 0u64);
        queue.set_property_from_str("leaky", "downstream");
        if scaling {
            videoscale.set_property("add-borders", false);
            scale_caps.set_property(
                "caps",
                gst::Caps::builder("video/x-raw")
                    .field("format", "NV12")
                    .field("colorimetry", FULL_RANGE_BT709_COLORIMETRY)
                    .field("width", i32::from(profile.width))
                    .field("height", i32::from(profile.height))
                    .field("framerate", gst::Fraction::new(i32::from(profile.fps), 1))
                    .build(),
            );
        }
        flip.set_property_from_str("method", videoflip_method(profile.rotation));
        let watermark = TimestampWatermark::DEFAULT;
        watermark
            .validate()
            .map_err(|_| MediaError::PipelineFailed)?;
        overlay.set_property_from_str("time-format", watermark.time_format);
        overlay.set_property_from_str(
            "font-desc",
            &format!("{} {}px", WATERMARK_FONT_FAMILY, watermark.font_size),
        );
        overlay.set_property_from_str("halignment", watermark.position.halign());
        overlay.set_property_from_str("valignment", watermark.position.valign());
        let padding = i32::try_from(watermark.padding).map_err(|_| MediaError::PipelineFailed)?;
        overlay.set_property("xpad", padding);
        overlay.set_property("ypad", padding);
        overlay.set_property("shaded-background", watermark.shaded_background);
        encoder.set_property_from_str("profile", "baseline");
        encoder.set_property_from_str("level", h264_level(profile));
        encoder.set_property("gop", i32::from(profile.fps));
        encoder.set_property("bps", profile.bitrate_bps);
        parser.set_property("config-interval", -1i32);
        h264_caps.set_property(
            "caps",
            gst::Caps::builder("video/x-h264")
                .field("stream-format", "byte-stream")
                .field("alignment", "au")
                .build(),
        );

        let appsink = sink_element
            .clone()
            .downcast::<gst_app::AppSink>()
            .map_err(|_| MediaError::PipelineFailed)?;
        appsink.set_max_buffers(2);
        appsink.set_drop(true);
        appsink.set_sync(false);
        appsink.set_wait_on_eos(false);

        let frames = Arc::new(FrameHub::new());
        let last_pts = Arc::new(Mutex::new(None));
        let callback_frames = Arc::clone(&frames);
        let callback_pts = Arc::clone(&last_pts);
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Error)?;
                    let caps = sample.caps().ok_or(gst::FlowError::NotNegotiated)?;
                    let caps_text = caps.to_string();
                    if !caps_text.contains("video/x-h264")
                        || !caps_text.contains("stream-format=(string)byte-stream")
                        || !caps_text.contains("alignment=(string)au")
                    {
                        return Err(gst::FlowError::NotNegotiated);
                    }
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let pts = buffer.pts().ok_or(gst::FlowError::Error)?.nseconds();
                    let mut previous = callback_pts
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let media_time_90khz =
                        pts_ns_to_90khz(*previous, pts).map_err(|_| gst::FlowError::Error)?;
                    *previous = Some(pts);
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                    if map.size() == 0 || map.size() > MAX_ENCODED_FRAME_BYTES {
                        return Err(gst::FlowError::Error);
                    }
                    callback_frames.push(EncodedFrame {
                        data: Arc::<[u8]>::from(map.as_slice()),
                        media_time_90khz,
                        is_keyframe: !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT),
                    });
                    Ok(gst::FlowSuccess::Ok)
                })
                .eos({
                    let frames = Arc::clone(&frames);
                    move |_| frames.close()
                })
                .build(),
        );

        let mut chain: Vec<&gst::Element> = vec![&source, &capture_caps, &queue];
        if scaling {
            chain.push(&videoscale);
            chain.push(&scale_caps);
        }
        chain.push(&flip);
        chain.push(&overlay);
        chain.push(&encoder);
        chain.push(&parser);
        chain.push(&h264_caps);
        chain.push(sink_element.upcast_ref());
        pipeline
            .add_many(&chain)
            .map_err(|_| MediaError::PipelineFailed)?;
        gst::Element::link_many(&chain).map_err(|_| MediaError::PipelineFailed)?;

        let stopping = Arc::new(AtomicBool::new(false));
        let state = Arc::new(AtomicU8::new(state_code(CameraPipelineState::Starting)));
        let bus = pipeline.bus().ok_or(MediaError::PipelineFailed)?;
        let bus_stopping = Arc::clone(&stopping);
        let bus_state = Arc::clone(&state);
        let bus_frames = Arc::clone(&frames);
        let bus_thread = thread::Builder::new()
            .name("hyz-camera-gst-bus".to_owned())
            .spawn(move || {
                while !bus_stopping.load(Ordering::Acquire) {
                    let Some(message) = bus.timed_pop(gst::ClockTime::from_mseconds(200)) else {
                        continue;
                    };
                    match message.view() {
                        gst::MessageView::Error(error) => {
                            eprintln!(
                                "video pipeline error: {} (debug: {:?})",
                                error.error(),
                                error.debug()
                            );
                            bus_state
                                .store(state_code(CameraPipelineState::Failed), Ordering::Release);
                            bus_frames.close();
                            break;
                        }
                        gst::MessageView::Eos(_) => {
                            bus_state
                                .store(state_code(CameraPipelineState::Failed), Ordering::Release);
                            bus_frames.close();
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|_| MediaError::PipelineFailed)?;

        if pipeline.set_state(gst::State::Playing).is_err() {
            eprintln!("video pipeline failed to enter PLAYING state");
            stopping.store(true, Ordering::Release);
            let _ = bus_thread.join();
            let _ = pipeline.set_state(gst::State::Null);
            return Err(MediaError::PipelineFailed);
        }
        state.store(
            state_code(CameraPipelineState::Streaming),
            Ordering::Release,
        );
        match frames.wait_first_frame(PIPELINE_START_DEADLINE) {
            Ok(()) => {}
            Err(_) => {
                eprintln!("video pipeline produced no first frame within the start deadline");
                stopping.store(true, Ordering::Release);
                frames.close();
                let _ = pipeline.set_state(gst::State::Null);
                let _ = bus_thread.join();
                state.store(state_code(CameraPipelineState::Failed), Ordering::Release);
                return Err(MediaError::PipelineFailed);
            }
        }

        Ok(Box::new(GStreamerRunningMedia {
            pipeline,
            encoder,
            frames,
            state,
            stopping,
            bus_thread: Some(bus_thread),
        }))
    }
}

/// 将产品旋转方向映射为 GStreamer `videoflip` 的 method（顺时针与 CSS 旋转一致）。
fn videoflip_method(rotation: CameraRotation) -> &'static str {
    match rotation {
        CameraRotation::Deg0 => "none",
        CameraRotation::Deg90 => "clockwise",
        CameraRotation::Deg180 => "rotate-180",
        CameraRotation::Deg270 => "counterclockwise",
    }
}

/// H.264 level 按帧大小选择：Level 4.0 的 MaxFS 为 2,097,152 像素，超过（1440p/4K）
/// 必须使用 Level 5.1，否则 SPS level 与帧大小不符，解码端可能拒绝。
fn h264_level(profile: CameraStreamProfile) -> &'static str {
    const LEVEL_40_MAX_PIXELS: u32 = 2_097_152;
    if u32::from(profile.width) * u32::from(profile.height) > LEVEL_40_MAX_PIXELS {
        "5.1"
    } else {
        "4"
    }
}

fn make(factory: &str, name: &str) -> Result<gst::Element, MediaError> {
    gst::ElementFactory::make(factory)
        .name(name)
        .build()
        .map_err(|_| {
            if factory == "mpph264enc" {
                MediaError::EncoderUnavailable
            } else {
                MediaError::PipelineFailed
            }
        })
}

struct GStreamerRunningMedia {
    pipeline: gst::Pipeline,
    encoder: gst::Element,
    frames: Arc<FrameHub>,
    state: Arc<AtomicU8>,
    stopping: Arc<AtomicBool>,
    bus_thread: Option<JoinHandle<()>>,
}

struct GStreamerKeyframeRequester {
    encoder: gst::Element,
}

struct GStreamerMediaTerminator {
    pipeline: gst::Pipeline,
    frames: Arc<FrameHub>,
    state: Arc<AtomicU8>,
    stopping: Arc<AtomicBool>,
}

impl MediaTerminator for GStreamerMediaTerminator {
    fn terminate(&self) {
        if self.stopping.swap(true, Ordering::AcqRel) {
            return;
        }
        self.state
            .store(state_code(CameraPipelineState::Stopping), Ordering::Release);
        self.frames.close();
        let next = stop_pipeline_bounded(&self.pipeline, "video");
        self.state.store(state_code(next), Ordering::Release);
    }
}

impl KeyframeRequester for GStreamerKeyframeRequester {
    fn request_keyframe(&self) -> Result<(), MediaError> {
        let structure = gst::Structure::builder("GstForceKeyUnit")
            .field("all-headers", true)
            .build();
        if self
            .encoder
            .send_event(gst::event::CustomUpstream::new(structure))
        {
            Ok(())
        } else {
            Err(MediaError::PipelineFailed)
        }
    }
}

impl RunningMedia for GStreamerRunningMedia {
    fn subscribe(&self) -> Arc<BoundedFrameQueue> {
        self.frames.subscribe()
    }

    fn unsubscribe(&self, queue: &Arc<BoundedFrameQueue>) {
        self.frames.unsubscribe(queue);
    }

    fn keyframe_requester(&self) -> Arc<dyn KeyframeRequester> {
        Arc::new(GStreamerKeyframeRequester {
            encoder: self.encoder.clone(),
        })
    }

    fn terminator(&self) -> Arc<dyn MediaTerminator> {
        Arc::new(GStreamerMediaTerminator {
            pipeline: self.pipeline.clone(),
            frames: Arc::clone(&self.frames),
            state: Arc::clone(&self.state),
            stopping: Arc::clone(&self.stopping),
        })
    }

    fn state(&self) -> CameraPipelineState {
        decode_state(self.state.load(Ordering::Acquire))
    }

    fn stop(mut self: Box<Self>) -> Result<(), MediaError> {
        let terminator = GStreamerMediaTerminator {
            pipeline: self.pipeline.clone(),
            frames: Arc::clone(&self.frames),
            state: Arc::clone(&self.state),
            stopping: Arc::clone(&self.stopping),
        };
        terminator.terminate();
        if let Some(thread) = self.bus_thread.take() {
            let _ = thread.join();
        }
        if self.state() == CameraPipelineState::Failed {
            Err(MediaError::PipelineFailed)
        } else {
            Ok(())
        }
    }
}

fn state_code(state: CameraPipelineState) -> u8 {
    match state {
        CameraPipelineState::Stopped => 0,
        CameraPipelineState::Starting => 1,
        CameraPipelineState::Streaming => 2,
        CameraPipelineState::Stopping => 3,
        CameraPipelineState::Failed => 4,
        CameraPipelineState::Unknown => 5,
    }
}

fn decode_state(value: u8) -> CameraPipelineState {
    match value {
        0 => CameraPipelineState::Stopped,
        1 => CameraPipelineState::Starting,
        2 => CameraPipelineState::Streaming,
        3 => CameraPipelineState::Stopping,
        4 => CameraPipelineState::Failed,
        _ => CameraPipelineState::Unknown,
    }
}

/// 有界停止管线：见 `PIPELINE_STOP_DEADLINE`。超时返回 Failed 并 dump 元素状态。
fn stop_pipeline_bounded(pipeline: &gst::Pipeline, tag: &str) -> CameraPipelineState {
    let owned = pipeline.clone();
    let (tx, rx) = mpsc::channel();
    let _ = thread::Builder::new()
        .name(format!("{tag}-pipeline-stop"))
        .spawn(move || {
            let _ = tx.send(owned.set_state(gst::State::Null).is_ok());
        });
    match rx.recv_timeout(PIPELINE_STOP_DEADLINE) {
        Ok(true) => {
            eprintln!("{tag} pipeline stopped");
            CameraPipelineState::Stopped
        }
        Ok(false) => {
            eprintln!("{tag} pipeline stop returned failure");
            CameraPipelineState::Failed
        }
        Err(_) => {
            eprintln!(
                "{tag} pipeline stop timed out after {:?}; element states:",
                PIPELINE_STOP_DEADLINE
            );
            for child in pipeline.children() {
                eprintln!("  {} state {:?}", child.name(), child.current_state());
            }
            CameraPipelineState::Failed
        }
    }
}

// ==================== 全双工语音对讲音频管线 ====================

/// 固定 ALSA 设备 probe：读 `/proc/asound/cards`（只读），要求 card 0 精确为
/// RK809（`rockchiprk809`）。foreign/未知卡不得视为 owned/ready（fail-degraded）。
fn probe_fixed_audio_device() -> Result<(), MediaError> {
    let cards = std::fs::read_to_string("/proc/asound/cards")
        .map_err(|_| MediaError::AudioDeviceNotFound)?;
    let card_zero_id = cards.lines().next().and_then(|line| {
        line.split_once('[')
            .and_then(|(_, rest)| rest.split_whitespace().next())
    });
    match card_zero_id {
        Some(id) if id == EXPECTED_ALSA_CARD_ID => Ok(()),
        _ => Err(MediaError::AudioDeviceNotFound),
    }
}

fn make_audio(factory: &str, name: &str) -> Result<gst::Element, MediaError> {
    gst::ElementFactory::make(factory)
        .name(name)
        .build()
        .inspect_err(|_| eprintln!("audio element {name} ({factory}) could not be created"))
        .map_err(|_| MediaError::AudioPipelineFailed)
}

fn fixed_pcm_caps() -> Result<gst::Caps, MediaError> {
    let rate = i32::try_from(AUDIO_SAMPLE_RATE_HZ).map_err(|_| MediaError::AudioPipelineFailed)?;
    Ok(gst::Caps::builder("audio/x-raw")
        .field("format", "S16LE")
        .field("rate", rate)
        .field("channels", i32::from(AUDIO_CHANNELS))
        .build())
}

/// 采集专用 caps：mono + channel-mask=FL（左声道）。
///
/// hw:0 采集必须是 2+ 声道，且本板麦克风信号只在 L（FL）声道，R 为空接近静音。
/// 不带 mask 时 audioconvert 的默认 2→1 混音按 (L+R)/2 处理，实测把 L 衰减约
/// 10dB（输入偏小的主因）；带上 channel-mask=FL 后混音矩阵保留 L 满电平，
/// 也避免把空接 R 声道的噪声混进来。
fn mic_pcm_caps() -> Result<gst::Caps, MediaError> {
    let rate = i32::try_from(AUDIO_SAMPLE_RATE_HZ).map_err(|_| MediaError::AudioPipelineFailed)?;
    Ok(gst::Caps::builder("audio/x-raw")
        .field("format", "S16LE")
        .field("rate", rate)
        .field("channels", i32::from(AUDIO_CHANNELS))
        .field("channel-mask", 0x1u64)
        .build())
}

/// 在 webrtcdsp 之后挂 RNNoise 神经网络降噪探针（nnnoiseless，纯 Rust 移植）。
///
/// 为什么放在 dsp 之后：webrtcdsp 输出固定的 48kHz mono 10ms（480 样本）帧，
/// 正好是 RNNoise 的固有帧长；探针在 dsp 的 src pad 上原地改写 S16LE 数据，
/// 不引入任何额外缓冲元素。WebRTC NS high 负责互补抑噪，RNNoise 是采集链的最终
/// 神经网络降噪器；输出增益在探针中位于 RNNoise 之后，避免先放大模拟噪声。
///
/// 说明：
/// - RNNoise 输入/输出是 i16 数值范围的 f32（demo 直接读 short 到 float），
///   不需要归一化到 [-1, 1]。
/// - 文档要求丢弃首帧（fade-in 伪影），首帧写静音。
/// - 非 480 整数倍的缓冲区直接透传（理论上不会出现：webrtcdsp 按 10ms 帧出
///   数据；万一出现只警告一次，避免刷日志）。
fn attach_rnnoise_probe(dsp: &gst::Element) -> Result<(), MediaError> {
    let src_pad = dsp
        .static_pad("src")
        .ok_or(MediaError::AudioPipelineFailed)?;
    let state: Arc<Mutex<Box<nnnoiseless::DenoiseState<'static>>>> =
        Arc::new(Mutex::new(nnnoiseless::DenoiseState::new()));
    let first = Arc::new(AtomicBool::new(true));
    let warned = Arc::new(AtomicBool::new(false));
    let frame_size = nnnoiseless::DenoiseState::FRAME_SIZE;
    let post_gain = 10.0f32.powf(RNNOISE_POST_GAIN_DB / 20.0);
    src_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, info| {
        let Some(buffer) = info.buffer_mut() else {
            return gst::PadProbeReturn::Ok;
        };
        let Some(buffer) = buffer.get_mut() else {
            return gst::PadProbeReturn::Ok;
        };
        let Ok(mut map) = buffer.map_writable() else {
            return gst::PadProbeReturn::Ok;
        };
        let bytes: &mut [u8] = &mut map;
        let samples = bytes.len() / 2;
        if samples == 0 || samples % frame_size != 0 {
            if !warned.swap(true, Ordering::Relaxed) {
                eprintln!("rnnoise: unexpected buffer size {samples} samples; passthrough");
            }
            return gst::PadProbeReturn::Ok;
        }
        let mut state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut input = [0f32; 480];
        let mut output = [0f32; 480];
        for frame in bytes.chunks_exact_mut(frame_size * 2) {
            for (i, chunk) in frame.chunks_exact_mut(2).enumerate() {
                input[i] = f32::from(i16::from_ne_bytes([chunk[0], chunk[1]]));
            }
            if first.swap(false, Ordering::Relaxed) {
                output.fill(0.0);
            } else {
                state.process_frame(&mut output, &input);
            }
            for (i, chunk) in frame.chunks_exact_mut(2).enumerate() {
                let value = (output[i] * post_gain).round().clamp(-32768.0, 32767.0) as i16;
                chunk.copy_from_slice(&value.to_ne_bytes());
            }
        }
        gst::PadProbeReturn::Ok
    });
    Ok(())
}

fn opus_caps() -> Result<gst::Caps, MediaError> {
    let rate = i32::try_from(AUDIO_SAMPLE_RATE_HZ).map_err(|_| MediaError::AudioPipelineFailed)?;
    // WebRTC RTP 的 Opus 载荷不带 OpusHead/streamheader；opusdec（gst-plugins-base）
    // 在无 header 路径用 gst_codec_utils_opus_parse_caps 解析 caps，该函数强制要求
    // channel-mapping-family 字段。只给 rate 时 appsrc 推入首帧即 not-negotiated，
    // 总线报错会拖垮整条音频管线（含采集）。
    // channels 必须与编码器实际发送一致：WebRTC 端（Chrome）默认单声道（fmtp 无
    // stereo=1），若配置 channels=2，opusdec 建多流双声道解码器解单声道帧会输出
    // 错乱信号（实测为低电平噪声，喇叭只有底噪没有语音）。
    Ok(gst::Caps::builder("audio/x-opus")
        .field("rate", rate)
        .field("channel-mapping-family", 0)
        .field("channels", 1)
        .build())
}

/// 音频元素名唯一性：session id 形如 `{generation}.{48 位随机 token}`，前缀
/// 是同代共享的 generation，唯一部分是点号后的随机 token；并发会话必须
/// 得到不同的元素名，否则同一管线内的元素名冲突。
fn audio_element_suffix(session_id: &str) -> String {
    session_id
        .rsplit_once('.')
        .map(|(_, token)| token.chars().take(16).collect())
        .unwrap_or_else(|| session_id.chars().take(16).collect())
}

pub struct GStreamerAudioAdapter;

impl GStreamerAudioAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GStreamerAudioAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraAudioPort for GStreamerAudioAdapter {
    fn probe(&self) -> Result<(), MediaError> {
        probe_fixed_audio_device()?;
        for factory in [
            "alsasrc",
            "audioconvert",
            "audioresample",
            "webrtcdsp",
            "opusenc",
            "appsrc",
            "opusdec",
            "audiomixer",
            "webrtcechoprobe",
            "alsasink",
            "capsfilter",
            "queue",
            "appsink",
        ] {
            if gst::ElementFactory::find(factory).is_none() {
                return Err(MediaError::AudioPipelineFailed);
            }
        }
        Ok(())
    }

    fn start(&self) -> Result<Box<dyn RunningAudioMedia>, MediaError> {
        self.probe().map_err(|error| {
            eprintln!("audio probe failed before start: {error:?}");
            error
        })?;
        let pipeline = gst::Pipeline::with_name("hyz-camera-audio-pipeline");

        // —— 采集支路：mic → AEC → Opus（appsrc 由浏览器 RTP 注入）——
        // hw:0 采集仅支持 2+ 声道：按设备原生格式采集，转换后再落到定格式（S16LE 48k mono）
        let source = make_audio("alsasrc", "mic-source")?;
        let capture_convert = make_audio("audioconvert", "mic-convert")?;
        let capture_resample = make_audio("audioresample", "mic-resample")?;
        let capture_caps = make_audio("capsfilter", "mic-caps")?;
        let dsp = make_audio("webrtcdsp", "echo-canceller")?;
        let encoder = make_audio("opusenc", "opus-encoder")?;
        let sink_element = make_audio("appsink", "encoded-audio-frames")?;
        // Restore the original complementary filters for the board's whooshing noise:
        // a gentle 150 Hz high-pass removes low-frequency airflow/mains rumble, while
        // the 8 kHz low-pass limits broadband ADC/power noise. The filters are optional
        // at runtime so an older rootfs still degrades to the RNNoise path.
        let mut capture_filters: Vec<gst::Element> = Vec::new();
        if gst::ElementFactory::find("audiocheblimit").is_some() {
            let filter_in = make_audio("audioconvert", "mic-filter-in")?;
            let hpf = make_audio("audiocheblimit", "mic-highpass")?;
            hpf.set_property_from_str("mode", "high-pass");
            hpf.set_property("cutoff", 150.0f32);
            hpf.set_property("poles", 4i32);
            let lpf = make_audio("audiocheblimit", "mic-lowpass")?;
            lpf.set_property_from_str("mode", "low-pass");
            lpf.set_property("cutoff", 8000.0f32);
            lpf.set_property("poles", 4i32);
            let filter_out = make_audio("audioconvert", "mic-filter-out")?;
            capture_filters.extend([filter_in, hpf, lpf, filter_out]);
        } else {
            eprintln!("audiocheblimit unavailable: capture band filtering disabled");
        }
        source.set_property("device", FIXED_ALSA_DEVICE);
        capture_caps.set_property("caps", mic_pcm_caps()?);
        // Keep WebRTC's echo canceller and high-pass filter, plus NS high as a
        // complementary suppressor for the board's non-stationary whooshing noise;
        // RNNoise remains the final denoiser.
        dsp.set_property("echo-cancel", true);
        dsp.set_property("high-pass-filter", true);
        dsp.set_property("noise-suppression", true);
        dsp.set_property_from_str("noise-suppression-level", "high");
        // Use adaptive digital gain with a small headroom. Adaptive gain restores
        // speech level after AEC/NS suppresses correlated speaker audio, while the
        // limited compression gain and post-RNNoise gain avoid excessive noise.
        dsp.set_property("gain-control", true);
        dsp.set_property_from_str("gain-control-mode", "adaptive-digital");
        dsp.set_property("target-level-dbfs", 3i32);
        // Keep a small pre-denoise gain for AEC/voice level, then apply the remaining
        // output gain after RNNoise so the denoiser sees less amplified noise.
        dsp.set_property("compression-gain-db", 6i32);
        dsp.set_property("limiter", true);
        encoder.set_property(
            "bitrate",
            i32::try_from(AUDIO_BITRATE_BPS).map_err(|_| MediaError::AudioPipelineFailed)?,
        );
        // frame-size 是 GstOpusEncFrameSize 枚举（值名 "20" 等），不能用整数属性设置。
        encoder.set_property_from_str("frame-size", &AUDIO_FRAME_MS.to_string());
        // DTX：静音段每 400ms 才发一帧，不再持续编码麦克风残余噪声底
        // （Opus SILK 在 DTX 关闭时对静音输出约 -56dB 的低电平噪声，见
        // xiph/opus#2353）。audio-type=voice 走 SILK 语音编码，对 32kbps
        // 对讲是更优解，词间噪声进一步收敛。
        encoder.set_property("dtx", true);
        encoder.set_property_from_str("audio-type", "voice");

        // —— 回放支路：audiomixer（会话按需 request pad）→ AEC 参考 → 喇叭 ——
        // hw:0 回放仅支持 2+ 声道：AEC 参考保持 mono 48k，到喇叭前再转回设备原生格式
        let mixer = make_audio("audiomixer", "talker-mixer")?;
        let tail_caps = make_audio("capsfilter", "speaker-caps")?;
        // webrtcdsp 按固定名 "webrtcechoprobe0" 查找回放参考，probe 不能改名。
        let echo_probe = make_audio("webrtcechoprobe", "webrtcechoprobe0")?;
        let tail_convert = make_audio("audioconvert", "speaker-convert")?;
        let tail_resample = make_audio("audioresample", "speaker-resample")?;
        let speaker = make_audio("alsasink", "speaker-sink")?;
        tail_caps.set_property("caps", fixed_pcm_caps()?);
        speaker.set_property("device", FIXED_ALSA_DEVICE);
        // 会话建立初期回放尾链无数据（浏览器音频稍后才到）：async=false 让
        // alsasink 不等首帧就完成状态切换，否则整条管线卡在 PAUSED 阻塞采集。
        speaker.set_property("async", false);
        // 管线时钟是 alsasrc 提供的 GstAudioClock（从时钟，阶梯式跳变）：
        // sync=true 时 alsasink 等待时间戳与时钟步进错位，实测回放变成一段段
        // 短突发（"发报声"）；sync=false 让数据到即播，配合 mixer 前的 queue
        // 吸收网络抖动，实测为连续纯净播放。
        speaker.set_property("sync", false);
        // 回放电平诊断：记录进入 alsasink 的缓冲区 RMS 峰值（每 200 帧打印），
        // 用于定位"喇叭无声/只有底噪"是数据未到、电平过低还是内容被解码破坏。
        let speaker_rms = Arc::new(Mutex::new((0u64, 0.0f64)));
        let speaker_sink_pad = speaker
            .static_pad("sink")
            .ok_or(MediaError::AudioPipelineFailed)?;
        {
            let speaker_rms = Arc::clone(&speaker_rms);
            speaker_sink_pad.add_probe(gst::PadProbeType::BUFFER, move |_pad, info| {
                let Some(buffer) = info.buffer() else {
                    return gst::PadProbeReturn::Ok;
                };
                let Ok(map) = buffer.map_readable() else {
                    return gst::PadProbeReturn::Ok;
                };
                let bytes = map.as_slice();
                let samples = bytes.len() / 2;
                if samples > 0 {
                    let mut sum = 0i64;
                    for chunk in bytes.chunks_exact(2).take(480) {
                        let value = i16::from_ne_bytes([chunk[0], chunk[1]]) as i64;
                        sum += value * value;
                    }
                    let rms = ((sum / (samples.min(480) as i64)) as f64).sqrt();
                    let mut state = speaker_rms.lock().unwrap_or_else(|p| p.into_inner());
                    state.0 += 1;
                    if rms > state.1 {
                        state.1 = rms;
                    }
                    if state.0 % 200 == 0 {
                        let peak = state.1;
                        let db = if peak > 0.0 {
                            20.0 * (peak / 32768.0).log10()
                        } else {
                            -120.0
                        };
                        eprintln!(
                            "speaker sink rms peak={peak:.0} ({db:.0} dBFS) after {} buffers",
                            state.0
                        );
                        state.1 = 0.0;
                    }
                }
                gst::PadProbeReturn::Ok
            });
        }

        let appsink = sink_element
            .clone()
            .downcast::<gst_app::AppSink>()
            .map_err(|_| MediaError::AudioPipelineFailed)?;
        appsink.set_max_buffers(8);
        appsink.set_drop(true);
        appsink.set_sync(false);
        appsink.set_wait_on_eos(false);

        let frames = Arc::new(AudioHub::new());
        let last_pts = Arc::new(Mutex::new(None));
        let callback_frames = Arc::clone(&frames);
        let callback_pts = Arc::clone(&last_pts);
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Error)?;
                    let caps = sample.caps().ok_or(gst::FlowError::NotNegotiated)?;
                    if !caps.to_string().contains("audio/x-opus") {
                        return Err(gst::FlowError::NotNegotiated);
                    }
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let pts = buffer.pts().ok_or(gst::FlowError::Error)?.nseconds();
                    let mut previous = callback_pts
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let media_time_48khz =
                        pts_ns_to_48khz(*previous, pts).map_err(|_| gst::FlowError::Error)?;
                    *previous = Some(pts);
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                    if map.size() == 0 || map.size() > MAX_AUDIO_FRAME_BYTES {
                        return Err(gst::FlowError::Error);
                    }
                    callback_frames.push(AudioFrame {
                        data: Arc::<[u8]>::from(map.as_slice()),
                        media_time_48khz,
                    });
                    Ok(gst::FlowSuccess::Ok)
                })
                .eos({
                    let frames = Arc::clone(&frames);
                    move |_| frames.close()
                })
                .build(),
        );

        let mut capture_chain: Vec<&gst::Element> =
            vec![&source, &capture_convert, &capture_resample, &capture_caps];
        capture_chain.extend(capture_filters.iter());
        capture_chain.extend([&dsp, &encoder, &sink_element]);
        let playback_tail: Vec<&gst::Element> = vec![
            &mixer,
            &tail_caps,
            &echo_probe,
            &tail_convert,
            &tail_resample,
            &speaker,
        ];
        let mut all: Vec<&gst::Element> = capture_chain.clone();
        all.extend(playback_tail.iter().copied());
        pipeline
            .add_many(&all)
            .map_err(|_| MediaError::AudioPipelineFailed)?;
        gst::Element::link_many(&capture_chain).map_err(|_| MediaError::AudioPipelineFailed)?;
        // RNNoise 探针必须挂在链接之后（pad 已就绪），且在管线进 PLAYING 之前，
        // 确保从第一帧音频就开始降噪。
        attach_rnnoise_probe(&dsp)?;
        gst::Element::link_many(&playback_tail).map_err(|_| MediaError::AudioPipelineFailed)?;

        let stopping = Arc::new(AtomicBool::new(false));
        let state = Arc::new(AtomicU8::new(state_code(CameraPipelineState::Starting)));
        let bus = pipeline.bus().ok_or(MediaError::AudioPipelineFailed)?;
        let bus_stopping = Arc::clone(&stopping);
        let bus_state = Arc::clone(&state);
        let bus_frames = Arc::clone(&frames);
        let bus_thread = thread::Builder::new()
            .name("hyz-camera-gst-audio-bus".to_owned())
            .spawn(move || {
                while !bus_stopping.load(Ordering::Acquire) {
                    let Some(message) = bus.timed_pop(gst::ClockTime::from_mseconds(200)) else {
                        continue;
                    };
                    match message.view() {
                        gst::MessageView::Error(error) => {
                            eprintln!(
                                "audio pipeline error: {} (debug: {:?})",
                                error.error(),
                                error.debug()
                            );
                            bus_state
                                .store(state_code(CameraPipelineState::Failed), Ordering::Release);
                            bus_frames.close();
                            break;
                        }
                        gst::MessageView::Eos(_) => {
                            bus_state
                                .store(state_code(CameraPipelineState::Failed), Ordering::Release);
                            bus_frames.close();
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|_| MediaError::AudioPipelineFailed)?;

        if pipeline.set_state(gst::State::Playing).is_err() {
            eprintln!("audio pipeline failed to enter PLAYING state");
            stopping.store(true, Ordering::Release);
            let _ = bus_thread.join();
            let _ = pipeline.set_state(gst::State::Null);
            return Err(MediaError::AudioPipelineFailed);
        }
        state.store(
            state_code(CameraPipelineState::Streaming),
            Ordering::Release,
        );
        match frames.wait_first_frame(PIPELINE_START_DEADLINE) {
            Ok(()) => {}
            Err(_) => {
                eprintln!("audio pipeline produced no first frame within the start deadline");
                for element in [
                    &source,
                    &capture_convert,
                    &capture_resample,
                    &capture_caps,
                    &dsp,
                    &encoder,
                    &sink_element,
                    &mixer,
                    &tail_caps,
                    &echo_probe,
                    &tail_convert,
                    &tail_resample,
                    &speaker,
                ] {
                    eprintln!(
                        "  audio element {} state: {:?}",
                        element.name(),
                        element.current_state()
                    );
                }
                stopping.store(true, Ordering::Release);
                frames.close();
                let _ = pipeline.set_state(gst::State::Null);
                let _ = bus_thread.join();
                state.store(state_code(CameraPipelineState::Failed), Ordering::Release);
                return Err(MediaError::AudioPipelineFailed);
            }
        }

        let sink = Arc::new(GStreamerAudioPlaybackSink {
            pipeline: pipeline.clone(),
            mixer,
            chains: Mutex::new(HashMap::new()),
        });
        eprintln!("audio pipeline started (capture producing frames)");
        Ok(Box::new(GStreamerRunningAudioMedia {
            pipeline,
            frames,
            sink,
            state,
            stopping,
            bus_thread: Some(bus_thread),
        }))
    }
}

struct GStreamerAudioPlaybackSink {
    pipeline: gst::Pipeline,
    mixer: gst::Element,
    chains: Mutex<HashMap<String, SessionPlaybackChain>>,
}

struct SessionPlaybackChain {
    appsrc: gst_app::AppSrc,
    elements: Vec<gst::Element>,
    mixer_pad: gst::Pad,
}

impl AudioSink for GStreamerAudioPlaybackSink {
    fn register(&self, session_id: &str) -> Result<(), MediaError> {
        let mut chains = self
            .chains
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if chains.contains_key(session_id) {
            return Ok(());
        }
        let suffix = audio_element_suffix(session_id);
        let appsrc = make_audio("appsrc", &format!("talker-{suffix}-src"))?;
        let decoder = make_audio("opusdec", &format!("talker-{suffix}-dec"))?;
        let convert = make_audio("audioconvert", &format!("talker-{suffix}-convert"))?;
        let resample = make_audio("audioresample", &format!("talker-{suffix}-resample"))?;
        let queue = make_audio("queue", &format!("talker-{suffix}-queue"))?;

        let app_src = appsrc
            .clone()
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| MediaError::AudioPipelineFailed)?;
        app_src.set_property("format", gst::Format::Time);
        app_src.set_property("is-live", true);
        // RTP 时间戳是随机大偏移（常见 20-40 亿），直接作 PTS 会让 alsasink 等到
        // 时钟追上天（几十小时后）才出声，表现为"远端没声音"且回放链淤积。
        // do-timestamp 按管线运行时间打时间戳，数据到达即播放（LAN 上 20ms 抖动
        // 可忽略）。
        app_src.set_property("do-timestamp", true);
        app_src.set_caps(Some(&opus_caps()?));

        let elements = vec![
            appsrc.clone().upcast(),
            decoder.clone().upcast(),
            convert.clone().upcast(),
            resample.clone().upcast(),
            queue.clone().upcast(),
        ];
        self.pipeline
            .add_many(&elements)
            .inspect_err(|_| eprintln!("audio pipeline add_many failed for session {session_id}"))
            .map_err(|_| MediaError::AudioPipelineFailed)?;
        let chain: Vec<&gst::Element> = vec![&appsrc, &decoder, &convert, &resample, &queue];
        gst::Element::link_many(&chain)
            .map_err(|_| MediaError::AudioPipelineFailed)
            .inspect_err(|_| {
                eprintln!("audio playback chain link failed for session {session_id}")
            })?;
        let mixer_pad = self
            .mixer
            .request_pad_simple("sink_%u")
            .ok_or(MediaError::AudioPipelineFailed)
            .inspect_err(|_| eprintln!("audiomixer request pad failed for session {session_id}"))?;
        let queue_src = match queue.static_pad("src") {
            Some(pad) => pad,
            None => {
                eprintln!("audio queue src pad missing for session {session_id}");
                return Err(MediaError::AudioPipelineFailed);
            }
        };
        queue_src
            .link(&mixer_pad)
            .map_err(|_| MediaError::AudioPipelineFailed)
            .inspect_err(|_| eprintln!("queue->mixer link failed for session {session_id}"))?;
        for element in &chain {
            element
                .sync_state_with_parent()
                .map_err(|_| MediaError::AudioPipelineFailed)
                .inspect_err(|_| {
                    eprintln!("playback chain sync_state failed for session {session_id}")
                })?;
        }
        chains.insert(
            session_id.to_owned(),
            SessionPlaybackChain {
                appsrc: app_src,
                elements,
                mixer_pad,
            },
        );
        eprintln!("playback chain registered for session {session_id}");
        Ok(())
    }

    fn unregister(&self, session_id: &str) {
        let chain = {
            let mut chains = self
                .chains
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            chains.remove(session_id)
        };
        let Some(chain) = chain else {
            return;
        };
        // 元素 stop/remove 与 mixer pad 释放放到有界后台线程：管线卡住时
        // element.set_state(Null) 会永久阻塞（见 PIPELINE_STOP_DEADLINE），若在
        // close_session/reap 主线程路径上执行会连带卡死整个 control socket。
        // 后台线程超时由进程生命周期兜底；残留元素随管线销毁。
        let pipeline = self.pipeline.clone();
        let mixer = self.mixer.clone();
        let _ = thread::Builder::new()
            .name("audio-chain-unreg".to_owned())
            .spawn(move || {
                for element in &chain.elements {
                    let _ = element.set_state(gst::State::Null);
                }
                mixer.release_request_pad(&chain.mixer_pad);
                for element in &chain.elements {
                    let _ = pipeline.remove(element);
                }
            });
    }

    fn push_opus(&self, session_id: &str, frame: AudioFrame) {
        let chains = self
            .chains
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(chain) = chains.get(session_id) else {
            return;
        };
        // do-timestamp 会按管线运行时间重写 PTS（见 register），这里只保留时长。
        let mut buffer = gst::Buffer::from_mut_slice(frame.data.to_vec());
        let buffer_ref = buffer.make_mut();
        buffer_ref.set_duration(Some(gst::ClockTime::from_mseconds(u64::from(
            AUDIO_FRAME_MS,
        ))));
        let _ = chain.appsrc.push_buffer(buffer);
    }
}

struct GStreamerRunningAudioMedia {
    pipeline: gst::Pipeline,
    frames: Arc<AudioHub>,
    sink: Arc<GStreamerAudioPlaybackSink>,
    state: Arc<AtomicU8>,
    stopping: Arc<AtomicBool>,
    bus_thread: Option<JoinHandle<()>>,
}

struct GStreamerAudioTerminator {
    pipeline: gst::Pipeline,
    frames: Arc<AudioHub>,
    state: Arc<AtomicU8>,
    stopping: Arc<AtomicBool>,
}

impl MediaTerminator for GStreamerAudioTerminator {
    fn terminate(&self) {
        if self.stopping.swap(true, Ordering::AcqRel) {
            return;
        }
        self.state
            .store(state_code(CameraPipelineState::Stopping), Ordering::Release);
        self.frames.close();
        let next = stop_pipeline_bounded(&self.pipeline, "audio");
        self.state.store(state_code(next), Ordering::Release);
    }
}

impl RunningAudioMedia for GStreamerRunningAudioMedia {
    fn subscribe_capture(&self) -> Arc<BoundedAudioQueue> {
        self.frames.subscribe()
    }

    fn unsubscribe_capture(&self, queue: &Arc<BoundedAudioQueue>) {
        self.frames.unsubscribe(queue);
    }

    fn terminator(&self) -> Arc<dyn MediaTerminator> {
        Arc::new(GStreamerAudioTerminator {
            pipeline: self.pipeline.clone(),
            frames: Arc::clone(&self.frames),
            state: Arc::clone(&self.state),
            stopping: Arc::clone(&self.stopping),
        })
    }

    fn playback(&self) -> Arc<dyn AudioSink> {
        let sink: Arc<dyn AudioSink> = self.sink.clone();
        sink
    }

    fn state(&self) -> CameraPipelineState {
        decode_state(self.state.load(Ordering::Acquire))
    }

    fn stop(mut self: Box<Self>) -> Result<(), MediaError> {
        let terminator = GStreamerAudioTerminator {
            pipeline: self.pipeline.clone(),
            frames: Arc::clone(&self.frames),
            state: Arc::clone(&self.state),
            stopping: Arc::clone(&self.stopping),
        };
        terminator.terminate();
        if let Some(thread) = self.bus_thread.take() {
            let _ = thread.join();
        }
        if self.state() == CameraPipelineState::Failed {
            Err(MediaError::AudioPipelineFailed)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::audio_element_suffix;

    #[test]
    fn audio_element_suffix_is_unique_across_sessions_of_the_same_generation() {
        // session id = {generation}.{48 位随机 token}：前缀在同代内共享，
        // 元素名必须来自点号后的随机部分，否则第二个会话的音频元素与
        // 第一个重名，add_many 会失败并拒绝整个会话。
        let generation = "0123456789ab";
        let first = format!("{generation}.{}", "a".repeat(48));
        let second = format!("{generation}.{}", "b".repeat(48));
        let first_suffix = audio_element_suffix(&first);
        let second_suffix = audio_element_suffix(&second);
        assert_ne!(first_suffix, second_suffix);
        assert_eq!(first_suffix, "a".repeat(16));
        assert_eq!(second_suffix, "b".repeat(16));
        assert!(!first_suffix.starts_with(generation));
    }
}
