use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

pub const FIXED_CAMERA_DEVICE: &str = "/dev/video0";
pub const FRAME_QUEUE_CAPACITY: usize = 2;
pub const MAX_ENCODED_FRAME_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PTS_JUMP_NS: u64 = 5_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraVideoCodec {
    H264Baseline,
}

/// 画面旋转是媒体管线属性：`videoflip` 在编码前应用，时间戳水印始终叠加在
/// 最终方向画面的左上角，浏览器端不再做 CSS 旋转。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraRotation {
    #[serde(rename = "deg_0")]
    Deg0,
    #[serde(rename = "deg_90")]
    Deg90,
    #[serde(rename = "deg_180")]
    Deg180,
    #[serde(rename = "deg_270")]
    Deg270,
}

impl CameraRotation {
    pub const ALL: [Self; 4] = [Self::Deg0, Self::Deg90, Self::Deg180, Self::Deg270];

    pub const fn degrees(self) -> u16 {
        match self {
            Self::Deg0 => 0,
            Self::Deg90 => 90,
            Self::Deg180 => 180,
            Self::Deg270 => 270,
        }
    }

    /// 「旋转画面」每次点击的循环顺序：0 → 270 → 180 → 90 → 0。
    pub const fn next_rotation(self) -> Self {
        match self {
            Self::Deg0 => Self::Deg270,
            Self::Deg270 => Self::Deg180,
            Self::Deg180 => Self::Deg90,
            Self::Deg90 => Self::Deg0,
        }
    }

    /// 90/270 旋转会交换输出宽高。
    pub const fn swaps_dimensions(self) -> bool {
        matches!(self, Self::Deg90 | Self::Deg270)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CameraStreamProfile {
    pub width: u16,
    pub height: u16,
    pub fps: u8,
    pub bitrate_bps: u32,
    pub codec: CameraVideoCodec,
    pub rotation: CameraRotation,
}

impl CameraStreamProfile {
    /// 旋转应用后的实际显示高度（90/270 时宽高互换）。
    pub const fn display_height(self) -> u16 {
        if self.rotation.swaps_dimensions() {
            self.width
        } else {
            self.height
        }
    }
}

pub const FIXED_CAPTURE_WIDTH: u16 = 3840;
pub const FIXED_CAPTURE_HEIGHT: u16 = 2160;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CameraStreamPreset {
    Uhd4k20m,
    Qhd1440p10m,
    Fhd1080p5m,
    #[default]
    Hd720p25m,
}

impl CameraStreamPreset {
    pub const ALL: [Self; 4] = [
        Self::Uhd4k20m,
        Self::Qhd1440p10m,
        Self::Fhd1080p5m,
        Self::Hd720p25m,
    ];

    pub const fn profile(self) -> CameraStreamProfile {
        match self {
            Self::Uhd4k20m => CameraStreamProfile {
                width: FIXED_CAPTURE_WIDTH,
                height: FIXED_CAPTURE_HEIGHT,
                fps: 30,
                bitrate_bps: 20_000_000,
                codec: CameraVideoCodec::H264Baseline,
                rotation: CameraRotation::Deg0,
            },
            Self::Qhd1440p10m => CameraStreamProfile {
                width: 2560,
                height: 1440,
                fps: 30,
                bitrate_bps: 10_000_000,
                codec: CameraVideoCodec::H264Baseline,
                rotation: CameraRotation::Deg0,
            },
            Self::Fhd1080p5m => CameraStreamProfile {
                width: 1920,
                height: 1080,
                fps: 30,
                bitrate_bps: 5_000_000,
                codec: CameraVideoCodec::H264Baseline,
                rotation: CameraRotation::Deg0,
            },
            Self::Hd720p25m => CameraStreamProfile {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate_bps: 2_500_000,
                codec: CameraVideoCodec::H264Baseline,
                rotation: CameraRotation::Deg0,
            },
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Uhd4k20m => "uhd4k20m",
            Self::Qhd1440p10m => "qhd1440p10m",
            Self::Fhd1080p5m => "fhd1080p5m",
            Self::Hd720p25m => "hd720p25m",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Uhd4k20m => "4K · 20 Mbps",
            Self::Qhd1440p10m => "1440p · 10 Mbps",
            Self::Fhd1080p5m => "1080p · 5 Mbps",
            Self::Hd720p25m => "720p · 2.5 Mbps",
        }
    }
}

#[derive(Clone, Debug)]
pub struct EncodedFrame {
    pub data: Arc<[u8]>,
    pub media_time_90khz: u64,
    pub is_keyframe: bool,
}

#[derive(Debug)]
struct FrameQueueInner {
    frames: VecDeque<EncodedFrame>,
    closed: bool,
}

#[derive(Debug)]
pub struct BoundedFrameQueue {
    capacity: usize,
    inner: Mutex<FrameQueueInner>,
    available: Condvar,
}

impl BoundedFrameQueue {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            capacity,
            inner: Mutex::new(FrameQueueInner {
                frames: VecDeque::with_capacity(capacity),
                closed: false,
            }),
            available: Condvar::new(),
        }
    }

