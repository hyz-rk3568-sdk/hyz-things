use crate::application::ports::{LifecycleLease, PlatformError};
use std::{
    ffi::{c_char, CString},
    fs::{self, DirBuilder, File, Metadata, OpenOptions},
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const BRIDGE_OWNER: &str = "/run/hyz-router/br-lan.owned";
pub const ROUTER_FIREWALL_OWNER: &str = "/run/hyz-router/firewall.owned";
pub const TUN_FIREWALL_OWNER: &str = "/run/hyz-mihomo/tun.owned";
pub const TUN_FIREWALL_UPLINK: &str = "/run/hyz-mihomo/tun.uplink";
pub const TAILSCALE_FIREWALL_OWNER: &str = super::paths::TAILSCALE_FIREWALL_OWNER;
pub const TAILSCALE_SUBNET_FIREWALL_OWNER: &str = super::paths::TAILSCALE_SUBNET_FIREWALL_OWNER;
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
    if path == super::paths::MIHOMO_DATA_DIR {
        super::geodata::ensure_mihomo_geodata(path)?;
    }
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

const LOCK_OWNER_NAME: &str = "pid";
const MAX_LOCK_OWNER_BYTES: usize = 1024;
const ROOT_UID: u32 = 0;
const O_NOFOLLOW: i32 = 0o400000;
const AT_FDCWD: i32 = -100;
const RENAME_NOREPLACE: u32 = 1;

unsafe extern "C" {
    fn renameat2(
        old_directory: i32,
        old_path: *const c_char,
        new_directory: i32,
        new_path: *const c_char,
        flags: u32,
    ) -> i32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FilesystemIdentity {
    device: u64,
    inode: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcessIdentity {
    pid: u32,
    start_time: u64,
}

fn unique_sibling(path: &Path, role: &str) -> Result<PathBuf, PlatformError> {
    let parent = path
        .parent()
        .ok_or_else(|| PlatformError::InvalidState("lock path has no parent".to_owned()))?;
    let base = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| PlatformError::InvalidState("lock path is not UTF-8".to_owned()))?;
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(
        ".{base}.{}.{}.{role}",
        std::process::id(),
        sequence
    )))
}

fn rename_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "source path contains NUL")
    })?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination path contains NUL",
        )
    })?;
    let result = unsafe {
        renameat2(
            AT_FDCWD,
            source.as_ptr(),
            AT_FDCWD,
            destination.as_ptr(),
            RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn sync_directory(path: &Path) -> Result<(), PlatformError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| PlatformError::Io(format!("sync {}: {error}", path.display())))
}

pub(crate) fn acquire_lock(path: &'static str) -> Result<LifecycleLease, PlatformError> {
    let pid = std::process::id();
    let start_time = super::process::process_start_time(pid)?
        .ok_or_else(|| PlatformError::InvalidState("cannot identify current process".to_owned()))?;
    acquire_lock_with(path, ROOT_UID, ProcessIdentity { pid, start_time }, |pid| {
        super::process::process_start_time(pid)
    })
}

