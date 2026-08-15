use crate::{
    application::ports::{
        CreatedWebRtcSession, KeyframeRequester, MediaTerminator, RunningWebRtcSession,
        WebRtcError, WebRtcSessionPort,
    },
    domain::{
        BoundedFrameQueue, CameraAccessScope, CameraSessionId, FramePopOutcome,
        CAMERA_UDP_PORT_END, CAMERA_UDP_PORT_START,
    },
};
use std::{
    io::ErrorKind,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
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
    media::{MediaKind, MediaTime, Mid, Pt},
    net::{Protocol, Receive},
    Candidate, Event, IceConnectionState, Input, Output, Rtc,
};

pub const MAX_SDP_BYTES: usize = 32 * 1024;
pub const MAX_SDP_LINE_BYTES: usize = 2048;
pub const MAX_CANDIDATES: usize = 32;
pub const MAX_PAYLOAD_TYPES: usize = 64;
pub const MAX_ICE_CREDENTIAL_BYTES: usize = 256;
pub const NEGOTIATION_DEADLINE: Duration = Duration::from_secs(30);
pub const TRANSPORT_IDLE_DEADLINE: Duration = Duration::from_secs(90);
const DRIVER_POLL_SLICE: Duration = Duration::from_millis(10);
const UDP_BUFFER_BYTES: usize = 2048;

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
        validate_offer_sdp(offer_sdp)
    }

    fn create(
        &self,
        session_id: &CameraSessionId,
        scope: CameraAccessScope,
        offer_sdp: &str,
        frames: Arc<BoundedFrameQueue>,
        keyframe_requester: Arc<dyn KeyframeRequester>,
        media_terminator: Arc<dyn MediaTerminator>,
    ) -> Result<CreatedWebRtcSession, WebRtcError> {
        validate_offer_sdp(offer_sdp)?;
        let offer =
            SdpOffer::from_sdp_string(offer_sdp).map_err(|_| WebRtcError::UnsupportedSdp)?;
        let (socket, port) = bind_fixed_udp_port(scope.address())?;
        socket
            .set_nonblocking(true)
            .map_err(|_| WebRtcError::TransportFailed)?;
        let candidate_addr = SocketAddr::new(scope.address().into(), port);

        let mut rtc = Rtc::new(Instant::now());
        let candidate =
            Candidate::host(candidate_addr, "udp").map_err(|_| WebRtcError::NegotiationFailed)?;
        rtc.add_local_candidate(candidate);
        let answer = rtc
            .sdp_api()
            .accept_offer(offer)
            .map_err(|_| WebRtcError::NegotiationFailed)?;
        let answer_sdp = answer.to_sdp_string();
        if answer_sdp.len() > MAX_SDP_BYTES {
            return Err(WebRtcError::NegotiationFailed);
        }

        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let thread_name = format!("hyz-camera-webrtc-{}", &session_id.as_str()[..8]);
        let join = thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                drive_session(
                    rtc,
                    socket,
                    candidate_addr,
                    frames,
                    keyframe_requester,
                    media_terminator,
                    shutdown_rx,
                )
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
            Err(_) => return Err(WebRtcError::TransportFailed),
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

fn drive_session(
    mut rtc: Rtc,
    socket: UdpSocket,
    candidate_addr: SocketAddr,
    frames: Arc<BoundedFrameQueue>,
    keyframe_requester: Arc<dyn KeyframeRequester>,
    media_terminator: Arc<dyn MediaTerminator>,
    shutdown_rx: mpsc::Receiver<()>,
) -> Result<(), WebRtcError> {
    let _termination_guard = MediaTerminationGuard(media_terminator);
    let started = Instant::now();
    let mut last_transport_activity = started;
    let mut connected = false;
    let mut waiting_for_keyframe = true;
    let mut video_mid: Option<Mid> = None;
    let mut video_pt: Option<Pt> = None;
    let mut recv_buf = [0u8; UDP_BUFFER_BYTES];

    loop {
        let timeout = loop {
            match rtc
                .poll_output()
                .map_err(|_| WebRtcError::TransportFailed)?
            {
                Output::Transmit(transmit) => {
                    socket
                        .send_to(&transmit.contents, transmit.destination)
                        .map_err(|_| WebRtcError::TransportFailed)?;
                }
                Output::Event(event) => match event {
                    Event::Connected => {
                        connected = true;
                        waiting_for_keyframe = true;
                        let _ = keyframe_requester.request_keyframe();
                    }
                    Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                        return Ok(());
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
                        }
                    }
                    Event::KeyframeRequest(_) => {
                        let _ = keyframe_requester.request_keyframe();
                    }
                    _ => {}
                },
                Output::Timeout(at) => break at,
            }
        };

        if shutdown_rx.try_recv().is_ok() {
            return Ok(());
        }
        let now = Instant::now();
        if !connected && now.duration_since(started) >= NEGOTIATION_DEADLINE {
            return Err(WebRtcError::TransportFailed);
        }
        if connected && now.duration_since(last_transport_activity) >= TRANSPORT_IDLE_DEADLINE {
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
                            .map_err(|_| WebRtcError::TransportFailed)?;
                        waiting_for_keyframe = false;
                        continue;
                    }
                    waiting_for_keyframe = true;
                    let _ = keyframe_requester.request_keyframe();
                }
                FramePopOutcome::Closed => return Ok(()),
                FramePopOutcome::Timeout => {}
            }
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

