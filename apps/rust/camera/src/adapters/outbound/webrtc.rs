use crate::{
    application::ports::{
        CreatedWebRtcSession, KeyframeRequester, MediaTerminator, RunningWebRtcSession,
        SessionAudio, SessionCreate, WebRtcError, WebRtcSessionPort,
    },
    domain::{
        validate_offer_sdp, AudioFrame, AudioPopOutcome, BoundedAudioQueue, BoundedFrameQueue,
        FramePopOutcome, SdpValidationError, CAMERA_UDP_PORT_END, CAMERA_UDP_PORT_START,
        MAX_SDP_BYTES,
    },
};
use std::{
    io::ErrorKind,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    os::fd::AsRawFd,
    sync::{
        mpsc::{self, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use str0m::{
    change::SdpOffer,
    format::Codec,
    media::{Frequency, MediaKind, MediaTime, Mid, Pt},
    net::{Protocol, Receive},
    Candidate, Event, IceConnectionState, Input, Output, Rtc,
};

pub const NEGOTIATION_DEADLINE: Duration = Duration::from_secs(30);
pub const TRANSPORT_IDLE_DEADLINE: Duration = Duration::from_secs(90);
const DRIVER_POLL_SLICE: Duration = Duration::from_millis(10);
const UDP_BUFFER_BYTES: usize = 2048;
/// 4K 高码率下单个 RTP 突发可超过内核默认发送缓冲（~212 KiB）；调大并保留
/// 突发余量，避免 send_to 频繁 EAGAIN。
const SEND_BUFFER_BYTES: usize = 1 << 20;
/// send_to 返回 WouldBlock 时的重试上限（10ms × 20 = 200ms），之后才视为传输失败。
const SEND_RETRY_ATTEMPTS: usize = 20;

fn map_sdp_validation_error(error: SdpValidationError) -> WebRtcError {
    match error {
        SdpValidationError::TooLarge => WebRtcError::SdpTooLarge,
        SdpValidationError::Unsupported => WebRtcError::UnsupportedSdp,
    }
}

pub struct Str0mWebRtcAdapter;

impl Str0mWebRtcAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Str0mWebRtcAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl WebRtcSessionPort for Str0mWebRtcAdapter {
    fn validate_offer(&self, offer_sdp: &str) -> Result<(), WebRtcError> {
        validate_offer_sdp(offer_sdp).map_err(map_sdp_validation_error)
    }

    fn create(&self, session: SessionCreate) -> Result<CreatedWebRtcSession, WebRtcError> {
        let SessionCreate {
            session_id,
            scope,
            offer_sdp,
            frames,
            keyframe_requester,
            media_terminator,
            audio,
        } = session;
        validate_offer_sdp(&offer_sdp).map_err(map_sdp_validation_error)?;
        let offer =
            SdpOffer::from_sdp_string(&offer_sdp).map_err(|_| WebRtcError::UnsupportedSdp)?;
        let (socket, port) = bind_fixed_udp_port(scope.address())?;
        socket
            .set_nonblocking(true)
            .map_err(|_| WebRtcError::TransportFailed)?;
        let buffer_size: libc::c_int = SEND_BUFFER_BYTES as libc::c_int;
        let setsockopt = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                &buffer_size as *const libc::c_int as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if setsockopt != 0 {
            eprintln!("camera: SO_SNDBUF failed for session {session_id}");
            return Err(WebRtcError::TransportFailed);
        }
        let candidate_addr = SocketAddr::new(scope.address().into(), port);

        let mut rtc = Rtc::new(Instant::now());
        let candidate =
            Candidate::host(candidate_addr, "udp").map_err(|_| WebRtcError::NegotiationFailed)?;
        rtc.add_local_candidate(candidate);
        let answer = rtc.sdp_api().accept_offer(offer).map_err(|_| {
            eprintln!("camera: accept_offer failed for session {session_id}");
            WebRtcError::NegotiationFailed
        })?;
        let answer_sdp = answer.to_sdp_string();
        if answer_sdp.len() > MAX_SDP_BYTES {
            return Err(WebRtcError::NegotiationFailed);
        }

        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let thread_name = format!("hyz-camera-webrtc-{}", &session_id.as_str()[..8]);
        let driver = SessionDriver {
            session_id: session_id.as_str().to_owned(),
            frames,
            keyframe_requester,
            media_terminator,
            audio,
        };
        let join = thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let result = drive_session(rtc, socket, candidate_addr, driver, shutdown_rx);
                if let Err(error) = &result {
                    eprintln!("session thread {session_id}: exited with error: {error:?}");
                }
                result
            })
            .map_err(|_| WebRtcError::TransportFailed)?;

        Ok(CreatedWebRtcSession {
            answer_sdp,
            running: Box::new(Str0mRunningSession {
                shutdown_tx,
                join: Some(join),
            }),
        })
    }
}

fn bind_fixed_udp_port(address: Ipv4Addr) -> Result<(UdpSocket, u16), WebRtcError> {
    for port in CAMERA_UDP_PORT_START..=CAMERA_UDP_PORT_END {
        match UdpSocket::bind(SocketAddr::from((address, port))) {
            Ok(socket) => return Ok((socket, port)),
            Err(error) if error.kind() == ErrorKind::AddrInUse => continue,
            Err(error) => {
                eprintln!("camera: UDP bind {address}:{port} failed: {error}");
                return Err(WebRtcError::TransportFailed);
            }
        }
    }
    Err(WebRtcError::ResourceExhausted)
}

struct Str0mRunningSession {
    shutdown_tx: Sender<()>,
    join: Option<JoinHandle<Result<(), WebRtcError>>>,
}

impl RunningWebRtcSession for Str0mRunningSession {
    fn is_finished(&self) -> bool {
        self.join.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn stop(mut self: Box<Self>) -> Result<(), WebRtcError> {
        let _ = self.shutdown_tx.send(());
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        join.join().map_err(|_| WebRtcError::TransportFailed)?
    }
}

struct MediaTerminationGuard(Arc<dyn MediaTerminator>);

impl Drop for MediaTerminationGuard {
    fn drop(&mut self) {
        self.0.terminate();
    }
}

/// `drive_session` 的会话参数（收敛签名，避免过多参数）。
struct SessionDriver {
    session_id: String,
    frames: Arc<BoundedFrameQueue>,
    keyframe_requester: Arc<dyn KeyframeRequester>,
    media_terminator: Arc<dyn MediaTerminator>,
    audio: Option<SessionAudio>,
}

fn drive_session(
    mut rtc: Rtc,
    socket: UdpSocket,
    candidate_addr: SocketAddr,
    driver: SessionDriver,
    shutdown_rx: mpsc::Receiver<()>,
) -> Result<(), WebRtcError> {
    let SessionDriver {
        session_id,
        frames,
        keyframe_requester,
        media_terminator,
        audio,
    } = driver;
    let _termination_guard = MediaTerminationGuard(media_terminator);
    // 音频引用计数 guard：会话线程退出时递减，最后一个音频会话退出时停止音频管线。
    let _audio_termination_guard = audio
        .as_ref()
        .map(|session_audio| MediaTerminationGuard(Arc::clone(&session_audio.media_terminator)));
    let audio_frames = audio
        .as_ref()
        .map(|session_audio| Arc::clone(&session_audio.frames));
    let audio_sink = audio
        .as_ref()
        .map(|session_audio| Arc::clone(&session_audio.sink));

    let started = Instant::now();
    let mut last_transport_activity = started;
    let mut connected = false;
    let mut waiting_for_keyframe = true;
    let mut video_mid: Option<Mid> = None;
    let mut video_pt: Option<Pt> = None;
    let mut audio_mid: Option<Mid> = None;
    let mut audio_pt: Option<Pt> = None;
    let mut recv_buf = [0u8; UDP_BUFFER_BYTES];

    loop {
        let timeout = loop {
            match rtc
                .poll_output()
                .map_err(|_| WebRtcError::TransportFailed)?
            {
                Output::Transmit(transmit) => {
                    let destination = transmit.destination;
                    let mut attempts = 0;
                    loop {
                        match socket.send_to(&transmit.contents, destination) {
                            Ok(_) => break,
                            Err(error)
                                if error.kind() == ErrorKind::WouldBlock
                                    && attempts < SEND_RETRY_ATTEMPTS =>
                            {
                                attempts += 1;
                                thread::sleep(DRIVER_POLL_SLICE);
                            }
                            Err(error) => {
                                eprintln!("session {session_id}: udp send_to failed: {error}");
                                return Err(WebRtcError::TransportFailed);
                            }
                        }
                    }
                }
                Output::Event(event) => match event {
                    Event::Connected => {
                        connected = true;
                        waiting_for_keyframe = true;
                        let _ = keyframe_requester.request_keyframe();
                    }
                    Event::IceConnectionStateChange(state) => {
                        eprintln!("session {session_id}: ice state change to {state:?}");
                        if state == IceConnectionState::Disconnected {
                            return Ok(());
                        }
                    }
                    Event::MediaAdded(media) => {
                        if media.kind == MediaKind::Video && video_mid.is_none() {
                            video_mid = Some(media.mid);
                            video_pt = rtc.writer(media.mid).and_then(|writer| {
                                writer
                                    .payload_params()
                                    .find(|params| params.spec().codec == Codec::H264)
                                    .map(|params| params.pt())
                            });
                        } else if media.kind == MediaKind::Audio
                            && audio_mid.is_none()
                            && audio_frames.is_some()
                        {
                            audio_mid = Some(media.mid);
                            audio_pt = rtc.writer(media.mid).and_then(|writer| {
                                writer
                                    .payload_params()
                                    .find(|params| params.spec().codec == Codec::Opus)
                                    .map(|params| params.pt())
                            });
                        }
                    }
                    Event::KeyframeRequest(_) => {
                        let _ = keyframe_requester.request_keyframe();
                    }
                    Event::MediaData(media) => {
                        if Some(media.mid) == audio_mid {
                            if let Some(sink) = &audio_sink {
                                sink.push_opus(
                                    &session_id,
                                    AudioFrame {
                                        data: Arc::from(media.data),
                                        media_time_48khz: media
                                            .time
                                            .rebase(Frequency::FORTY_EIGHT_KHZ)
                                            .numer(),
                                    },
                                );
                            }
                        }
                    }
                    _ => {}
                },
                Output::Timeout(at) => break at,
            }
        };

        if shutdown_rx.try_recv().is_ok() {
            eprintln!("session {session_id}: shutdown requested");
            return Ok(());
        }
        let now = Instant::now();
        if !connected && now.duration_since(started) >= NEGOTIATION_DEADLINE {
            eprintln!("session {session_id}: negotiation deadline exceeded");
            return Err(WebRtcError::TransportFailed);
        }
        if connected && now.duration_since(last_transport_activity) >= TRANSPORT_IDLE_DEADLINE {
            eprintln!("session {session_id}: transport idle deadline exceeded");
            return Ok(());
        }

        if connected {
            match frames.pop_timeout(Duration::ZERO) {
                FramePopOutcome::Frame(frame) => {
                    if waiting_for_keyframe && !frame.is_keyframe {
                        continue;
                    }
                    if let (Some(mid), Some(pt)) = (video_mid, video_pt) {
                        let writer = rtc.writer(mid).ok_or(WebRtcError::TransportFailed)?;
                        writer
                            .write(
                                pt,
                                Instant::now(),
                                MediaTime::from_90khz(frame.media_time_90khz),
                                frame.data.to_vec(),
                            )
                            .map_err(|error| {
                                eprintln!(
                                    "session {session_id}: video writer.write failed: {error:?}"
                                );
                                WebRtcError::TransportFailed
                            })?;
                        waiting_for_keyframe = false;
                        continue;
                    }
                    waiting_for_keyframe = true;
                    let _ = keyframe_requester.request_keyframe();
                }
                FramePopOutcome::Closed => {
                    eprintln!("session {session_id}: video frame queue closed");
                    return Ok(());
                }
                FramePopOutcome::Timeout => {}
            }
            drain_audio(&mut rtc, audio_mid, audio_pt, &audio_frames)?;
        }

        match socket.recv_from(&mut recv_buf) {
            Ok((length, source)) => {
                let receive =
                    Receive::new(Protocol::Udp, source, candidate_addr, &recv_buf[..length])
                        .map_err(|_| WebRtcError::TransportFailed)?;
                rtc.handle_input(Input::Receive(Instant::now(), receive))
                    .map_err(|_| WebRtcError::TransportFailed)?;
                last_transport_activity = Instant::now();
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                let now = Instant::now();
                if timeout <= now {
                    rtc.handle_input(Input::Timeout(now))
                        .map_err(|_| WebRtcError::TransportFailed)?;
                } else {
                    thread::sleep((timeout - now).min(DRIVER_POLL_SLICE));
                }
            }
            Err(_) => return Err(WebRtcError::TransportFailed),
        }
    }
}

/// 从采集队列取 Opus AU 写入音频 writer（48 kHz RTP 时钟）。音频管线关闭
/// （`Closed`）时停止发送但会话继续（video-only 降级）。
fn drain_audio(
    rtc: &mut Rtc,
    audio_mid: Option<Mid>,
    audio_pt: Option<Pt>,
    audio_frames: &Option<Arc<BoundedAudioQueue>>,
) -> Result<(), WebRtcError> {
    let (Some(mid), Some(pt), Some(frames)) = (audio_mid, audio_pt, audio_frames.as_ref()) else {
        return Ok(());
    };
    loop {
        match frames.pop_timeout(Duration::ZERO) {
            AudioPopOutcome::Frame(frame) => {
                let writer = rtc.writer(mid).ok_or(WebRtcError::TransportFailed)?;
                writer
                    .write(
                        pt,
                        Instant::now(),
                        MediaTime::new(frame.media_time_48khz, Frequency::FORTY_EIGHT_KHZ),
                        frame.data.to_vec(),
                    )
                    .map_err(|_| WebRtcError::TransportFailed)?;
            }
            AudioPopOutcome::Timeout | AudioPopOutcome::Closed => return Ok(()),
        }
    }
}
