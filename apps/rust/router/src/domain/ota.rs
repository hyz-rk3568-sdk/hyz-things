use std::error::Error;
use std::fmt;

pub const RKFW_MAGIC: &[u8; 4] = b"RKFW";
pub const BCB_OFFSET: u64 = 16 * 1024;
pub const BCB_SIZE: usize = 1088;
pub const BCB_COMMAND_SIZE: usize = 32;
pub const BCB_STATUS_SIZE: usize = 32;
pub const BCB_RECOVERY_SIZE: usize = 768;
pub const BCB_NEED_UPDATE_OFFSET: usize = BCB_COMMAND_SIZE + BCB_STATUS_SIZE + BCB_RECOVERY_SIZE;
pub const RECOVERY_UPDATE_MASK: u32 = 0x003b_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    pub fn parse(value: &str) -> Result<Self, OtaDomainError> {
        let value = value.trim();
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(OtaDomainError::InvalidDigest);
        }

        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let offset = index * 2;
            *byte = (hex_nibble(value.as_bytes()[offset])? << 4)
                | hex_nibble(value.as_bytes()[offset + 1])?;
        }
        Ok(Self(bytes))
    }

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        let mut result = String::with_capacity(64);
        for byte in self.0 {
            use fmt::Write as _;
            let _ = write!(result, "{byte:02x}");
        }
        result
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

fn hex_nibble(byte: u8) -> Result<u8, OtaDomainError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(OtaDomainError::InvalidDigest),
    }
}

pub fn verify_rkfw_prefix(prefix: &[u8]) -> Result<(), OtaDomainError> {
    if prefix == RKFW_MAGIC {
        Ok(())
    } else {
        Err(OtaDomainError::InvalidRkfwMagic)
    }
}

pub fn verify_digest(expected: Sha256Digest, actual: Sha256Digest) -> Result<(), OtaDomainError> {
    if expected == actual {
        Ok(())
    } else {
        Err(OtaDomainError::DigestMismatch { expected, actual })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootloaderMessage([u8; BCB_SIZE]);

impl BootloaderMessage {
    pub fn for_update_package(path: &str) -> Result<Self, OtaDomainError> {
        if path.contains(['\0', '\n', '\r']) {
            return Err(OtaDomainError::UnsupportedFirmwarePath);
        }

        let recovery = format!("recovery\n--update_package={path}\n");
        if recovery.len() >= BCB_RECOVERY_SIZE {
            return Err(OtaDomainError::FirmwarePathTooLong);
        }

        let mut message = [0_u8; BCB_SIZE];
        message[..b"boot-recovery".len()].copy_from_slice(b"boot-recovery");
        let recovery_offset = BCB_COMMAND_SIZE + BCB_STATUS_SIZE;
        message[recovery_offset..recovery_offset + recovery.len()]
            .copy_from_slice(recovery.as_bytes());
        message[BCB_NEED_UPDATE_OFFSET..BCB_NEED_UPDATE_OFFSET + 4]
            .copy_from_slice(&RECOVERY_UPDATE_MASK.to_le_bytes());
        Ok(Self(message))
    }

    pub const fn as_bytes(&self) -> &[u8; BCB_SIZE] {
        &self.0
    }
}

pub fn ensure_no_pending_command(actual: &[u8]) -> Result<(), OtaDomainError> {
    ensure_bcb_size(actual)?;
    if actual[..BCB_COMMAND_SIZE].iter().any(|byte| *byte != 0) {
        Err(OtaDomainError::PendingBootloaderCommand)
    } else {
        Ok(())
    }
}

pub fn verify_bcb_exact(expected: &BootloaderMessage, actual: &[u8]) -> Result<(), OtaDomainError> {
    ensure_bcb_size(actual)?;
    if actual == expected.as_bytes() {
        Ok(())
    } else {
        Err(OtaDomainError::BcbVerificationFailed)
    }
}

fn ensure_bcb_size(actual: &[u8]) -> Result<(), OtaDomainError> {
    if actual.len() == BCB_SIZE {
        Ok(())
    } else {
        Err(OtaDomainError::InvalidBcbSize {
            expected: BCB_SIZE,
            actual: actual.len(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OtaDomainError {
    InvalidDigest,
    InvalidRkfwMagic,
    DigestMismatch {
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    UnsupportedFirmwarePath,
    FirmwarePathTooLong,
    InvalidBcbSize {
        expected: usize,
        actual: usize,
    },
    PendingBootloaderCommand,
    BcbVerificationFailed,
}

impl fmt::Display for OtaDomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDigest => {
                formatter.write_str("SHA-256 must be exactly 64 hexadecimal characters")
            }
            Self::InvalidRkfwMagic => formatter.write_str("firmware is not a Rockchip RKFW image"),
            Self::DigestMismatch { expected, actual } => {
                write!(
                    formatter,
                    "SHA-256 mismatch: expected {expected}, got {actual}"
                )
            }
            Self::UnsupportedFirmwarePath => {
                formatter.write_str("firmware path contains an unsupported control character")
            }
            Self::FirmwarePathTooLong => {
                formatter.write_str("firmware path is too long for the recovery BCB")
            }
            Self::InvalidBcbSize { expected, actual } => write!(
                formatter,
                "invalid BCB size: expected {expected} bytes, got {actual}"
            ),
            Self::PendingBootloaderCommand => {
                formatter.write_str("misc already contains a pending bootloader command")
            }
            Self::BcbVerificationFailed => {
                formatter.write_str("misc BCB verification failed after staging")
            }
        }
    }
}

impl Error for OtaDomainError {}
