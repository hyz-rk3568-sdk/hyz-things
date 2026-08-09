use crate::application::ota::{
    FirmwareIdentity, FirmwareMutationLock, FirmwarePlatformPort, PlatformError, TrustedFirmware,
};
use crate::domain::ota::{BootloaderMessage, BCB_OFFSET, BCB_SIZE};
use sha2::{Digest, Sha256};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const TEMPORARY_ATTEMPTS: u64 = 128;
const LOCK_ATTEMPTS: usize = 3;
const STAGED_FIRMWARE_NAME: &str = "upgrade.fw";
const LOCK_OWNER_NAME: &str = "owner";
const O_NOFOLLOW: i32 = 0o400000;
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct FirmwareAdapterConfig {
    pub update_engine: PathBuf,
    pub reboot: PathBuf,
    pub misc_partition: PathBuf,
    pub recovery_partition: PathBuf,
    pub staging_directory: PathBuf,
    /// Router's inode-safe directory lock. It does not serialize with the legacy `hyz-ota`
    /// process; operators must not run both OTA implementations during migration.
    pub mutation_lock: PathBuf,
}

impl Default for FirmwareAdapterConfig {
    fn default() -> Self {
        Self {
            update_engine: PathBuf::from("/usr/bin/updateEngine"),
            reboot: PathBuf::from("/sbin/reboot"),
            misc_partition: PathBuf::from("/dev/block/by-name/misc"),
            recovery_partition: PathBuf::from("/dev/block/by-name/recovery"),
            staging_directory: PathBuf::from("/userdata/hyz-router/ota"),
            mutation_lock: PathBuf::from("/userdata/hyz-router/ota-mutation.lock"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FirmwareAdapter {
    config: FirmwareAdapterConfig,
}

impl FirmwareAdapter {
    pub const fn new(config: FirmwareAdapterConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &FirmwareAdapterConfig {
        &self.config
    }

    fn staging_destination(&self) -> PathBuf {
        self.config.staging_directory.join(STAGED_FIRMWARE_NAME)
    }

    fn ensure_staging_directory(&self) -> Result<(), PlatformError> {
        ensure_secure_root_directory(&self.config.staging_directory, "OTA staging")?;
        fs::set_permissions(
            &self.config.staging_directory,
            fs::Permissions::from_mode(0o700),
        )
        .map_err(|error| platform_error("secure OTA staging directory", error))?;
        let metadata = metadata_for_path(&self.config.staging_directory)?;
        if metadata.uid() != 0 || metadata.mode() & 0o077 != 0 {
            return Err(PlatformError::new(
                "OTA staging directory must be root-owned mode 0700",
            ));
        }
        Ok(())
    }

    fn revalidate_committed(&self, firmware: &OpenedFirmware) -> Result<(), PlatformError> {
        self.ensure_staging_directory()?;
        if firmware.staged_temporary || firmware.path != self.staging_destination() {
            return Err(PlatformError::new(
                "firmware installation is restricted to the fixed OTA staging path",
            ));
        }
        revalidate(firmware)
    }

    fn write_bcb(&self, message: &BootloaderMessage) -> Result<(), PlatformError> {
        let mut misc = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.config.misc_partition)
            .map_err(|error| {
                platform_error(
                    format!("open {}", self.config.misc_partition.display()),
                    error,
                )
            })?;
        misc.seek(SeekFrom::Start(BCB_OFFSET))
            .and_then(|_| misc.write_all(message.as_bytes()))
            .and_then(|_| misc.flush())
            .and_then(|_| misc.sync_all())
            .map_err(|error| platform_error("write and sync Rockchip BCB", error))
    }
}

pub struct OpenedFirmware {
    file: File,
    path: PathBuf,
    identity: FirmwareIdentity,
    staged_temporary: bool,
}

impl TrustedFirmware for OpenedFirmware {
    fn path(&self) -> &Path {
        &self.path
    }

    fn identity(&self) -> FirmwareIdentity {
        self.identity
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct LockInode {
    device: u64,
    inode: u64,
}

struct DirectoryMutationLock {
    path: PathBuf,
    identity: LockInode,
}

impl FirmwareMutationLock for DirectoryMutationLock {}

impl Drop for DirectoryMutationLock {
    fn drop(&mut self) {
        remove_owned_lock_directory(&self.path, self.identity);
    }
}

struct LockCreationCleanup {
    path: PathBuf,
    identity: LockInode,
    armed: bool,
}

impl LockCreationCleanup {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for LockCreationCleanup {
    fn drop(&mut self) {
        if self.armed {
            remove_owned_lock_directory(&self.path, self.identity);
        }
    }
}

struct TemporaryCleanup {
    path: PathBuf,
    armed: bool,
}

impl TemporaryCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl FirmwarePlatformPort for FirmwareAdapter {
    type Firmware = OpenedFirmware;

    fn acquire_mutation_lock(&self) -> Result<Box<dyn FirmwareMutationLock + '_>, PlatformError> {
        let parent = parent_directory(&self.config.mutation_lock);
        ensure_secure_root_directory(parent, "OTA lock parent")?;

        for _ in 0..LOCK_ATTEMPTS {
            match create_directory(&self.config.mutation_lock, 0o700) {
                Ok(()) => {
                    let identity = lock_inode(&self.config.mutation_lock)?;
                    let cleanup = LockCreationCleanup {
                        path: self.config.mutation_lock.clone(),
                        identity,
                        armed: true,
                    };
                    fs::set_permissions(
                        &self.config.mutation_lock,
                        fs::Permissions::from_mode(0o700),
                    )
                    .map_err(|error| platform_error("secure OTA mutation lock", error))?;
                    if metadata_for_path(&self.config.mutation_lock)?.uid() != 0 {
                        return Err(PlatformError::new(
                            "OTA mutation lock directory must be owned by root",
                        ));
                    }
                    let process_identity = current_process_identity()?;
                    let owner_path = self.config.mutation_lock.join(LOCK_OWNER_NAME);
                    let mut owner = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(&owner_path)
                        .map_err(|error| platform_error("create OTA lock owner identity", error))?;
                    writeln!(
                        owner,
                        "{} {}",
                        process_identity.pid, process_identity.start_time
                    )
                    .and_then(|_| owner.sync_all())
                    .map_err(|error| platform_error("persist OTA lock owner identity", error))?;
                    File::open(&self.config.mutation_lock)
                        .and_then(|directory| directory.sync_all())
                        .map_err(|error| {
                            platform_error("sync OTA mutation lock directory", error)
                        })?;
                    cleanup.disarm();
                    return Ok(Box::new(DirectoryMutationLock {
                        path: self.config.mutation_lock.clone(),
                        identity,
                    }));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if !reclaim_stale_lock(&self.config.mutation_lock)? {
                        return Err(PlatformError::new(
                            "another unified-router OTA mutation is active",
                        ));
                    }
                }
                Err(error) => {
                    return Err(platform_error("acquire OTA mutation directory lock", error));
                }
            }
        }
        Err(PlatformError::new(
            "could not acquire OTA mutation directory lock",
        ))
    }

    fn open_firmware(&self, path: &Path) -> Result<Self::Firmware, PlatformError> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW)
            .open(path)
            .map_err(|error| platform_error("open firmware", error))?;
        opened_firmware(file, path.to_path_buf(), false)
    }

    fn copy_to_staging_temporary(
        &self,
        source: &mut Self::Firmware,
    ) -> Result<Self::Firmware, PlatformError> {
        self.ensure_staging_directory()?;
        revalidate(source)?;
        source
            .file
            .seek(SeekFrom::Start(0))
            .map_err(|error| platform_error("rewind source firmware", error))?;
        let (path, mut target) = create_unique_temporary(&self.config.staging_directory)?;
        let cleanup = TemporaryCleanup::new(path.clone());
        io::copy(&mut source.file, &mut target)
            .map_err(|error| platform_error("copy firmware into OTA staging", error))?;
        target
            .flush()
            .and_then(|_| target.sync_all())
            .map_err(|error| platform_error("sync staged firmware copy", error))?;
        revalidate(source)?;
        let firmware = opened_firmware(target, path, true)?;
        cleanup.disarm();
        Ok(firmware)
    }

    fn download_to_staging_temporary(&self, source: &str) -> Result<Self::Firmware, PlatformError> {
        validate_source_url(source)?;
        self.ensure_staging_directory()?;
        let (path, mut target) = create_unique_temporary(&self.config.staging_directory)?;
        let cleanup = TemporaryCleanup::new(path.clone());

        let mut response = ureq::get(source)
            .call()
            .map_err(|_| PlatformError::new("firmware download request failed"))?;
        let mut body = response.body_mut().as_reader();
        io::copy(&mut body, &mut target)
            .map_err(|_| PlatformError::new("firmware download body failed"))?;
        target
            .flush()
            .and_then(|_| target.sync_all())
            .map_err(|error| platform_error("sync downloaded firmware", error))?;
        let firmware = opened_firmware(target, path, true)?;
        cleanup.disarm();
        Ok(firmware)
    }

    fn commit_staged(&self, firmware: &mut Self::Firmware) -> Result<(), PlatformError> {
        self.ensure_staging_directory()?;
        if !firmware.staged_temporary
            || firmware.path.parent() != Some(self.config.staging_directory.as_path())
        {
            return Err(PlatformError::new(
                "only an OTA staging temporary may be committed",
            ));
        }
        revalidate(firmware)?;
        let destination = self.staging_destination();
        fs::rename(&firmware.path, &destination)
            .map_err(|error| platform_error("atomically commit staged firmware", error))?;
        firmware.path = destination;
        firmware.staged_temporary = false;
        revalidate(firmware)?;
        sync_directory(&self.config.staging_directory)
    }

    fn discard_staged(&self, firmware: &Self::Firmware) -> Result<(), PlatformError> {
        if !firmware.staged_temporary {
            return Ok(());
        }
        revalidate(firmware)?;
        match fs::remove_file(&firmware.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(platform_error("remove temporary staged firmware", error)),
        }
    }

    fn read_firmware_prefix(
        &self,
        firmware: &mut Self::Firmware,
    ) -> Result<Vec<u8>, PlatformError> {
        firmware
            .file
            .seek(SeekFrom::Start(0))
            .map_err(|error| platform_error("rewind firmware for prefix", error))?;
        let mut prefix = [0_u8; 4];
        let count = firmware
            .file
            .read(&mut prefix)
            .map_err(|error| platform_error("read firmware prefix", error))?;
        Ok(prefix[..count].to_vec())
    }

    fn sha256(&self, firmware: &mut Self::Firmware) -> Result<[u8; 32], PlatformError> {
        firmware
            .file
            .seek(SeekFrom::Start(0))
            .map_err(|error| platform_error("rewind firmware for SHA-256", error))?;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = firmware
                .file
                .read(&mut buffer)
                .map_err(|error| platform_error("read firmware for SHA-256", error))?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Ok(digest.finalize().into())
    }

    fn sync_firmware(&self, firmware: &Self::Firmware) -> Result<(), PlatformError> {
        firmware
            .file
            .sync_all()
            .map_err(|error| platform_error("sync opened firmware", error))
    }

    fn revalidate_firmware(&self, firmware: &Self::Firmware) -> Result<(), PlatformError> {
        revalidate(firmware)
    }

    fn recovery_partition_exists(&self) -> Result<bool, PlatformError> {
        Ok(self.config.recovery_partition.exists())
    }

    fn read_bcb(&self) -> Result<Vec<u8>, PlatformError> {
        let mut misc = OpenOptions::new()
            .read(true)
            .open(&self.config.misc_partition)
            .map_err(|error| {
                platform_error(
                    format!("open {}", self.config.misc_partition.display()),
                    error,
                )
            })?;
        misc.seek(SeekFrom::Start(BCB_OFFSET))
            .and_then(|_| {
                let mut bytes = vec![0_u8; BCB_SIZE];
                misc.read_exact(&mut bytes)?;
                Ok(bytes)
            })
            .map_err(|error| platform_error("read Rockchip BCB", error))
    }

    fn commit_recovery_update(
        &self,
        firmware: &Self::Firmware,
        message: &BootloaderMessage,
    ) -> Result<(), PlatformError> {
        self.revalidate_committed(firmware)?;
        self.write_bcb(message)
    }

    fn stage_with_update_engine(&self, firmware: &Self::Firmware) -> Result<(), PlatformError> {
        self.revalidate_committed(firmware)?;
        if !self.config.update_engine.is_file() {
            return Err(PlatformError::new(format!(
                "Rockchip update engine is missing at {}",
                self.config.update_engine.display()
            )));
        }
        let status = Command::new(&self.config.update_engine)
            .arg(format!("--image_url={}", firmware.path.display()))
            .arg("--update")
            .status()
            .map_err(|error| platform_error("start Rockchip updateEngine", error))?;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError::new(format!(
                "updateEngine exited with {status}"
            )))
        }
    }

    fn reboot(&self) -> Result<(), PlatformError> {
        let status = Command::new(&self.config.reboot)
            .status()
            .map_err(|error| platform_error("request reboot", error))?;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError::new(format!(
                "reboot command exited with {status}"
            )))
        }
    }
}

