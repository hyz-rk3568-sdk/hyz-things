//! 全双工语音对讲音频值对象与有界队列（纯 domain，不依赖 ALSA/GStreamer）。
//!
//! 固定媒体 profile：48 kHz / 单声道 / 20 ms 帧 / Opus 32 kbps。采集与回放固定
//! 使用 ALSA `hw:0`（RK809 codec），启动前必须 probe 到期望的 card id。

use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

pub const AUDIO_SAMPLE_RATE_HZ: u32 = 48_000;
pub const AUDIO_CHANNELS: u8 = 1;
pub const AUDIO_FRAME_MS: u16 = 20;
pub const AUDIO_BITRATE_BPS: u32 = 32_000;
pub const FIXED_ALSA_DEVICE: &str = "hw:0";
pub const EXPECTED_ALSA_CARD_ID: &str = "rockchiprk809";
pub const AUDIO_QUEUE_CAPACITY: usize = 32;
pub const MAX_AUDIO_FRAME_BYTES: usize = 4 * 1024;
pub const MAX_AUDIO_PTS_JUMP_NS: u64 = 5_000_000_000;

/// 编码后的 Opus access unit 与 48 kHz RTP 时钟时间戳。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioFrame {
    pub data: Arc<[u8]>,
    pub media_time_48khz: u64,
}

#[derive(Debug)]
struct AudioQueueInner {
    frames: VecDeque<AudioFrame>,
    closed: bool,
}

/// 单消费者有界音频队列：慢消费者独立丢旧帧，不阻塞采集线程。
#[derive(Debug)]
pub struct BoundedAudioQueue {
    capacity: usize,
    inner: Mutex<AudioQueueInner>,
    available: Condvar,
}

impl BoundedAudioQueue {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            capacity,
            inner: Mutex::new(AudioQueueInner {
                frames: VecDeque::with_capacity(capacity),
                closed: false,
            }),
            available: Condvar::new(),
        }
    }

    pub fn push(&self, frame: AudioFrame) -> AudioPushOutcome {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.closed {
            return AudioPushOutcome::Closed;
        }
        let mut dropped = false;
        if inner.frames.len() == self.capacity {
            inner.frames.pop_front();
            dropped = true;
        }
        inner.frames.push_back(frame);
        self.available.notify_one();
        if dropped {
            AudioPushOutcome::DroppedOldFrame
        } else {
            AudioPushOutcome::Queued
        }
    }

    pub fn pop_timeout(&self, timeout: Duration) -> AudioPopOutcome {
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
            AudioPopOutcome::Frame(frame)
        } else if inner.closed {
            AudioPopOutcome::Closed
        } else {
            AudioPopOutcome::Timeout
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

/// 采集扇出：一路 Opus 采集流转发给多个音频会话，每个会话持有独立
/// `BoundedAudioQueue`（单消费者）。慢会话独立丢旧帧，互不影响。
#[derive(Debug)]
pub struct AudioHub {
    inner: Mutex<AudioHubInner>,
    available: Condvar,
}

#[derive(Debug)]
struct AudioHubInner {
    subscribers: Vec<Arc<BoundedAudioQueue>>,
    first_frame_seen: bool,
    closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioHubError {
    Timeout,
    Closed,
}

impl Default for AudioHub {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioHub {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(AudioHubInner {
                subscribers: Vec::new(),
                first_frame_seen: false,
                closed: false,
            }),
            available: Condvar::new(),
        }
    }

    /// 注册一个新会话队列；hub 已关闭时返回已关闭队列，消费者立即看到
    /// `AudioPopOutcome::Closed`。
    pub fn subscribe(&self) -> Arc<BoundedAudioQueue> {
        let queue = Arc::new(BoundedAudioQueue::new(AUDIO_QUEUE_CAPACITY));
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

    pub fn unsubscribe(&self, queue: &Arc<BoundedAudioQueue>) {
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

    pub fn push(&self, frame: AudioFrame) {
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

    pub fn wait_first_frame(&self, timeout: Duration) -> Result<(), AudioHubError> {
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
            Err(AudioHubError::Closed)
        } else {
            Err(AudioHubError::Timeout)
        }
    }

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
pub enum AudioPushOutcome {
    Queued,
    DroppedOldFrame,
    Closed,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AudioPopOutcome {
    Frame(AudioFrame),
    Timeout,
    Closed,
}

pub fn pts_ns_to_48khz(previous_ns: Option<u64>, pts_ns: u64) -> Result<u64, AudioPtsError> {
    if let Some(previous) = previous_ns {
        if pts_ns <= previous {
            return Err(AudioPtsError::NotMonotonic);
        }
        if pts_ns - previous > MAX_AUDIO_PTS_JUMP_NS {
            return Err(AudioPtsError::JumpTooLarge);
        }
    }
    let ticks = (u128::from(pts_ns) * u128::from(AUDIO_SAMPLE_RATE_HZ)) / 1_000_000_000;
    u64::try_from(ticks).map_err(|_| AudioPtsError::Overflow)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AudioPtsError {
    #[error("audio frame PTS is not monotonic")]
    NotMonotonic,
    #[error("audio frame PTS jump is too large")]
    JumpTooLarge,
    #[error("audio frame PTS overflows the 48 kHz clock")]
    Overflow,
}