fn acquire_lock_with(
    path: &'static str,
    required_uid: u32,
    current: ProcessIdentity,
    mut process_start_time: impl FnMut(u32) -> Result<Option<u64>, PlatformError>,
) -> Result<LifecycleLease, PlatformError> {
    let path_ref = Path::new(path);
    let parent = path_ref
        .parent()
        .ok_or_else(|| PlatformError::InvalidState("lock path has no parent".to_owned()))?;
    let identity = format!("{}:{}\n", current.pid, current.start_time);
    loop {
        let temporary = unique_sibling(path_ref, "pending")?;
        match DirBuilder::new().mode(0o700).create(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(PlatformError::Io(format!(
                    "create temporary lock directory {}: {error}",
                    temporary.display()
                )))
            }
        }
        let temporary_identity = secure_directory_identity(&temporary, required_uid)?;
        let owner = temporary.join(LOCK_OWNER_NAME);
        let prepare = (|| {
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
                .map_err(|error| PlatformError::Io(format!("write lock owner: {error}")))?;
            let (_, owner_identity) = read_secure_lock_owner(&owner, required_uid)?;
            require_directory_identity(&temporary, required_uid, temporary_identity)?;
            require_path_identity(&owner, required_uid, owner_identity)?;
            sync_directory(&temporary)
        })();
        if let Err(error) = prepare {
            remove_owned_lock_directory(&temporary, required_uid, temporary_identity);
            return Err(error);
        }
        match rename_noreplace(&temporary, path_ref) {
            Ok(()) => {
                if let Err(error) = sync_directory(parent) {
                    remove_owned_lock_directory(path_ref, required_uid, temporary_identity);
                    return Err(error);
                }
                return Ok(LifecycleLease {
                    path,
                    identity,
                    directory_device: temporary_identity.device,
                    directory_inode: temporary_identity.inode,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                remove_owned_lock_directory(&temporary, required_uid, temporary_identity);
                match reclaim_stale_lock(path, required_uid, &mut process_start_time) {
                    Ok(()) => {}
                    Err(error) if is_lock_disappearance_race(&error) => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(error) => {
                remove_owned_lock_directory(&temporary, required_uid, temporary_identity);
                return Err(PlatformError::Io(format!(
                    "publish lock directory {path}: {error}"
                )));
            }
        }
    }
}

fn detach_lock_directory(
    path: &Path,
    required_uid: u32,
    expected: FilesystemIdentity,
    role: &str,
) -> Result<PathBuf, PlatformError> {
    loop {
        let detached = unique_sibling(path, role)?;
        match rename_noreplace(path, &detached) {
            Ok(()) => {
                let actual = secure_directory_identity(&detached, required_uid)?;
                if actual != expected {
                    let restore = rename_noreplace(&detached, path);
                    return Err(PlatformError::Conflict(format!(
                        "lifecycle lock directory changed during {role}; restore={restore:?}"
                    )));
                }
                return Ok(detached);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(PlatformError::Conflict(format!(
                    "lifecycle lock directory disappeared during {role}"
                )))
            }
            Err(error) => {
                return Err(PlatformError::Io(format!(
                    "detach lifecycle lock during {role}: {error}"
                )))
            }
        }
    }
}

fn is_lock_disappearance_race(error: &PlatformError) -> bool {
    matches!(
        error,
        PlatformError::Conflict(reason)
            if reason == "lifecycle lock directory disappeared"
                || reason.starts_with("lifecycle lock directory disappeared during ")
    )
}

fn reclaim_stale_lock(
    path: &str,
    required_uid: u32,
    process_start_time: &mut impl FnMut(u32) -> Result<Option<u64>, PlatformError>,
) -> Result<(), PlatformError> {
    let path = Path::new(path);
    let directory_identity = secure_directory_identity(path, required_uid)?;
    let owner = path.join(LOCK_OWNER_NAME);
    let (owner_record, owner_identity) = read_secure_lock_owner(&owner, required_uid)?;
    require_directory_identity(path, required_uid, directory_identity)?;
    let process = parse_lock_owner(&owner_record)?;
    if process_start_time(process.pid)? == Some(process.start_time) {
        return Err(PlatformError::Busy(format!(
            "{} is held by live PID {} with matching start time",
            path.display(),
            process.pid
        )));
    }

    require_directory_identity(path, required_uid, directory_identity)?;
    require_path_identity(&owner, required_uid, owner_identity)?;
    let detached = detach_lock_directory(path, required_uid, directory_identity, "reclaim")?;
    let detached_owner = detached.join(LOCK_OWNER_NAME);
    let (detached_record, detached_owner_identity) =
        read_secure_lock_owner(&detached_owner, required_uid)?;
    if detached_record != owner_record || detached_owner_identity != owner_identity {
        let restore = rename_noreplace(&detached, path);
        return Err(PlatformError::Conflict(format!(
            "lifecycle lock owner changed during reclaim; restore={restore:?}"
        )));
    }
    fs::remove_file(&detached_owner).map_err(|error| {
        PlatformError::Io(format!(
            "remove stale lock owner {}: {error}",
            detached_owner.display()
        ))
    })?;
    fs::remove_dir(&detached).map_err(|error| {
        PlatformError::Io(format!(
            "remove stale lock directory {}: {error}",
            detached.display()
        ))
    })
}

pub(crate) fn release_lock(lease: &LifecycleLease) -> Result<(), PlatformError> {
    release_lock_with(lease, ROOT_UID)
}

fn release_lock_with(lease: &LifecycleLease, required_uid: u32) -> Result<(), PlatformError> {
    let path = Path::new(lease.path);
    let directory_identity = FilesystemIdentity {
        device: lease.directory_device,
        inode: lease.directory_inode,
    };
    require_directory_identity(path, required_uid, directory_identity)?;
    let owner = path.join(LOCK_OWNER_NAME);
    let (current, owner_identity) = read_secure_lock_owner(&owner, required_uid)?;
    if current != lease.identity {
        return Err(PlatformError::Conflict(
            "lifecycle lock identity changed; refusing unlink".to_owned(),
        ));
    }
    require_directory_identity(path, required_uid, directory_identity)?;
    require_path_identity(&owner, required_uid, owner_identity)?;
    let detached = detach_lock_directory(path, required_uid, directory_identity, "release")?;
    let detached_owner = detached.join(LOCK_OWNER_NAME);
    let (detached_record, detached_owner_identity) =
        read_secure_lock_owner(&detached_owner, required_uid)?;
    if detached_record != lease.identity || detached_owner_identity != owner_identity {
        let restore = rename_noreplace(&detached, path);
        return Err(PlatformError::Conflict(format!(
            "lifecycle lock owner changed during release; restore={restore:?}"
        )));
    }
    fs::remove_file(&detached_owner).map_err(|error| {
        PlatformError::Io(format!(
            "remove lock owner {}: {error}",
            detached_owner.display()
        ))
    })?;
    fs::remove_dir(&detached).map_err(|error| {
        PlatformError::Io(format!(
            "remove lock directory {}: {error}",
            detached.display()
        ))
    })
}

fn parse_lock_owner(value: &str) -> Result<ProcessIdentity, PlatformError> {
    let value = value.strip_suffix('\n').ok_or_else(invalid_lock_owner)?;
    if value.contains('\n') || value.contains('\r') {
        return Err(invalid_lock_owner());
    }
    let (pid, start_time) = value.split_once(':').ok_or_else(invalid_lock_owner)?;
    if pid.is_empty() || start_time.is_empty() || start_time.contains(':') {
        return Err(invalid_lock_owner());
    }
    let pid = pid.parse::<u32>().map_err(|_| invalid_lock_owner())?;
    let start_time = start_time
        .parse::<u64>()
        .map_err(|_| invalid_lock_owner())?;
    if pid == 0 || start_time == 0 {
        return Err(invalid_lock_owner());
    }
    Ok(ProcessIdentity { pid, start_time })
}

fn read_secure_lock_owner(
    path: &Path,
    required_uid: u32,
) -> Result<(String, FilesystemIdentity), PlatformError> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                PlatformError::InvalidState("lifecycle lock owner is missing".to_owned())
            } else {
                PlatformError::Io(format!("open lock owner {}: {error}", path.display()))
            }
        })?;
    let metadata = file
        .metadata()
        .map_err(|error| PlatformError::Io(format!("inspect lock owner: {error}")))?;
    validate_owner_metadata(path, &metadata, required_uid)?;
    let identity = filesystem_identity(&metadata);
    let mut bytes = Vec::new();
    (&mut file)
        .take((MAX_LOCK_OWNER_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| PlatformError::Io(format!("read lock owner: {error}")))?;
    if bytes.len() > MAX_LOCK_OWNER_BYTES {
        return Err(PlatformError::InvalidState(
            "lifecycle lock owner exceeds size limit".to_owned(),
        ));
    }
    require_path_identity(path, required_uid, identity)?;
    let value = String::from_utf8(bytes).map_err(|_| invalid_lock_owner())?;
    Ok((value, identity))
}

