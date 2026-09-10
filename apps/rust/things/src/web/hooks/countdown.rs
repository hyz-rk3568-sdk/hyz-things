use super::*;

pub(crate) const EXAM_COUNTDOWN_TICK_MS: u32 = 1_000;
pub(crate) const EXAM_DAY_SECONDS: u64 = 24 * 60 * 60;
pub(crate) const EXAM_SOON_SECONDS: u64 = 120 * EXAM_DAY_SECONDS;
pub(crate) const EXAM_URGENT_SECONDS: u64 = 45 * EXAM_DAY_SECONDS;
pub(crate) const CUSTOM_COUNTDOWN_STORAGE_KEY: &str = "hyz-things.custom-countdowns.v2";
pub(crate) const LEGACY_CUSTOM_COUNTDOWN_STORAGE_KEY: &str = "hyz-things.custom-countdown.v1";
pub(crate) const CUSTOM_COUNTDOWN_DEFAULT_SECONDS: u64 = 25 * 60;
pub(crate) const CUSTOM_COUNTDOWN_MAX_HOURS: u64 = 99;
// 画中画：Document PiP（Chromium）优先；Safari/其他走 canvas 视频流 PiP；
// 都不支持时只显示一条提示，不再有全屏弹窗。
pub(crate) const EXAM_PIP_SCRIPT: &str = "/pip-countdown.js";
pub(crate) const EXAM_PIP_WINDOW_WIDTH: u32 = 520;
pub(crate) const EXAM_PIP_WINDOW_HEIGHT: u32 = 400;
pub(crate) const EXAM_PIP_NOTICE_MS: u32 = 5_000;
pub(crate) const EXAM_VIDEO_PIP_WIDTH: u32 = 960;
pub(crate) const EXAM_VIDEO_PIP_HEIGHT: u32 = 540;
pub(crate) const EXAM_VIDEO_PIP_FPS: f64 = 10.0;
// Safari 对 1fps 的画布流更易出现黑帧/不更新，用较高频率泵帧；
// 倒计时内容本身每秒才变化，多出的帧只是重复内容。
pub(crate) const EXAM_VIDEO_PIP_TICK_MS: u32 = 250;
// WebCodecs VideoFrame 时间戳单位是微秒；10fps 一帧间隔 100ms。
pub(crate) const EXAM_VIDEO_PIP_FRAME_US: u64 = 100_000;
pub(crate) const EXAM_VIDEO_PIP_POLL_MS: u32 = 150;
pub(crate) const EXAM_VIDEO_PIP_ACTIVATE_TIMEOUT_MS: u32 = 5_000;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExamCountdownTarget {
    pub(crate) id: &'static str,
    pub(crate) title: &'static str,
    pub(crate) eyebrow: &'static str,
    pub(crate) target_iso: &'static str,
    pub(crate) target_label: &'static str,
    pub(crate) target_note: &'static str,
    pub(crate) start_iso: &'static str,
}

pub(crate) const CUSTOM_COUNTDOWN_TARGETS: [ExamCountdownTarget; 3] = [
    ExamCountdownTarget {
        id: "custom-morning-countdown",
        title: "上午",
        eyebrow: "",
        target_iso: "1970-01-01T00:00:00+00:00",
        target_label: "",
        target_note: "",
        start_iso: "1970-01-01T00:00:00+00:00",
    },
    ExamCountdownTarget {
        id: "custom-afternoon-countdown",
        title: "下午",
        eyebrow: "",
        target_iso: "1970-01-01T00:00:00+00:00",
        target_label: "",
        target_note: "",
        start_iso: "1970-01-01T00:00:00+00:00",
    },
    ExamCountdownTarget {
        id: "custom-evening-countdown",
        title: "晚上",
        eyebrow: "",
        target_iso: "1970-01-01T00:00:00+00:00",
        target_label: "",
        target_note: "",
        start_iso: "1970-01-01T00:00:00+00:00",
    },
];

// 2027 年度考试公告尚未全部发布；未确认日期统一标注“预计”，并按预计首个笔试日排序。
pub(crate) const EXAM_COUNTDOWN_TARGETS: [ExamCountdownTarget; 4] = [
    ExamCountdownTarget {
        id: "national-exam",
        title: "下一次国考",
        eyebrow: "2027 年度 · 预计",
        target_iso: "2026-11-29T00:00:00+08:00",
        target_label: "预计 2026.11.29",
        target_note: "预计公共科目笔试日",
        start_iso: "2025-11-29T00:00:00+08:00",
    },
    ExamCountdownTarget {
        id: "guangdong-exam",
        title: "广东省考",
        eyebrow: "2027 年度 · 预计",
        target_iso: "2026-12-06T00:00:00+08:00",
        target_label: "预计 2026.12.06",
        target_note: "预计首个笔试日",
        start_iso: "2025-12-06T00:00:00+08:00",
    },
    ExamCountdownTarget {
        id: "hunan-civil-service",
        title: "湖南省考",
        eyebrow: "2027 年 · 预计",
        target_iso: "2027-03-14T00:00:00+08:00",
        target_label: "预计 2027.03.14",
        target_note: "预计首个笔试日",
        start_iso: "2026-03-14T00:00:00+08:00",
    },
    ExamCountdownTarget {
        id: "hunan-public-institution",
        title: "湖南事业编",
        eyebrow: "2027 年第一次 · 预计",
        target_iso: "2027-03-27T00:00:00+08:00",
        target_label: "预计 2027.03.27",
        target_note: "参考 2026 年第一次首个笔试日",
        start_iso: "2026-03-27T00:00:00+08:00",
    },
];

