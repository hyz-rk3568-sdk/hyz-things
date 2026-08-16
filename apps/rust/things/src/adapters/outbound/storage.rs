//! Root-only private state storage for the portal's persisted admin
//! credential. Mirrors the security semantics of the router core's storage:
//! no symlink following, root-only ownership, bounded reads, atomic writes
//! with fsync, and a root-owned 0700 parent directory.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::application::ports::PlatformError;

const ROOT_UID: u32 = 0;
const O_NOFOLLOW: i32 = 0o400000;
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn require_private_root_file_optional(path: &str) -> Result<(), PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(_) => require_private_root_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PlatformError::Io(format!("inspect {path}: {error}"))),
    }
}

pub fn read_private_small_optional(
    path: &str,
    maximum: usize,
) -> Result<Option<String>, PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(_) => require_private_root_file(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PlatformError::Io(format!("inspect {path}: {error}"))),
    }
    let bytes = match fs::read(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PlatformError::Io(format!("read {path}: {error}"))),
    };
    if bytes.len() > maximum {
        return Err(PlatformError::InvalidState(format!(
            "private state record {path} exceeds {maximum} bytes"
        )));
    }
    String::from_utf8(bytes).map(Some).map_err(|_| {
        PlatformError::InvalidState(format!("private state record {path} is not UTF-8"))
    })
}

pub fn atomic_write_private(path: &str, bytes: &[u8]) -> Result<(), PlatformError> {
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
            .custom_flags(O_NOFOLLOW)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(PlatformError::Io(format!("create {candidate:?}: {error}"))),
        }
    };
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(PlatformError::Io(format!("write {temporary:?}: {error}")));
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(PlatformError::Io(format!("commit {path:?}: {error}")));
    }
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| PlatformError::Io(format!("sync {parent_text}: {error}")))
}

fn require_private_root_file(path: &str) -> Result<(), PlatformError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| PlatformError::Io(format!("inspect {path}: {error}")))?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_file()
        || metadata.uid() != ROOT_UID
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(PlatformError::InvalidState(format!(
            "private state record {path} must be a root-owned mode 0600 regular file"
        )));
    }
    Ok(())
}

fn ensure_private_dir(path: &str) -> Result<(), PlatformError> {
    let directory = Path::new(path);
    match fs::symlink_metadata(directory) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.file_type().is_dir()
                || metadata.uid() != ROOT_UID
                || metadata.mode() & 0o777 != 0o700
            {
                return Err(PlatformError::InvalidState(format!(
                    "private state parent {path} must be a root-owned mode 0700 directory"
                )));
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(directory)
                .map_err(|error| PlatformError::Io(format!("create {path}: {error}")))?;
            let metadata = fs::symlink_metadata(directory)
                .map_err(|error| PlatformError::Io(format!("inspect {path}: {error}")))?;
            if metadata.uid() != ROOT_UID || metadata.mode() & 0o777 != 0o700 {
                return Err(PlatformError::InvalidState(format!(
                    "private state parent {path} must be root-owned mode 0700"
                )));
            }
            Ok(())
        }
        Err(error) => Err(PlatformError::Io(format!("inspect {path}: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn running_as_root() -> bool {
        let uid = unsafe { libc::geteuid() };
        uid == 0
    }

    #[test]
    fn round_trip_preserves_root_only_private_semantics() {
        if !running_as_root() {
            return;
        }
        let directory =
            std::env::temp_dir().join(format!("hyz-things-storage-{}", std::process::id()));
        let path = directory.join("credential.json");
        let path = path.to_str().unwrap();
        atomic_write_private(path, b"{\"secret\":true}").unwrap();
        assert_eq!(
            read_private_small_optional(path, 128).unwrap().as_deref(),
            Some("{\"secret\":true}")
        );
        let metadata = fs::symlink_metadata(path).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn reads_reject_oversized_and_missing_files() {
        if !running_as_root() {
            return;
        }
        let directory =
            std::env::temp_dir().join(format!("hyz-things-storage2-{}", std::process::id()));
        let path = directory.join("credential.json");
        let path = path.to_str().unwrap();
        atomic_write_private(path, b"12345").unwrap();
        assert!(read_private_small_optional(path, 4).is_err());
        let missing = directory.join("missing.json");
        assert_eq!(
            read_private_small_optional(missing.to_str().unwrap(), 128).unwrap(),
            None
        );
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn rejects_symlinked_state() {
        if !running_as_root() {
            return;
        }
        let directory =
            std::env::temp_dir().join(format!("hyz-things-storage3-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("target.json");
        fs::write(&target, b"x").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let link = directory.join("link.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(read_private_small_optional(link.to_str().unwrap(), 128).is_err());
        let _ = fs::remove_dir_all(&directory);
    }
}
