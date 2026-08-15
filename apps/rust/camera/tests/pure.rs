use hyz_camera::{
    adapters::{
        inbound::unix_control::{decode_control_request, ControlOperation, SetRotationRequest},
        outbound::webrtc::{validate_offer_sdp, MAX_PAYLOAD_TYPES, MAX_SDP_BYTES},
    },
    domain::{
        pts_ns_to_90khz, BoundedFrameQueue, CameraAccessKind, CameraAccessScope, CameraRotation,
        CameraStreamPreset, EncodedFrame, FramePopOutcome, FramePushOutcome, PtsError,
        TimestampWatermark, WatermarkPosition, FIXED_LAN_ADDRESS, WATERMARK_FONT_FAMILY,
        WATERMARK_TIME_FORMAT,
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
        Err(hyz_camera::application::ports::WebRtcError::SdpTooLarge)
    ));
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
fn timestamp_watermark_is_burned_into_every_profile() {
    for preset in CameraStreamPreset::ALL {
        let watermark = TimestampWatermark::for_profile(preset.profile());
        assert_eq!(watermark.time_format, WATERMARK_TIME_FORMAT);
        assert_eq!(watermark.position, WatermarkPosition::TopLeft);
        assert!(watermark.shaded_background);
        assert!(watermark.font_size >= 18);
        assert!(watermark.font_size <= 54);
        assert!(WATERMARK_FONT_FAMILY.contains("DejaVu Sans"));
        assert_eq!(watermark.validate(), Ok(()));
    }
}
