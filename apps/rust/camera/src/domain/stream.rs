use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

pub use hyz_contract::camera::{
    CameraRotation, CameraStreamPreset, CameraStreamProfile, CameraVideoCodec,
    FIXED_CAPTURE_HEIGHT, FIXED_CAPTURE_WIDTH,
};

pub const FIXED_CAMERA_DEVICE: &str = "/dev/video0";
pub const FRAME_QUEUE_CAPACITY: usize = 2;
pub const MAX_ENCODED_FRAME_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PTS_JUMP_NS: u64 = 5_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
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

/// 编码帧扇出：单个 pipeline 的一路编码流转发给多个 viewer，每个 viewer 持有
/// 独立的 `BoundedFrameQueue`（单消费者）。`push` 零拷贝（`EncodedFrame` 数据为
/// `Arc`），慢 viewer 在各自队列独立丢旧帧，互不影响。
#[derive(Debug)]
pub struct FrameHub {
    inner: Mutex<FrameHubInner>,
    available: Condvar,
}

#[derive(Debug)]
struct FrameHubInner {
    subscribers: Vec<Arc<BoundedFrameQueue>>,
    first_frame_seen: bool,
    closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameHubError {
    Timeout,
    Closed,
}

impl Default for FrameHub {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameHub {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(FrameHubInner {
                subscribers: Vec::new(),
                first_frame_seen: false,
                closed: false,
            }),
            available: Condvar::new(),
        }
    }

    /// 注册一个新 viewer 队列。hub 已关闭时返回一个已关闭队列，消费者立即看到
    /// `FramePopOutcome::Closed`。
    pub fn subscribe(&self) -> Arc<BoundedFrameQueue> {
        let queue = Arc::new(BoundedFrameQueue::new(FRAME_QUEUE_CAPACITY));
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.closed {
            queue.close();
        } else {
            inner.subscribers.push(Arc::clone(&queue));
        }
        queue
    }

    pub fn unsubscribe(&self, queue: &Arc<BoundedFrameQueue>) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner
            .subscribers
            .retain(|candidate| !Arc::ptr_eq(candidate, queue));
    }

    pub fn subscriber_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .subscribers
            .len()
    }

    /// 向所有订阅者转发一帧，并记录首帧以便 `wait_first_frame` 返回。hub 已关闭
    /// 时忽略，避免向关闭队列投递。
    pub fn push(&self, frame: EncodedFrame) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.closed {
            return;
        }
        inner.first_frame_seen = true;
        for queue in &inner.subscribers {
            let _ = queue.push(frame.clone());
        }
        self.available.notify_all();
    }

    /// 等待第一帧产出（pipeline 启动校验；无订阅者时也生效）。返回首帧、关闭或超时。
    pub fn wait_first_frame(&self, timeout: Duration) -> Result<(), FrameHubError> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (inner, _) = self
            .available
            .wait_timeout_while(inner, timeout, |state| {
                !state.first_frame_seen && !state.closed
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.first_frame_seen {
            Ok(())
        } else if inner.closed {
            Err(FrameHubError::Closed)
        } else {
            Err(FrameHubError::Timeout)
        }
    }

    /// 关闭 hub：唤醒首帧等待，并关闭所有订阅者队列（pipeline 故障/EOS 时所有
    /// viewer 线程收到 `FramePopOutcome::Closed` 退出）。
    pub fn close(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.closed {
            return;
        }
        inner.closed = true;
        for queue in inner.subscribers.drain(..) {
            queue.close();
        }
        self.available.notify_all();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramePushOutcome {
    Queued,
    DroppedOldFrame,
    Closed,
}

#[derive(Debug, PartialEq, Eq)]
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

    #[test]
    fn hub_fans_out_every_frame_to_all_subscribers() {
        let hub = FrameHub::new();
        let first = hub.subscribe();
        let second = hub.subscribe();
        hub.push(EncodedFrame {
            data: Arc::<[u8]>::from([1]),
            media_time_90khz: 90,
            is_keyframe: true,
        });
        hub.push(EncodedFrame {
            data: Arc::<[u8]>::from([2]),
            media_time_90khz: 180,
            is_keyframe: false,
        });
        for queue in [&first, &second] {
            match queue.pop_timeout(Duration::ZERO) {
                FramePopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[1]),
                other => panic!("unexpected queue result: {other:?}"),
            }
            match queue.pop_timeout(Duration::ZERO) {
                FramePopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[2]),
                other => panic!("unexpected queue result: {other:?}"),
            }
        }
        assert_eq!(hub.subscriber_count(), 2);
    }

    #[test]
    fn hub_unsubscribe_stops_delivery_to_that_viewer_only() {
        let hub = FrameHub::new();
        let first = hub.subscribe();
        let second = hub.subscribe();
        hub.unsubscribe(&first);
        assert_eq!(hub.subscriber_count(), 1);
        hub.push(EncodedFrame {
            data: Arc::<[u8]>::from([9]),
            media_time_90khz: 90,
            is_keyframe: true,
        });
        assert_eq!(first.pop_timeout(Duration::ZERO), FramePopOutcome::Timeout);
        match second.pop_timeout(Duration::ZERO) {
            FramePopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[9]),
            other => panic!("unexpected queue result: {other:?}"),
        }
    }

    #[test]
    fn hub_subscribers_drop_old_frames_independently() {
        let hub = FrameHub::new();
        let first = hub.subscribe();
        let second = hub.subscribe();
        for value in [1u8, 2, 3] {
            hub.push(EncodedFrame {
                data: Arc::<[u8]>::from([value]),
                media_time_90khz: u64::from(value) * 90,
                is_keyframe: value == 1,
            });
        }
        // 每个订阅者容量 2：第 2 帧被第 3 帧挤掉，且只影响各自队列。
        for queue in [&first, &second] {
            match queue.pop_timeout(Duration::ZERO) {
                FramePopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[1]),
                other => panic!("unexpected queue result: {other:?}"),
            }
            match queue.pop_timeout(Duration::ZERO) {
                FramePopOutcome::Frame(frame) => assert_eq!(&*frame.data, &[3]),
                other => panic!("unexpected queue result: {other:?}"),
            }
        }
    }

    #[test]
    fn hub_first_frame_wait_is_immediate_after_a_push() {
        let hub = FrameHub::new();
        assert_eq!(
            hub.wait_first_frame(Duration::ZERO),
            Err(FrameHubError::Timeout)
        );
        hub.push(EncodedFrame {
            data: Arc::<[u8]>::from([1]),
            media_time_90khz: 90,
            is_keyframe: true,
        });
        assert_eq!(hub.wait_first_frame(Duration::ZERO), Ok(()));
    }

    #[test]
    fn hub_close_unblocks_wait_and_closes_subscriber_queues() {
        let hub = FrameHub::new();
        let queue = hub.subscribe();
        hub.close();
        assert_eq!(
            hub.wait_first_frame(Duration::ZERO),
            Err(FrameHubError::Closed)
        );
        assert_eq!(hub.subscriber_count(), 0);
        assert_eq!(queue.pop_timeout(Duration::ZERO), FramePopOutcome::Closed);
        assert_eq!(
            hub.subscribe().pop_timeout(Duration::ZERO),
            FramePopOutcome::Closed
        );
    }
}
