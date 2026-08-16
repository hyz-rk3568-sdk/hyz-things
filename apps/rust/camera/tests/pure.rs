use hyz_camera::{
    adapters::inbound::unix_control::{
        decode_control_request, ControlOperation, SetRotationRequest,
    },
    domain::{
        pts_ns_to_48khz, pts_ns_to_90khz, validate_offer_sdp, AudioFrame, AudioHub,
        AudioPopOutcome, AudioPtsError, AudioPushOutcome, BoundedAudioQueue, BoundedFrameQueue,
        CameraAccessKind, CameraAccessScope, CameraRotation, EncodedFrame, FramePopOutcome,
        FramePushOutcome, PtsError, SdpValidationError, TimestampWatermark, WatermarkPosition,
        AUDIO_BITRATE_BPS, AUDIO_CHANNELS, AUDIO_FRAME_MS, AUDIO_SAMPLE_RATE_HZ,
        EXPECTED_ALSA_CARD_ID, FIXED_ALSA_DEVICE, FIXED_LAN_ADDRESS, MAX_PAYLOAD_TYPES,
        MAX_SDP_BYTES, WATERMARK_FONT_FAMILY, WATERMARK_TIME_FORMAT,
    },
};
use std::{net::Ipv4Addr, sync::Arc, time::Duration};

const VIDEO_OFFER: &str = "v=0\r\n\
o=- 1 2 IN IP4 0.0.0.0\r\n\
s=-\r\n\
t=0 0\r\n\
a=ice-ufrag:abcd\r\n\
a=ice-pwd:abcdefghijklmnopqrstuvwx\r\n\
a=fingerprint:sha-256 00:11:22:33\r\n\
m=video 9 UDP/TLS/RTP/SAVPF 96\r\n\
a=recvonly\r\n\
a=rtpmap:96 H264/90000\r\n\
a=candidate:1 1 udp 1 192.0.2.1 50000 typ host\r\n";

/// 全双工对讲 offer：video recvonly + audio sendrecv（浏览器将发送自己的麦克风，
/// 并接收设备麦克风），Opus 48 kHz。
const AUDIO_OFFER: &str = "v=0\r\n\
o=- 1 2 IN IP4 0.0.0.0\r\n\
s=-\r\n\
t=0 0\r\n\
a=ice-ufrag:abcd\r\n\
a=ice-pwd:abcdefghijklmnopqrstuvwx\r\n\
a=fingerprint:sha-256 00:11:22:33\r\n\
m=video 9 UDP/TLS/RTP/SAVPF 96\r\n\
a=recvonly\r\n\
a=rtpmap:96 H264/90000\r\n\
m=audio 9 UDP/TLS/RTP/SAVPF 111\r\n\
a=sendrecv\r\n\
a=rtpmap:111 opus/48000/2\r\n\
a=candidate:1 1 udp 1 192.0.2.1 50000 typ host\r\n";

#[test]
fn validates_fixed_lan_and_tailscale_scopes() {
    assert_eq!(
        CameraAccessScope::validate(CameraAccessKind::Lan, FIXED_LAN_ADDRESS).unwrap(),
        CameraAccessScope::Lan {
            address: FIXED_LAN_ADDRESS
        }
    );
    assert!(
        CameraAccessScope::validate(CameraAccessKind::Lan, Ipv4Addr::new(192, 168, 8, 2)).is_err()
    );
    assert!(CameraAccessScope::validate(
        CameraAccessKind::Tailscale,
        Ipv4Addr::new(100, 127, 255, 254)
    )
    .is_ok());
    assert!(CameraAccessScope::validate(
        CameraAccessKind::Tailscale,
        Ipv4Addr::new(100, 128, 0, 1)
    )
    .is_err());
}

