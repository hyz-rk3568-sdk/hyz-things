//! Router root-only Unix control server and shared protocol entry points.
//!
//! The wire contract (types, validation, client transport) lives in
//! `hyz-contract`; this module owns the daemon-side socket lifecycle,
//! ownership, accept loop and peer-credential gate, plus the `ControlHandler`
//! trait the router's composition root implements.

use async_trait::async_trait;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{watch, Semaphore},
    task::JoinSet,
    time::timeout,
};

pub use hyz_contract::client::{read_frame, request, require_root, write_frame};
pub use hyz_contract::router::{
    ControlError, ControlOperation, ControlProxyMode, ControlRequest, ControlResponse,
    ControlResult, CONTROL_SOCKET, IO_TIMEOUT, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};

pub const CONTROL_RUNTIME_DIR: &str = "/run/hyz-router";
pub const DAEMON_LOCK_DIR: &str = "/run/hyz-router/daemon.lock";
const DAEMON_OWNER_FILE: &str = "/run/hyz-router/daemon.lock/owner";
const MAX_CONNECTIONS: usize = 16;

#[async_trait]
pub trait ControlHandler: Send + Sync + 'static {
    async fn handle(&self, operation: ControlOperation) -> Result<ControlResult, String>;
}

pub struct DaemonOwnership {
    identity: String,
    directory_device: u64,
    directory_inode: u64,
    socket_identity: Option<(u64, u64)>,
}

pub fn acquire_daemon_ownership() -> io::Result<DaemonOwnership> {
    ensure_runtime_directory()?;
    match fs::DirBuilder::new().mode(0o700).create(DAEMON_LOCK_DIR) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "daemon ownership lock already exists; stale or malformed locks require explicit operator removal",
            ));
        }
        Err(error) => return Err(error),
    }
    let result: io::Result<DaemonOwnership> = (|| {
        fs::set_permissions(DAEMON_LOCK_DIR, fs::Permissions::from_mode(0o700))?;
        let directory = fs::symlink_metadata(DAEMON_LOCK_DIR)?;
        if directory.uid() != 0
            || directory.mode() & 0o777 != 0o700
            || !directory.file_type().is_dir()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon lock must be a root-owned mode 0700 directory",
            ));
        }
        let identity = current_daemon_identity()?;
        let mut owner = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(DAEMON_OWNER_FILE)?;
        owner.write_all(identity.as_bytes())?;
        owner.sync_all()?;
        let owner_metadata = fs::symlink_metadata(DAEMON_OWNER_FILE)?;
        if owner_metadata.uid() != 0
            || owner_metadata.mode() & 0o777 != 0o600
            || !owner_metadata.file_type().is_file()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon owner identity must be a root-owned mode 0600 file",
            ));
        }
        Ok(DaemonOwnership {
            identity,
            directory_device: directory.dev(),
            directory_inode: directory.ino(),
            socket_identity: None,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(DAEMON_OWNER_FILE);
        let _ = fs::remove_dir(DAEMON_LOCK_DIR);
    }
    result
}

impl DaemonOwnership {
    fn verify_owned(&self) -> io::Result<()> {
        if current_daemon_identity()? != self.identity {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon process identity changed",
            ));
        }
        let directory = fs::symlink_metadata(DAEMON_LOCK_DIR)?;
        if directory.uid() != 0
            || directory.mode() & 0o777 != 0o700
            || !directory.file_type().is_dir()
            || directory.dev() != self.directory_device
            || directory.ino() != self.directory_inode
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon lock directory identity changed",
            ));
        }
        let owner = fs::symlink_metadata(DAEMON_OWNER_FILE)?;
        if owner.uid() != 0
            || owner.mode() & 0o777 != 0o600
            || !owner.file_type().is_file()
            || owner.len() > 8192
            || fs::read_to_string(DAEMON_OWNER_FILE)? != self.identity
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon owner identity changed",
            ));
        }
        Ok(())
    }

    pub fn release(self) -> io::Result<()> {
        self.verify_owned()?;
        if self.socket_identity.is_some() {
            match fs::symlink_metadata(CONTROL_SOCKET) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "refusing to release daemon ownership while the control socket exists",
                    ));
                }
                Err(error) => return Err(error),
            }
        }
        fs::remove_file(DAEMON_OWNER_FILE)?;
        fs::remove_dir(DAEMON_LOCK_DIR)
    }
}

