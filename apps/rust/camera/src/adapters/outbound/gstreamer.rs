use super::v4l2::probe_fixed_camera_device;
use crate::{
    application::ports::{
        CameraMediaPort, KeyframeRequester, MediaError, MediaTerminator, RunningMedia,
    },
    domain::{
        pts_ns_to_90khz, BoundedFrameQueue, CameraPipelineState, CameraStreamProfile, EncodedFrame,
        FramePopOutcome, FIXED_CAMERA_DEVICE, FIXED_CAPTURE_HEIGHT, FIXED_CAPTURE_WIDTH,
        FRAME_QUEUE_CAPACITY, MAX_ENCODED_FRAME_BYTES,
    },
};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const PIPELINE_START_DEADLINE: Duration = Duration::from_secs(3);
const SYSTEM_PLUGIN_DIRECTORY: &str = "/usr/lib/gstreamer-1.0";
const FULL_RANGE_BT709_COLORIMETRY: &str = "1:3:5:1";

pub struct GStreamerMediaAdapter;

impl GStreamerMediaAdapter {
    pub fn new() -> Result<Self, MediaError> {
        gst::init().map_err(|_| MediaError::PipelineFailed)?;
        let registry = gst::Registry::get();
        registry.scan_path(SYSTEM_PLUGIN_DIRECTORY);
        if let Some(paths) =
            std::env::var_os("GST_PLUGIN_PATH_1_0").or_else(|| std::env::var_os("GST_PLUGIN_PATH"))
        {
            for path in std::env::split_paths(&paths).filter(|path| path.is_absolute()) {
                registry.scan_path(path);
            }
        }
        Ok(Self)
    }
}

impl CameraMediaPort for GStreamerMediaAdapter {
    fn probe(&self) -> Result<(), MediaError> {
        probe_fixed_camera_device()?;
        for factory in [
            "v4l2src",
            "queue",
            "videoscale",
            "mpph264enc",
            "h264parse",
            "capsfilter",
            "appsink",
        ] {
            if gst::ElementFactory::find(factory).is_none() {
                return Err(if factory == "mpph264enc" {
                    MediaError::EncoderUnavailable
                } else {
                    MediaError::PipelineFailed
                });
            }
        }
        Ok(())
    }

