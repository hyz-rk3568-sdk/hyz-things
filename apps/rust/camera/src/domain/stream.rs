use serde::Serialize;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CameraStreamProfile {
    pub width: u16,
    pub height: u16,
    pub fps: u8,
    pub bitrate_bps: u32,
    pub codec: CameraVideoCodec,
}

pub const FIXED_CAPTURE_WIDTH: u16 = 3840;
pub const FIXED_CAPTURE_HEIGHT: u16 = 2160;
pub const FIXED_STREAM_PROFILE: CameraStreamProfile = CameraStreamProfile {
    width: 1920,
    height: 1080,
    fps: 30,
    bitrate_bps: 4_000_000,
    codec: CameraVideoCodec::H264Baseline,
};

pub const FIXED_CROP_LEFT: u32 =
    (FIXED_CAPTURE_WIDTH as u32 - FIXED_STREAM_PROFILE.width as u32) / 2;
pub const FIXED_CROP_RIGHT: u32 = FIXED_CROP_LEFT;
pub const FIXED_CROP_TOP: u32 =
    (FIXED_CAPTURE_HEIGHT as u32 - FIXED_STREAM_PROFILE.height as u32) / 2;
pub const FIXED_CROP_BOTTOM: u32 = FIXED_CROP_TOP;

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
    fn fixed_stream_is_a_centered_1080p_crop() {
        assert_eq!(FIXED_STREAM_PROFILE.width, 1920);
        assert_eq!(FIXED_STREAM_PROFILE.height, 1080);
        assert_eq!(FIXED_STREAM_PROFILE.fps, 30);
        assert_eq!(FIXED_STREAM_PROFILE.bitrate_bps, 4_000_000);
        assert_eq!(FIXED_CROP_LEFT, 960);
        assert_eq!(FIXED_CROP_RIGHT, 960);
        assert_eq!(FIXED_CROP_TOP, 540);
        assert_eq!(FIXED_CROP_BOTTOM, 540);
        assert_eq!(
            u32::from(FIXED_STREAM_PROFILE.width) + FIXED_CROP_LEFT + FIXED_CROP_RIGHT,
            u32::from(FIXED_CAPTURE_WIDTH)
        );
        assert_eq!(
            u32::from(FIXED_STREAM_PROFILE.height) + FIXED_CROP_TOP + FIXED_CROP_BOTTOM,
            u32::from(FIXED_CAPTURE_HEIGHT)
        );
    }
}