fn secure_directory_identity(
    path: &Path,
    required_uid: u32,
) -> Result<FilesystemIdentity, PlatformError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            PlatformError::Conflict("lifecycle lock directory disappeared".to_owned())
        } else {
            PlatformError::Io(format!(
                "inspect lock directory {}: {error}",
                path.display()
            ))
        }
    })?;
    validate_directory_metadata(path, &metadata, required_uid)?;
    Ok(filesystem_identity(&metadata))
}

fn require_directory_identity(
    path: &Path,
    required_uid: u32,
    expected: FilesystemIdentity,
) -> Result<(), PlatformError> {
    let current = secure_directory_identity(path, required_uid)?;
    if current != expected {
        return Err(PlatformError::Conflict(
            "lifecycle lock directory inode changed".to_owned(),
        ));
    }
    Ok(())
}

fn require_path_identity(
    path: &Path,
    required_uid: u32,
    expected: FilesystemIdentity,
) -> Result<(), PlatformError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            PlatformError::Conflict("lifecycle lock owner changed".to_owned())
        } else {
            PlatformError::Io(format!("inspect lock owner {}: {error}", path.display()))
        }
    })?;
    validate_owner_metadata(path, &metadata, required_uid)?;
    if filesystem_identity(&metadata) != expected {
        return Err(PlatformError::Conflict(
            "lifecycle lock owner inode changed".to_owned(),
        ));
    }
    Ok(())
}