fn opened_firmware(
    file: File,
    path: PathBuf,
    staged_temporary: bool,
) -> Result<OpenedFirmware, PlatformError> {
    let metadata = file
        .metadata()
        .map_err(|error| platform_error("inspect opened firmware", error))?;
    if !metadata.is_file() {
        return Err(PlatformError::new("firmware must be a regular file"));
    }
    Ok(OpenedFirmware {
        file,
        path,
        identity: identity(&metadata),
        staged_temporary,
    })
}

fn revalidate(firmware: &OpenedFirmware) -> Result<(), PlatformError> {
    let descriptor = firmware
        .file
        .metadata()
        .map_err(|error| platform_error("revalidate opened firmware descriptor", error))?;
    if !descriptor.is_file() || identity(&descriptor) != firmware.identity {
        return Err(PlatformError::new(
            "opened firmware identity or size changed",
        ));
    }
    let link = fs::symlink_metadata(&firmware.path)
        .map_err(|error| platform_error("revalidate staged firmware path", error))?;
    if link.file_type().is_symlink() || identity(&link) != firmware.identity {
        return Err(PlatformError::new(
            "firmware path no longer names the verified opened file",
        ));
    }
    Ok(())
}

fn create_unique_temporary(directory: &Path) -> Result<(PathBuf, File), PlatformError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for _ in 0..TEMPORARY_ATTEMPTS {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = directory.join(format!(
            ".upgrade.fw.part.{}.{}.{}",
            std::process::id(),
            timestamp,
            sequence
        ));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(platform_error("create staged firmware temporary", error)),
        }
    }
    Err(PlatformError::new(
        "could not allocate a staged firmware temporary",
    ))
}

