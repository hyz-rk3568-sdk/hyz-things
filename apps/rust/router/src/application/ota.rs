use crate::domain::ota::{
    ensure_no_pending_command, verify_bcb_exact, verify_digest, verify_rkfw_prefix,
    BootloaderMessage, OtaDomainError, Sha256Digest,
};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const OTA_OPERATION_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareIdentity {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
}

pub trait TrustedFirmware {
    fn path(&self) -> &Path;
    fn identity(&self) -> FirmwareIdentity;
}

pub trait FirmwareMutationLock {}

pub trait FirmwarePlatformPort {
    type Firmware: TrustedFirmware;

    fn acquire_mutation_lock(&self) -> Result<Box<dyn FirmwareMutationLock + '_>, PlatformError>;
    fn open_firmware(&self, path: &Path) -> Result<Self::Firmware, PlatformError>;
    fn copy_to_staging_temporary(
        &self,
        source: &mut Self::Firmware,
    ) -> Result<Self::Firmware, PlatformError>;
    fn download_to_staging_temporary(
        &self,
        source: &str,
        deadline: Instant,
    ) -> Result<Self::Firmware, PlatformError>;
    fn commit_staged(&self, firmware: &mut Self::Firmware) -> Result<(), PlatformError>;
    fn discard_staged(&self, firmware: &Self::Firmware) -> Result<(), PlatformError>;
    fn read_firmware_prefix(&self, firmware: &mut Self::Firmware)
        -> Result<Vec<u8>, PlatformError>;
    fn sha256(&self, firmware: &mut Self::Firmware) -> Result<[u8; 32], PlatformError>;
    fn sync_firmware(&self, firmware: &Self::Firmware) -> Result<(), PlatformError>;
    fn revalidate_firmware(&self, firmware: &Self::Firmware) -> Result<(), PlatformError>;
    fn recovery_partition_exists(&self) -> Result<bool, PlatformError>;
    fn read_bcb(&self) -> Result<Vec<u8>, PlatformError>;
    fn commit_recovery_update(
        &self,
        firmware: &Self::Firmware,
        message: &BootloaderMessage,
    ) -> Result<(), PlatformError>;
    fn stage_with_update_engine(
        &self,
        firmware: &Self::Firmware,
        deadline: Instant,
    ) -> Result<(), PlatformError>;
    fn reboot(&self, deadline: Instant) -> Result<(), PlatformError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformError {
    message: String,
}

impl PlatformError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PlatformError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallMode {
    RecoveryFree,
    IncludeRecovery,
}

pub struct OtaService<P> {
    platform: P,
}

impl<P> OtaService<P>
where
    P: FirmwarePlatformPort,
{
    pub const fn new(platform: P) -> Self {
        Self { platform }
    }

    pub fn platform(&self) -> &P {
        &self.platform
    }

    pub fn verify(&self, firmware: &Path, expected: &str) -> Result<(), OtaError> {
        let expected = Sha256Digest::parse(expected)?;
        let mut firmware = self.platform.open_firmware(firmware)?;
        self.verify_firmware(&mut firmware, expected)
    }

    pub fn download(&self, source: &str, expected: &str) -> Result<PathBuf, OtaError> {
        let expected = Sha256Digest::parse(expected)?;
        let deadline = Instant::now() + OTA_OPERATION_TIMEOUT;
        let _lock = self.platform.acquire_mutation_lock()?;
        let firmware = self.download_locked(source, expected, deadline)?;
        Ok(firmware.path().to_path_buf())
    }

    pub fn install(
        &self,
        firmware: &Path,
        expected: &str,
        mode: InstallMode,
        reboot: bool,
    ) -> Result<PathBuf, OtaError> {
        let expected = Sha256Digest::parse(expected)?;
        let deadline = Instant::now() + OTA_OPERATION_TIMEOUT;
        let _lock = self.platform.acquire_mutation_lock()?;
        let mut source = self.platform.open_firmware(firmware)?;
        let mut staged = self.platform.copy_to_staging_temporary(&mut source)?;
        if let Err(error) = self.verify_and_commit(&mut staged, expected) {
            let _ = self.platform.discard_staged(&staged);
            return Err(error);
        }
        self.install_committed(&staged, mode, reboot, deadline)
    }

    pub fn apply(&self, source: &str, expected: &str, reboot: bool) -> Result<PathBuf, OtaError> {
        let expected = Sha256Digest::parse(expected)?;
        let deadline = Instant::now() + OTA_OPERATION_TIMEOUT;
        let _lock = self.platform.acquire_mutation_lock()?;
        let firmware = self.download_locked(source, expected, deadline)?;
        self.install_committed(&firmware, InstallMode::RecoveryFree, reboot, deadline)
    }

    fn download_locked(
        &self,
        source: &str,
        expected: Sha256Digest,
        deadline: Instant,
    ) -> Result<P::Firmware, OtaError> {
        let mut temporary = self
            .platform
            .download_to_staging_temporary(source, deadline)?;
        if let Err(error) = self.verify_and_commit(&mut temporary, expected) {
            let _ = self.platform.discard_staged(&temporary);
            return Err(error);
        }
        Ok(temporary)
    }

    fn verify_and_commit(
        &self,
        firmware: &mut P::Firmware,
        expected: Sha256Digest,
    ) -> Result<(), OtaError> {
        self.verify_firmware(firmware, expected)?;
        self.platform.commit_staged(firmware)?;
        self.platform.revalidate_firmware(firmware)?;
        Ok(())
    }

    fn install_committed(
        &self,
        firmware: &P::Firmware,
        mode: InstallMode,
        reboot: bool,
        deadline: Instant,
    ) -> Result<PathBuf, OtaError> {
        self.platform.sync_firmware(firmware)?;

        let current = self.platform.read_bcb()?;
        ensure_no_pending_command(&current)?;

        let path = firmware
            .path()
            .to_str()
            .ok_or(OtaError::InvalidFirmwarePathEncoding)?;
        let message = BootloaderMessage::for_update_package(path)?;

        match mode {
            InstallMode::RecoveryFree => {
                if !self.platform.recovery_partition_exists()? {
                    return Err(OtaError::RecoveryPartitionMissing);
                }
                self.platform.revalidate_firmware(firmware)?;
                self.platform.commit_recovery_update(firmware, &message)?;
            }
            InstallMode::IncludeRecovery => {
                self.platform.revalidate_firmware(firmware)?;
                self.platform.stage_with_update_engine(firmware, deadline)?;
            }
        }

        let staged = self.platform.read_bcb()?;
        verify_bcb_exact(&message, &staged)?;
        if reboot {
            self.platform.reboot(deadline)?;
        }
        Ok(firmware.path().to_path_buf())
    }

    fn verify_firmware(
        &self,
        firmware: &mut P::Firmware,
        expected: Sha256Digest,
    ) -> Result<(), OtaError> {
        self.platform.revalidate_firmware(firmware)?;
        let prefix = self.platform.read_firmware_prefix(firmware)?;
        verify_rkfw_prefix(&prefix)?;
        let actual = Sha256Digest::from_bytes(self.platform.sha256(firmware)?);
        verify_digest(expected, actual)?;
        self.platform.revalidate_firmware(firmware)?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum OtaError {
    Domain(OtaDomainError),
    Platform(PlatformError),
    InvalidFirmwarePathEncoding,
    RecoveryPartitionMissing,
}

impl fmt::Display for OtaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(error) => error.fmt(formatter),
            Self::Platform(error) => error.fmt(formatter),
            Self::InvalidFirmwarePathEncoding => {
                formatter.write_str("firmware path must be valid UTF-8")
            }
            Self::RecoveryPartitionMissing => formatter.write_str("recovery partition is missing"),
        }
    }
}

impl Error for OtaError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Domain(error) => Some(error),
            Self::Platform(error) => Some(error),
            Self::InvalidFirmwarePathEncoding | Self::RecoveryPartitionMissing => None,
        }
    }
}

impl From<OtaDomainError> for OtaError {
    fn from(error: OtaDomainError) -> Self {
        Self::Domain(error)
    }
}

impl From<PlatformError> for OtaError {
    fn from(error: PlatformError) -> Self {
        Self::Platform(error)
    }
}
