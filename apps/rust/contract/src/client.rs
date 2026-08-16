//! Pure Unix domain socket transport for the router control protocol.
//!
//! Shared by the router CLI and the hyz-things portal. The server side lives
//! in the router package; only framing, connection, version checks and the
//! root-only mutation guard live here.

use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::timeout,
};

use super::router::{
    ControlOperation, ControlRequest, ControlResponse, ControlResult, CLIENT_OPERATION_WAIT,
    CONTROL_SOCKET, IO_TIMEOUT, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};

pub async fn request(operation: ControlOperation) -> std::io::Result<ControlResult> {
    operation
        .validate()
        .map_err(|message| std::io::Error::new(std::io::ErrorKind::InvalidInput, message))?;
    if operation.mutates() {
        require_root("control mutation")?;
    }
    let mut stream = timeout(IO_TIMEOUT, UnixStream::connect(CONTROL_SOCKET))
        .await
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "control connect timed out")
        })??;
    timeout(
        IO_TIMEOUT,
        write_frame(&mut stream, &ControlRequest::new(operation)),
    )
    .await
    .map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::TimedOut, "control request timed out")
    })??;
    let response: ControlResponse =
        timeout(CLIENT_OPERATION_WAIT + IO_TIMEOUT, read_frame(&mut stream))
            .await
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "control response wait expired; operation completion is unknown",
                )
            })??;
    if response.version != PROTOCOL_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsupported control response version",
        ));
    }
    response
        .result
        .map_err(|error| std::io::Error::other(format!("{}: {}", error.code, error.message)))
}

pub fn require_root(role: &str) -> std::io::Result<()> {
    if effective_uid()? == 0 {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{role} requires effective UID 0"),
        ))
    }
}

fn effective_uid() -> std::io::Result<u32> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    let line = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "process UID is unavailable",
            )
        })?;
    line.split_whitespace()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "effective UID is malformed",
            )
        })
}

pub async fn read_frame<T: DeserializeOwned>(stream: &mut UnixStream) -> std::io::Result<T> {
    let length = stream.read_u32().await? as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "control frame length is outside bounds",
        ));
    }
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

pub async fn write_frame<T: Serialize>(stream: &mut UnixStream, value: &T) -> std::io::Result<()> {
    let payload = serde_json::to_vec(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "control frame exceeds size limit",
        ));
    }
    stream.write_u32(payload.len() as u32).await?;
    stream.write_all(&payload).await?;
    stream.flush().await
}