fn validate_directory_metadata(
    path: &Path,
    metadata: &Metadata,
    required_uid: u32,
) -> Result<(), PlatformError> {
    if !metadata.file_type().is_dir()
        || metadata.uid() != required_uid
        || metadata.mode() & 0o022 != 0
    {
        return Err(PlatformError::InvalidState(format!(
            "{} must be a root-owned directory that is not group/world writable",
            path.display()
        )));
    }
    Ok(())
}

fn validate_owner_metadata(
    path: &Path,
    metadata: &Metadata,
    required_uid: u32,
) -> Result<(), PlatformError> {
    if !metadata.file_type().is_file()
        || metadata.uid() != required_uid
        || metadata.mode() & 0o022 != 0
        || metadata.nlink() != 1
    {
        return Err(PlatformError::InvalidState(format!(
            "{} must be a singly-linked root-owned regular file that is not group/world writable",
            path.display()
        )));
    }
    Ok(())
}

fn filesystem_identity(metadata: &Metadata) -> FilesystemIdentity {
    FilesystemIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn remove_owned_lock_directory(path: &Path, required_uid: u32, expected: FilesystemIdentity) {
    if secure_directory_identity(path, required_uid).ok() != Some(expected) {
        return;
    }
    let _ = fs::remove_file(path.join(LOCK_OWNER_NAME));
    if fs::symlink_metadata(path)
        .ok()
        .map(|metadata| filesystem_identity(&metadata))
        == Some(expected)
    {
        let _ = fs::remove_dir(path);
    }
}

fn invalid_lock_owner() -> PlatformError {
    PlatformError::InvalidState("malformed lifecycle lock owner".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestLock {
        root: PathBuf,
        path: &'static str,
        uid: u32,
    }

    impl TestLock {
        fn new() -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "hyz-router-storage-test-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&root).unwrap();
            let uid = fs::symlink_metadata(&root).unwrap().uid();
            let path: &'static str = Box::leak(
                root.join("lifecycle.lock")
                    .to_string_lossy()
                    .into_owned()
                    .into_boxed_str(),
            );
            Self { root, path, uid }
        }

        fn path(&self) -> &Path {
            Path::new(self.path)
        }

        fn create(&self, owner: &str) {
            create_lock_fixture(self.path(), owner);
        }
    }

    impl Drop for TestLock {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn create_lock_fixture(path: &Path, owner: &str) {
        fs::create_dir(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        let owner_path = path.join(LOCK_OWNER_NAME);
        fs::write(&owner_path, owner).unwrap();
        fs::set_permissions(owner_path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn current_identity() -> ProcessIdentity {
        ProcessIdentity {
            pid: 900,
            start_time: 901,
        }
    }

    #[test]
    fn matching_live_pid_and_start_time_remains_busy() {
        let lock = TestLock::new();
        lock.create("100:200\n");

        let error = acquire_lock_with(lock.path, lock.uid, current_identity(), |pid| {
            assert_eq!(pid, 100);
            Ok(Some(200))
        })
        .unwrap_err();

        assert!(matches!(error, PlatformError::Busy(_)));
        assert_eq!(
            fs::read_to_string(lock.path().join(LOCK_OWNER_NAME)).unwrap(),
            "100:200\n"
        );
    }

    #[test]
    fn dead_owner_is_reclaimed_and_replaced_by_current_identity() {
        let lock = TestLock::new();
        lock.create("100:200\n");
        fs::set_permissions(lock.path(), fs::Permissions::from_mode(0o755)).unwrap();

        let lease =
            acquire_lock_with(lock.path, lock.uid, current_identity(), |_| Ok(None)).unwrap();

        assert_eq!(lease.identity, "900:901\n");
        assert_eq!(
            fs::read_to_string(lock.path().join(LOCK_OWNER_NAME)).unwrap(),
            lease.identity
        );
        release_lock_with(&lease, lock.uid).unwrap();
        assert!(!lock.path().exists());
    }

    #[test]
    fn reused_pid_with_different_start_time_is_reclaimed() {
        let lock = TestLock::new();
        lock.create("100:200\n");

        let lease =
            acquire_lock_with(lock.path, lock.uid, current_identity(), |_| Ok(Some(201))).unwrap();

        assert_eq!(lease.identity, "900:901\n");
        release_lock_with(&lease, lock.uid).unwrap();
    }

    #[test]
    fn malformed_or_missing_owner_fails_closed() {
        let malformed = TestLock::new();
        malformed.create("not-a-process\n");
        let error = acquire_lock_with(malformed.path, malformed.uid, current_identity(), |_| {
            panic!("malformed owner must not be probed")
        })
        .unwrap_err();
        assert!(matches!(error, PlatformError::InvalidState(_)));
        assert!(malformed.path().exists());

        let missing = TestLock::new();
        fs::create_dir(missing.path()).unwrap();
        fs::set_permissions(missing.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let error = acquire_lock_with(missing.path, missing.uid, current_identity(), |_| {
            panic!("missing owner must not be probed")
        })
        .unwrap_err();
        assert!(matches!(error, PlatformError::InvalidState(_)));
        assert!(missing.path().exists());
    }

    #[test]
    fn unsafe_directory_or_owner_permissions_fail_closed() {
        let directory = TestLock::new();
        directory.create("100:200\n");
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o770)).unwrap();
        let error = acquire_lock_with(directory.path, directory.uid, current_identity(), |_| {
            panic!("unsafe directory must not be probed")
        })
        .unwrap_err();
        assert!(matches!(error, PlatformError::InvalidState(_)));
        assert!(directory.path().exists());

        let owner = TestLock::new();
        owner.create("100:200\n");
        fs::set_permissions(
            owner.path().join(LOCK_OWNER_NAME),
            fs::Permissions::from_mode(0o620),
        )
        .unwrap();
        let error = acquire_lock_with(owner.path, owner.uid, current_identity(), |_| {
            panic!("unsafe owner must not be probed")
        })
        .unwrap_err();
        assert!(matches!(error, PlatformError::InvalidState(_)));
        assert!(owner.path().exists());

        let owner_type = TestLock::new();
        fs::create_dir(owner_type.path()).unwrap();
        fs::set_permissions(owner_type.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(owner_type.path().join(LOCK_OWNER_NAME)).unwrap();
        let error = acquire_lock_with(owner_type.path, owner_type.uid, current_identity(), |_| {
            panic!("unsafe owner type must not be probed")
        })
        .unwrap_err();
        assert!(matches!(error, PlatformError::InvalidState(_)));
        assert!(owner_type.path().exists());
    }

    #[test]
    fn unexpected_owner_uid_fails_closed() {
        let lock = TestLock::new();
        lock.create("100:200\n");
        let unexpected_uid = lock.uid.wrapping_add(1);

        let error = acquire_lock_with(lock.path, unexpected_uid, current_identity(), |_| {
            panic!("unexpected ownership must not be probed")
        })
        .unwrap_err();

        assert!(matches!(error, PlatformError::InvalidState(_)));
        assert!(lock.path().exists());
    }

    #[test]
    fn lock_release_race_is_retried_after_directory_disappears() {
        let lock = TestLock::new();
        lock.create("100:200\n");
        let mut released = false;

        let lease = acquire_lock_with(lock.path, lock.uid, current_identity(), |pid| {
            assert_eq!(pid, 100);
            if !released {
                released = true;
                fs::remove_file(lock.path().join(LOCK_OWNER_NAME)).unwrap();
                fs::remove_dir(lock.path()).unwrap();
            }
            Ok(None)
        })
        .unwrap();

        assert_eq!(lease.identity, "900:901\n");
        release_lock_with(&lease, lock.uid).unwrap();
        assert!(!lock.path().exists());
    }

    #[test]
    fn stale_reclaim_refuses_a_replaced_directory_inode() {
        let lock = TestLock::new();
        lock.create("100:200\n");
        let displaced = lock.root.join("displaced.lock");

        let error = acquire_lock_with(lock.path, lock.uid, current_identity(), |_| {
            fs::rename(lock.path(), &displaced).unwrap();
            create_lock_fixture(lock.path(), "300:400\n");
            Ok(None)
        })
        .unwrap_err();

        assert!(matches!(error, PlatformError::Conflict(_)));
        assert_eq!(
            fs::read_to_string(lock.path().join(LOCK_OWNER_NAME)).unwrap(),
            "300:400\n"
        );
    }

    #[test]
    fn release_requires_both_recorded_directory_inode_and_owner() {
        let changed_owner = TestLock::new();
        let lease = acquire_lock_with(
            changed_owner.path,
            changed_owner.uid,
            current_identity(),
            |_| Ok(None),
        )
        .unwrap();
        fs::write(changed_owner.path().join(LOCK_OWNER_NAME), "902:903\n").unwrap();
        let error = release_lock_with(&lease, changed_owner.uid).unwrap_err();
        assert!(matches!(error, PlatformError::Conflict(_)));
        assert!(changed_owner.path().exists());

        let changed_inode = TestLock::new();
        let lease = acquire_lock_with(
            changed_inode.path,
            changed_inode.uid,
            current_identity(),
            |_| Ok(None),
        )
        .unwrap();
        let displaced = changed_inode.root.join("leased.lock");
        fs::rename(changed_inode.path(), displaced).unwrap();
        create_lock_fixture(changed_inode.path(), &lease.identity);
        let error = release_lock_with(&lease, changed_inode.uid).unwrap_err();
        assert!(matches!(error, PlatformError::Conflict(_)));
        assert!(changed_inode.path().exists());
    }
}