#[test]
fn validates_narrow_video_only_sdp() {
    assert!(validate_offer_sdp(VIDEO_OFFER).is_ok());
    assert!(validate_offer_sdp(&VIDEO_OFFER.replace("m=video", "m=audio")).is_err());
    assert!(validate_offer_sdp(&VIDEO_OFFER.replace("H264/90000", "VP8/90000")).is_err());
    assert!(matches!(
        validate_offer_sdp(&"x".repeat(MAX_SDP_BYTES + 1)),
        Err(SdpValidationError::TooLarge)
    ));
}

#[test]
fn accepts_full_duplex_audio_offer_and_rejects_narrow_audio_deviations() {
    // 全双工：video + audio sendrecv 通过。
    assert!(validate_offer_sdp(AUDIO_OFFER).is_ok());
    // audio 只有 recvonly（纯收听客户端）也允许。
    assert!(validate_offer_sdp(&AUDIO_OFFER.replace("a=sendrecv", "a=recvonly")).is_ok());
    // 纯喊话（sendonly）不在产品模型内：设备总是发送麦克风。
    assert!(validate_offer_sdp(&AUDIO_OFFER.replace("a=sendrecv", "a=sendonly")).is_err());
    // audio 缺少 Opus rtpmap。
    assert!(validate_offer_sdp(&AUDIO_OFFER.replace("opus/48000/2", "pcmu/8000/1")).is_err());
    // 双 audio m-line。
    let double_audio = format!("{AUDIO_OFFER}\r\nm=audio 9 UDP/TLS/RTP/SAVPF 112\r\na=sendrecv\r\na=rtpmap:112 opus/48000/2\r\n");
    assert!(validate_offer_sdp(&double_audio).is_err());
    // application/DataChannel 仍拒绝。
    assert!(validate_offer_sdp(&format!(
        "{AUDIO_OFFER}\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n"
    ))
    .is_err());
    // audio 方向缺失仍拒绝。
    assert!(validate_offer_sdp(&AUDIO_OFFER.replace("\r\na=sendrecv", "")).is_err());
}

#[test]
fn audio_payload_types_count_toward_the_same_bound() {
    let payloads = (0..MAX_PAYLOAD_TYPES.saturating_sub(1))
        .map(|payload_type| payload_type.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let inflated = AUDIO_OFFER.replace(
        "m=video 9 UDP/TLS/RTP/SAVPF 96",
        &format!("m=video 9 UDP/TLS/RTP/SAVPF {payloads}"),
    );
    assert!(validate_offer_sdp(&inflated).is_ok());
    let over = format!("{inflated}\r\nm=audio 9 UDP/TLS/RTP/SAVPF 200\r\na=sendrecv\r\na=rtpmap:200 opus/48000/2\r\n");
    assert!(validate_offer_sdp(&over).is_err());
}

/// 真实 Chrome offer 在 BUNDLE 下每个 m-section 各带一份相同 ICE 凭据。
#[test]
fn full_duplex_offer_with_per_section_ice_credentials_is_accepted() {
    let per_section = AUDIO_OFFER.replace(
        "m=audio 9 UDP/TLS/RTP/SAVPF 111",
        "a=ice-ufrag:abcd\r\na=ice-pwd:abcdefghijklmnopqrstuvwx\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111",
    );
    assert!(validate_offer_sdp(&per_section).is_ok());
    // 单 m-line 的 offer 重复携带两份凭据仍拒绝（凭据份数受 m-line 数约束）。
    let duplicated = VIDEO_OFFER.replace(
        "m=video 9 UDP/TLS/RTP/SAVPF 96",
        "a=ice-ufrag:abcd\r\na=ice-pwd:abcdefghijklmnopqrstuvwx\r\nm=video 9 UDP/TLS/RTP/SAVPF 96",
    );
    assert!(validate_offer_sdp(&duplicated).is_err());
}

#[test]
fn fixed_audio_profile_constants_are_webrtc_opus_values() {
    assert_eq!(AUDIO_SAMPLE_RATE_HZ, 48_000);
    assert_eq!(AUDIO_CHANNELS, 1);
    assert_eq!(AUDIO_FRAME_MS, 20);
    assert_eq!(AUDIO_BITRATE_BPS, 32_000);
    assert_eq!(FIXED_ALSA_DEVICE, "hw:0");
    assert_eq!(EXPECTED_ALSA_CARD_ID, "rockchiprk809");
}

#[test]
fn audio_pts_conversion_rejects_regression_and_large_jump() {
    assert_eq!(pts_ns_to_48khz(None, 1_000_000_000).unwrap(), 48_000);
    assert_eq!(
        pts_ns_to_48khz(Some(2_000), 2_000),
        Err(AudioPtsError::NotMonotonic)
    );
    assert_eq!(
        pts_ns_to_48khz(Some(1), 6_000_000_002),
        Err(AudioPtsError::JumpTooLarge)
    );
}

#[test]
fn audio_queue_drops_oldest_frame_when_full() {
    let queue = BoundedAudioQueue::new(2);
    let frame = |value| AudioFrame {
        data: Arc::<[u8]>::from([value]),
        media_time_48khz: u64::from(value) * 480,
    };
    assert_eq!(queue.push(frame(1)), AudioPushOutcome::Queued);
    assert_eq!(queue.push(frame(2)), AudioPushOutcome::Queued);
    assert_eq!(queue.push(frame(3)), AudioPushOutcome::DroppedOldFrame);
    match queue.pop_timeout(Duration::ZERO) {
        AudioPopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[2]),
        other => panic!("unexpected audio queue result: {other:?}"),
    }
    match queue.pop_timeout(Duration::ZERO) {
        AudioPopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[3]),
        other => panic!("unexpected audio queue result: {other:?}"),
    }
    assert_eq!(queue.pop_timeout(Duration::ZERO), AudioPopOutcome::Timeout);
}