    fn start(&self, profile: CameraStreamProfile) -> Result<Box<dyn RunningMedia>, MediaError> {
        self.probe()?;
        let pipeline = gst::Pipeline::with_name("hyz-camera-pipeline");
        let source = make("v4l2src", "camera-source")?;
        let capture_caps = make("capsfilter", "capture-caps")?;
        let queue = make("queue", "capture-queue")?;
        let scaling = profile.width != FIXED_CAPTURE_WIDTH || profile.height != FIXED_CAPTURE_HEIGHT;
        let videoscale = make("videoscale", "video-scaler")?;
        let scale_caps = make("capsfilter", "scale-caps")?;
        let encoder = make("mpph264enc", "h264-encoder")?;
        let parser = make("h264parse", "h264-parser")?;
        let h264_caps = make("capsfilter", "h264-caps")?;
        let sink_element = make("appsink", "encoded-frames")?;

        source.set_property("device", FIXED_CAMERA_DEVICE);
        capture_caps.set_property(
            "caps",
            gst::Caps::builder("video/x-raw")
                .field("format", "NV12")
                .field("colorimetry", FULL_RANGE_BT709_COLORIMETRY)
                .field("width", i32::from(FIXED_CAPTURE_WIDTH))
                .field("height", i32::from(FIXED_CAPTURE_HEIGHT))
                .field(
                    "framerate",
                    gst::Fraction::new(i32::from(profile.fps), 1),
                )
                .build(),
        );
        queue.set_property("max-size-buffers", 2u32);
        queue.set_property("max-size-bytes", 0u32);
        queue.set_property("max-size-time", 0u64);
        queue.set_property_from_str("leaky", "downstream");
        if scaling {
            videoscale.set_property("add-borders", false);
            scale_caps.set_property(
                "caps",
                gst::Caps::builder("video/x-raw")
                    .field("format", "NV12")
                    .field("colorimetry", FULL_RANGE_BT709_COLORIMETRY)
                    .field("width", i32::from(profile.width))
                    .field("height", i32::from(profile.height))
                    .field(
                        "framerate",
                        gst::Fraction::new(i32::from(profile.fps), 1),
                    )
                    .build(),
            );
        }
        encoder.set_property_from_str("profile", "baseline");
        encoder.set_property_from_str("level", "4");
        encoder.set_property("gop", i32::from(profile.fps));
        encoder.set_property("bps", profile.bitrate_bps);
        parser.set_property("config-interval", -1i32);
        h264_caps.set_property(
            "caps",
            gst::Caps::builder("video/x-h264")
                .field("stream-format", "byte-stream")
                .field("alignment", "au")
                .build(),
        );

        let appsink = sink_element
            .clone()
            .downcast::<gst_app::AppSink>()
            .map_err(|_| MediaError::PipelineFailed)?;
        appsink.set_max_buffers(2);
        appsink.set_drop(true);
        appsink.set_sync(false);
        appsink.set_wait_on_eos(false);

        let frames = Arc::new(BoundedFrameQueue::new(FRAME_QUEUE_CAPACITY));
        let last_pts = Arc::new(Mutex::new(None));
        let callback_frames = Arc::clone(&frames);
        let callback_pts = Arc::clone(&last_pts);
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Error)?;
                    let caps = sample.caps().ok_or(gst::FlowError::NotNegotiated)?;
                    let caps_text = caps.to_string();
                    if !caps_text.contains("video/x-h264")
                        || !caps_text.contains("stream-format=(string)byte-stream")
                        || !caps_text.contains("alignment=(string)au")
                    {
                        return Err(gst::FlowError::NotNegotiated);
                    }
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let pts = buffer.pts().ok_or(gst::FlowError::Error)?.nseconds();
                    let mut previous = callback_pts
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let media_time_90khz =
                        pts_ns_to_90khz(*previous, pts).map_err(|_| gst::FlowError::Error)?;
                    *previous = Some(pts);
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                    if map.size() == 0 || map.size() > MAX_ENCODED_FRAME_BYTES {
                        return Err(gst::FlowError::Error);
                    }
                    callback_frames.push(EncodedFrame {
                        data: Arc::<[u8]>::from(map.as_slice()),
                        media_time_90khz,
                        is_keyframe: !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT),
                    });
                    Ok(gst::FlowSuccess::Ok)
                })
                .eos({
                    let frames = Arc::clone(&frames);
                    move |_| frames.close()
                })
                .build(),
        );

        let mut chain: Vec<&gst::Element> = vec![&source, &capture_caps, &queue];
        if scaling {
            chain.push(&videoscale);
            chain.push(&scale_caps);
        }
        chain.push(&encoder);
        chain.push(&parser);
        chain.push(&h264_caps);
        chain.push(sink_element.upcast_ref());
        pipeline.add_many(&chain).map_err(|_| MediaError::PipelineFailed)?;
        gst::Element::link_many(&chain).map_err(|_| MediaError::PipelineFailed)?;

        let stopping = Arc::new(AtomicBool::new(false));
        let state = Arc::new(AtomicU8::new(state_code(CameraPipelineState::Starting)));
        let bus = pipeline.bus().ok_or(MediaError::PipelineFailed)?;
        let bus_stopping = Arc::clone(&stopping);
        let bus_state = Arc::clone(&state);
        let bus_frames = Arc::clone(&frames);
        let bus_thread = thread::Builder::new()
            .name("hyz-camera-gst-bus".to_owned())
            .spawn(move || {
                while !bus_stopping.load(Ordering::Acquire) {
                    let Some(message) = bus.timed_pop(gst::ClockTime::from_mseconds(200)) else {
                        continue;
                    };
                    match message.view() {
                        gst::MessageView::Error(_) | gst::MessageView::Eos(_) => {
                            bus_state
                                .store(state_code(CameraPipelineState::Failed), Ordering::Release);
                            bus_frames.close();
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|_| MediaError::PipelineFailed)?;

        if pipeline.set_state(gst::State::Playing).is_err() {
            stopping.store(true, Ordering::Release);
            let _ = bus_thread.join();
            let _ = pipeline.set_state(gst::State::Null);
            return Err(MediaError::PipelineFailed);
        }
        state.store(
            state_code(CameraPipelineState::Streaming),
            Ordering::Release,
        );
        match frames.pop_timeout(PIPELINE_START_DEADLINE) {
            FramePopOutcome::Frame(frame) => {
                frames.push(frame);
            }
            FramePopOutcome::Timeout | FramePopOutcome::Closed => {
                stopping.store(true, Ordering::Release);
                frames.close();
                let _ = pipeline.set_state(gst::State::Null);
                let _ = bus_thread.join();
                state.store(state_code(CameraPipelineState::Failed), Ordering::Release);
                return Err(MediaError::PipelineFailed);
            }
        }

        Ok(Box::new(GStreamerRunningMedia {
            pipeline,
            encoder,
            frames,
            state,
            stopping,
            bus_thread: Some(bus_thread),
        }))
    }
}

fn make(factory: &str, name: &str) -> Result<gst::Element, MediaError> {
    gst::ElementFactory::make(factory)
        .name(name)
        .build()
        .map_err(|_| {
            if factory == "mpph264enc" {
                MediaError::EncoderUnavailable
            } else {
                MediaError::PipelineFailed
            }
        })
}

struct GStreamerRunningMedia {
    pipeline: gst::Pipeline,
    encoder: gst::Element,
    frames: Arc<BoundedFrameQueue>,
    state: Arc<AtomicU8>,
    stopping: Arc<AtomicBool>,
    bus_thread: Option<JoinHandle<()>>,
}

struct GStreamerKeyframeRequester {
    encoder: gst::Element,
}

struct GStreamerMediaTerminator {
    pipeline: gst::Pipeline,
    frames: Arc<BoundedFrameQueue>,
    state: Arc<AtomicU8>,
    stopping: Arc<AtomicBool>,
}

impl MediaTerminator for GStreamerMediaTerminator {
    fn terminate(&self) {
        if self.stopping.swap(true, Ordering::AcqRel) {
            return;
        }
        self.state
            .store(state_code(CameraPipelineState::Stopping), Ordering::Release);
        self.frames.close();
        let next = if self.pipeline.set_state(gst::State::Null).is_ok() {
            CameraPipelineState::Stopped
        } else {
            CameraPipelineState::Failed
        };
        self.state.store(state_code(next), Ordering::Release);
    }
}

impl KeyframeRequester for GStreamerKeyframeRequester {
    fn request_keyframe(&self) -> Result<(), MediaError> {
        let structure = gst::Structure::builder("GstForceKeyUnit")
            .field("all-headers", true)
            .build();
        if self
            .encoder
            .send_event(gst::event::CustomUpstream::new(structure))
        {
            Ok(())
        } else {
            Err(MediaError::PipelineFailed)
        }
    }
}

impl RunningMedia for GStreamerRunningMedia {
    fn frames(&self) -> Arc<BoundedFrameQueue> {
        Arc::clone(&self.frames)
    }

    fn keyframe_requester(&self) -> Arc<dyn KeyframeRequester> {
        Arc::new(GStreamerKeyframeRequester {
            encoder: self.encoder.clone(),
        })
    }

    fn terminator(&self) -> Arc<dyn MediaTerminator> {
        Arc::new(GStreamerMediaTerminator {
            pipeline: self.pipeline.clone(),
            frames: Arc::clone(&self.frames),
            state: Arc::clone(&self.state),
            stopping: Arc::clone(&self.stopping),
        })
    }

    fn state(&self) -> CameraPipelineState {
        decode_state(self.state.load(Ordering::Acquire))
    }

    fn stop(mut self: Box<Self>) -> Result<(), MediaError> {
        let terminator = GStreamerMediaTerminator {
            pipeline: self.pipeline.clone(),
            frames: Arc::clone(&self.frames),
            state: Arc::clone(&self.state),
            stopping: Arc::clone(&self.stopping),
        };
        terminator.terminate();
        if let Some(thread) = self.bus_thread.take() {
            let _ = thread.join();
        }
        if self.state() == CameraPipelineState::Failed {
            Err(MediaError::PipelineFailed)
        } else {
            Ok(())
        }
    }
}

fn state_code(state: CameraPipelineState) -> u8 {
    match state {
        CameraPipelineState::Stopped => 0,
        CameraPipelineState::Starting => 1,
        CameraPipelineState::Streaming => 2,
        CameraPipelineState::Stopping => 3,
        CameraPipelineState::Failed => 4,
        CameraPipelineState::Unknown => 5,
    }
}

fn decode_state(value: u8) -> CameraPipelineState {
    match value {
        0 => CameraPipelineState::Stopped,
        1 => CameraPipelineState::Starting,
        2 => CameraPipelineState::Streaming,
        3 => CameraPipelineState::Stopping,
        4 => CameraPipelineState::Failed,
        _ => CameraPipelineState::Unknown,
    }
}
