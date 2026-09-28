//! Pure Unix domain socket transport for the router control protocol.
//!
//! Shared by the router CLI and the hyz-things portal. The server side lives
//! in the router package; only framing, connection, version checks and the
//! root-only mutation guard live here.

use serde::{de::DeserializeOwned, Serialize};
use std::{error::Error, fmt, io};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::timeout,
};

use super::router::{
    ControlError, ControlOperation, ControlRequest, ControlResponse, ControlResult,
    CLIENT_OPERATION_WAIT, CONTROL_SOCKET, IO_TIMEOUT, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};

#[derive(Debug)]
pub enum ControlClientError {
    Transport(io::Error),
    Protocol(String),
    Remote(ControlError),
}

impl ControlClientError {
    pub fn remote(&self) -> Option<&ControlError> {
        match self {
            Self::Remote(error) => Some(error),
            Self::Transport(_) | Self::Protocol(_) => None,
        }
    }
}

impl fmt::Display for ControlClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(formatter, "control transport failed: {error}"),
            Self::Protocol(message) => write!(formatter, "control protocol failed: {message}"),
            Self::Remote(error) => write!(formatter, "{}: {}", error.code, error.message),
        }
    }
}

impl Error for ControlClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Protocol(_) | Self::Remote(_) => None,
        }
    }
}

impl From<io::Error> for ControlClientError {
    fn from(value: io::Error) -> Self {
        Self::Transport(value)
    }
}

pub async fn request(operation: ControlOperation) -> Result<ControlResult, ControlClientError> {
    operation
        .validate()
        .map_err(|message| ControlClientError::Protocol(message.to_owned()))?;
    if operation.mutates() {
        require_root("control mutation")?;
    }
    let mut stream = timeout(IO_TIMEOUT, UnixStream::connect(CONTROL_SOCKET))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control connect timed out"))??;
    timeout(
        IO_TIMEOUT,
        write_frame(&mut stream, &ControlRequest::new(operation)),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control request timed out"))??;
    let response: ControlResponse =
        timeout(CLIENT_OPERATION_WAIT + IO_TIMEOUT, read_frame(&mut stream))
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "control response wait expired; operation completion is unknown",
                )
            })??;
    decode_response(response)
}

fn decode_response(response: ControlResponse) -> Result<ControlResult, ControlClientError> {
    if response.version != PROTOCOL_VERSION {
        return Err(ControlClientError::Protocol(
            "unsupported control response version".to_owned(),
        ));
    }
    response.result.map_err(ControlClientError::Remote)
}

pub fn require_root(role: &str) -> io::Result<()> {
    if effective_uid()? == 0 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{role} requires effective UID 0"),
        ))
    }
}

fn effective_uid() -> io::Result<u32> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    let line = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "process UID is unavailable",
            )
        })?;
    line.split_whitespace()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "effective UID is malformed"))
}

pub async fn read_frame<T: DeserializeOwned>(stream: &mut UnixStream) -> io::Result<T> {
    let length = stream.read_u32().await? as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control frame length is outside bounds",
        ));
    }
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub async fn write_frame<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    let payload = serde_json::to_vec(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control frame exceeds size limit",
        ));
    }
    stream.write_u32(payload.len() as u32).await?;
    stream.write_all(&payload).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_control_error_keeps_machine_readable_code() {
        let response = ControlResponse::error(
            "device_policy_generation_conflict",
            "device policy generation changed",
        );
        let error = decode_response(response).expect_err("remote failure");
        let remote = error.remote().expect("typed remote error");
        assert_eq!(remote.code, "device_policy_generation_conflict");
        assert_eq!(remote.message, "device policy generation changed");
    }

    #[test]
    fn protocol_mismatch_is_not_reported_as_a_remote_failure() {
        let response = ControlResponse {
            version: PROTOCOL_VERSION + 1,
            result: Ok(ControlResult::Completed {
                message: "ignored".to_owned(),
            }),
        };
        let error = decode_response(response).expect_err("protocol mismatch");
        assert!(matches!(error, ControlClientError::Protocol(_)));
        assert!(error.remote().is_none());
    }
}
