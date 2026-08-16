use crate::{
    application::{CameraApplication, CameraApplicationError, CreateSessionResult},
    domain::CameraSessionId,
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::Path,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

pub use hyz_contract::camera::{
    CloseSessionRequest, ControlErrorCode, ControlErrorResponse, ControlOperation, ControlOutcome,
    ControlRequest, ControlResponse, ControlResult, CreateSessionRequest, SessionCreatedResponse,
    SetProfileRequest, SetRotationRequest, CONTROL_PROTOCOL_VERSION, CONTROL_REQUEST_DEADLINE,
    CONTROL_SOCKET_PATH, MAX_CONTROL_FRAME_BYTES,
};
pub const CONTROL_OWNER_PATH: &str = "/run/hyz-camera/daemon.owner";
pub const CONTROL_RUNTIME_DIRECTORY: &str = "/run/hyz-camera";
const CONTROL_ACCEPT_POLL: Duration = Duration::from_millis(50);

impl From<CreateSessionResult> for SessionCreatedResponse {
    fn from(result: CreateSessionResult) -> Self {
        Self {
            session_id: result.session_id,
            answer_sdp: result.answer_sdp,
            negotiation_timeout_seconds: result.negotiation_timeout_seconds,
        }
    }
}

pub fn decode_control_request(bytes: &[u8]) -> Result<ControlRequest, ControlErrorCode> {
    if bytes.len() > MAX_CONTROL_FRAME_BYTES {
        return Err(ControlErrorCode::RequestTooLarge);
    }
    let request: ControlRequest =
        serde_json::from_slice(bytes).map_err(|_| ControlErrorCode::InvalidRequest)?;
    if request.version != CONTROL_PROTOCOL_VERSION {
        return Err(ControlErrorCode::InvalidVersion);
    }
    Ok(request)
}

pub fn serve_root_control(
    application: Arc<CameraApplication>,
    shutdown: Arc<AtomicBool>,
    generation: &str,
) -> io::Result<()> {
    let owner_value = format!("{} {generation}\n", std::process::id());
    let (listener, mut ownership) = bind_root_control_socket(&owner_value)?;
    listener.set_nonblocking(true)?;
    while !shutdown.load(Ordering::Acquire) {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(CONTROL_ACCEPT_POLL);
                continue;
            }
            Err(error) => return Err(error),
        };
        if peer_uid(&stream)? != 0 {
            let response = error_response(ControlErrorCode::PermissionDenied);
            let _ = write_response(&mut stream, &response);
            continue;
        }
        stream.set_read_timeout(Some(CONTROL_REQUEST_DEADLINE))?;
        stream.set_write_timeout(Some(CONTROL_REQUEST_DEADLINE))?;
        let response = match read_request(&mut stream)
            .and_then(|request| handle_request(&application, &shutdown, request))
        {
            Ok(response) => response,
            Err(code) => error_response(code),
        };
        write_response(&mut stream, &response)?;
    }
    application
        .shutdown()
        .map_err(|error| io::Error::other(format!("camera shutdown failed: {error}")))?;
    drop(listener);
    ownership.cleanup()
}

struct ControlOwnership {
    owner_value: String,
    active: bool,
}

