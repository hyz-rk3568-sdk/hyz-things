use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CameraViewPhase {
    Idle,
    Starting,
    Connecting,
    Playing,
}

impl CameraViewPhase {
    const fn label(self) -> &'static str {
        match self {
            Self::Idle => "未播放",
            Self::Starting => "正在协商",
            Self::Connecting => "正在连接",
            Self::Playing => "直播中",
        }
    }
}

pub(super) struct CameraSessionRuntime {
    peer: RtcPeerConnection,
    _on_track: Closure<dyn FnMut(RtcTrackEvent)>,
    _on_connection_state_change: Closure<dyn FnMut(Event)>,
    session_id: Option<String>,
    csrf: String,
    /// audio transceiver 的发送端：对讲开关通过 `replace_track` 挂载/摘下麦克风，
    /// 不重协商。
    audio_sender: Option<RtcRtpSender>,
    mic_track: Option<MediaStreamTrack>,
}

pub(super) type CameraRuntime = Rc<RefCell<Option<CameraSessionRuntime>>>;

#[derive(Properties, PartialEq)]
pub(super) struct CameraLiveViewProps {
    /// Panel bootstrap CSRF token, used only for administrator mutations
    /// (profile/rotation) while an administrator session is present.
    pub(super) admin_csrf: String,
    /// Whether an administrator session is currently authenticated without a
    /// forced password change. Anonymous viewers must not use the (public)
    /// panel CSRF as their session token; they get a short-lived viewer token.
    pub(super) is_admin: bool,
    pub(super) stop_generation: u32,
}

pub(super) fn camera_pipeline_label(state: CameraPipelineState) -> &'static str {
    match state {
        CameraPipelineState::Stopped => "已停止",
        CameraPipelineState::Starting => "启动中",
        CameraPipelineState::Streaming => "推流中",
        CameraPipelineState::Stopping => "停止中",
        CameraPipelineState::Failed => "故障",
        CameraPipelineState::Unknown => "未知",
    }
}

pub(super) fn camera_access_label(access: CameraAccessKind) -> &'static str {
    match access {
        CameraAccessKind::Lan => "LAN",
        CameraAccessKind::Tailscale => "Tailscale",
    }
}

pub(super) fn camera_error_label(error: CameraErrorCategory) -> &'static str {
    match error {
        CameraErrorCategory::CameraNotFound => "未找到摄像头",
        CameraErrorCategory::CameraBusy => "摄像头占用中",
        CameraErrorCategory::MediaPipelineFailed => "媒体管线故障",
        CameraErrorCategory::EncoderUnavailable => "编码器不可用",
        CameraErrorCategory::ControlUnavailable => "控制服务不可用",
        CameraErrorCategory::WebrtcNegotiationFailed => "WebRTC 协商失败",
        CameraErrorCategory::WebrtcTransportFailed => "WebRTC 传输失败",
        CameraErrorCategory::ResourceExhausted => "资源不足",
        CameraErrorCategory::Unknown => "未知故障",
    }
}

pub(super) fn icon_play() -> Html {
    html! {
        <svg class={CAMERA_ICON} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M8 5.14v13.72a1 1 0 0 0 1.53.85l10.29-6.86a1 1 0 0 0 0-1.66L9.53 4.29A1 1 0 0 0 8 5.14Z" />
        </svg>
    }
}

pub(super) fn icon_pause() -> Html {
    html! {
        <svg class={CAMERA_ICON} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <rect x="6" y="5" width="4" height="14" rx="1" />
            <rect x="14" y="5" width="4" height="14" rx="1" />
        </svg>
    }
}

pub(super) fn icon_stop() -> Html {
    html! {
        <svg class={CAMERA_ICON} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <rect x="5" y="5" width="14" height="14" rx="2" />
        </svg>
    }
}

pub(super) fn icon_microphone(enabled: bool) -> Html {
    html! {
        <svg class={CAMERA_ICON} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="9" y="3" width="6" height="11" rx="3" />
            <path d="M5 11a7 7 0 0 0 14 0" />
            <path d="M12 18v3" />
            <path d="M8 21h8" />
            if !enabled {
                <path d="m4 4 16 16" />
            }
        </svg>
    }
}

pub(super) fn icon_picture_in_picture() -> Html {
    html! {
        <svg class={CAMERA_ICON} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="3" y="5" width="18" height="14" rx="2" />
            <path d="M13 13h6v4h-6z" />
        </svg>
    }
}

pub(super) fn icon_rotate() -> Html {
    html! {
        <svg class={CAMERA_ICON} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M20 11a8 8 0 0 0-14.9-3.9L3 10" />
            <path d="M3 5v5h5" />
            <path d="M4 13a8 8 0 0 0 14.9 3.9L21 14" />
            <path d="M21 19v-5h-5" />
        </svg>
    }
}

pub(super) struct MediaSessionRegistration {
    session: JsValue,
    _handlers: Vec<Closure<dyn FnMut(JsValue)>>,
}

pub(super) fn browser_media_session() -> Option<JsValue> {
    let window = web_sys::window()?;
    let navigator: JsValue = window.navigator().into();
    Reflect::get(&navigator, &JsValue::from_str("mediaSession"))
        .ok()
        .filter(|value| !value.is_null() && !value.is_undefined())
}

pub(super) fn media_session_set_action_handler(
    session: &JsValue,
    action: &str,
    handler: Option<&JsValue>,
) {
    let Ok(value) = Reflect::get(session, &JsValue::from_str("setActionHandler")) else {
        return;
    };
    let Ok(method) = value.dyn_into::<Function>() else {
        return;
    };
    let callback = handler.cloned().unwrap_or(JsValue::NULL);
    let _ = method.call2(session, &JsValue::from_str(action), &callback);
}

pub(super) fn clear_media_session(session: &JsValue) {
    let _ = Reflect::set(session, &JsValue::from_str("metadata"), &JsValue::NULL);
    let _ = Reflect::set(
        session,
        &JsValue::from_str("playbackState"),
        &JsValue::from_str("none"),
    );
    for action in ["play", "pause", "stop"] {
        media_session_set_action_handler(session, action, None);
    }
}

pub(super) fn media_metadata_value() -> JsValue {
    let init = js_sys::Object::new();
    let _ = Reflect::set(
        &init,
        &JsValue::from_str("title"),
        &JsValue::from_str("摄像头直播"),
    );
    let _ = Reflect::set(
        &init,
        &JsValue::from_str("artist"),
        &JsValue::from_str("hyz things"),
    );

    let constructed = web_sys::window()
        .and_then(|window| Reflect::get(window.as_ref(), &JsValue::from_str("MediaMetadata")).ok())
        .and_then(|value| value.dyn_into::<Function>().ok())
        .and_then(|constructor| Reflect::construct(&constructor, &js_sys::Array::of1(&init)).ok());
    constructed.unwrap_or_else(|| init.into())
}