fn validate_source_url(source: &str) -> Result<(), PlatformError> {
    let authority = if let Some(rest) = source.strip_prefix("https://") {
        rest
    } else if let Some(rest) = source.strip_prefix("http://") {
        rest
    } else {
        return Err(PlatformError::new(
            "only HTTP and HTTPS firmware sources are accepted",
        ));
    };
    let authority = authority.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return Err(PlatformError::new("firmware source URL has no host"));
    }
    if authority.contains('@') {
        return Err(PlatformError::new(
            "firmware source URLs containing userinfo are rejected",
        ));
    }
    Ok(())
}

fn create_directory(path: &Path, mode: u32) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(mode).create(path)
}

fn ensure_secure_root_directory(path: &Path, purpose: &str) -> Result<(), PlatformError> {
    if !path.is_absolute() {
        return Err(PlatformError::new(format!(
            "{purpose} directory must be an absolute path"
        )));
    }

    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(Path::new("/")),
            Component::Normal(part) => current.push(part),
            _ => {
                return Err(PlatformError::new(format!(
                    "{purpose} directory path must be normalized"
                )))
            }
        }
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                create_directory(&current, 0o755).map_err(|error| {
                    platform_error(format!("create {purpose} directory component"), error)
                })?;
                fs::set_permissions(&current, fs::Permissions::from_mode(0o755)).map_err(
                    |error| platform_error(format!("secure {purpose} directory component"), error),
                )?;
                fs::symlink_metadata(&current).map_err(|error| {
                    platform_error(format!("inspect created {purpose} directory"), error)
                })?
            }
            Err(error) => {
                return Err(platform_error(
                    format!("inspect {purpose} directory"),
                    error,
                ))
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(PlatformError::new(format!(
                "{purpose} path must contain only real directories"
            )));
        }
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(PlatformError::new(format!(
                "{purpose} path must be root-owned and not group/world writable"
            )));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ProcessIdentity {
    pid: u32,
    start_time: u64,
}