impl ControlOwnership {
    fn cleanup(&mut self) -> io::Result<()> {
        verify_owned_runtime(&self.owner_value)?;
        fs::remove_file(CONTROL_SOCKET_PATH)?;
        fs::remove_file(CONTROL_OWNER_PATH)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for ControlOwnership {
    fn drop(&mut self) {
        if self.active {
            let _ = self.cleanup();
        }
    }
}

fn handle_request(
    application: &CameraApplication,
    shutdown: &AtomicBool,
    request: ControlRequest,
) -> Result<ControlResponse, ControlErrorCode> {
    let result = match request.operation {
        ControlOperation::Status => ControlResult::Status(application.status()),
        ControlOperation::CreateSession(request) => {
            let created = application
                .create_session(request.scope, request.address, &request.offer_sdp)
                .map_err(|error| {
                    eprintln!("camera: session create rejected: {error:?}");
                    map_application_error(error)
                })?;
            ControlResult::SessionCreated(created.into())
        }
        ControlOperation::CloseSession(request) => {
            let id = CameraSessionId::from_str(&request.session_id)
                .map_err(|_| ControlErrorCode::UnknownSession)?;
            application
                .close_session(&id)
                .map_err(map_application_error)?;
            ControlResult::SessionClosed
        }
        ControlOperation::SetProfile(request) => {
            application
                .set_profile(request.preset)
                .map_err(map_application_error)?;
            ControlResult::ProfileSet
        }
        ControlOperation::SetRotation(request) => {
            application
                .set_rotation(request.rotation)
                .map_err(map_application_error)?;
            ControlResult::RotationSet
        }
        ControlOperation::Shutdown => {
            application.shutdown().map_err(map_application_error)?;
            shutdown.store(true, Ordering::Release);
            ControlResult::ShutdownAccepted
        }
    };
    Ok(ControlResponse {
        version: CONTROL_PROTOCOL_VERSION,
        outcome: ControlOutcome::Ok(result),
    })
}

fn read_request(stream: &mut UnixStream) -> Result<ControlRequest, ControlErrorCode> {
    let mut length = [0u8; 4];
    stream
        .read_exact(&mut length)
        .map_err(|_| ControlErrorCode::InvalidRequest)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_CONTROL_FRAME_BYTES {
        return Err(ControlErrorCode::RequestTooLarge);
    }
    let mut body = vec![0u8; length];
    stream
        .read_exact(&mut body)
        .map_err(|_| ControlErrorCode::InvalidRequest)?;
    decode_control_request(&body)
}

fn write_response(stream: &mut UnixStream, response: &ControlResponse) -> io::Result<()> {
    let body =
        serde_json::to_vec(response).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    if body.len() > MAX_CONTROL_FRAME_BYTES {
        return Err(io::Error::from(io::ErrorKind::OutOfMemory));
    }
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(&body)
}

fn error_response(code: ControlErrorCode) -> ControlResponse {
    ControlResponse {
        version: CONTROL_PROTOCOL_VERSION,
        outcome: ControlOutcome::Error(ControlErrorResponse { code }),
    }
}

fn map_application_error(error: CameraApplicationError) -> ControlErrorCode {
    use crate::application::ports::{MediaError, WebRtcError};
    match error {
        CameraApplicationError::InvalidAccessScope => ControlErrorCode::InvalidAccessScope,
        CameraApplicationError::InvalidProfile => ControlErrorCode::InvalidRequest,
        CameraApplicationError::ShuttingDown => ControlErrorCode::ShuttingDown,
        CameraApplicationError::TooManyViewers => ControlErrorCode::ResourceExhausted,
        CameraApplicationError::SessionBusy => ControlErrorCode::SessionBusy,
        CameraApplicationError::UnknownSession => ControlErrorCode::UnknownSession,
        CameraApplicationError::Media(MediaError::CameraNotFound) => {
            ControlErrorCode::CameraNotFound
        }
        CameraApplicationError::Media(MediaError::CameraBusy) => ControlErrorCode::CameraBusy,
        CameraApplicationError::Media(MediaError::EncoderUnavailable) => {
            ControlErrorCode::EncoderUnavailable
        }
        CameraApplicationError::Media(_) => ControlErrorCode::MediaPipelineFailed,
        CameraApplicationError::WebRtc(WebRtcError::UnsupportedSdp | WebRtcError::SdpTooLarge) => {
            ControlErrorCode::UnsupportedSdp
        }
        CameraApplicationError::WebRtc(WebRtcError::ResourceExhausted) => {
            ControlErrorCode::ResourceExhausted
        }
        CameraApplicationError::WebRtc(WebRtcError::NegotiationFailed) => {
            ControlErrorCode::WebRtcNegotiationFailed
        }
        CameraApplicationError::WebRtc(WebRtcError::TransportFailed) => {
            ControlErrorCode::WebRtcTransportFailed
        }
        CameraApplicationError::InvalidGeneration | CameraApplicationError::RandomUnavailable => {
            ControlErrorCode::Internal
        }
    }
}

fn bind_root_control_socket(owner_value: &str) -> io::Result<(UnixListener, ControlOwnership)> {
    let directory = Path::new(CONTROL_RUNTIME_DIRECTORY);
    match fs::symlink_metadata(directory) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() || metadata.uid() != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unsafe camera runtime directory",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(directory)?;
        }
        Err(error) => return Err(error),
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;

    for path in [CONTROL_SOCKET_PATH, CONTROL_OWNER_PATH] {
        match fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "camera runtime ownership already exists",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }

    let listener = UnixListener::bind(CONTROL_SOCKET_PATH)?;
    fs::set_permissions(CONTROL_SOCKET_PATH, fs::Permissions::from_mode(0o600))?;
    let owner_result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(CONTROL_OWNER_PATH)
        .and_then(|mut owner| {
            owner.write_all(owner_value.as_bytes())?;
            owner.sync_all()
        });
    if let Err(error) = owner_result {
        let _ = fs::remove_file(CONTROL_SOCKET_PATH);
        return Err(error);
    }
    Ok((
        listener,
        ControlOwnership {
            owner_value: owner_value.to_owned(),
            active: true,
        },
    ))
}

fn verify_owned_runtime(owner_value: &str) -> io::Result<()> {
    let socket = fs::symlink_metadata(CONTROL_SOCKET_PATH)?;
    if socket.file_type().is_symlink() || !socket.file_type().is_socket() || socket.uid() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "camera control socket ownership changed",
        ));
    }
    let owner = fs::symlink_metadata(CONTROL_OWNER_PATH)?;
    if owner.file_type().is_symlink() || !owner.is_file() || owner.uid() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "camera daemon owner file is unsafe",
        ));
    }
    let observed = fs::read_to_string(CONTROL_OWNER_PATH)?;
    if observed != owner_value {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "camera daemon ownership changed",
        ));
    }
    Ok(())
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result == 0 && length as usize == std::mem::size_of::<libc::ucred>() {
        Ok(credentials.uid)
    } else {
        Err(io::Error::last_os_error())
    }
}
