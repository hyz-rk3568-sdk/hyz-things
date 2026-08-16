//! Application-level ports for the hyz-things portal.
//!
//! The portal's only outbound dependency is the versioned router control
//! protocol; everything else (admin credentials, clock, randomness) is a
//! small local port implemented by the composition root or adapters.

use hyz_contract::router::{ControlOperation, ControlResult};
use std::error::Error;
use std::fmt;

use crate::domain::admin::AdminCredential;

#[derive(Debug)]
pub enum PlatformError {
    Busy(String),
    Conflict(String),
    ProbeFailed(String),
    CommandFailed(String),
    InvalidState(String),
    NotImplemented(String),
    UnsafeToCutOver(String),
    Io(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, detail) = match self {
            Self::Busy(detail) => ("busy", detail),
            Self::Conflict(detail) => ("conflict", detail),
            Self::ProbeFailed(detail) => ("probe failed", detail),
            Self::CommandFailed(detail) => ("command failed", detail),
            Self::InvalidState(detail) => ("invalid state", detail),
            Self::NotImplemented(detail) => ("not implemented", detail),
            Self::UnsafeToCutOver(detail) => ("unsafe to cut over", detail),
            Self::Io(detail) => ("I/O error", detail),
        };
        write!(formatter, "{kind}: {detail}")
    }
}

impl Error for PlatformError {}

/// Typed router control boundary used by the HTTP layer. Production uses the
/// root-only Unix socket client; the host harness fakes it in-process.
#[async_trait::async_trait]
pub trait PortalControlHandler: Send + Sync + 'static {
    async fn handle(&self, operation: ControlOperation) -> Result<ControlResult, String>;
}

pub trait AdminCredentialStorePort: Send + Sync {
    fn load_admin_credential(&self) -> Result<Option<AdminCredential>, PlatformError>;
    fn save_admin_credential(&self, credential: &AdminCredential) -> Result<(), PlatformError>;
}

pub trait AdminRandomPort: Send + Sync {
    fn fill_random(&self, destination: &mut [u8]) -> Result<(), PlatformError>;
}

pub trait ClockPort: Send + Sync {
    fn unix_time_millis(&self) -> u64;

    fn ownership_token(&self, prefix: &str) -> Result<String, PlatformError> {
        Ok(format!("{prefix}-{}", self.unix_time_millis()))
    }
}
