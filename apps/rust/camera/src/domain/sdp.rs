//! SDP offer 校验（纯函数，不依赖 str0m/GStreamer/Linux）。
//!
//! 全双工对讲扩展了第一版 allowlist：允许 0 或 1 个 audio m-line（Opus），
//! video 仍必须恰 1 个；audio 方向只允许 `recvonly`/`sendrecv`（浏览器视角，
//! 设备始终发送麦克风，`sendonly` 的纯喊话客户端不在本产品模型内）。
//! DataChannel/application m-line、未知 m-line 与不可解析字段仍拒绝。

pub const MAX_SDP_BYTES: usize = 32 * 1024;
pub const MAX_SDP_LINE_BYTES: usize = 2048;
pub const MAX_CANDIDATES: usize = 32;
pub const MAX_PAYLOAD_TYPES: usize = 64;
pub const MAX_ICE_CREDENTIAL_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdpValidationError {
    TooLarge,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    None,
    Video,
    Audio,
    Application,
}

pub fn validate_offer_sdp(sdp: &str) -> Result<(), SdpValidationError> {
    if sdp.len() > MAX_SDP_BYTES {
        return Err(SdpValidationError::TooLarge);
    }
    if sdp.is_empty() || sdp.contains('\0') {
        return Err(SdpValidationError::Unsupported);
    }

    let mut video_lines = 0usize;
    let mut audio_lines = 0usize;
    let mut application_lines = 0usize;
    let mut candidates = 0usize;
    let mut payload_types = 0usize;
    let mut video_has_h264 = false;
    let mut video_has_recv_direction = false;
    let mut audio_has_opus = false;
    let mut audio_direction_ok = false;
    let mut has_sha256_fingerprint = false;
    let mut ice_ufrag = 0usize;
    let mut ice_pwd = 0usize;
    let mut section = Section::None;

    for raw_line in sdp.split_terminator('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() || line.len() > MAX_SDP_LINE_BYTES {
            return Err(SdpValidationError::Unsupported);
        }
        if let Some(rest) = line.strip_prefix("m=") {
            section = if rest.starts_with("video ") {
                video_lines += 1;
                Section::Video
            } else if rest.starts_with("audio ") {
                audio_lines += 1;
                Section::Audio
            } else if rest.starts_with("application ") {
                application_lines += 1;
                Section::Application
            } else {
                return Err(SdpValidationError::Unsupported);
            };
            // m=<media> <port> <proto> <fmt>...；payload 类型从下标 3 开始。
            let fields: Vec<_> = rest.split_ascii_whitespace().collect();
            if fields.len() < 3 {
                return Err(SdpValidationError::Unsupported);
            }
            payload_types += fields[3..].len();
        } else if line.starts_with("a=rtpmap:") {
            let rtpmap = line.to_ascii_uppercase();
            match section {
                Section::Video => {
                    if rtpmap.contains(" H264/90000") {
                        video_has_h264 = true;
                    }
                }
                Section::Audio => {
                    if rtpmap.contains(" OPUS/48000") {
                        audio_has_opus = true;
                    }
                }
                Section::None | Section::Application => {}
            }
        } else if matches!(line, "a=recvonly" | "a=sendrecv") {
            match section {
                Section::Video => video_has_recv_direction = true,
                Section::Audio => audio_direction_ok = true,
                Section::None | Section::Application => {}
            }
        } else if let Some(value) = line.strip_prefix("a=fingerprint:") {
            has_sha256_fingerprint |= value
                .split_ascii_whitespace()
                .next()
                .is_some_and(|algorithm| algorithm.eq_ignore_ascii_case("sha-256"));
        } else if let Some(value) = line.strip_prefix("a=ice-ufrag:") {
            ice_ufrag += 1;
            if value.is_empty() || value.len() > MAX_ICE_CREDENTIAL_BYTES {
                return Err(SdpValidationError::Unsupported);
            }
        } else if let Some(value) = line.strip_prefix("a=ice-pwd:") {
            ice_pwd += 1;
            if value.is_empty() || value.len() > MAX_ICE_CREDENTIAL_BYTES {
                return Err(SdpValidationError::Unsupported);
            }
        } else if line.starts_with("a=candidate:") {
            candidates += 1;
        }
    }

    let m_lines = video_lines + audio_lines;
    let valid = video_lines == 1
        && audio_lines <= 1
        && application_lines == 0
        && video_has_h264
        && video_has_recv_direction
        && (audio_lines == 0 || (audio_has_opus && audio_direction_ok))
        && has_sha256_fingerprint
        // BUNDLE 下浏览器每个 m-section 各带一份相同 ICE 凭据，也可能只带一份；
        // ufrag/pwd 必须成对且份数不超过 m-line 数。
        && ice_ufrag == ice_pwd
        && (1..=m_lines).contains(&ice_ufrag)
        && candidates > 0
        && candidates <= MAX_CANDIDATES
        && payload_types > 0
        && payload_types <= MAX_PAYLOAD_TYPES;

    if valid {
        Ok(())
    } else {
        Err(SdpValidationError::Unsupported)
    }
}

/// 扫描 offer 是否协商了 audio m-line（application 层据此决定是否启动音频管线）。
pub fn offer_negotiates_audio(sdp: &str) -> bool {
    sdp.split_terminator('\n').any(|line| {
        line.strip_suffix('\r')
            .unwrap_or(line)
            .starts_with("m=audio ")
    })
}