#[test]
fn audio_queue_close_unblocks_pop() {
    let queue = BoundedAudioQueue::new(2);
    queue.close();
    assert_eq!(queue.pop_timeout(Duration::ZERO), AudioPopOutcome::Closed);
    assert_eq!(
        queue.push(AudioFrame {
            data: Arc::<[u8]>::from([1]),
            media_time_48khz: 480,
        }),
        AudioPushOutcome::Closed
    );
}

#[test]
fn audio_hub_fans_out_and_unsubscribes_independently() {
    let hub = AudioHub::new();
    let first = hub.subscribe();
    let second = hub.subscribe();
    hub.push(AudioFrame {
        data: Arc::<[u8]>::from([7]),
        media_time_48khz: 480,
    });
    hub.unsubscribe(&first);
    assert_eq!(hub.subscriber_count(), 1);
    hub.push(AudioFrame {
        data: Arc::<[u8]>::from([8]),
        media_time_48khz: 960,
    });
    match first.pop_timeout(Duration::ZERO) {
        AudioPopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[7]),
        other => panic!("unexpected audio queue result: {other:?}"),
    }
    assert_eq!(first.pop_timeout(Duration::ZERO), AudioPopOutcome::Timeout);
    match second.pop_timeout(Duration::ZERO) {
        AudioPopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[7]),
        other => panic!("unexpected audio queue result: {other:?}"),
    }
    match second.pop_timeout(Duration::ZERO) {
        AudioPopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[8]),
        other => panic!("unexpected audio queue result: {other:?}"),
    }
}

#[test]
fn audio_hub_close_unblocks_wait_and_closes_subscriber_queues() {
    let hub = AudioHub::new();
    let queue = hub.subscribe();
    hub.close();
    assert_eq!(hub.subscriber_count(), 0);
    assert_eq!(queue.pop_timeout(Duration::ZERO), AudioPopOutcome::Closed);
    assert_eq!(
        hub.subscribe().pop_timeout(Duration::ZERO),
        AudioPopOutcome::Closed
    );
}