pub(crate) fn is_custom_countdown_target(target: &ExamCountdownTarget) -> bool {
    target.id.starts_with("custom-")
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct CountdownSnapshot {
    pub(crate) remaining_seconds: u64,
    pub(crate) days: u64,
    pub(crate) hours: u64,
    pub(crate) minutes: u64,
    pub(crate) seconds: u64,
    pub(crate) progress_percent: u8,
    pub(crate) finished: bool,
}

pub(crate) fn countdown_snapshot(now_ms: i64, start_ms: i64, target_ms: i64) -> CountdownSnapshot {
    let total_ms = target_ms.saturating_sub(start_ms).max(0);
    let remaining_ms = target_ms.saturating_sub(now_ms).max(0);
    let elapsed_ms = now_ms.saturating_sub(start_ms).clamp(0, total_ms);
    let progress_percent = if total_ms == 0 {
        u8::from(now_ms >= target_ms) * 100
    } else {
        ((elapsed_ms.saturating_mul(100) / total_ms).min(100)) as u8
    };
    let remaining_seconds = remaining_ms
        .saturating_add(999)
        .checked_div(1_000)
        .unwrap_or(0) as u64;

    CountdownSnapshot {
        remaining_seconds,
        days: remaining_seconds / EXAM_DAY_SECONDS,
        hours: (remaining_seconds % EXAM_DAY_SECONDS) / (60 * 60),
        minutes: (remaining_seconds % (60 * 60)) / 60,
        seconds: remaining_seconds % 60,
        progress_percent,
        finished: now_ms >= target_ms,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct CustomCountdownTimer {
    total_seconds: u64,
    start_ms: i64,
    target_ms: i64,
    paused: bool,
    remaining_seconds: u64,
    progress_percent: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct CustomCountdownState {
    duration_seconds: u64,
    timer: Option<CustomCountdownTimer>,
}

impl Default for CustomCountdownState {
    fn default() -> Self {
        Self {
            duration_seconds: CUSTOM_COUNTDOWN_DEFAULT_SECONDS,
            timer: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct PipCountdownConfig {
    target: ExamCountdownTarget,
    start_ms: i64,
    target_ms: i64,
    completion_ms: i64,
    total_seconds: u64,
    paused: bool,
    remaining_seconds: u64,
    progress_percent: u8,
}

impl PipCountdownConfig {
    fn snapshot(self, now_ms: i64) -> CountdownSnapshot {
        if self.paused {
            frozen_countdown_snapshot(
                self.total_seconds,
                self.remaining_seconds,
                self.progress_percent,
            )
        } else {
            countdown_snapshot(now_ms, self.start_ms, self.target_ms)
        }
    }
}

pub(crate) fn frozen_countdown_snapshot(
    total_seconds: u64,
    remaining_seconds: u64,
    progress_percent: u8,
) -> CountdownSnapshot {
    let remaining_seconds = remaining_seconds.min(total_seconds);
    CountdownSnapshot {
        remaining_seconds,
        days: remaining_seconds / EXAM_DAY_SECONDS,
        hours: (remaining_seconds % EXAM_DAY_SECONDS) / (60 * 60),
        minutes: (remaining_seconds % (60 * 60)) / 60,
        seconds: remaining_seconds % 60,
        progress_percent: progress_percent.min(100),
        finished: remaining_seconds == 0,
    }
}

pub(crate) fn custom_countdown_snapshot(
    now_ms: i64,
    state: CustomCountdownState,
) -> CountdownSnapshot {
    match state.timer {
        Some(timer) if timer.paused => frozen_countdown_snapshot(
            timer.total_seconds,
            timer.remaining_seconds,
            timer.progress_percent,
        ),
        Some(timer) => countdown_snapshot(now_ms, timer.start_ms, timer.target_ms),
        None => {
            let remaining_seconds = state.duration_seconds;
            CountdownSnapshot {
                remaining_seconds,
                days: remaining_seconds / EXAM_DAY_SECONDS,
                hours: (remaining_seconds % EXAM_DAY_SECONDS) / (60 * 60),
                minutes: (remaining_seconds % (60 * 60)) / 60,
                seconds: remaining_seconds % 60,
                progress_percent: 0,
                finished: false,
            }
        }
    }
}

pub(crate) fn custom_duration_from_parts(
    hours: u64,
    minutes: u64,
    seconds: u64,
) -> Result<u64, &'static str> {
    if hours > CUSTOM_COUNTDOWN_MAX_HOURS {
        return Err("小时必须在 0–99 之间");
    }
    if minutes > 59 {
        return Err("分钟必须在 0–59 之间");
    }
    if seconds > 59 {
        return Err("秒必须在 0–59 之间");
    }
    let total = hours * 60 * 60 + minutes * 60 + seconds;
    if total == 0 {
        return Err("倒计时时长至少为 1 秒");
    }
    Ok(total)
}

pub(crate) fn custom_duration_parts(seconds: u64) -> (u64, u64, u64) {
    (
        seconds / (60 * 60),
        (seconds % (60 * 60)) / 60,
        seconds % 60,
    )
}

pub(crate) fn duration_millis(seconds: u64) -> i64 {
    seconds.saturating_mul(1_000).min(i64::MAX as u64) as i64
}

pub(crate) fn custom_pip_config(
    target: &ExamCountdownTarget,
    state: CustomCountdownState,
    now_ms: i64,
) -> PipCountdownConfig {
    let (
        start_ms,
        target_ms,
        total_seconds,
        paused,
        remaining_seconds,
        progress_percent,
        completion_ms,
    ) = match state.timer {
        Some(timer) => {
            let snapshot = custom_countdown_snapshot(now_ms, state);
            let completion_ms = if timer.paused && snapshot.remaining_seconds > 0 {
                now_ms.saturating_add(duration_millis(snapshot.remaining_seconds))
            } else {
                timer.target_ms
            };
            (
                timer.start_ms,
                timer.target_ms,
                timer.total_seconds,
                timer.paused,
                snapshot.remaining_seconds,
                snapshot.progress_percent,
                completion_ms,
            )
        }
        None => {
            let completion_ms = now_ms.saturating_add(duration_millis(state.duration_seconds));
            (
                now_ms,
                completion_ms,
                state.duration_seconds,
                true,
                state.duration_seconds,
                0,
                completion_ms,
            )
        }
    };
    PipCountdownConfig {
        target: *target,
        start_ms,
        target_ms,
        completion_ms,
        total_seconds,
        paused,
        remaining_seconds,
        progress_percent,
    }
}

pub(crate) fn custom_pip_configs(
    states: [CustomCountdownState; 3],
    now_ms: i64,
) -> [PipCountdownConfig; 3] {
    std::array::from_fn(|index| {
        custom_pip_config(&CUSTOM_COUNTDOWN_TARGETS[index], states[index], now_ms)
    })
}

pub(crate) fn exam_pip_config(target: &ExamCountdownTarget, now_ms: i64) -> PipCountdownConfig {
    let start_ms = exam_timestamp(target.start_iso);
    let target_ms = exam_timestamp(target.target_iso);
    let snapshot = countdown_snapshot(now_ms, start_ms, target_ms);
    PipCountdownConfig {
        target: *target,
        start_ms,
        target_ms,
        completion_ms: target_ms,
        total_seconds: target_ms
            .saturating_sub(start_ms)
            .checked_div(1_000)
            .unwrap_or(0) as u64,
        paused: false,
        remaining_seconds: snapshot.remaining_seconds,
        progress_percent: snapshot.progress_percent,
    }
}

pub(crate) fn custom_countdown_status(
    state: CustomCountdownState,
    snapshot: CountdownSnapshot,
) -> &'static str {
    match state.timer {
        None => "待开始",
        Some(_) if snapshot.finished => "已结束",
        Some(timer) if timer.paused => "已暂停",
        Some(_) => "计时中",
    }
}

pub(crate) fn custom_card_tone(
    state: CustomCountdownState,
    snapshot: CountdownSnapshot,
) -> &'static str {
    if snapshot.finished && state.timer.is_some() {
        EXAM_CARD_FINISHED
    } else if state.timer.is_some_and(|timer| timer.paused) {
        EXAM_CARD_SOON
    } else {
        ""
    }
}

pub(crate) fn custom_countdown_storage() -> Option<Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub(crate) fn encode_custom_countdown_state(state: CustomCountdownState) -> String {
    match state.timer {
        Some(timer) => format!(
            "1|{}|{}|{}|{}|{}|{}|{}",
            state.duration_seconds,
            if timer.paused { "paused" } else { "running" },
            timer.total_seconds,
            timer.start_ms,
            timer.target_ms,
            timer.remaining_seconds,
            timer.progress_percent,
        ),
        None => format!("1|{}|idle", state.duration_seconds),
    }
}

pub(crate) fn decode_custom_countdown_state(value: &str) -> Option<CustomCountdownState> {
    let mut fields = value.split('|');
    if fields.next()? != "1" {
        return None;
    }
    let duration_seconds = fields.next()?.parse::<u64>().ok()?;
    let (hours, minutes, seconds) = custom_duration_parts(duration_seconds);
    if custom_duration_from_parts(hours, minutes, seconds).is_err() {
        return None;
    }
    match fields.next()? {
        "idle" if fields.next().is_none() => Some(CustomCountdownState {
            duration_seconds,
            timer: None,
        }),
        status @ ("running" | "paused") => {
            let total_seconds = fields.next()?.parse::<u64>().ok()?;
            let start_ms = fields.next()?.parse::<i64>().ok()?;
            let target_ms = fields.next()?.parse::<i64>().ok()?;
            let remaining_seconds = fields.next()?.parse::<u64>().ok()?;
            let progress_percent = fields.next()?.parse::<u8>().ok()?;
            if fields.next().is_some()
                || total_seconds == 0
                || remaining_seconds > total_seconds
                || progress_percent > 100
            {
                return None;
            }
            Some(CustomCountdownState {
                duration_seconds,
                timer: Some(CustomCountdownTimer {
                    total_seconds,
                    start_ms,
                    target_ms,
                    paused: status == "paused",
                    remaining_seconds,
                    progress_percent,
                }),
            })
        }
        _ => None,
    }
}

pub(crate) fn encode_custom_countdowns(states: [CustomCountdownState; 3]) -> String {
    let encoded = states
        .iter()
        .map(|state| encode_custom_countdown_state(*state))
        .collect::<Vec<_>>();
    format!("2;{}", encoded.join(";"))
}

pub(crate) fn decode_custom_countdowns(value: &str) -> Option<[CustomCountdownState; 3]> {
    let mut fields = value.split(';');
    if fields.next()? != "2" {
        return None;
    }
    let states = [
        decode_custom_countdown_state(fields.next()?)?,
        decode_custom_countdown_state(fields.next()?)?,
        decode_custom_countdown_state(fields.next()?)?,
    ];
    fields.next().is_none().then_some(states)
}

pub(crate) fn load_custom_countdown_states() -> [CustomCountdownState; 3] {
    let Some(storage) = custom_countdown_storage() else {
        return [CustomCountdownState::default(); 3];
    };
    storage
        .get_item(CUSTOM_COUNTDOWN_STORAGE_KEY)
        .ok()
        .flatten()
        .and_then(|value| decode_custom_countdowns(&value))
        .or_else(|| {
            storage
                .get_item(LEGACY_CUSTOM_COUNTDOWN_STORAGE_KEY)
                .ok()
                .flatten()
                .and_then(|value| decode_custom_countdown_state(&value))
                .map(|state| {
                    [
                        state,
                        CustomCountdownState::default(),
                        CustomCountdownState::default(),
                    ]
                })
        })
        .unwrap_or([CustomCountdownState::default(); 3])
}

pub(crate) fn save_custom_countdown_states(states: [CustomCountdownState; 3]) {
    if let Some(storage) = custom_countdown_storage() {
        let _ = storage.set_item(
            CUSTOM_COUNTDOWN_STORAGE_KEY,
            &encode_custom_countdowns(states),
        );
    }
}

pub(crate) fn exam_timestamp(iso: &str) -> i64 {
    Date::parse(iso) as i64
}

pub(crate) fn exam_card_tone(snapshot: CountdownSnapshot) -> &'static str {
    if snapshot.finished {
        EXAM_CARD_FINISHED
    } else if snapshot.remaining_seconds <= EXAM_URGENT_SECONDS {
        EXAM_CARD_URGENT
    } else if snapshot.remaining_seconds <= EXAM_SOON_SECONDS {
        EXAM_CARD_SOON
    } else {
        ""
    }
}

pub(crate) fn exam_status_label(snapshot: CountdownSnapshot) -> &'static str {
    if snapshot.finished {
        "已结束"
    } else if snapshot.remaining_seconds <= EXAM_URGENT_SECONDS {
        "冲刺期"
    } else if snapshot.remaining_seconds <= EXAM_SOON_SECONDS {
        "临近"
    } else {
        "备考中"
    }
}

pub(crate) fn pip_countdown_status(
    config: &PipCountdownConfig,
    snapshot: CountdownSnapshot,
) -> &'static str {
    if is_custom_countdown_target(&config.target) {
        if snapshot.finished {
            "已结束"
        } else if config.paused {
            "已暂停"
        } else {
            "计时中"
        }
    } else {
        exam_status_label(snapshot)
    }
}

pub(crate) struct DocumentPipHandles {
    window: Window,
    _on_hide: Closure<dyn FnMut(Event)>,
}

/// 预创建的 canvas 视频流 PiP 单元：canvas 每秒重绘，captureStream 喂给隐藏
/// video，video 持续静音播放。双击时视频已就绪并在播放中，手势内同步请求
/// 系统画中画才能同时满足 WebKit 的两条约束：正在处理手势（280837）且视频
/// 正在播放（iOS 拒绝未播放视频的画中画请求）。Safari/iPadOS 没有 Document
/// PiP，走这条路径；生命周期由倒计时面板的挂载/卸载管理。
pub(crate) struct PreparedVideoPip {
    video: HtmlVideoElement,
    source_canvas: HtmlCanvasElement,
    frame_source: PipFrameSource,
    stream: MediaStream,
    countdown: Rc<RefCell<PipCountdownConfig>>,
    cancelled: Rc<Cell<bool>>,
    _audio: Option<(AudioContext, MediaStreamAudioDestinationNode)>,
    _on_leave: Closure<dyn FnMut(Event)>,
}

/// 倒计时视频帧来源。Safari/iPadOS 的 canvas.captureStream 不可靠
/// （WebKit 235215 未修复，视频出黑帧/不更新），WebKit 上用 WebCodecs
/// VideoFrame + VideoTrackGenerator 泵真实视频帧；Chromium 继续用
/// captureStream（第二 canvas 复制帧，安卓已验证稳定）。
pub(crate) enum PipFrameSource {
    CanvasCapture(HtmlCanvasElement),
    TrackGenerator {
        generator: Rc<JsValue>,
        writer: Rc<JsValue>,
    },
}

/// WebKit（Safari/iPadOS）没有 Document PiP，且其 canvas.captureStream
/// 存在黑帧问题；取流方式与同步请求策略都以它为准。
pub(crate) fn is_webkit_only(window: &Window) -> bool {
    let Some(document) = window.document() else {
        return false;
    };
    let Ok(video_element) = document.create_element("video") else {
        return false;
    };
    let video_target: JsValue = video_element.into();
    picture_in_picture_method(&video_target, "webkitSetPresentationMode").is_some()
        || window.navigator().user_agent().is_ok_and(|agent| {
            agent.contains("AppleWebKit")
                && !agent.contains("Chrome")
                && !agent.contains("CriOS")
                && !agent.contains("Chromium")
        })
}

/// 尝试创建 WebCodecs VideoTrackGenerator（Safari 18+/iPadOS 26）。
/// 返回 (generator, writer)；老 Safari 没有该 API 时返回 None。
pub(crate) fn try_build_track_generator(window: &Window) -> Option<(Rc<JsValue>, Rc<JsValue>)> {
    let ctor_value = Reflect::get(window, &JsValue::from_str("VideoTrackGenerator")).ok()?;
    if !ctor_value.is_function() {
        return None;
    }
    let ctor: Function = ctor_value.dyn_into().ok()?;
    let generator = Reflect::construct(&ctor, &js_sys::Array::new()).ok()?;
    let track = Reflect::get(&generator, &JsValue::from_str("track")).ok()?;
    if track.is_undefined() || track.is_null() || !track.has_type::<MediaStreamTrack>() {
        return None;
    }
    let writable = Reflect::get(&generator, &JsValue::from_str("writable")).ok()?;
    let get_writer: Function = Reflect::get(&writable, &JsValue::from_str("getWriter"))
        .ok()?
        .dyn_into()
        .ok()?;
    let writer = get_writer.call0(&writable).ok()?;
    Some((Rc::new(generator), Rc::new(writer)))
}

/// 构建倒计时视频流：WebKit 优先 WebCodecs 轨道生成器（真实视频帧），
/// 其余浏览器用 captureStream（第二 canvas 复制帧，Chrome 上稳定）。
pub(crate) fn build_pip_frame_source(
    window: &Window,
    document: &Document,
    body: &Element,
    canvas: &HtmlCanvasElement,
) -> Option<(MediaStream, PipFrameSource)> {
    if is_webkit_only(window) {
        if let Some((generator, writer)) = try_build_track_generator(window) {
            let track = Reflect::get(&generator, &JsValue::from_str("track"))
                .ok()?
                .dyn_into::<MediaStreamTrack>()
                .ok()?;
            let stream = MediaStream::new().ok()?;
            stream.add_track(&track);
            return Some((stream, PipFrameSource::TrackGenerator { generator, writer }));
        }
    }
    // captureStream：把源 canvas 内容复制到第二张 canvas，对第二张取流。
    // capture canvas 放视口内但 opacity:0：保证 WebKit 确实绘制/合成它，
    // 同时完全不可见、不挡交互；多张卡叠在右下角也无妨。
    let Ok(capture_element) = document.create_element("canvas") else {
        return None;
    };
    let Ok(capture_canvas) = capture_element.dyn_into::<HtmlCanvasElement>() else {
        return None;
    };
    capture_canvas.set_width(EXAM_VIDEO_PIP_WIDTH);
    capture_canvas.set_height(EXAM_VIDEO_PIP_HEIGHT);
    let _ = capture_canvas.set_attribute("data-countdown-capture-canvas", "");
    let _ = capture_canvas.set_attribute("aria-hidden", "true");
    let capture_style = capture_canvas.style();
    let _ = capture_style.set_property("position", "fixed");
    let _ = capture_style.set_property("right", "0");
    let _ = capture_style.set_property("bottom", "0");
    let _ = capture_style.set_property("opacity", "0");
    let _ = capture_style.set_property("pointer-events", "none");
    if body.append_child(&capture_canvas).is_err() {
        return None;
    }
    // 首帧立即复制进 capture canvas，取流后马上有内容。
    copy_countdown_canvas(canvas, &capture_canvas);
    let capture = picture_in_picture_method(&capture_canvas.clone().into(), "captureStream")?;
    let stream_value = capture
        .call1(
            &capture_canvas.clone().into(),
            &JsValue::from_f64(EXAM_VIDEO_PIP_FPS),
        )
        .ok()?;
    let Ok(stream) = stream_value.dyn_into::<MediaStream>() else {
        return None;
    };
    Some((stream, PipFrameSource::CanvasCapture(capture_canvas)))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PipActivation {
    Activated,
    Failed,
    TimedOut,
}

pub(crate) fn has_document_picture_in_picture(window: &Window) -> bool {
    Reflect::get(
        window.as_ref(),
        &JsValue::from_str("documentPictureInPicture"),
    )
    .map(|value| value.is_object())
    .unwrap_or(false)
}

/// 必须在用户手势里同步调用（transient activation 要求）。
pub(crate) fn request_document_pip_window(
    window: &Window,
    width: u32,
    height: u32,
) -> Option<Promise> {
    let target = Reflect::get(
        window.as_ref(),
        &JsValue::from_str("documentPictureInPicture"),
    )
    .ok()?;
    if !target.is_object() {
        return None;
    }
    let request_window = picture_in_picture_method(&target, "requestWindow")?;
    let options = Object::new();
    Reflect::set(
        &options,
        &JsValue::from_str("width"),
        &JsValue::from_f64(width as f64),
    )
    .ok()?;
    Reflect::set(
        &options,
        &JsValue::from_str("height"),
        &JsValue::from_f64(height as f64),
    )
    .ok()?;
    let promise = request_window.call1(&target, &options.into()).ok()?;
    promise.dyn_into::<Promise>().ok()
}

/// 在画中画窗口里重建卡片：复制主题/样式表与卡片 DOM，注入倒计时引擎脚本。
/// 引擎脚本在画中画窗口自己的上下文里每秒重算，主标签页被切后台也不停。
pub(crate) fn populate_pip_window(
    pip_window: &Window,
    config: &PipCountdownConfig,
) -> Result<(), JsValue> {
    let parent_window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let parent_document = parent_window
        .document()
        .ok_or_else(|| JsValue::from_str("no parent document"))?;
    let pip_document = pip_window
        .document()
        .ok_or_else(|| JsValue::from_str("no pip document"))?;

    if let Some(parent_html) = parent_document.document_element() {
        if let Some(pip_html) = pip_document.document_element() {
            for name in ["data-theme", "lang", "class"] {
                if let Some(value) = parent_html.get_attribute(name) {
                    pip_html.set_attribute(name, &value)?;
                }
            }
        }
    }
    // 样式表复制（同源 <link>，满足 style-src 'self'；内联 <style> 会被 CSP 拦下，忽略即可）。
    let styles = parent_document.query_selector_all("style, link[rel='stylesheet']")?;
    if let Some(pip_head) = pip_document.head() {
        for index in 0..styles.length() {
            if let Some(node) = styles.item(index) {
                let clone = node.clone_node_with_deep(true)?;
                pip_head.append_child(&clone)?;
            }
        }
    }
    let selector = format!("[data-exam-id=\"{}\"]", config.target.id);
    let card = parent_document
        .query_selector(&selector)?
        .ok_or_else(|| JsValue::from_str("card missing"))?;
    let card_clone = card.clone_node_with_deep(true)?;
    if let Some(pip_body) = pip_document.body() {
        pip_body.append_child(&card_clone)?;
    }
    Reflect::set(
        pip_window.as_ref(),
        &JsValue::from_str("__hyzPipExamId"),
        &JsValue::from_str(config.target.id),
    )?;
    Reflect::set(
        pip_window.as_ref(),
        &JsValue::from_str("__hyzPipStartMs"),
        &JsValue::from_f64(config.start_ms as f64),
    )?;
    Reflect::set(
        pip_window.as_ref(),
        &JsValue::from_str("__hyzPipTargetMs"),
        &JsValue::from_f64(config.target_ms as f64),
    )?;
    Reflect::set(
        pip_window.as_ref(),
        &JsValue::from_str("__hyzPipCompletionMs"),
        &JsValue::from_f64(config.completion_ms as f64),
    )?;
    Reflect::set(
        pip_window.as_ref(),
        &JsValue::from_str("__hyzPipTotalSeconds"),
        &JsValue::from_f64(config.total_seconds as f64),
    )?;
    Reflect::set(
        pip_window.as_ref(),
        &JsValue::from_str("__hyzPipPaused"),
        &JsValue::from_bool(config.paused),
    )?;
    Reflect::set(
        pip_window.as_ref(),
        &JsValue::from_str("__hyzPipRemainingSeconds"),
        &JsValue::from_f64(config.remaining_seconds as f64),
    )?;
    Reflect::set(
        pip_window.as_ref(),
        &JsValue::from_str("__hyzPipProgressPercent"),
        &JsValue::from_f64(config.progress_percent as f64),
    )?;
    Reflect::set(
        pip_window.as_ref(),
        &JsValue::from_str("__hyzPipMode"),
        &JsValue::from_str(if is_custom_countdown_target(&config.target) {
            "custom"
        } else {
            "exam"
        }),
    )?;
    // 引擎脚本最后注入：脚本加载时会立刻查询卡片 DOM。
    let script = pip_document.create_element("script")?;
    script.set_attribute("src", EXAM_PIP_SCRIPT)?;
    if let Some(pip_head) = pip_document.head() {
        pip_head.append_child(&script)?;
    }
    Ok(())
}

pub(crate) fn populate_document_pip(
    pip_window: &Window,
    config: PipCountdownConfig,
    index: usize,
    pip_document: &Rc<RefCell<Option<DocumentPipHandles>>>,
    pip_exam: &UseStateHandle<Option<usize>>,
    pip_notice: &UseStateHandle<Option<&'static str>>,
    pip_notice_seq: &Rc<RefCell<u32>>,
) {
    if populate_pip_window(pip_window, &config).is_err() {
        let _ = pip_window.close();
        show_pip_notice(pip_notice, pip_notice_seq, "画中画内容初始化失败");
        return;
    }
    let pip_exam = pip_exam.clone();
    let pip_exam_for_hide = pip_exam.clone();
    let pip_document_for_hide = pip_document.clone();
    let on_hide = Closure::<dyn FnMut(Event)>::new(move |_event: Event| {
        *pip_document_for_hide.borrow_mut() = None;
        pip_exam_for_hide.set(None);
    });
    if pip_window
        .add_event_listener_with_callback("pagehide", on_hide.as_ref().unchecked_ref())
        .is_err()
    {
        let _ = pip_window.close();
        show_pip_notice(pip_notice, pip_notice_seq, "画中画内容初始化失败");
        return;
    }
    *pip_document.borrow_mut() = Some(DocumentPipHandles {
        window: pip_window.clone(),
        _on_hide: on_hide,
    });
    pip_exam.set(Some(index));
}

pub(crate) fn show_pip_notice(
    pip_notice: &UseStateHandle<Option<&'static str>>,
    pip_notice_seq: &Rc<RefCell<u32>>,
    message: &'static str,
) {
    let sequence = {
        let mut sequence = pip_notice_seq.borrow_mut();
        *sequence += 1;
        *sequence
    };
    pip_notice.set(Some(message));
    let pip_notice = pip_notice.clone();
    let pip_notice_seq = pip_notice_seq.clone();
    spawn_local(async move {
        TimeoutFuture::new(EXAM_PIP_NOTICE_MS).await;
        if *pip_notice_seq.borrow() == sequence {
            pip_notice.set(None);
        }
    });
}

/// 双击卡片的统一入口：优先 Document PiP（Chromium），其次 canvas 视频流
/// PiP（Safari/iPadOS、Firefox 等），都不支持时只显示一条提示。
pub(crate) fn handle_open_pip(
    index: usize,
    config: PipCountdownConfig,
    pip_document: &Rc<RefCell<Option<DocumentPipHandles>>>,
    pip_exam: &UseStateHandle<Option<usize>>,
    pip_notice: &UseStateHandle<Option<&'static str>>,
    pip_notice_seq: &Rc<RefCell<u32>>,
    prepared: &Rc<RefCell<Option<Vec<PreparedVideoPip>>>>,
) {
    if **pip_exam == Some(index) {
        if let Some(handles) = pip_document.borrow().as_ref() {
            let _ = handles.window.close();
        }
        *pip_document.borrow_mut() = None;
        pip_exam.set(None);
        return;
    }
    if pip_document.borrow().is_some() {
        if let Some(handles) = pip_document.borrow().as_ref() {
            let _ = handles.window.close();
        }
        *pip_document.borrow_mut() = None;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    if has_document_picture_in_picture(&window) {
        if let Some(promise) =
            request_document_pip_window(&window, EXAM_PIP_WINDOW_WIDTH, EXAM_PIP_WINDOW_HEIGHT)
        {
            let pip_document = pip_document.clone();
            let pip_exam = pip_exam.clone();
            let pip_notice = pip_notice.clone();
            let pip_notice_seq = pip_notice_seq.clone();
            spawn_local(async move {
                match JsFuture::from(promise).await {
                    // requestWindow 解析出的 Window 属于画中画窗口自己的 realm，
                    // dyn_into 的 instanceof 检查跨 realm 必失败，这里直接无校验包装。
                    Ok(value) => {
                        let pip_window = Window::from(value);
                        populate_document_pip(
                            &pip_window,
                            config,
                            index,
                            &pip_document,
                            &pip_exam,
                            &pip_notice,
                            &pip_notice_seq,
                        );
                    }
                    Err(_) => {
                        show_pip_notice(&pip_notice, &pip_notice_seq, "画中画打开失败");
                    }
                }
            });
            return;
        }
    }
    // video 流画中画：该单元已在画中画时再次点击 = 退出，否则打开。
    if let Some(item) = prepared.borrow().as_ref().and_then(|list| list.get(index)) {
        *item.countdown.borrow_mut() = config;
        if picture_in_picture_active(&item.video) {
            let _ = picture_in_picture_exit(&item.video);
            return;
        }
    }
    if video_pip_capable(&window)
        && open_video_pip(index, &window, prepared, pip_notice, pip_notice_seq)
    {
        return;
    }
    show_pip_notice(pip_notice, pip_notice_seq, "当前浏览器不支持画中画");
}

pub(crate) fn video_pip_capable(window: &Window) -> bool {
    let Some(document) = window.document() else {
        return false;
    };
    let Ok(canvas) = document.create_element("canvas") else {
        return false;
    };
    let has_capture = picture_in_picture_method(&canvas.into(), "captureStream").is_some()
        || Reflect::get(window, &JsValue::from_str("VideoTrackGenerator"))
            .is_ok_and(|ctor| ctor.is_function());
    let Ok(video_element) = document.create_element("video") else {
        return false;
    };
    let target: JsValue = video_element.into();
    let has_request = picture_in_picture_method(&target, "requestPictureInPicture").is_some();
    let has_webkit = picture_in_picture_method(&target, "webkitSetPresentationMode").is_some();
    has_capture && (has_request || has_webkit)
}

/// 预创建所有卡片的隐藏 canvas 视频流（见 `PreparedVideoPip`）。返回 None
/// 表示当前浏览器没有可用的视频画中画 API（与 `video_pip_capable` 一致），
/// 面板不挂任何隐藏元素。
pub(crate) fn prepare_exam_pip_videos(
    targets: &[ExamCountdownTarget],
) -> Option<Vec<PreparedVideoPip>> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let body = document.body()?;
    if !video_pip_capable(&window) {
        return None;
    }
    let mut prepared = Vec::with_capacity(targets.len());
    for target in targets.iter() {
        if let Some(item) = prepare_exam_pip_video(target, &window, &document, &body) {
            prepared.push(item);
        }
    }
    (!prepared.is_empty()).then_some(prepared)
}

pub(crate) fn prepare_custom_pip_videos(
    countdowns: [Rc<RefCell<PipCountdownConfig>>; 3],
) -> Option<Vec<PreparedVideoPip>> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let body = document.body()?;
    if !video_pip_capable(&window) {
        return None;
    }
    let mut prepared = Vec::with_capacity(countdowns.len());
    for countdown in countdowns {
        let Some(item) = prepare_custom_pip_video(countdown, &window, &document, &body) else {
            teardown_prepared_videos(&mut prepared);
            return None;
        };
        prepared.push(item);
    }
    Some(prepared)
}

pub(crate) fn prepare_exam_pip_video(
    target: &ExamCountdownTarget,
    window: &Window,
    document: &Document,
    body: &Element,
) -> Option<PreparedVideoPip> {
    let countdown = Rc::new(RefCell::new(exam_pip_config(target, Date::now() as i64)));
    prepare_countdown_pip_video(countdown, window, document, body)
}

pub(crate) fn prepare_custom_pip_video(
    countdown: Rc<RefCell<PipCountdownConfig>>,
    window: &Window,
    document: &Document,
    body: &Element,
) -> Option<PreparedVideoPip> {
    prepare_countdown_pip_video(countdown, window, document, body)
}

pub(crate) fn prepare_countdown_pip_video(
    countdown: Rc<RefCell<PipCountdownConfig>>,
    window: &Window,
    document: &Document,
    body: &Element,
) -> Option<PreparedVideoPip> {
    let Ok(canvas_element) = document.create_element("canvas") else {
        return None;
    };
    let Ok(canvas) = canvas_element.dyn_into::<HtmlCanvasElement>() else {
        return None;
    };
    canvas.set_width(EXAM_VIDEO_PIP_WIDTH);
    canvas.set_height(EXAM_VIDEO_PIP_HEIGHT);
    let _ = canvas.set_attribute("data-countdown-canvas", "");
    // 源 canvas 挂到 DOM（屏幕外），方便调试与测试断言。
    let _ = canvas.set_attribute("aria-hidden", "true");
    let canvas_style = canvas.style();
    let _ = canvas_style.set_property("position", "fixed");
    let _ = canvas_style.set_property("left", "-10000px");
    let _ = canvas_style.set_property("top", "0");
    let _ = canvas_style.set_property("pointer-events", "none");
    if body.append_child(&canvas).is_err() {
        return None;
    }
    // 先画首帧再取流：确保后续帧都带内容。
    let initial_config = *countdown.borrow();
    draw_countdown_canvas(
        &canvas,
        &initial_config,
        initial_config.snapshot(Date::now() as i64),
    );
    let (stream, frame_source) = build_pip_frame_source(window, document, body, &canvas)?;
    // iPadOS 对纯视频（无音轨）的画布流进画中画有兼容性问题，静默补一条
    // 静音音轨；创建失败（如 autoplay 策略）则退回纯视频流。引用保存在
    // 单元里，保证 AudioContext/节点不被 GC，teardown 时再释放。
    let audio = (|| -> Option<(AudioContext, MediaStreamAudioDestinationNode)> {
        let context = AudioContext::new().ok()?;
        let destination = context.create_media_stream_destination().ok()?;
        let audio_track = destination.stream().get_audio_tracks().get(0);
        let Ok(audio_track) = audio_track.dyn_into::<MediaStreamTrack>() else {
            return None;
        };
        stream.add_track(&audio_track);
        Some((context, destination))
    })();
    let Ok(video_element) = document.create_element("video") else {
        return None;
    };
    let Ok(video) = video_element.dyn_into::<HtmlVideoElement>() else {
        return None;
    };
    video.set_muted(true);
    video.set_autoplay(true);
    let _ = video.set_attribute("playsinline", "");
    let _ = video.set_attribute("webkit-playsinline", "");
    let _ = video.set_attribute("aria-hidden", "true");
    let _ = video.set_attribute("data-countdown-video", "");
    // WebKit 241152：muted 的 video 若在视口外设置 srcObject，WebKit
    // 不渲染它、直接黑屏（画中画随之黑）。必须先放进视口（右下角、
    // opacity:0 不可见、不挡交互）再赋 srcObject。
    let style = video.style();
    let _ = style.set_property("position", "fixed");
    let _ = style.set_property("right", "0");
    let _ = style.set_property("bottom", "0");
    let _ = style.set_property("opacity", "0");
    let _ = style.set_property("width", &format!("{}px", EXAM_VIDEO_PIP_WIDTH));
    let _ = style.set_property("height", &format!("{}px", EXAM_VIDEO_PIP_HEIGHT));
    let _ = style.set_property("pointer-events", "none");
    if body.append_child(&video).is_err() {
        return None;
    }
    // WebKit 262479：Safari 对 srcObject 视频流进画中画会渲染黑帧，
    // 官方绕法是在赋值 srcObject 之前打开 controls、赋值后立刻关闭。
    video.set_controls(true);
    video.set_src_object(Some(&stream));
    video.set_controls(false);
    // 持续以较高频率泵帧，让流一直有内容；video 静音自动播放，
    // 双击发起画中画时视频已就绪且正在播放。
    play_media_ignoring_interruption(&video);
    let cancelled = Rc::new(Cell::new(false));
    {
        let cancelled = cancelled.clone();
        let canvas = canvas.clone();
        let pump_source = match &frame_source {
            PipFrameSource::CanvasCapture(capture) => {
                PipFrameSource::CanvasCapture(capture.clone())
            }
            PipFrameSource::TrackGenerator { generator, writer } => {
                PipFrameSource::TrackGenerator {
                    generator: generator.clone(),
                    writer: writer.clone(),
                }
            }
        };
        let pump_countdown = countdown.clone();
        spawn_local(async move {
            let mut frame_seq: u64 = 0;
            while !cancelled.get() {
                TimeoutFuture::new(EXAM_VIDEO_PIP_TICK_MS).await;
                if !cancelled.get() {
                    let current_config = *pump_countdown.borrow();
                    draw_countdown_canvas(
                        &canvas,
                        &current_config,
                        current_config.snapshot(Date::now() as i64),
                    );
                    match &pump_source {
                        PipFrameSource::CanvasCapture(capture) => {
                            copy_countdown_canvas(&canvas, capture);
                        }
                        PipFrameSource::TrackGenerator { writer, .. } => {
                            let init = VideoFrameInit::new();
                            init.set_timestamp_f64((frame_seq * EXAM_VIDEO_PIP_FRAME_US) as f64);
                            init.set_duration_f64(EXAM_VIDEO_PIP_FRAME_US as f64);
                            if let Ok(frame) =
                                VideoFrame::new_with_html_canvas_element_and_video_frame_init(
                                    &canvas, &init,
                                )
                            {
                                let write =
                                    Reflect::get(writer.as_ref(), &JsValue::from_str("write"))
                                        .and_then(|f| f.dyn_into::<Function>())
                                        .and_then(|f| f.call1(writer.as_ref(), &frame));
                                match write {
                                    Ok(promise) => {
                                        // 写入由轨道生成器接管；等待其完成以串行化写入。
                                        let _ = JsFuture::from(Promise::from(promise)).await;
                                    }
                                    Err(_) => {
                                        // 同步抛错时帧未被接管，主动关闭防止泄漏。
                                        frame.close();
                                    }
                                }
                                frame_seq += 1;
                            }
                        }
                    }
                }
            }
        });
    }
    // 退出画中画（系统关闭按钮或 Esc）后复位状态：隐藏 video 继续静音播放。
    let on_leave = {
        let video = video.clone();
        Closure::<dyn FnMut(Event)>::new(move |_event: Event| {
            video.set_muted(true);
            let _ = video.remove_attribute("data-countdown-pip-active");
        })
    };
    let _ = video.add_event_listener_with_callback(
        "leavepictureinpicture",
        on_leave.as_ref().unchecked_ref(),
    );
    Some(PreparedVideoPip {
        video,
        source_canvas: canvas,
        frame_source,
        stream,
        countdown,
        cancelled,
        _audio: audio,
        _on_leave: on_leave,
    })
}

/// 让某张卡片的预创建 video 进入系统画中画。必须在 dblclick 手势栈内
/// 同步调用（WebKit 280837）；预播放保证请求时视频已就绪且正在播放
/// （iOS 拒绝未播放视频的画中画请求）。
pub(crate) fn open_video_pip(
    index: usize,
    window: &Window,
    prepared: &Rc<RefCell<Option<Vec<PreparedVideoPip>>>>,
    pip_notice: &UseStateHandle<Option<&'static str>>,
    pip_notice_seq: &Rc<RefCell<u32>>,
) -> bool {
    let (video, audio_to_resume) = {
        let prepared_guard = prepared.borrow();
        let Some(item) = prepared_guard.as_ref().and_then(|list| list.get(index)) else {
            return false;
        };
        (
            item.video.clone(),
            item._audio.as_ref().map(|(context, _)| context.clone()),
        )
    };
    // 双击手势内解除静音（音轨本身是静音，无声音）并恢复音频上下文，
    // 让 iOS 认为视频“有音频且正在播放”，提升画中画受理率。
    video.set_muted(false);
    if let Some(context) = audio_to_resume {
        let _ = context.resume();
    }
    play_media_ignoring_interruption(&video);
    let _ = video.set_attribute("data-countdown-pip-active", "");

    let outcome = Rc::new(RefCell::new(None::<PipActivation>));
    let requested = Rc::new(Cell::new(false));
    // 手势栈内同步发起请求：Safari/iPadOS 只认“正在处理手势”（WebKit
    // 280837），且 WebKit 不消耗 transient activation（313741）。
    // Chromium 恰好相反：请求时会先消耗激活（就绪前还会拒绝），所以
    // Chromium 只在视频已就绪时才同步请求，避免浪费激活导致后续异步
    // 重试必然失败（NotAllowedError）。
    let webkit_only = is_webkit_only(window);
    if video.ready_state() >= 2 || webkit_only {
        if let Ok(value) = picture_in_picture_request(&video) {
            requested.set(true);
            if !value.is_undefined() && !value.is_null() {
                let requested = requested.clone();
                let video = video.clone();
                spawn_local(async move {
                    if await_picture_in_picture(value).await.is_err()
                        && !picture_in_picture_active(&video)
                    {
                        requested.set(false);
                    }
                });
            }
        }
    }
    {
        let video = video.clone();
        let outcome = outcome.clone();
        let requested = requested.clone();
        spawn_local(async move {
            let mut elapsed_ms = 0u32;
            loop {
                TimeoutFuture::new(EXAM_VIDEO_PIP_POLL_MS).await;
                elapsed_ms += EXAM_VIDEO_PIP_POLL_MS;
                if picture_in_picture_active(&video) {
                    *outcome.borrow_mut() = Some(PipActivation::Activated);
                    return;
                }
                if elapsed_ms >= EXAM_VIDEO_PIP_ACTIVATE_TIMEOUT_MS {
                    *outcome.borrow_mut() = Some(PipActivation::TimedOut);
                    return;
                }
                if !requested.get() && video.ready_state() >= 2 {
                    match picture_in_picture_request(&video) {
                        Ok(value) => {
                            requested.set(true);
                            if !value.is_undefined() && !value.is_null() {
                                let video = video.clone();
                                let outcome = outcome.clone();
                                spawn_local(async move {
                                    if await_picture_in_picture(value).await.is_err()
                                        && !picture_in_picture_active(&video)
                                        && outcome.borrow().is_none()
                                    {
                                        *outcome.borrow_mut() = Some(PipActivation::Failed);
                                    }
                                });
                            }
                        }
                        Err(_) => {
                            if outcome.borrow().is_none() {
                                *outcome.borrow_mut() = Some(PipActivation::Failed);
                            }
                            return;
                        }
                    }
                }
            }
        });
    }
    {
        let video = video.clone();
        let outcome = outcome.clone();
        let pip_notice = pip_notice.clone();
        let pip_notice_seq = pip_notice_seq.clone();
        spawn_local(async move {
            loop {
                TimeoutFuture::new(EXAM_VIDEO_PIP_POLL_MS).await;
                match *outcome.borrow() {
                    Some(PipActivation::Activated) => return,
                    Some(PipActivation::Failed) | Some(PipActivation::TimedOut) => {
                        video.set_muted(true);
                        let _ = video.remove_attribute("data-countdown-pip-active");
                        show_pip_notice(&pip_notice, &pip_notice_seq, "画中画激活失败，请再试一次");
                        return;
                    }
                    None => {}
                }
            }
        });
    }
    true
}

/// 面板卸载时释放所有预创建单元：退出画中画、停流、关音频、移除元素。
pub(crate) fn teardown_prepared_videos(prepared: &mut Vec<PreparedVideoPip>) {
    for mut item in prepared.drain(..) {
        item.cancelled.set(true);
        if picture_in_picture_active(&item.video) {
            let _ = picture_in_picture_exit(&item.video);
        }
        if let Some((context, _destination)) = item._audio.take() {
            let _ = context.close();
        }
        for track in item.stream.get_tracks().iter() {
            if let Ok(track) = track.dyn_into::<MediaStreamTrack>() {
                track.stop();
            }
        }
        if let Some(parent) = item.video.parent_node() {
            let _ = parent.remove_child(&item.video);
        }
        if let Some(parent) = item.source_canvas.parent_node() {
            let _ = parent.remove_child(&item.source_canvas);
        }
        if let PipFrameSource::CanvasCapture(capture) = &item.frame_source {
            if let Some(parent) = capture.parent_node() {
                let _ = parent.remove_child(capture);
            }
        }
    }
}

/// 把源 canvas 内容复制到 capture canvas（Safari 黑帧绕法的核心）。
pub(crate) fn copy_countdown_canvas(source: &HtmlCanvasElement, target: &HtmlCanvasElement) {
    let Ok(context) = target.get_context("2d") else {
        return;
    };
    let Some(context) = context.and_then(|value| value.dyn_into::<CanvasRenderingContext2d>().ok())
    else {
        return;
    };
    let _ = context.draw_image_with_html_canvas_element(source, 0.0, 0.0);
}

pub(crate) fn pip_completion_ms(
    config: &PipCountdownConfig,
    snapshot: CountdownSnapshot,
    now_ms: i64,
) -> i64 {
    if config.paused && snapshot.remaining_seconds > 0 {
        now_ms.saturating_add(duration_millis(snapshot.remaining_seconds))
    } else {
        config.completion_ms
    }
}

pub(crate) fn format_pip_completion(
    config: &PipCountdownConfig,
    snapshot: CountdownSnapshot,
    now_ms: i64,
) -> String {
    let completion_ms = pip_completion_ms(config, snapshot, now_ms);
    if completion_ms <= 0 {
        return String::new();
    }
    let completion = Date::new(&JsValue::from_f64(completion_ms as f64));
    let current = Date::new(&JsValue::from_f64(now_ms as f64));
    let time = format!(
        "{:02}:{:02}",
        completion.get_hours(),
        completion.get_minutes()
    );
    let datetime = if completion.get_full_year() == current.get_full_year()
        && completion.get_month() == current.get_month()
        && completion.get_date() == current.get_date()
    {
        time
    } else {
        format!(
            "{}年{:02}月{:02}日 {}",
            completion.get_full_year(),
            completion.get_month() + 1,
            completion.get_date(),
            time,
        )
    };
    let prefix = if snapshot.finished {
        "完成于"
    } else if config.paused {
        "继续后预计完成"
    } else {
        "预计完成"
    };
    format!("{prefix} {datetime}")
}

pub(crate) fn draw_countdown_canvas(
    canvas: &HtmlCanvasElement,
    config: &PipCountdownConfig,
    snapshot: CountdownSnapshot,
) {
    let Ok(context) = canvas.get_context("2d") else {
        return;
    };
    let Some(context) = context.and_then(|value| value.dyn_into::<CanvasRenderingContext2d>().ok())
    else {
        return;
    };
    let width = EXAM_VIDEO_PIP_WIDTH as f64;
    let height = EXAM_VIDEO_PIP_HEIGHT as f64;
    let font_stack = "system-ui, -apple-system, 'PingFang SC', 'Noto Sans CJK SC', sans-serif";
    let mono_stack = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";

    context.set_fill_style_str("#282a36");
    context.fill_rect(0.0, 0.0, width, height);
    context.set_stroke_style_str("#44475a");
    context.set_line_width(2.0);
    context.stroke_rect(1.0, 1.0, width - 2.0, height - 2.0);

    context.set_font(&format!("700 24px {font_stack}"));
    context.set_fill_style_str("#6272a4");
    context.set_text_align("left");
    let _ = context.fill_text(config.target.eyebrow, 64.0, 100.0);
    let status = pip_countdown_status(config, snapshot);
    context.set_font(&format!("700 22px {font_stack}"));
    context.set_text_align("right");
    let _ = context.fill_text(status, width - 64.0, 100.0);

    context.set_text_align("left");
    context.set_font(&format!("800 46px {font_stack}"));
    context.set_fill_style_str("#f8f8f2");
    let _ = context.fill_text(config.target.title, 64.0, 176.0);

    let mut x = 64.0;
    let baseline = 330.0;
    if snapshot.finished {
        context.set_font(&format!("700 88px {mono_stack}"));
        context.set_fill_style_str("#f8f8f2");
        let _ = context.fill_text(
            if is_custom_countdown_target(&config.target) {
                "时间到"
            } else {
                "考试日已过"
            },
            x,
            baseline,
        );
    } else {
        let days = format!("{}", snapshot.days);
        let hours = format!("{:02}", snapshot.hours);
        let minutes = format!("{:02}", snapshot.minutes);
        let seconds = format!("{:02}", snapshot.seconds);
        for (value, unit) in [
            (&days, "天"),
            (&hours, "时"),
            (&minutes, "分"),
            (&seconds, "秒"),
        ] {
            context.set_font(&format!("700 104px {mono_stack}"));
            context.set_fill_style_str("#f8f8f2");
            let _ = context.fill_text(value, x, baseline);
            x += context
                .measure_text(value)
                .map(|metrics| metrics.width())
                .unwrap_or(0.0)
                + 10.0;
            context.set_font(&format!("700 34px {font_stack}"));
            context.set_fill_style_str("#6272a4");
            let _ = context.fill_text(unit, x, baseline - 22.0);
            x += context
                .measure_text(unit)
                .map(|metrics| metrics.width())
                .unwrap_or(0.0)
                + 48.0;
        }
    }

    let track_x = 64.0;
    let track_width = width - 128.0;
    let track_y = 400.0;
    let track_height = 14.0;
    context.set_fill_style_str("#44475a");
    context.fill_rect(track_x, track_y, track_width, track_height);
    context.set_fill_style_str("#bd93f9");
    context.fill_rect(
        track_x,
        track_y,
        track_width * snapshot.progress_percent as f64 / 100.0,
        track_height,
    );

    context.set_font(&format!("600 22px {font_stack}"));
    context.set_fill_style_str("#6272a4");
    let _ = context.fill_text(
        if is_custom_countdown_target(&config.target) {
            "倒计时进度"
        } else {
            "年度备考进度"
        },
        64.0,
        452.0,
    );
    context.set_text_align("right");
    context.set_fill_style_str("#f8f8f2");
    let _ = context.fill_text(
        &format!("{}%", snapshot.progress_percent),
        width - 64.0,
        452.0,
    );
    context.set_text_align("left");

    context.set_font(&format!("500 20px {font_stack}"));
    context.set_fill_style_str("#f8f8f2");
    let completion = format_pip_completion(config, snapshot, Date::now() as i64);
    let _ = context.fill_text(&completion, 64.0, 498.0);
    context.set_text_align("left");
}

pub(crate) fn custom_timer_for_duration(total_seconds: u64, now_ms: i64) -> CustomCountdownTimer {
    CustomCountdownTimer {
        total_seconds,
        start_ms: now_ms,
        target_ms: now_ms.saturating_add(duration_millis(total_seconds)),
        paused: false,
        remaining_seconds: total_seconds,
        progress_percent: 0,
    }
}

pub(crate) fn restart_custom_countdown(
    state: CustomCountdownState,
    now_ms: i64,
) -> CustomCountdownState {
    CustomCountdownState {
        timer: Some(custom_timer_for_duration(state.duration_seconds, now_ms)),
        ..state
    }
}

pub(crate) fn pause_custom_countdown(
    state: CustomCountdownState,
    now_ms: i64,
) -> CustomCountdownState {
    let Some(timer) = state.timer else {
        return state;
    };
    let snapshot = custom_countdown_snapshot(now_ms, state);
    if timer.paused || snapshot.finished {
        return state;
    }
    CustomCountdownState {
        timer: Some(CustomCountdownTimer {
            paused: true,
            remaining_seconds: snapshot.remaining_seconds,
            progress_percent: snapshot.progress_percent,
            ..timer
        }),
        ..state
    }
}

pub(crate) fn resume_custom_countdown(
    state: CustomCountdownState,
    now_ms: i64,
) -> CustomCountdownState {
    let Some(timer) = state.timer else {
        return state;
    };
    if !timer.paused || timer.remaining_seconds == 0 {
        return state;
    }
    let elapsed_seconds = timer.total_seconds.saturating_sub(timer.remaining_seconds);
    CustomCountdownState {
        timer: Some(CustomCountdownTimer {
            start_ms: now_ms.saturating_sub(duration_millis(elapsed_seconds)),
            target_ms: now_ms.saturating_add(duration_millis(timer.remaining_seconds)),
            paused: false,
            ..timer
        }),
        ..state
    }
}

#[derive(Clone, Copy)]
pub(crate) enum CustomDurationPart {
    Hours,
    Minutes,
    Seconds,
}

pub(crate) fn update_custom_duration(
    state: CustomCountdownState,
    part: CustomDurationPart,
    value: u64,
) -> CustomCountdownState {
    let (mut hours, mut minutes, mut seconds) = custom_duration_parts(state.duration_seconds);
    match part {
        CustomDurationPart::Hours => hours = value.min(CUSTOM_COUNTDOWN_MAX_HOURS),
        CustomDurationPart::Minutes => minutes = value.min(59),
        CustomDurationPart::Seconds => seconds = value.min(59),
    }
    let duration_seconds = custom_duration_from_parts(hours, minutes, seconds).unwrap_or(0);
    CustomCountdownState {
        duration_seconds,
        ..state
    }
}

pub(crate) fn custom_duration_input_callback(
    states: &UseStateHandle<[CustomCountdownState; 3]>,
    index: usize,
    part: CustomDurationPart,
) -> Callback<InputEvent> {
    let states = states.clone();
    Callback::from(move |event: InputEvent| {
        let input: HtmlInputElement = event.target_unchecked_into();
        let value = input.value().parse::<u64>().unwrap_or(0);
        let mut next = *states;
        next[index] = update_custom_duration(next[index], part, value);
        states.set(next);
    })
}

pub(crate) fn countdown_display(snapshot: CountdownSnapshot) -> CountdownDisplay {
    CountdownDisplay {
        remaining_seconds: snapshot.remaining_seconds,
        days: snapshot.days,
        hours: snapshot.hours,
        minutes: snapshot.minutes,
        seconds: snapshot.seconds,
        progress_percent: snapshot.progress_percent,
        finished: snapshot.finished,
    }
}

pub(crate) fn render_exam_countdown_card(
    target: &ExamCountdownTarget,
    snapshot: CountdownSnapshot,
    index: usize,
    on_double_click: Callback<MouseEvent>,
) -> Html {
    let status = exam_status_label(snapshot);
    let tone = exam_card_tone(snapshot);
    let card_label = if snapshot.finished {
        format!("{}，考试已结束", target.title)
    } else {
        format!("{}，距离考试 {} 天", target.title, snapshot.days)
    };
    html! {
        <CountdownCard
            id={target.id}
            title={target.title}
            eyebrow={target.eyebrow}
            target_iso={target.target_iso}
            target_label={target.target_label}
            target_note={target.target_note}
            display={countdown_display(snapshot)}
            index={index}
            status={status}
            tone={classes!(tone)}
            card_label={card_label}
            finished_label="考试日已过"
            progress_label="年度备考进度"
            progress_aria_label={format!("{}冲刺进度 {}%", target.title, snapshot.progress_percent)}
            on_double_click={on_double_click}
        />
    }
}

#[function_component(CustomCountdownPanel)]
pub(crate) fn custom_countdown_panel() -> Html {
    let now_ms = use_state(|| Date::now() as i64);
    let countdown_states = use_state(load_custom_countdown_states);
    let pip_document = use_mut_ref(|| None::<DocumentPipHandles>);
    let pip_index = use_state(|| None::<usize>);
    let pip_notice = use_state(|| None::<&'static str>);
    let pip_notice_seq = use_mut_ref(|| 0u32);
    let prepared = use_mut_ref(|| None::<Vec<PreparedVideoPip>>);
    let pip_configs = use_mut_ref(|| {
        custom_pip_configs(*countdown_states, *now_ms).map(|config| Rc::new(RefCell::new(config)))
    });

    {
        let now_ms = now_ms.clone();
        use_effect_with((), move |_| {
            let cancelled = Rc::new(Cell::new(false));
            let task_cancelled = cancelled.clone();
            spawn_local(async move {
                while !task_cancelled.get() {
                    TimeoutFuture::new(EXAM_COUNTDOWN_TICK_MS).await;
                    if !task_cancelled.get() {
                        now_ms.set(Date::now() as i64);
                    }
                }
            });
            move || cancelled.set(true)
        });
    }

    {
        let states = *countdown_states;
        use_effect_with(states, move |states| {
            save_custom_countdown_states(*states);
            || ()
        });
    }

    {
        let pip_configs = pip_configs.clone();
        let states = *countdown_states;
        let now_ms = *now_ms;
        use_effect_with((states, now_ms), move |(states, now_ms)| {
            let next_configs = custom_pip_configs(*states, *now_ms);
            let slots = pip_configs.borrow().clone();
            for (slot, config) in slots.into_iter().zip(next_configs) {
                *slot.borrow_mut() = config;
            }
            || ()
        });
    }

    {
        let prepared = prepared.clone();
        let pip_configs = pip_configs.clone();
        use_effect_with((), move |_| {
            *prepared.borrow_mut() = prepare_custom_pip_videos(pip_configs.borrow().clone());
            move || {
                if let Some(list) = prepared.borrow_mut().as_mut() {
                    teardown_prepared_videos(list);
                }
            }
        });
    }

    // Esc 退出 video 流画中画（Document PiP 窗口自己有 Esc 处理）。
    {
        let prepared = prepared.clone();
        use_effect_with((), move |_| {
            let listener =
                Closure::<dyn FnMut(KeyboardEvent)>::new(move |key_event: KeyboardEvent| {
                    if !is_escape_key(&key_event) {
                        return;
                    }
                    if let Some(list) = prepared.borrow().as_ref() {
                        for item in list {
                            if picture_in_picture_active(&item.video) {
                                let _ = picture_in_picture_exit(&item.video);
                            }
                        }
                    }
                });
            let document = web_sys::window().and_then(|window| window.document());
            if let Some(document) = document.as_ref() {
                let _ = document
                    .add_event_listener_with_callback("keydown", listener.as_ref().unchecked_ref());
            }
            move || {
                if let Some(document) = document.as_ref() {
                    let _ = document.remove_event_listener_with_callback(
                        "keydown",
                        listener.as_ref().unchecked_ref(),
                    );
                }
            }
        });
    }

    let close_pip = {
        let pip_document = pip_document.clone();
        let pip_index = pip_index.clone();
        let prepared = prepared.clone();
        Rc::new(move || {
            if let Some(handles) = pip_document.borrow().as_ref() {
                let _ = handles.window.close();
            }
            *pip_document.borrow_mut() = None;
            if (*pip_index).is_some() {
                pip_index.set(None);
            }
            if let Some(list) = prepared.borrow().as_ref() {
                for item in list {
                    if picture_in_picture_active(&item.video) {
                        let _ = picture_in_picture_exit(&item.video);
                    }
                }
            }
        })
    };

    let timer_cards = CUSTOM_COUNTDOWN_TARGETS
        .iter()
        .enumerate()
        .map(|(index, target)| {
            let state = (*countdown_states)[index];
            let snapshot = custom_countdown_snapshot(*now_ms, state);
            let status = custom_countdown_status(state, snapshot);
            let tone = custom_card_tone(state, snapshot);
            let (hours, minutes, seconds) = custom_duration_parts(state.duration_seconds);
            let set_hours =
                custom_duration_input_callback(&countdown_states, index, CustomDurationPart::Hours);
            let set_minutes = custom_duration_input_callback(
                &countdown_states,
                index,
                CustomDurationPart::Minutes,
            );
            let set_seconds = custom_duration_input_callback(
                &countdown_states,
                index,
                CustomDurationPart::Seconds,
            );

            let restart = {
                let countdown_states = countdown_states.clone();
                let close_pip = close_pip.clone();
                Callback::from(move |_| {
                    let current = (*countdown_states)[index];
                    if current.duration_seconds == 0 {
                        return;
                    }
                    close_pip();
                    let mut next = *countdown_states;
                    next[index] = restart_custom_countdown(current, Date::now() as i64);
                    countdown_states.set(next);
                })
            };
            let pause = {
                let countdown_states = countdown_states.clone();
                let close_pip = close_pip.clone();
                Callback::from(move |_| {
                    let current = (*countdown_states)[index];
                    let next_state = pause_custom_countdown(current, Date::now() as i64);
                    if next_state != current {
                        close_pip();
                        let mut next = *countdown_states;
                        next[index] = next_state;
                        countdown_states.set(next);
                    }
                })
            };
            let resume = {
                let countdown_states = countdown_states.clone();
                let close_pip = close_pip.clone();
                Callback::from(move |_| {
                    let current = (*countdown_states)[index];
                    let next_state = resume_custom_countdown(current, Date::now() as i64);
                    if next_state != current {
                        close_pip();
                        let mut next = *countdown_states;
                        next[index] = next_state;
                        countdown_states.set(next);
                    }
                })
            };
            let open_pip = {
                let countdown_states = countdown_states.clone();
                let pip_document = pip_document.clone();
                let pip_index = pip_index.clone();
                let pip_notice = pip_notice.clone();
                let pip_notice_seq = pip_notice_seq.clone();
                let prepared = prepared.clone();
                Callback::from(move |_| {
                    let state = (*countdown_states)[index];
                    if state.timer.is_none() {
                        pip_notice.set(Some("请先开始倒计时"));
                        return;
                    }
                    handle_open_pip(
                        index,
                        custom_pip_config(target, state, Date::now() as i64),
                        &pip_document,
                        &pip_index,
                        &pip_notice,
                        &pip_notice_seq,
                        &prepared,
                    );
                })
            };

            let active = state.timer.is_some();
            let can_pause = state.timer.is_some_and(|timer| !timer.paused) && !snapshot.finished;
            let can_resume = state.timer.is_some_and(|timer| timer.paused) && !snapshot.finished;
            let card_label = if snapshot.finished && active {
                format!("{}倒计时已结束", target.title)
            } else if state.timer.is_some_and(|timer| timer.paused) {
                format!(
                    "{}倒计时已暂停，剩余 {} 秒",
                    target.title, snapshot.remaining_seconds
                )
            } else if active {
                format!(
                    "{}倒计时，剩余 {} 秒",
                    target.title, snapshot.remaining_seconds
                )
            } else {
                format!("{}倒计时，待开始", target.title)
            };
            let start_label = if active {
                format!("重新开始{}倒计时", target.title)
            } else {
                format!("开始{}倒计时", target.title)
            };

            html! {
                <CountdownEditor
                    id={target.id}
                    title={target.title}
                    display={countdown_display(snapshot)}
                    status={status}
                    tone={classes!(tone)}
                    hours={hours}
                    minutes={minutes}
                    seconds={seconds}
                    active={active}
                    can_pause={can_pause}
                    can_resume={can_resume}
                    card_label={card_label}
                    start_label={start_label}
                    on_hours={set_hours}
                    on_minutes={set_minutes}
                    on_seconds={set_seconds}
                    on_restart={restart}
                    on_pause={pause}
                    on_resume={resume}
                    on_open_pip={open_pip}
                />
            }
        })
        .collect::<Vec<_>>();

    html! {
        <section id="custom-countdown" class={EXAM_SECTION} role="region" aria-labelledby="custom-countdown-title">
            <span class={EXAM_DECORATION} aria-hidden="true"></span>
            <h2 id="custom-countdown-title" class={SECTION_TITLE}>{"自定义倒计时"}</h2>
            if let Some(message) = *pip_notice {
                <p class={EXAM_PIP_NOTICE} data-custom-countdown-notice="" role="status" aria-live="polite">{message}</p>
            }
            <div class={CUSTOM_TIMER_GRID}>
                {for timer_cards}
            </div>
        </section>
    }
}

#[function_component(ExamCountdownPanel)]
pub(crate) fn exam_countdown_panel() -> Html {
    let now_ms = use_state(|| Date::now() as i64);
    let pip_document = use_mut_ref(|| None::<DocumentPipHandles>);
    let pip_exam = use_state(|| None::<usize>);
    let pip_notice = use_state(|| None::<&'static str>);
    let pip_notice_seq = use_mut_ref(|| 0u32);
    let prepared = use_mut_ref(|| None::<Vec<PreparedVideoPip>>);

    {
        let now_ms = now_ms.clone();
        use_effect_with((), move |_| {
            let cancelled = Rc::new(Cell::new(false));
            let task_cancelled = cancelled.clone();
            spawn_local(async move {
                while !task_cancelled.get() {
                    TimeoutFuture::new(EXAM_COUNTDOWN_TICK_MS).await;
                    if !task_cancelled.get() {
                        now_ms.set(Date::now() as i64);
                    }
                }
            });
            move || cancelled.set(true)
        });
    }

    // 预创建并持续播放所有卡片的隐藏视频流，保证双击手势内视频已就绪
    // （iOS 画中画要求正在播放 + WebKit 要求手势内同步请求，二者缺一不可）。
    {
        let prepared = prepared.clone();
        use_effect_with((), move |_| {
            *prepared.borrow_mut() = prepare_exam_pip_videos(&EXAM_COUNTDOWN_TARGETS);
            move || {
                if let Some(list) = prepared.borrow_mut().as_mut() {
                    teardown_prepared_videos(list);
                }
            }
        });
    }

    // Esc 退出 video 流画中画（Document PiP 窗口自己有 Esc 处理）。
    {
        let prepared = prepared.clone();
        use_effect_with((), move |_| {
            let listener =
                Closure::<dyn FnMut(KeyboardEvent)>::new(move |key_event: KeyboardEvent| {
                    if !is_escape_key(&key_event) {
                        return;
                    }
                    if let Some(list) = prepared.borrow().as_ref() {
                        for item in list {
                            if picture_in_picture_active(&item.video) {
                                let _ = picture_in_picture_exit(&item.video);
                            }
                        }
                    }
                });
            let document = web_sys::window().and_then(|window| window.document());
            if let Some(document) = document.as_ref() {
                let _ = document
                    .add_event_listener_with_callback("keydown", listener.as_ref().unchecked_ref());
            }
            move || {
                if let Some(document) = document.as_ref() {
                    let _ = document.remove_event_listener_with_callback(
                        "keydown",
                        listener.as_ref().unchecked_ref(),
                    );
                }
                drop(listener);
            }
        });
    }

    html! {
        <section id="exam-countdown" class={EXAM_SECTION} role="region" aria-labelledby="exam-countdown-title">
            <span class={EXAM_DECORATION} aria-hidden="true"></span>
            <div class={EXAM_HEAD}>
                <div>
                    <p class={EYEBROW}>{"EXAM COUNTDOWN"}</p>
                    <h2 id="exam-countdown-title" class={SECTION_TITLE}>{"考试冲刺倒计时"}</h2>
                </div>
                <div class={EXAM_HEAD_META}>
                    <span class={EXAM_BADGE}><span aria-hidden="true">{"!"}</span>{"按时间先后排序"}</span>
                    <span>{"双击卡片开启画中画 · 预计日期以官方公告为准"}</span>
                </div>
            </div>
            if let Some(message) = *pip_notice {
                <p class={EXAM_PIP_NOTICE} data-exam-countdown-notice="" role="status">{message}</p>
            }
            <div class={EXAM_GRID}>
                {for EXAM_COUNTDOWN_TARGETS.iter().enumerate().map(|(index, target)| {
                    let pip_document = pip_document.clone();
                    let pip_exam = pip_exam.clone();
                    let pip_notice = pip_notice.clone();
                    let pip_notice_seq = pip_notice_seq.clone();
                    let prepared = prepared.clone();
                    let open_pip = Callback::from(move |event: MouseEvent| {
                        event.prevent_default();
                        handle_open_pip(
                            index,
                            exam_pip_config(target, Date::now() as i64),
                            &pip_document,
                            &pip_exam,
                            &pip_notice,
                            &pip_notice_seq,
                            &prepared,
                        );
                    });
                    let snapshot = countdown_snapshot(
                        *now_ms,
                        exam_timestamp(target.start_iso),
                        exam_timestamp(target.target_iso),
                    );
                    render_exam_countdown_card(target, snapshot, index, open_pip)
                })}
            </div>
        </section>
    }
}