fn current_process_identity() -> Result<ProcessIdentity, PlatformError> {
    let pid = std::process::id();
    let start_time = process_start_time(pid)?
        .ok_or_else(|| PlatformError::new("cannot read current process start identity"))?;
    Ok(ProcessIdentity { pid, start_time })
}

fn process_start_time(pid: u32) -> Result<Option<u64>, PlatformError> {
    let path = PathBuf::from(format!("/proc/{pid}/stat"));
    let stat = match fs::read_to_string(path) {
        Ok(stat) => stat,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(platform_error("read OTA lock process identity", error)),
    };
    let after_name = stat
        .rsplit_once(')')
        .map(|(_, fields)| fields.trim())
        .ok_or_else(|| PlatformError::new("invalid process identity data"))?;
    let start_time = after_name
        .split_whitespace()
        .nth(19)
        .and_then(|field| field.parse::<u64>().ok())
        .ok_or_else(|| PlatformError::new("invalid process start identity"))?;
    Ok(Some(start_time))
}

fn reclaim_stale_lock(path: &Path) -> Result<bool, PlatformError> {
    let lock_metadata = metadata_for_path(path)?;
    if !lock_metadata.is_dir() || lock_metadata.uid() != 0 || lock_metadata.mode() & 0o022 != 0 {
        return Err(PlatformError::new(
            "existing OTA mutation lock is not a secure root-owned directory",
        ));
    }
    let lock_identity = lock_inode_from_metadata(&lock_metadata);
    let owner = fs::read_to_string(path.join(LOCK_OWNER_NAME))
        .map_err(|error| platform_error("read OTA lock owner identity", error))?;
    let mut fields = owner.split_whitespace();
    let pid = fields
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| PlatformError::new("invalid OTA lock owner PID"))?;
    let start_time = fields
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| PlatformError::new("invalid OTA lock owner start identity"))?;
    if process_start_time(pid)? == Some(start_time) {
        return Ok(false);
    }
    if lock_inode(path)? != lock_identity {
        return Ok(true);
    }
    match fs::remove_file(path.join(LOCK_OWNER_NAME)) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(platform_error("remove stale OTA lock owner", error)),
    }
    if lock_inode(path)? == lock_identity {
        match fs::remove_dir(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(platform_error("remove stale OTA mutation lock", error)),
        }
    }
    Ok(true)
}