pub(super) fn install_media_session(
    paused: bool,
    on_play: Rc<dyn Fn()>,
    on_pause: Rc<dyn Fn()>,
    on_stop: Rc<dyn Fn()>,
) -> Option<MediaSessionRegistration> {
    let session = browser_media_session()?;
    let metadata = media_metadata_value();
    let _ = Reflect::set(&session, &JsValue::from_str("metadata"), &metadata);
    let _ = Reflect::set(
        &session,
        &JsValue::from_str("playbackState"),
        &JsValue::from_str(if paused { "paused" } else { "playing" }),
    );

    let play_handler = {
        let on_play = on_play.clone();
        Closure::<dyn FnMut(JsValue)>::new(move |_| {
            if paused {
                on_play();
            }
        })
    };
    let pause_handler = {
        let on_pause = on_pause.clone();
        Closure::<dyn FnMut(JsValue)>::new(move |_| {
            if !paused {
                on_pause();
            }
        })
    };
    let stop_handler = Closure::<dyn FnMut(JsValue)>::new(move |_| on_stop());
    let handlers = vec![play_handler, pause_handler, stop_handler];
    for (action, handler) in ["play", "pause", "stop"].into_iter().zip(handlers.iter()) {
        media_session_set_action_handler(&session, action, Some(handler.as_ref()));
    }

    Some(MediaSessionRegistration {
        session,
        _handlers: handlers,
    })
}

pub(super) fn picture_in_picture_method(target: &JsValue, name: &str) -> Option<Function> {
    Reflect::get(target, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.dyn_into::<Function>().ok())
}

pub(super) fn picture_in_picture_supported(video: &HtmlVideoElement) -> bool {
    let target: JsValue = video.clone().into();
    if picture_in_picture_method(&target, "requestPictureInPicture").is_some() {
        return true;
    }
    picture_in_picture_method(&target, "webkitSupportsPresentationMode")
        .and_then(|method| {
            method
                .call1(&target, &JsValue::from_str("picture-in-picture"))
                .ok()
        })
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

pub(super) fn picture_in_picture_ready(video: &HtmlVideoElement) -> bool {
    // HAVE_FUTURE_DATA：视频至少已经有可播放的媒体数据，避免在 WebRTC 首帧到达前调用 PiP。
    video.ready_state() >= 3
}

pub(super) fn picture_in_picture_active(video: &HtmlVideoElement) -> bool {
    let target: JsValue = video.clone().into();
    let standard_active = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| {
            Reflect::get(
                document.as_ref(),
                &JsValue::from_str("pictureInPictureElement"),
            )
            .ok()
        })
        .is_some_and(|element| element == target);
    let webkit_active = Reflect::get(&target, &JsValue::from_str("webkitPresentationMode"))
        .ok()
        .and_then(|value| value.as_string())
        .is_some_and(|mode| mode == "picture-in-picture");
    standard_active || webkit_active
}

pub(super) fn picture_in_picture_request(video: &HtmlVideoElement) -> Result<JsValue, JsValue> {
    let target: JsValue = video.clone().into();
    if let Some(method) = picture_in_picture_method(&target, "requestPictureInPicture") {
        return method.call0(&target);
    }
    if let Some(method) = picture_in_picture_method(&target, "webkitSetPresentationMode") {
        method.call1(&target, &JsValue::from_str("picture-in-picture"))?;
        return Ok(JsValue::UNDEFINED);
    }
    Err(JsValue::from_str("浏览器不支持画中画"))
}

pub(super) fn picture_in_picture_exit(video: &HtmlVideoElement) -> Result<JsValue, JsValue> {
    let target: JsValue = video.clone().into();
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        let document_value: JsValue = document.into();
        if picture_in_picture_method(&document_value, "exitPictureInPicture").is_some() {
            if let Some(method) = picture_in_picture_method(&document_value, "exitPictureInPicture")
            {
                return method.call0(&document_value);
            }
        }
    }
    if let Some(method) = picture_in_picture_method(&target, "webkitSetPresentationMode") {
        method.call1(&target, &JsValue::from_str("inline"))?;
        return Ok(JsValue::UNDEFINED);
    }
    Err(JsValue::from_str("无法退出画中画"))
}

pub(super) async fn await_picture_in_picture(value: JsValue) -> Result<(), JsValue> {
    if value.is_undefined() || value.is_null() {
        return Ok(());
    }
    let promise: Promise = value.dyn_into()?;
    JsFuture::from(promise).await.map(|_| ())
}

pub(super) fn camera_js_error(context: &str, error: JsValue) -> String {
    let detail = error.as_string().unwrap_or_else(|| format!("{error:?}"));
    format!("{context}：{detail}")
}