pub fn validate_offer_sdp(sdp: &str) -> Result<(), WebRtcError> {
    if sdp.len() > MAX_SDP_BYTES {
        return Err(WebRtcError::SdpTooLarge);
    }
    if sdp.is_empty() || sdp.contains('\0') {
        return Err(WebRtcError::UnsupportedSdp);
    }

    let mut video_lines = 0usize;
    let mut audio_lines = 0usize;
    let mut application_lines = 0usize;
    let mut candidates = 0usize;
    let mut payload_types = 0usize;
    let mut has_h264 = false;
    let mut has_recv_direction = false;
    let mut has_sha256_fingerprint = false;
    let mut ice_ufrag = 0usize;
    let mut ice_pwd = 0usize;

    for raw_line in sdp.split_terminator('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() || line.len() > MAX_SDP_LINE_BYTES {
            return Err(WebRtcError::UnsupportedSdp);
        }
        if let Some(rest) = line.strip_prefix("m=video ") {
            video_lines += 1;
            let fields: Vec<_> = rest.split_ascii_whitespace().collect();
            // m=<media> <port> <proto> <fmt>...；payload 类型从下标 2 开始。
            if fields.len() < 3 {
                return Err(WebRtcError::UnsupportedSdp);
            }
            payload_types = fields[2..].len();
        } else if line.starts_with("m=audio ") {
            audio_lines += 1;
        } else if line.starts_with("m=application ") {
            application_lines += 1;
        } else if line.starts_with("m=") {
            return Err(WebRtcError::UnsupportedSdp);
        } else if line.starts_with("a=rtpmap:") && line.to_ascii_uppercase().contains(" H264/90000")
        {
            has_h264 = true;
        } else if matches!(line, "a=recvonly" | "a=sendrecv") {
            has_recv_direction = true;
        } else if let Some(value) = line.strip_prefix("a=fingerprint:") {
            has_sha256_fingerprint |= value
                .split_ascii_whitespace()
                .next()
                .is_some_and(|algorithm| algorithm.eq_ignore_ascii_case("sha-256"));
        } else if let Some(value) = line.strip_prefix("a=ice-ufrag:") {
            ice_ufrag += 1;
            if value.is_empty() || value.len() > MAX_ICE_CREDENTIAL_BYTES {
                return Err(WebRtcError::UnsupportedSdp);
            }
        } else if let Some(value) = line.strip_prefix("a=ice-pwd:") {
            ice_pwd += 1;
            if value.is_empty() || value.len() > MAX_ICE_CREDENTIAL_BYTES {
                return Err(WebRtcError::UnsupportedSdp);
            }
        } else if line.starts_with("a=candidate:") {
            candidates += 1;
        }
    }

    if video_lines != 1
        || audio_lines != 0
        || application_lines != 0
        || !has_h264
        || !has_recv_direction
        || !has_sha256_fingerprint
        || ice_ufrag != 1
        || ice_pwd != 1
        || candidates == 0
        || candidates > MAX_CANDIDATES
        || payload_types == 0
        || payload_types > MAX_PAYLOAD_TYPES
    {
        return Err(WebRtcError::UnsupportedSdp);
    }
    Ok(())
}