fn current_daemon_identity() -> io::Result<String> {
    let pid = std::process::id();
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let end = stat.rfind(')').ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon process stat is malformed",
        )
    })?;
    let start_time = stat[end + 1..]
        .split_whitespace()
        .nth(19)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "daemon start time is malformed")
        })?;
    let executable = fs::read_link(format!("/proc/{pid}/exe"))?;
    let executable = executable.to_str().filter(|value| {
        !value.is_empty() && !value.bytes().any(|byte| matches!(byte, 0 | b'\n' | b'\r'))
    });
    let executable = executable.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon executable identity is invalid",
        )
    })?;
    Ok(format!(
        "hyz-daemon-v1\n{pid}\n{start_time}\n{executable}\n"
    ))
}

pub fn bind_control_socket(ownership: &mut DaemonOwnership) -> io::Result<UnixListener> {
    ownership.verify_owned()?;
    let path = Path::new(CONTROL_SOCKET);
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            let description = if metadata.uid() == 0 && metadata.file_type().is_socket() {
                "control socket already exists; refusing automatic stale or active takeover"
            } else {
                "control path exists with an unsafe identity"
            };
            return Err(io::Error::new(io::ErrorKind::AddrInUse, description));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(path)?;
    let secure = (|| {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        let metadata = fs::symlink_metadata(path)?;
        if metadata.uid() != 0
            || metadata.mode() & 0o777 != 0o600
            || !metadata.file_type().is_socket()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "control socket must be root-owned mode 0600",
            ));
        }
        Ok((metadata.dev(), metadata.ino()))
    })();
    let socket_identity = match secure {
        Ok(identity) => identity,
        Err(error) => {
            drop(listener);
            let _ = fs::remove_file(path);
            return Err(error);
        }
    };
    ownership.socket_identity = Some(socket_identity);
    Ok(listener)
}

pub fn remove_control_socket(ownership: &DaemonOwnership) -> io::Result<()> {
    ownership.verify_owned()?;
    let expected = ownership.socket_identity.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "control socket was not owned",
        )
    })?;
    let metadata = fs::symlink_metadata(CONTROL_SOCKET)?;
    if metadata.uid() != 0
        || metadata.mode() & 0o777 != 0o600
        || !metadata.file_type().is_socket()
        || (metadata.dev(), metadata.ino()) != expected
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to remove a changed control socket identity",
        ));
    }
    fs::remove_file(CONTROL_SOCKET)
}

fn ensure_runtime_directory() -> io::Result<()> {
    match fs::symlink_metadata(CONTROL_RUNTIME_DIR) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "control runtime path is not a directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(CONTROL_RUNTIME_DIR)?;
        }
        Err(error) => return Err(error),
    }
    fs::set_permissions(CONTROL_RUNTIME_DIR, fs::Permissions::from_mode(0o700))?;
    let metadata = fs::symlink_metadata(CONTROL_RUNTIME_DIR)?;
    if metadata.uid() != 0 || metadata.mode() & 0o777 != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "control runtime directory must be root-owned mode 0700",
        ));
    }
    Ok(())
}

pub async fn serve_control(
    listener: UnixListener,
    handler: std::sync::Arc<dyn ControlHandler>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let concurrency = std::sync::Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut connections = JoinSet::new();
    let mut accept_error = None;
    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        accept_error = Some(error);
                        break;
                    }
                };
                let permit = match concurrency.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => continue,
                };
                let handler = handler.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let _ = handle_connection(stream, handler).await;
                });
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    }
    drop(listener);
    while connections.join_next().await.is_some() {}
    match accept_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn handle_connection(
    mut stream: UnixStream,
    handler: std::sync::Arc<dyn ControlHandler>,
) -> io::Result<()> {
    let credentials = stream.peer_cred()?;
    if credentials.uid() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "control peer is not root",
        ));
    }
    let request: ControlRequest = timeout(IO_TIMEOUT, read_frame(&mut stream))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control request timed out"))??;
    let response = if !(PROTOCOL_VERSION - 1..=PROTOCOL_VERSION).contains(&request.version) {
        ControlResponse::error(
            "unsupported_version",
            "unsupported control protocol version",
        )
    } else if let Err(message) = request.operation.validate() {
        ControlResponse::error("invalid_request", message)
    } else {
        let operation = request.operation;
        match handler.handle(operation).await {
            Ok(result) => ControlResponse::success(result),
            Err(message) => ControlResponse::error("operation_failed", message),
        }
    };
    timeout(IO_TIMEOUT, write_frame(&mut stream, &response))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control response timed out"))??;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn singleton_and_shutdown_guards_remain_in_the_control_source() {
        let source = include_str!("control.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        assert!(production.contains("stale or malformed locks require explicit operator removal"));
        assert!(production.contains("while connections.join_next().await.is_some()"));
        assert!(production.contains("current_daemon_identity()? != self.identity"));
    }
}