/// 播放/暂停竞争（清理时 pause 打断 play）会让 play() 的 Promise 拒绝；await 并
/// 忽略其结果，避免未处理拒绝变成 pageerror。
pub(super) fn play_media_ignoring_interruption(element: &HtmlMediaElement) {
    if let Ok(promise) = element.play() {
        spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

pub(super) fn next_camera_generation(generation: &Rc<RefCell<u64>>) -> u64 {
    let mut current = generation.borrow_mut();
    *current = current.wrapping_add(1);
    *current
}

pub(super) fn camera_generation_is_current(generation: &Rc<RefCell<u64>>, expected: u64) -> bool {
    *generation.borrow() == expected
}

pub(super) async fn close_camera_session(session_id: String, csrf: String) {
    let _ = post_json(
        CAMERA_SESSION_CLOSE_ENDPOINT,
        &csrf,
        &CameraSessionCloseRequestDto { session_id },
        "摄像头会话关闭",
    )
    .await;
}

pub(super) async fn fetch_camera_viewer_token() -> Result<String, String> {
    let response = Request::post(CAMERA_VIEWER_TOKEN_ENDPOINT)
        .credentials(RequestCredentials::SameOrigin)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|error| format!("无法获取观看凭证：{error}"))?;
    if !response.ok() {
        return Err(format!("观看凭证接口返回 HTTP {}", response.status()));
    }
    response
        .json::<CameraViewerTokenDto>()
        .await
        .map(|dto| dto.token)
        .map_err(|error| format!("观看凭证数据格式无效：{error}"))
}

pub(super) fn take_camera_runtime(
    runtime: &CameraRuntime,
    video: &NodeRef,
    audio: &NodeRef,
) -> Option<(String, String)> {
    let session = runtime.borrow_mut().take().and_then(|mut active| {
        // 释放麦克风：先摘下 track 再停止，避免残留发送。
        if let Some(sender) = &active.audio_sender {
            let _ = sender.replace_track(None);
        }
        if let Some(track) = active.mic_track.take() {
            track.stop();
        }
        active.peer.set_ontrack(None);
        active.peer.set_onconnectionstatechange(None);
        active.peer.close();
        active
            .session_id
            .map(|session_id| (session_id, active.csrf))
    });
    if let Some(video) = video.cast::<HtmlVideoElement>() {
        let _ = video.pause();
        video.set_src_object(None);
        video.load();
    }
    if let Some(audio) = audio.cast::<HtmlMediaElement>() {
        let _ = audio.pause();
        audio.set_src_object(None);
        audio.load();
    }
    session
}

pub(super) fn close_camera_runtime(runtime: &CameraRuntime, video: &NodeRef, audio: &NodeRef) {
    if let Some((session_id, csrf)) = take_camera_runtime(runtime, video, audio) {
        spawn_local(close_camera_session(session_id, csrf));
    }
}

/// 取消页面隐藏时的延迟关闭计时（页面恢复可见 / 页面卸载 / 会话被其他路径
/// 关闭时调用）。句柄取出即视为取消，即使 window 不可用也清掉本地状态。
pub(super) fn cancel_hidden_close_timer(
    timer: &Rc<RefCell<Option<i32>>>,
    window: Option<&web_sys::Window>,
) {
    if let Some(handle) = timer.borrow_mut().take() {
        if let Some(window) = window {
            window.clear_timeout_with_handle(handle);
        }
    }
}

pub(super) async fn wait_for_camera_ice(peer: &RtcPeerConnection) -> Result<(), String> {
    let mut waited = 0;
    while peer.ice_gathering_state() != RtcIceGatheringState::Complete {
        if waited >= CAMERA_ICE_GATHER_TIMEOUT_MS {
            return Err("ICE 候选收集超时".to_owned());
        }
        TimeoutFuture::new(CAMERA_ICE_POLL_MS).await;
        waited += CAMERA_ICE_POLL_MS;
    }
    Ok(())
}

pub(super) async fn refresh_camera_status(
    status: UseStateHandle<Option<CameraStatus>>,
    presets: UseStateHandle<Vec<CameraStreamPreset>>,
    error: UseStateHandle<Option<String>>,
) {
    match fetch_json::<CameraStatusResponseDto>(CAMERA_STATUS_ENDPOINT, "摄像头状态").await {
        Ok(response) => {
            status.set(Some(response.camera));
            presets.set(response.available_presets);
            error.set(None);
        }
        Err(message) => {
            status.set(None);
            presets.set(Vec::new());
            error.set(Some(message));
        }
    }
}

#[function_component(CameraLiveView)]
pub(super) fn camera_live_view(props: &CameraLiveViewProps) -> Html {
    let video = use_node_ref();
    let audio = use_node_ref();
    let stage = use_node_ref();
    let status = use_state(|| None::<CameraStatus>);
    let presets = use_state(Vec::<CameraStreamPreset>::new);
    let status_error = use_state(|| None::<String>);
    let notice = use_state(|| None::<String>);
    let phase = use_state(|| CameraViewPhase::Idle);
    let paused = use_state(|| false);
    let talk_active = use_state(|| false);
    let pip_supported = use_state(|| false);
    let pip_ready = use_state(|| false);
    let pip_active = use_state(|| false);
    let runtime = use_mut_ref(|| None::<CameraSessionRuntime>);
    let generation = use_mut_ref(|| 0u64);
    // 页面隐藏时的延迟关闭计时器句柄（None 表示无待触发计时）。
    let hidden_timer = use_mut_ref(|| None::<i32>);
    let previous_stop_generation = use_mut_ref(|| props.stop_generation);
    // 乐观旋转状态：点击后立即推进 0 → 270 → 180 → 90 → 0，不依赖 2s 轮询的
    // 延迟刷新，保证「旋转画面」每次点击都严格逆时针 90°。
    let rotation = use_mut_ref(|| None::<CameraRotation>);

    {
        let video = video.clone();
        let pip_supported = pip_supported.clone();
        let pip_ready = pip_ready.clone();
        let pip_active = pip_active.clone();
        use_effect_with((), move |_| {
            let video_element = video.cast::<HtmlVideoElement>();
            let supported = video_element
                .as_ref()
                .is_some_and(picture_in_picture_supported);
            pip_supported.set(supported);
            pip_ready
                .set(supported && video_element.as_ref().is_some_and(picture_in_picture_ready));
            let entered = {
                let pip_active = pip_active.clone();
                Closure::<dyn FnMut(Event)>::new(move |_| pip_active.set(true))
            };
            let left = {
                let pip_active = pip_active.clone();
                Closure::<dyn FnMut(Event)>::new(move |_| pip_active.set(false))
            };
            let ready = {
                let pip_ready = pip_ready.clone();
                let video_element = video_element.clone();
                Closure::<dyn FnMut(Event)>::new(move |_| {
                    pip_ready.set(video_element.as_ref().is_some_and(picture_in_picture_ready));
                })
            };
            let reset = {
                let pip_ready = pip_ready.clone();
                Closure::<dyn FnMut(Event)>::new(move |_| pip_ready.set(false))
            };
            if let Some(video_element) = video_element.as_ref() {
                let _ = video_element.add_event_listener_with_callback(
                    "enterpictureinpicture",
                    entered.as_ref().unchecked_ref(),
                );
                let _ = video_element.add_event_listener_with_callback(
                    "leavepictureinpicture",
                    left.as_ref().unchecked_ref(),
                );
                let _ = video_element.add_event_listener_with_callback(
                    "webkitpresentationmodechanged",
                    left.as_ref().unchecked_ref(),
                );
                for event_name in ["loadedmetadata", "canplay", "playing"] {
                    let _ = video_element.add_event_listener_with_callback(
                        event_name,
                        ready.as_ref().unchecked_ref(),
                    );
                }
                for event_name in ["loadstart", "emptied"] {
                    let _ = video_element.add_event_listener_with_callback(
                        event_name,
                        reset.as_ref().unchecked_ref(),
                    );
                }
            }
            move || {
                if let Some(video_element) = video_element.as_ref() {
                    let _ = video_element.remove_event_listener_with_callback(
                        "enterpictureinpicture",
                        entered.as_ref().unchecked_ref(),
                    );
                    let _ = video_element.remove_event_listener_with_callback(
                        "leavepictureinpicture",
                        left.as_ref().unchecked_ref(),
                    );
                    let _ = video_element.remove_event_listener_with_callback(
                        "webkitpresentationmodechanged",
                        left.as_ref().unchecked_ref(),
                    );
                    for event_name in ["loadedmetadata", "canplay", "playing"] {
                        let _ = video_element.remove_event_listener_with_callback(
                            event_name,
                            ready.as_ref().unchecked_ref(),
                        );
                    }
                    for event_name in ["loadstart", "emptied"] {
                        let _ = video_element.remove_event_listener_with_callback(
                            event_name,
                            reset.as_ref().unchecked_ref(),
                        );
                    }
                }
                pip_ready.set(false);
            }
        });
    }

    {
        let status = status.clone();
        let rotation = rotation.clone();
        use_effect_with(status.clone(), move |_| {
            if rotation.borrow().is_none() {
                *rotation.borrow_mut() = status.as_ref().map(|camera| camera.profile.rotation);
            }
            || ()
        });
    }

    {
        let status = status.clone();
        let presets = presets.clone();
        let status_error = status_error.clone();
        use_effect_with((), move |_| {
            let cancelled = Rc::new(Cell::new(false));
            let task_cancelled = cancelled.clone();
            spawn_local(async move {
                while !task_cancelled.get() {
                    refresh_camera_status(status.clone(), presets.clone(), status_error.clone())
                        .await;
                    TimeoutFuture::new(POLL_DELAY_MS).await;
                }
            });
            move || cancelled.set(true)
        });
    }

    let viewer_token = use_state(|| None::<String>);
    let viewer_token_error = use_state(|| None::<String>);
    {
        let viewer_token = viewer_token.clone();
        let viewer_token_error = viewer_token_error.clone();
        use_effect_with(props.is_admin, move |is_admin| {
            viewer_token.set(None);
            viewer_token_error.set(None);
            if !is_admin {
                spawn_local(async move {
                    match fetch_camera_viewer_token().await {
                        Ok(token) => viewer_token.set(Some(token)),
                        Err(error) => viewer_token_error.set(Some(error)),
                    }
                });
            }
            || ()
        });
    }

    let session_token = if props.is_admin {
        props.admin_csrf.clone()
    } else {
        viewer_token.as_deref().unwrap_or("").to_owned()
    };
    let can_view = !session_token.is_empty();
    let can_control = props.is_admin && !props.admin_csrf.is_empty();

    {
        let runtime = runtime.clone();
        let video = video.clone();
        let audio = audio.clone();
        let phase = phase.clone();
        let paused = paused.clone();
        let notice = notice.clone();
        let generation = generation.clone();
        let hidden_timer = hidden_timer.clone();
        let previous = previous_stop_generation.clone();
        use_effect_with(props.stop_generation, move |requested| {
            let changed = *previous.borrow() != *requested;
            *previous.borrow_mut() = *requested;
            if changed {
                cancel_hidden_close_timer(&hidden_timer, web_sys::window().as_ref());
                next_camera_generation(&generation);
                close_camera_runtime(&runtime, &video, &audio);
                phase.set(CameraViewPhase::Idle);
                paused.set(false);
                notice.set(Some("摄像头直播已在退出登录前停止".to_owned()));
            }
            || ()
        });
    }

    {
        let runtime = runtime.clone();
        let video = video.clone();
        let audio = audio.clone();
        let phase = phase.clone();
        let notice = notice.clone();
        let generation = generation.clone();
        let hidden_timer = hidden_timer.clone();
        use_effect_with((), move |_| {
            let window = web_sys::window();
            let document = window.as_ref().and_then(|window| window.document());

            let pagehide_runtime = runtime.clone();
            let pagehide_video = video.clone();
            let pagehide_audio = audio.clone();
            let pagehide_generation = generation.clone();
            let pagehide_timer = hidden_timer.clone();
            let pagehide_window = window.clone();
            let pagehide = Closure::<dyn FnMut(Event)>::new(move |_| {
                cancel_hidden_close_timer(&pagehide_timer, pagehide_window.as_ref());
                next_camera_generation(&pagehide_generation);
                close_camera_runtime(&pagehide_runtime, &pagehide_video, &pagehide_audio);
            });

            let hidden_runtime = runtime.clone();
            let hidden_video = video.clone();
            let hidden_audio = audio.clone();
            let hidden_phase = phase.clone();
            let hidden_notice = notice.clone();
            let hidden_generation = generation.clone();
            let watched_video = video.clone();
            let hidden_timer = hidden_timer.clone();
            let cleanup_timer = hidden_timer.clone();
            let hidden_window = window.clone();
            let watched_document = document.clone();
            let visibility = Closure::<dyn FnMut(Event)>::new(move |_| {
                let Some(document) = watched_document.as_ref() else {
                    return;
                };
                if document.hidden() {
                    if watched_video
                        .cast::<HtmlVideoElement>()
                        .is_some_and(|video| picture_in_picture_active(&video))
                    {
                        cancel_hidden_close_timer(&hidden_timer, hidden_window.as_ref());
                        return;
                    }
                    // 页面隐藏不立即关闭会话：启动宽限期计时，期间回来就取消
                    // （取消走下方 else 分支），超时才真正关闭。已有计时器在
                    // 跑就不重复启动，保证宽限窗口从「最近一次可见」起算。
                    if hidden_timer.borrow().is_none() {
                        let timer_runtime = hidden_runtime.clone();
                        let timer_video = hidden_video.clone();
                        let timer_audio = hidden_audio.clone();
                        let timer_phase = hidden_phase.clone();
                        let timer_notice = hidden_notice.clone();
                        let timer_generation = hidden_generation.clone();
                        let timer_handle = hidden_timer.clone();
                        let timer_document = watched_document.clone();
                        let callback = Closure::once(move || {
                            *timer_handle.borrow_mut() = None;
                            // 计时触发时页面已恢复可见（回来与超时撞车）则不再关闭。
                            if timer_document
                                .as_ref()
                                .is_some_and(|document| !document.hidden())
                            {
                                return;
                            }
                            next_camera_generation(&timer_generation);
                            close_camera_runtime(&timer_runtime, &timer_video, &timer_audio);
                            timer_phase.set(CameraViewPhase::Idle);
                            timer_notice
                                .set(Some("页面离开超过 1 分钟，已停止摄像头直播".to_owned()));
                        });
                        if let Some(window) = hidden_window.as_ref() {
                            if let Ok(handle) = window
                                .set_timeout_with_callback_and_timeout_and_arguments_0(
                                    callback.as_ref().unchecked_ref(),
                                    CAMERA_HIDDEN_CLOSE_GRACE_MS as i32,
                                )
                            {
                                *hidden_timer.borrow_mut() = Some(handle);
                                callback.forget();
                            }
                        }
                    }
                } else {
                    cancel_hidden_close_timer(&hidden_timer, hidden_window.as_ref());
                }
            });

            if let Some(window) = &window {
                let _ = window.add_event_listener_with_callback(
                    "pagehide",
                    pagehide.as_ref().unchecked_ref(),
                );
            }
            if let Some(document) = &document {
                let _ = document.add_event_listener_with_callback(
                    "visibilitychange",
                    visibility.as_ref().unchecked_ref(),
                );
            }

            move || {
                if let Some(window) = &window {
                    let _ = window.remove_event_listener_with_callback(
                        "pagehide",
                        pagehide.as_ref().unchecked_ref(),
                    );
                }
                if let Some(document) = &document {
                    let _ = document.remove_event_listener_with_callback(
                        "visibilitychange",
                        visibility.as_ref().unchecked_ref(),
                    );
                }
                cancel_hidden_close_timer(&cleanup_timer, window.as_ref());
                next_camera_generation(&generation);
                close_camera_runtime(&runtime, &video, &audio);
            }
        });
    }

    let start = {
        let csrf = session_token.clone();
        let video = video.clone();
        let audio = audio.clone();
        let status = status.clone();
        let phase = phase.clone();
        let paused = paused.clone();
        let notice = notice.clone();
        let runtime = runtime.clone();
        let generation = generation.clone();
        let talk_active = talk_active.clone();
        Callback::from(move |_| {
            if csrf.is_empty() || runtime.borrow().is_some() {
                return;
            }
            talk_active.set(false);
            paused.set(false);
            close_camera_runtime(&runtime, &video, &audio);
            let attempt = next_camera_generation(&generation);
            // 音频能力来自 status：不支持时 offer 不带 audio m-line，保持 video-only。
            let audio_supported = status
                .as_ref()
                .is_some_and(|camera| camera.audio.as_ref().is_some_and(|audio| audio.supported));
            phase.set(CameraViewPhase::Starting);
            notice.set(None);

            let csrf = csrf.clone();
            let video = video.clone();
            let audio = audio.clone();
            let phase = phase.clone();
            let notice = notice.clone();
            let runtime = runtime.clone();
            let generation = generation.clone();
            spawn_local(async move {
                let result = async {
                    let peer = RtcPeerConnection::new()
                        .map_err(|error| camera_js_error("无法创建 WebRTC 连接", error))?;
                    let transceiver = RtcRtpTransceiverInit::new();
                    transceiver.set_direction(RtcRtpTransceiverDirection::Recvonly);
                    peer.add_transceiver_with_str_and_init("video", &transceiver);
                    // 全双工对讲：audio transceiver 固定 sendrecv，麦克风通过
                    // replaceTrack 挂载/摘下，不重协商。offer 因此携带 audio m-line。
                    let mut audio_sender = None;
                    if audio_supported {
                        let audio_init = RtcRtpTransceiverInit::new();
                        audio_init.set_direction(RtcRtpTransceiverDirection::Sendrecv);
                        let transceiver =
                            peer.add_transceiver_with_str_and_init("audio", &audio_init);
                        audio_sender = Some(transceiver.sender());
                    }

                    let track_video = video.clone();
                    let track_audio = audio.clone();
                    let track_phase = phase.clone();
                    let track_notice = notice.clone();
                    let on_track =
                        Closure::<dyn FnMut(RtcTrackEvent)>::new(move |event: RtcTrackEvent| {
                            let stream = event
                                .streams()
                                .get(0)
                                .dyn_into::<MediaStream>()
                                .ok()
                                .or_else(|| {
                                    let stream = MediaStream::new().ok()?;
                                    stream.add_track(&event.track());
                                    Some(stream)
                                });
                            if let Some(stream) = stream {
                                if event.track().kind() == "audio" {
                                    // 设备麦克风音频走独立 <audio> 元素（<video> 保持 muted）。
                                    if let Some(audio) = track_audio.cast::<HtmlMediaElement>() {
                                        audio.set_src_object(Some(&stream));
                                        play_media_ignoring_interruption(&audio);
                                    }
                                } else if let Some(video) = track_video.cast::<HtmlVideoElement>() {
                                    video.set_src_object(Some(&stream));
                                    play_media_ignoring_interruption(&video);
                                }
                                track_phase.set(CameraViewPhase::Playing);
                                track_notice.set(None);
                            }
                        });
                    peer.set_ontrack(Some(on_track.as_ref().unchecked_ref()));

                    let connection_peer = peer.clone();
                    let connection_runtime = runtime.clone();
                    let connection_video = video.clone();
                    let connection_audio = audio.clone();
                    let connection_phase = phase.clone();
                    let connection_notice = notice.clone();
                    let connection_generation = generation.clone();
                    let on_connection_state_change = Closure::<dyn FnMut(Event)>::new(move |_| {
                        if matches!(
                            connection_peer.connection_state(),
                            RtcPeerConnectionState::Disconnected | RtcPeerConnectionState::Failed
                        ) {
                            next_camera_generation(&connection_generation);
                            close_camera_runtime(
                                &connection_runtime,
                                &connection_video,
                                &connection_audio,
                            );
                            connection_phase.set(CameraViewPhase::Idle);
                            connection_notice.set(Some("摄像头 WebRTC 连接已中断".to_owned()));
                        }
                    });
                    peer.set_onconnectionstatechange(Some(
                        on_connection_state_change.as_ref().unchecked_ref(),
                    ));
                    *runtime.borrow_mut() = Some(CameraSessionRuntime {
                        peer: peer.clone(),
                        _on_track: on_track,
                        _on_connection_state_change: on_connection_state_change,
                        session_id: None,
                        csrf: csrf.clone(),
                        audio_sender,
                        mic_track: None,
                    });

                    let offer_value = JsFuture::from(peer.create_offer())
                        .await
                        .map_err(|error| camera_js_error("无法创建 WebRTC offer", error))?;
                    let offer: RtcSessionDescriptionInit = offer_value.unchecked_into();
                    JsFuture::from(peer.set_local_description(&offer))
                        .await
                        .map_err(|error| camera_js_error("无法设置本地 WebRTC 描述", error))?;
                    if !camera_generation_is_current(&generation, attempt) {
                        return Ok::<_, String>(());
                    }
                    wait_for_camera_ice(&peer).await?;
                    if !camera_generation_is_current(&generation, attempt) {
                        return Ok::<_, String>(());
                    }
                    let offer_sdp = peer
                        .local_description()
                        .map(|description| description.sdp())
                        .filter(|sdp| !sdp.is_empty())
                        .ok_or_else(|| "浏览器没有生成可提交的 WebRTC offer".to_owned())?;
                    let response = post_json_response::<_, CameraSessionCreateResponseDto>(
                        CAMERA_SESSION_CREATE_ENDPOINT,
                        &csrf,
                        &CameraSessionCreateRequestDto { offer_sdp },
                        "摄像头会话创建",
                    )
                    .await?;

                    if !camera_generation_is_current(&generation, attempt) {
                        spawn_local(close_camera_session(response.session_id, csrf.clone()));
                        return Ok::<_, String>(());
                    }
                    if let Some(active) = runtime.borrow_mut().as_mut() {
                        active.session_id = Some(response.session_id);
                    }

                    if !camera_generation_is_current(&generation, attempt) {
                        return Ok::<_, String>(());
                    }
                    phase.set(CameraViewPhase::Connecting);
                    let answer = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                    answer.set_sdp(&response.answer_sdp);
                    JsFuture::from(peer.set_remote_description(&answer))
                        .await
                        .map_err(|error| camera_js_error("无法设置摄像头 WebRTC answer", error))?;
                    if !camera_generation_is_current(&generation, attempt) {
                        return Ok::<_, String>(());
                    }

                    let timeout_ms =
                        u32::from(response.negotiation_timeout_seconds).saturating_mul(1_000);
                    let timeout_phase = phase.clone();
                    let timeout_notice = notice.clone();
                    let timeout_runtime = runtime.clone();
                    let timeout_video = video.clone();
                    let timeout_audio = audio.clone();
                    let timeout_generation = generation.clone();
                    spawn_local(async move {
                        TimeoutFuture::new(timeout_ms).await;
                        if camera_generation_is_current(&timeout_generation, attempt)
                            && *timeout_phase == CameraViewPhase::Connecting
                        {
                            next_camera_generation(&timeout_generation);
                            close_camera_runtime(&timeout_runtime, &timeout_video, &timeout_audio);
                            timeout_phase.set(CameraViewPhase::Idle);
                            timeout_notice.set(Some("摄像头 WebRTC 协商超时".to_owned()));
                        }
                    });
                    Ok(())
                }
                .await;

                if let Err(error) = result {
                    if camera_generation_is_current(&generation, attempt) {
                        next_camera_generation(&generation);
                        close_camera_runtime(&runtime, &video, &audio);
                        phase.set(CameraViewPhase::Idle);
                        notice.set(Some(format!("播放失败：{error}")));
                    }
                }
            });
        })
    };

    let stop_action: Rc<dyn Fn()> = {
        let runtime = runtime.clone();
        let video = video.clone();
        let audio = audio.clone();
        let phase = phase.clone();
        let paused = paused.clone();
        let notice = notice.clone();
        let generation = generation.clone();
        Rc::new(move || {
            next_camera_generation(&generation);
            close_camera_runtime(&runtime, &video, &audio);
            phase.set(CameraViewPhase::Idle);
            paused.set(false);
            notice.set(Some("摄像头直播已停止".to_owned()));
        })
    };
    let stop = {
        let stop_action = stop_action.clone();
        Callback::from(move |_| stop_action())
    };

    let rotate = {
        let csrf = props.admin_csrf.clone();
        let status = status.clone();
        let notice = notice.clone();
        let phase = phase.clone();
        let runtime = runtime.clone();
        let video = video.clone();
        let audio = audio.clone();
        let generation = generation.clone();
        let start = start.clone();
        let rotation = rotation.clone();
        Callback::from(move |_| {
            let current = rotation
                .borrow()
                .or_else(|| status.as_ref().map(|camera| camera.profile.rotation));
            let Some(current) = current else {
                return;
            };
            let next = current.next_rotation();
            *rotation.borrow_mut() = Some(next);
            let was_playing = *phase == CameraViewPhase::Playing;
            let pending_close = take_camera_runtime(&runtime, &video, &audio);
            if was_playing {
                phase.set(CameraViewPhase::Idle);
            }
            next_camera_generation(&generation);
            let csrf = csrf.clone();
            let notice = notice.clone();
            let phase = phase.clone();
            let start = start.clone();
            let rotation = rotation.clone();
            spawn_local(async move {
                if let Some((session_id, token)) = pending_close {
                    close_camera_session(session_id, token).await;
                }
                let mut applied = false;
                let mut failure = String::new();
                for attempt in 0..=CAMERA_UPDATE_RETRY_ATTEMPTS {
                    match post_json_response::<_, CameraRotationUpdateResponseDto>(
                        CAMERA_ROTATION_UPDATE_ENDPOINT,
                        &csrf,
                        &CameraRotationUpdateRequestDto { rotation: next },
                        "摄像头画面设置",
                    )
                    .await
                    {
                        Ok(response) if response.applied => {
                            applied = true;
                            break;
                        }
                        Ok(_) => failure = "画面旋转未生效".to_owned(),
                        Err(message) => failure = message,
                    }
                    if attempt < CAMERA_UPDATE_RETRY_ATTEMPTS {
                        TimeoutFuture::new(CAMERA_UPDATE_RETRY_DELAY_MS).await;
                    }
                }
                if applied {
                    notice.set(Some(format!("画面已旋转：{}°", next.degrees())));
                    if was_playing {
                        phase.set(CameraViewPhase::Idle);
                        start.emit(());
                    }
                } else {
                    *rotation.borrow_mut() = None;
                    notice.set(Some(failure));
                }
            });
        })
    };

    let set_profile = {
        let csrf = props.admin_csrf.clone();
        let notice = notice.clone();
        let phase = phase.clone();
        let runtime = runtime.clone();
        let video = video.clone();
        let audio = audio.clone();
        let generation = generation.clone();
        let start = start.clone();
        Callback::from(move |event: Event| {
            let select: HtmlSelectElement = event.target_unchecked_into();
            let Some(preset) = CameraStreamPreset::ALL
                .into_iter()
                .find(|candidate| candidate.id() == select.value())
            else {
                return;
            };
            let was_playing = *phase == CameraViewPhase::Playing;
            let pending_close = take_camera_runtime(&runtime, &video, &audio);
            if was_playing {
                phase.set(CameraViewPhase::Idle);
            }
            next_camera_generation(&generation);
            let csrf = csrf.clone();
            let notice = notice.clone();
            let phase = phase.clone();
            let start = start.clone();
            spawn_local(async move {
                if let Some((session_id, token)) = pending_close {
                    close_camera_session(session_id, token).await;
                }
                let mut applied = false;
                let mut failure = String::new();
                for attempt in 0..=CAMERA_UPDATE_RETRY_ATTEMPTS {
                    match post_json_response::<_, CameraProfileUpdateResponseDto>(
                        CAMERA_PROFILE_UPDATE_ENDPOINT,
                        &csrf,
                        &CameraProfileUpdateRequestDto { preset },
                        "摄像头画面设置",
                    )
                    .await
                    {
                        Ok(response) if response.applied => {
                            applied = true;
                            break;
                        }
                        Ok(_) => failure = "画面设置未生效".to_owned(),
                        Err(message) => failure = message,
                    }
                    if attempt < CAMERA_UPDATE_RETRY_ATTEMPTS {
                        TimeoutFuture::new(CAMERA_UPDATE_RETRY_DELAY_MS).await;
                    }
                }
                if applied {
                    notice.set(Some(format!("画面已切换：{}", preset.label())));
                    if was_playing {
                        phase.set(CameraViewPhase::Idle);
                        start.emit(());
                    }
                } else {
                    notice.set(Some(failure));
                }
            });
        })
    };

    let start_button = {
        let start = start.clone();
        Callback::from(move |_| start.emit(()))
    };

    let toggle_pause_action: Rc<dyn Fn()> = {
        let video = video.clone();
        let audio = audio.clone();
        let paused = paused.clone();
        let notice = notice.clone();
        Rc::new(move || {
            let next_paused = !*paused;
            if let Some(video) = video.cast::<HtmlVideoElement>() {
                if next_paused {
                    let _ = video.pause();
                } else {
                    play_media_ignoring_interruption(&video);
                }
            }
            if let Some(audio) = audio.cast::<HtmlMediaElement>() {
                if next_paused {
                    let _ = audio.pause();
                } else {
                    play_media_ignoring_interruption(&audio);
                }
            }
            paused.set(next_paused);
            notice.set(Some(if next_paused {
                "直播已暂停".to_owned()
            } else {
                "直播已继续".to_owned()
            }));
        })
    };
    let toggle_pause = {
        let toggle_pause_action = toggle_pause_action.clone();
        Callback::from(move |_| toggle_pause_action())
    };

    {
        let toggle_pause_action = toggle_pause_action.clone();
        let stop_action = stop_action.clone();
        use_effect_with((*phase, *paused), move |(current_phase, current_paused)| {
            let registration = if *current_phase == CameraViewPhase::Playing {
                install_media_session(
                    *current_paused,
                    toggle_pause_action.clone(),
                    toggle_pause_action.clone(),
                    stop_action.clone(),
                )
            } else {
                None
            };
            move || {
                if let Some(registration) = registration {
                    clear_media_session(&registration.session);
                }
            }
        });
    }

    let toggle_pip = {
        let video = video.clone();
        let pip_active = pip_active.clone();
        let notice = notice.clone();
        Callback::from(move |_| {
            let Some(video_element) = video.cast::<HtmlVideoElement>() else {
                return;
            };
            if !picture_in_picture_ready(&video_element) {
                notice.set(Some("画面正在准备，请稍后再试".to_owned()));
                return;
            }
            let was_active = picture_in_picture_active(&video_element);
            let operation = if was_active {
                picture_in_picture_exit(&video_element)
            } else {
                picture_in_picture_request(&video_element)
            };
            let pip_active = pip_active.clone();
            let notice = notice.clone();
            spawn_local(async move {
                match operation {
                    Ok(value) => {
                        if await_picture_in_picture(value).await.is_ok() {
                            pip_active.set(!was_active);
                            notice.set(Some(if was_active {
                                "已退出画中画".to_owned()
                            } else {
                                "已进入画中画".to_owned()
                            }));
                        } else {
                            notice.set(Some("当前浏览器或视频源不支持画中画".to_owned()));
                        }
                    }
                    Err(_) => {
                        notice.set(Some("当前浏览器或视频源不支持画中画".to_owned()));
                    }
                }
            });
        })
    };

    // 全双工对讲：点击按钮切换麦克风轨道，保持当前会话与音频 transceiver 不变。
    let toggle_talk = {
        let runtime = runtime.clone();
        let notice = notice.clone();
        let talk_active = talk_active.clone();
        Callback::from(move |_| {
            if runtime.borrow().is_none() {
                return;
            }
            let next_active = !*talk_active;
            let runtime = runtime.clone();
            let notice = notice.clone();
            let talk_active = talk_active.clone();
            spawn_local(async move {
                let result: Result<(), String> = async {
                    if next_active {
                        let window =
                            web_sys::window().ok_or_else(|| "浏览器没有 window 对象".to_owned())?;
                        let media_devices = window
                            .navigator()
                            .media_devices()
                            .map_err(|error| camera_js_error("无法访问麦克风设备", error))?;
                        let track_constraints = MediaTrackConstraints::new();
                        track_constraints.set_echo_cancellation_bool(true);
                        track_constraints.set_noise_suppression_bool(true);
                        let constraints = MediaStreamConstraints::new();
                        constraints.set_audio_media_track_constraints(&track_constraints);
                        let promise = media_devices
                            .get_user_media_with_constraints(&constraints)
                            .map_err(|error| camera_js_error("无法获取麦克风", error))?;
                        let stream: MediaStream = JsFuture::from(promise)
                            .await
                            .map_err(|error| camera_js_error("麦克风授权失败", error))?
                            .into();
                        let track = stream
                            .get_audio_tracks()
                            .get(0)
                            .dyn_into::<MediaStreamTrack>()
                            .map_err(|_| "浏览器没有返回音频轨道".to_owned())?;
                        let sender = runtime
                            .borrow()
                            .as_ref()
                            .and_then(|active| active.audio_sender.clone())
                            .ok_or_else(|| "当前会话未协商音频，无法对讲".to_owned())?;
                        JsFuture::from(sender.replace_track(Some(&track)))
                            .await
                            .map_err(|error| camera_js_error("挂载麦克风失败", error))?;
                        if let Some(active) = runtime.borrow_mut().as_mut() {
                            active.mic_track = Some(track);
                        }
                    } else {
                        let (sender, track) = {
                            let mut active_ref = runtime.borrow_mut();
                            let Some(active) = active_ref.as_mut() else {
                                return Ok(());
                            };
                            (active.audio_sender.clone(), active.mic_track.take())
                        };
                        if let Some(sender) = sender {
                            let _ = JsFuture::from(sender.replace_track(None)).await;
                        }
                        if let Some(track) = track {
                            track.stop();
                        }
                    }
                    Ok(())
                }
                .await;
                match result {
                    Ok(()) => {
                        talk_active.set(next_active);
                        notice.set(Some(if next_active {
                            "对讲已开启".to_owned()
                        } else {
                            "对讲已关闭".to_owned()
                        }));
                    }
                    Err(error) => notice.set(Some(format!("对讲失败：{error}"))),
                }
            });
        })
    };

    let audio_supported = status
        .as_ref()
        .is_some_and(|camera| camera.audio.as_ref().is_some_and(|audio| audio.supported));
    let camera_available = status.as_ref().is_some_and(|camera| camera.available);
    html! {
        <article class={CAMERA_CARD} aria-labelledby="camera-live-title">
            <div class={CAMERA_LAYOUT}>
                <div class={CAMERA_COPY}>
                    <div class={CONTROL_TITLE}>
                        <div>
                            <p class={EYEBROW}>{"CAMERA"}</p>
                            <h3 id="camera-live-title" class={CONTROL_HEADING}>{"摄像头直播"}</h3>
                        </div>
                        <span class={classes!(STATUS_BADGE, if *phase == CameraViewPhase::Playing { "text-success" } else { "text-base-content/60" })}>
                            <span class={STATUS_DOT_SMALL} aria-hidden="true"></span>
                            {phase.label()}
                        </span>
                    </div>
                    if let Some(camera) = status.as_ref() {
                        <dl class={CAMERA_METRICS}>
                            <div class={CAMERA_METRIC}><dt>{"设备"}</dt><dd>{if camera.available { "可用" } else { "不可用" }}</dd></div>
                            <div class={CAMERA_METRIC}><dt>{"管线"}</dt><dd>{camera_pipeline_label(camera.pipeline)}</dd></div>
                            <div class={CAMERA_METRIC}><dt>{"画面"}</dt><dd>{format!("{} × {} · {} fps · {:.1} Mbps · {} · 旋转 {}°", camera.profile.width, camera.profile.height, camera.profile.fps, camera.profile.bitrate_bps as f64 / 1_000_000.0, camera.profile.codec, camera.profile.rotation.degrees())}</dd></div>
                            <div class={CAMERA_METRIC}><dt>{"访问"}</dt><dd>{format!("{} · {} 个会话", camera_access_label(camera.access), camera.active_sessions)}</dd></div>
                        </dl>
                        if !presets.is_empty() {
                            <label class={FIELD}>
                                <span class={FIELD_LABEL}>{"画面分辨率 / 码率（切换会自动停止并重新打开直播）"}</span>
                                <select class={SELECT} onchange={set_profile} disabled={!camera_available || !can_control} aria-label="画面分辨率与码率">
                                    {for presets.iter().map(|preset| {
                                        let selected = camera.profile.width == preset.width()
                                            && camera.profile.height == preset.height()
                                            && camera.profile.bitrate_bps == preset.bitrate_bps();
                                        html! {
                                            <option value={preset.id()} selected={selected}>{preset.label()}</option>
                                        }
                                    })}
                                </select>
                            </label>
                        }
                        if let Some(error) = camera.error_category {
                            <p class="text-xs text-error" role="status">{camera_error_label(error)}</p>
                        }
                    } else if let Some(error) = status_error.as_ref() {
                        <p class="text-xs text-error" role="status">{format!("状态读取失败：{error}")}</p>
                    } else {
                        <p class={HELP_TEXT} role="status">{"正在读取摄像头状态…"}</p>
                    }
                    if let Some(error) = viewer_token_error.as_ref() {
                        <p class="text-xs text-error" role="status">{format!("观看凭证获取失败：{error}")}</p>
                    }
                    <p class={HELP_TEXT}>{"视频不会自动启动。点击麦克风图标即可开启或关闭对讲；播放中可以暂停画面、停止会话或尝试进入画中画。离开页面后会话保留 1 分钟，期间回来继续播放，超过 1 分钟未回来才停止。画面设置需要管理员登录。"}</p>
                    if let Some(message) = notice.as_ref() {
                        <p class={CAMERA_NOTICE} role="status" aria-live="polite">{message}</p>
                    }
                </div>
                <div class={CAMERA_MEDIA}>
                    <div ref={stage} class={CAMERA_STAGE}>
                        <video ref={video} class={CAMERA_VIDEO} autoplay=true playsinline=true muted=true aria-label="摄像头实时画面"></video>
                        <audio ref={audio} class="hidden" autoplay=true playsinline=true aria-label="摄像头麦克风"></audio>
                        if *phase == CameraViewPhase::Idle {
                            <div class={CAMERA_PLACEHOLDER} aria-hidden="true">
                                <span class={CAMERA_PLACEHOLDER_ICON}>{"LIVE"}</span>
                                <span>{"等待手动播放"}</span>
                            </div>
                        }
                    </div>
                    <div class={CAMERA_CONTROLS} role="toolbar" aria-label="直播控制">
                        if *phase == CameraViewPhase::Idle {
                            <button class={CAMERA_CONTROL_BUTTON} type="button" onclick={start_button} disabled={!camera_available || !can_view} aria-label="播放直播" title="播放直播">
                                {icon_play()}
                            </button>
                        } else {
                            <button class={CAMERA_CONTROL_BUTTON} type="button" onclick={toggle_pause} disabled={*phase != CameraViewPhase::Playing} aria-label={if *paused { "继续直播" } else { "暂停直播" }} title={if *paused { "继续直播" } else { "暂停直播" }}>
                                { if *paused { icon_play() } else { icon_pause() } }
                            </button>
                            <button class={classes!(CAMERA_CONTROL_BUTTON, CAMERA_CONTROL_BUTTON_DANGER)} type="button" onclick={stop} aria-label="停止直播" title="停止直播">
                                {icon_stop()}
                            </button>
                            if audio_supported {
                                <button
                                    class={classes!(CAMERA_CONTROL_BUTTON, (*talk_active).then_some(CAMERA_CONTROL_BUTTON_ACTIVE))}
                                    type="button"
                                    disabled={*phase != CameraViewPhase::Playing}
                                    aria-label={if *talk_active { "关闭对讲" } else { "开启对讲" }}
                                    title={if *talk_active { "关闭对讲" } else { "开启对讲" }}
                                    aria-pressed={talk_active.to_string()}
                                    onclick={toggle_talk.clone()}
                                >
                                    {icon_microphone(*talk_active)}
                                </button>
                            }
                            if *pip_supported {
                                <button
                                    class={classes!(CAMERA_CONTROL_BUTTON, (*pip_active).then_some(CAMERA_CONTROL_BUTTON_ACTIVE), (!*pip_ready).then_some(CAMERA_CONTROL_BUTTON_DISABLED))}
                                    type="button"
                                    onclick={toggle_pip}
                                    disabled={!*pip_ready || *phase != CameraViewPhase::Playing}
                                    aria-label={if !*pip_ready { "画面准备中" } else if *pip_active { "退出画中画" } else { "进入画中画" }}
                                    title={if !*pip_ready { "画面准备中" } else if *pip_active { "退出画中画" } else { "进入画中画" }}
                                >
                                    {icon_picture_in_picture()}
                                </button>
                            }
                            if can_control {
                                <button class={CAMERA_CONTROL_BUTTON} type="button" onclick={rotate} aria-label="旋转画面" title="旋转画面">
                                    {icon_rotate()}
                                </button>
                            }
                        }
                    </div>
                </div>
            </div>
        </article>
    }
}

#[function_component(CameraAvailability)]
pub(super) fn camera_availability() -> Html {
    let status = use_state(|| None::<CameraStatus>);
    let presets = use_state(Vec::<CameraStreamPreset>::new);
    let status_error = use_state(|| None::<String>);
    {
        let status = status.clone();
        let presets = presets.clone();
        let status_error = status_error.clone();
        use_effect_with((), move |_| {
            let cancelled = Rc::new(Cell::new(false));
            let task_cancelled = cancelled.clone();
            spawn_local(async move {
                while !task_cancelled.get() {
                    refresh_camera_status(status.clone(), presets.clone(), status_error.clone())
                        .await;
                    TimeoutFuture::new(POLL_DELAY_MS).await;
                }
            });
            move || cancelled.set(true)
        });
    }
    match (status.as_ref(), status_error.as_ref()) {
        (Some(camera), _) if camera.available => html! {
            <span class={classes!(STATUS_BADGE, "text-success")}><span class={STATUS_DOT_SMALL} aria-hidden="true"></span>{"可用"}</span>
        },
        (Some(_), _) => html! {
            <span class={classes!(STATUS_BADGE, "text-error")}><span class={STATUS_DOT_SMALL} aria-hidden="true"></span>{"不可用"}</span>
        },
        (None, Some(_)) => html! {
            <span class={STATUS_BADGE}><span class={STATUS_DOT_SMALL} aria-hidden="true"></span>{"状态未知"}</span>
        },
        (None, None) => html! {
            <span class={STATUS_BADGE}><span class={STATUS_DOT_SMALL} aria-hidden="true"></span>{"读取中"}</span>
        },
    }
}