fn metadata_for_path(path: &Path) -> Result<Metadata, PlatformError> {
    fs::symlink_metadata(path).map_err(|error| platform_error("inspect filesystem identity", error))
}

fn lock_inode(path: &Path) -> Result<LockInode, PlatformError> {
    metadata_for_path(path).map(|metadata| lock_inode_from_metadata(&metadata))
}

fn lock_inode_from_metadata(metadata: &Metadata) -> LockInode {
    LockInode {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn remove_owned_lock_directory(path: &Path, expected: LockInode) {
    if lock_inode(path).ok() != Some(expected) {
        return;
    }
    let _ = fs::remove_file(path.join(LOCK_OWNER_NAME));
    if lock_inode(path).ok() == Some(expected) {
        let _ = fs::remove_dir(path);
    }
}

fn identity(metadata: &Metadata) -> FirmwareIdentity {
    FirmwareIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.size(),
    }
}

fn sync_directory(path: &Path) -> Result<(), PlatformError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| platform_error("sync OTA staging directory", error))
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn platform_error(context: impl AsRef<str>, error: io::Error) -> PlatformError {
    PlatformError::new(format!("{}: {error}", context.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::validate_source_url;

    #[test]
    fn source_url_rejects_userinfo_without_reflecting_it() {
        let secret = "https://operator:query-secret@example.invalid/upgrade.fw?token=also-secret";
        let error = validate_source_url(secret).unwrap_err().to_string();
        assert!(error.contains("userinfo"));
        assert!(!error.contains("operator"));
        assert!(!error.contains("query-secret"));
        assert!(!error.contains("also-secret"));
    }

    #[test]
    fn source_url_accepts_http_without_userinfo() {
        validate_source_url("http://example.invalid/upgrade.fw?token=opaque").unwrap();
    }
}