    pub fn push(&self, frame: EncodedFrame) -> FramePushOutcome {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.closed {
            return FramePushOutcome::Closed;
        }

        let mut dropped = false;
        if inner.frames.len() == self.capacity {
            let position = inner
                .frames
                .iter()
                .position(|queued| !queued.is_keyframe)
                .unwrap_or(0);
            inner.frames.remove(position);
            dropped = true;
        }
        inner.frames.push_back(frame);
        self.available.notify_one();
        if dropped {
            FramePushOutcome::DroppedOldFrame
        } else {
            FramePushOutcome::Queued
        }
    }

    pub fn pop_timeout(&self, timeout: Duration) -> FramePopOutcome {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (mut inner, _) = self
            .available
            .wait_timeout_while(inner, timeout, |state| {
                state.frames.is_empty() && !state.closed
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(frame) = inner.frames.pop_front() {
            FramePopOutcome::Frame(frame)
        } else if inner.closed {
            FramePopOutcome::Closed
        } else {
            FramePopOutcome::Timeout
        }
    }

    pub fn close(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.closed = true;
        inner.frames.clear();
        self.available.notify_all();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramePushOutcome {
    Queued,
    DroppedOldFrame,
    Closed,
}

#[derive(Debug)]
pub enum FramePopOutcome {
    Frame(EncodedFrame),
    Timeout,
    Closed,
}

pub fn pts_ns_to_90khz(previous_ns: Option<u64>, pts_ns: u64) -> Result<u64, PtsError> {
    if let Some(previous) = previous_ns {
        if pts_ns <= previous {
            return Err(PtsError::NotMonotonic);
        }
        if pts_ns - previous > MAX_PTS_JUMP_NS {
            return Err(PtsError::JumpTooLarge);
        }
    }
    let ticks = (u128::from(pts_ns) * 90_000) / 1_000_000_000;
    u64::try_from(ticks).map_err(|_| PtsError::Overflow)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PtsError {
    #[error("encoded frame PTS is not monotonic")]
    NotMonotonic,
    #[error("encoded frame PTS jump is too large")]
    JumpTooLarge,
    #[error("encoded frame PTS overflows the 90 kHz clock")]
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preset_is_lowest_latency_720p() {
        assert_eq!(CameraStreamPreset::default(), CameraStreamPreset::Hd720p25m);
        let profile = CameraStreamPreset::default().profile();
        assert_eq!(profile.width, 1280);
        assert_eq!(profile.height, 720);
        assert_eq!(profile.fps, 30);
        assert_eq!(profile.bitrate_bps, 2_500_000);
    }

    #[test]
    fn uhd_preset_matches_full_capture() {
        let profile = CameraStreamPreset::Uhd4k20m.profile();
        assert_eq!(profile.width, FIXED_CAPTURE_WIDTH);
        assert_eq!(profile.height, FIXED_CAPTURE_HEIGHT);
        assert_eq!(profile.bitrate_bps, 20_000_000);
    }

    #[test]
    fn presets_cover_descending_16by9_resolutions() {
        let mut previous_width = u16::MAX;
        for preset in CameraStreamPreset::ALL {
            let profile = preset.profile();
            assert!(profile.width % 16 == 0 && profile.height % 9 == 0);
            assert_eq!(profile.width * 9, profile.height * 16);
            assert!(profile.width < previous_width);
            assert_eq!(profile.fps, 30);
            assert_eq!(profile.codec, CameraVideoCodec::H264Baseline);
            previous_width = profile.width;
        }
    }

    #[test]
    fn preset_ids_and_labels_are_fixed_and_distinct() {
        let mut ids = std::collections::HashSet::new();
        let mut labels = std::collections::HashSet::new();
        for preset in CameraStreamPreset::ALL {
            assert!(ids.insert(preset.id()));
            assert!(labels.insert(preset.label()));
        }
        assert_eq!(ids.len(), CameraStreamPreset::ALL.len());
        assert_eq!(CameraStreamPreset::Uhd4k20m.id(), "uhd4k20m");
        assert_eq!(CameraStreamPreset::Uhd4k20m.label(), "4K · 20 Mbps");
    }

    #[test]
    fn presets_are_not_rotated_by_default() {
        for preset in CameraStreamPreset::ALL {
            assert_eq!(preset.profile().rotation, CameraRotation::Deg0);
            assert_eq!(preset.profile().display_height(), preset.profile().height);
        }
    }

    #[test]
    fn rotation_cycles_in_product_order_and_swaps_dimensions() {
        assert_eq!(CameraRotation::Deg0.next_rotation(), CameraRotation::Deg270);
        assert_eq!(
            CameraRotation::Deg270.next_rotation(),
            CameraRotation::Deg180
        );
        assert_eq!(
            CameraRotation::Deg180.next_rotation(),
            CameraRotation::Deg90
        );
        assert_eq!(CameraRotation::Deg90.next_rotation(), CameraRotation::Deg0);
        for rotation in CameraRotation::ALL {
            assert_eq!(rotation.degrees(), rotation.degrees() % 360);
            assert_eq!(
                rotation.swaps_dimensions(),
                matches!(rotation, CameraRotation::Deg90 | CameraRotation::Deg270)
            );
        }
        let profile = CameraStreamPreset::Hd720p25m.profile();
        let rotated = CameraStreamProfile {
            rotation: CameraRotation::Deg90,
            ..profile
        };
        assert_eq!(rotated.display_height(), profile.width);
        assert_eq!(rotated.display_height(), 1280);
    }
}
