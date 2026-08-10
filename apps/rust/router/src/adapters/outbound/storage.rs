use crate::application::ports::{LifecycleLease, PlatformError};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub const BRIDGE_OWNER: &str = "/run/hyz-router/br-lan.owned";
pub const ROUTER_FIREWALL_OWNER: &str = "/run/hyz-router/firewall.owned";
pub const TUN_FIREWALL_OWNER: &str = "/run/hyz-mihomo/tun.owned";
pub const PREVIOUS_FORWARDING: &str = "/run/hyz-router/ip-forward.previous";
pub const MODE_FILE: &str = "/userdata/hyz-router/mihomo/mode";
pub const DISABLED_MARKER: &str = "/userdata/hyz-router/mihomo/disabled";

static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn validate_token(token: &str) -> Result<(), PlatformError> {
    if token.is_empty()
        || token.len() > 96
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(PlatformError::InvalidState(
            "ownership token has invalid characters or length".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_private_dir(path: &str) -> Result<(), PlatformError> {
    fs::create_dir_all(path)
        .map_err(|error| PlatformError::Io(format!("create private directory {path}: {error}")))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| PlatformError::Io(format!("inspect private directory {path}: {error}")))?;
    if !metadata.file_type().is_dir() || metadata.uid() != 0 {
        return Err(PlatformError::InvalidState(format!(
            "{path} must be a root-owned private directory"
        )));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| PlatformError::Io(format!("chmod private directory {path}: {error}")))?;
    Ok(())
}

pub(crate) fn require_private_root_file(path: &str) -> Result<(), PlatformError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| PlatformError::Io(format!("inspect private file {path}: {error}")))?;
    if !metadata.file_type().is_file() || metadata.uid() != 0 {
        return Err(PlatformError::InvalidState(format!(
            "{path} must be a root-owned private regular file"
        )));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| PlatformError::Io(format!("chmod private file {path}: {error}")))?;
    Ok(())
}

pub(crate) fn require_private_root_file_optional(path: &str) -> Result<(), PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(_) => require_private_root_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PlatformError::Io(format!("inspect {path}: {error}"))),
    }
}

pub(crate) fn read_private_small_optional(
    path: &str,
    maximum: usize,
) -> Result<Option<String>, PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(_) => require_private_root_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PlatformError::Io(format!("inspect {path}: {error}"))),
    }
    read_small_optional(path, maximum)
}

pub(crate) fn read_small_optional(
    path: &str,
    maximum: usize,
) -> Result<Option<String>, PlatformError> {
    let bytes = match fs::read(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PlatformError::Io(format!("read {path}: {error}"))),
    };
    if bytes.len() > maximum {
        return Err(PlatformError::InvalidState(format!(
            "{path} exceeds size limit"
        )));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| PlatformError::InvalidState(format!("{path} is not UTF-8")))
}

pub(crate) fn atomic_write_private(path: &str, bytes: &[u8]) -> Result<(), PlatformError> {
    let path = Path::new(path);
    let parent = path
        .parent()
        .ok_or_else(|| PlatformError::InvalidState("state path has no parent".to_owned()))?;
    let parent_text = parent
        .to_str()
        .ok_or_else(|| PlatformError::InvalidState("state parent is not UTF-8".to_owned()))?;
    ensure_private_dir(parent_text)?;
    let base = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state");
    let (temporary, mut file) = loop {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{base}.{}.{}.tmp", std::process::id(), sequence));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(PlatformError::Io(format!(
                    "create {}: {error}",
                    candidate.display()
                )))
            }
        }
    };
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| PlatformError::Io(format!("write state: {error}")))?;
        file.sync_all()
            .map_err(|error| PlatformError::Io(format!("sync state: {error}")))?;
        fs::rename(&temporary, path)
            .map_err(|error| PlatformError::Io(format!("commit {}: {error}", path.display())))?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| PlatformError::Io(format!("sync {}: {error}", parent.display())))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub(crate) fn remove_file_durable(path: &str) -> Result<(), PlatformError> {
    let path = Path::new(path);
    match fs::remove_file(path) {
        Ok(()) => {
            let parent = path.parent().ok_or_else(|| {
                PlatformError::InvalidState("removed path has no parent".to_owned())
            })?;
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| PlatformError::Io(format!("sync {}: {error}", parent.display())))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PlatformError::Io(format!(
            "remove {}: {error}",
            path.display()
        ))),
    }
}

pub(crate) fn acquire_lock(path: &'static str) -> Result<LifecycleLease, PlatformError> {
    let pid = std::process::id();
    let start = super::process::process_start_time(pid)?
        .ok_or_else(|| PlatformError::InvalidState("cannot identify current process".to_owned()))?;
    let identity = format!("{pid}:{start}\n");
    fs::create_dir(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            PlatformError::Busy(format!("{path} already exists; stale takeover is disabled"))
        } else {
            PlatformError::Io(format!("create lock directory {path}: {error}"))
        }
    })?;

    let owner = Path::new(path).join("pid");
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&owner)
            .map_err(|error| {
                PlatformError::Io(format!("create lock owner {}: {error}", owner.display()))
            })?;
        file.write_all(identity.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|error| PlatformError::Io(format!("write lock owner: {error}")))
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&owner);
        let _ = fs::remove_dir(path);
        return Err(error);
    }
    Ok(LifecycleLease { path, identity })
}

pub(crate) fn release_lock(lease: &LifecycleLease) -> Result<(), PlatformError> {
    let owner = Path::new(lease.path).join("pid");
    let owner_path = owner
        .to_str()
        .ok_or_else(|| PlatformError::InvalidState("lock path is not UTF-8".to_owned()))?;
    let current = read_small_optional(owner_path, 1024)?
        .ok_or_else(|| PlatformError::Conflict("lifecycle lock owner disappeared".to_owned()))?;
    if current != lease.identity {
        return Err(PlatformError::Conflict(
            "lifecycle lock identity changed; refusing unlink".to_owned(),
        ));
    }
    fs::remove_file(&owner).map_err(|error| {
        PlatformError::Io(format!("remove lock owner {}: {error}", owner.display()))
    })?;
    fs::remove_dir(lease.path).map_err(|error| {
        PlatformError::Io(format!("remove lock directory {}: {error}", lease.path))
    })
}