#[test]
fn accepts_a_browser_sized_payload_set_but_preserves_a_hard_limit() {
    let offer_with = |count: usize| {
        let payloads = (0..count.saturating_sub(1))
            .chain(std::iter::once(96))
            .map(|payload_type| payload_type.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        VIDEO_OFFER.replace(
            "m=video 9 UDP/TLS/RTP/SAVPF 96",
            &format!("m=video 9 UDP/TLS/RTP/SAVPF {payloads}"),
        )
    };

    assert!(validate_offer_sdp(&offer_with(MAX_PAYLOAD_TYPES)).is_ok());
    assert!(validate_offer_sdp(&offer_with(MAX_PAYLOAD_TYPES + 1)).is_err());
}

#[test]
fn frame_queue_drops_old_delta_without_blocking() {
    let queue = BoundedFrameQueue::new(2);
    let frame = |value, key| EncodedFrame {
        data: Arc::<[u8]>::from([value]),
        media_time_90khz: u64::from(value),
        is_keyframe: key,
    };
    assert_eq!(queue.push(frame(1, true)), FramePushOutcome::Queued);
    assert_eq!(queue.push(frame(2, false)), FramePushOutcome::Queued);
    assert_eq!(
        queue.push(frame(3, false)),
        FramePushOutcome::DroppedOldFrame
    );
    match queue.pop_timeout(Duration::ZERO) {
        FramePopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[1]),
        other => panic!("unexpected queue result: {other:?}"),
    }
    match queue.pop_timeout(Duration::ZERO) {
        FramePopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[3]),
        other => panic!("unexpected queue result: {other:?}"),
    }
}

#[test]
fn pts_conversion_rejects_regression_and_large_jump() {
    assert_eq!(pts_ns_to_90khz(None, 1_000_000_000).unwrap(), 90_000);
    assert_eq!(
        pts_ns_to_90khz(Some(2_000), 2_000),
        Err(PtsError::NotMonotonic)
    );
    assert_eq!(
        pts_ns_to_90khz(Some(1), 6_000_000_002),
        Err(PtsError::JumpTooLarge)
    );
}

#[test]
fn control_protocol_is_versioned_and_denies_unknown_fields() {
    let status = br#"{"version":2,"operation":"status"}"#;
    assert!(matches!(
        decode_control_request(status).unwrap().operation,
        ControlOperation::Status
    ));
    assert!(decode_control_request(br#"{"version":3,"operation":"status"}"#).is_err());
    assert!(decode_control_request(
        br#"{"version":2,"operation":"status","pipeline":"caller-value"}"#
    )
    .is_err());
}

#[test]
fn set_rotation_operation_is_typed_and_enum_scoped() {
    let request = br#"{"version":2,"operation":{"set_rotation":{"rotation":"deg_270"}}}"#;
    assert!(matches!(
        decode_control_request(request).unwrap().operation,
        ControlOperation::SetRotation(SetRotationRequest {
            rotation: CameraRotation::Deg270
        })
    ));
    assert!(decode_control_request(
        br#"{"version":2,"operation":{"set_rotation":{"rotation":"deg_45"}}}"#
    )
    .is_err());
    assert!(decode_control_request(
        br#"{"version":2,"operation":{"set_rotation":{"rotation":"deg_90","extra":1}}}"#
    )
    .is_err());
}

#[test]
fn timestamp_watermark_is_fixed_across_presets() {
    let watermark = TimestampWatermark::DEFAULT;
    assert_eq!(watermark.time_format, WATERMARK_TIME_FORMAT);
    assert_eq!(watermark.position, WatermarkPosition::TopLeft);
    assert!(watermark.shaded_background);
    assert_eq!(watermark.font_size, 20);
    assert!(WATERMARK_FONT_FAMILY.contains("DejaVu Sans"));
    assert_eq!(watermark.validate(), Ok(()));
}
