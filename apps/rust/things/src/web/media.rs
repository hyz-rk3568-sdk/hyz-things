use super::*;

pub(crate) type PipMediaPolicy = Rc<RefCell<Option<PipMediaPolicyState>>>;

pub(crate) struct PipMediaPolicyState {
    audio_session: Option<JsValue>,
    previous_audio_session_type: Option<JsValue>,
    previous_audio_muted: Option<bool>,
}

fn browser_audio_session() -> Option<JsValue> {
    let window = web_sys::window()?;
    Reflect::get(
        window.navigator().as_ref(),
        &JsValue::from_str("audioSession"),
    )
    .ok()
    .filter(|value| value.is_object())
}

fn set_audio_session_type(session: &JsValue, session_type: &JsValue) {
    let _ = Reflect::set(session, &JsValue::from_str("type"), session_type);
}

/// System PiP must stay silent and mixable with media from other apps.
///
/// The video element is always muted. Camera audio, when supplied separately,
/// is muted for the PiP lifetime and restored on exit. Safari's Audio Session
/// API is optional, so unsupported browsers simply keep the mute policy.
pub(crate) fn activate_pip_media_policy(
    video: &HtmlVideoElement,
    audio: Option<&HtmlMediaElement>,
    state: &PipMediaPolicy,
) {
    video.set_muted(true);

    if let Some(active) = state.borrow().as_ref() {
        if let Some(audio) = audio {
            audio.set_muted(true);
        }
        if let Some(session) = active.audio_session.as_ref() {
            set_audio_session_type(session, &JsValue::from_str("ambient"));
        }
        return;
    }

    let previous_audio_muted = audio.map(|element| element.muted());
    if let Some(audio) = audio {
        audio.set_muted(true);
    }

    let audio_session = browser_audio_session();
    let previous_audio_session_type = audio_session.as_ref().and_then(|session| {
        Reflect::get(session, &JsValue::from_str("type"))
            .ok()
            .filter(|value| !value.is_null() && !value.is_undefined())
    });
    if let Some(session) = audio_session.as_ref() {
        set_audio_session_type(session, &JsValue::from_str("ambient"));
    }

    *state.borrow_mut() = Some(PipMediaPolicyState {
        audio_session,
        previous_audio_session_type,
        previous_audio_muted,
    });
}

pub(crate) fn restore_pip_media_policy(
    video: &HtmlVideoElement,
    audio: Option<&HtmlMediaElement>,
    state: &PipMediaPolicy,
) {
    video.set_muted(true);

    let Some(previous) = state.borrow_mut().take() else {
        return;
    };

    if let (Some(audio), Some(was_muted)) = (audio, previous.previous_audio_muted) {
        audio.set_muted(was_muted);
    }
    if let Some(session) = previous.audio_session.as_ref() {
        let session_type = previous
            .previous_audio_session_type
            .unwrap_or_else(|| JsValue::from_str("auto"));
        set_audio_session_type(session, &session_type);
    }
}
