use crate::domain::{
    admin::AdminCredential,
    network::{NetworkAction, NetworkObserved},
    panel::{DisplayRequest, DisplayStatus, ProxyDelayResult, ProxyGroup, ProxySelectionRequest},
    proxy::{ProxyAction, ProxyObserved},
};
use std::{error::Error, fmt, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
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

pub trait AdminCredentialStorePort: Send + Sync {
    fn load_admin_credential(&self) -> Result<Option<AdminCredential>, PlatformError>;
    fn save_admin_credential(&self, credential: &AdminCredential) -> Result<(), PlatformError>;
}

pub trait AdminRandomPort: Send + Sync {
    fn fill_random(&self, destination: &mut [u8]) -> Result<(), PlatformError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleLease {
    pub path: &'static str,
    pub identity: String,
}

pub trait RouterPlatformPort: Send + Sync {
    fn acquire_lifecycle_lock(&self) -> Result<LifecycleLease, PlatformError>;
    fn release_lifecycle_lock(&self, lease: &LifecycleLease) -> Result<(), PlatformError>;
    fn apply_network(&self, action: &NetworkAction) -> Result<(), PlatformError>;
    fn apply_proxy(&self, action: &ProxyAction) -> Result<(), PlatformError>;
}

pub trait SystemProbePort: Send + Sync {
    fn observe_network(&self) -> Result<NetworkObserved, PlatformError>;
    fn observe_proxy(&self) -> Result<ProxyObserved, PlatformError>;
}

pub trait PanelPlatformPort: Send + Sync {
    fn display_status(&self) -> Result<DisplayStatus, PlatformError>;
    fn set_display(&self, request: &DisplayRequest) -> Result<DisplayStatus, PlatformError>;
    fn proxy_groups(&self) -> Result<Vec<ProxyGroup>, PlatformError>;
    fn select_proxy(&self, request: &ProxySelectionRequest) -> Result<(), PlatformError>;
    fn measure_proxy_delay(&self, proxy: &str) -> Result<ProxyDelayResult, PlatformError>;
    fn refresh_proxy_delays(&self) -> Result<Vec<ProxyGroup>, PlatformError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreIdentity {
    pub pid: u32,
    pub start_time: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreRecordState {
    ExpectedLive,
    ExpectedExited,
    Replaced,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailOpenRetryKind {
    CoreProbe,
    Lock,
    Cleanup,
    LockRelease,
}

pub trait FailOpenPlatformPort: Send + Sync {
    fn watcher_role_matches(&self, expected: CoreIdentity) -> Result<bool, PlatformError>;
    fn core_record_state(&self, expected: CoreIdentity) -> Result<CoreRecordState, PlatformError>;
    fn acquire_fail_open_lock(&self) -> Result<LifecycleLease, PlatformError>;
    fn release_fail_open_lock(&self, lease: &LifecycleLease) -> Result<(), PlatformError>;
    fn observe_fail_open_proxy(&self) -> Result<ProxyObserved, PlatformError>;
    fn apply_fail_open_proxy(&self, action: &ProxyAction) -> Result<(), PlatformError>;
    fn remove_fail_open_stale_state(&self, expected: CoreIdentity) -> Result<(), PlatformError>;
    fn sleep_fail_open_retry(&self, duration: Duration);
    fn record_fail_open_retry(&self, _kind: FailOpenRetryKind, _error: &PlatformError) {}
}

pub trait ClockPort: Send + Sync {
    fn unix_time_millis(&self) -> u64;

    fn ownership_token(&self, prefix: &str) -> Result<String, PlatformError> {
        Ok(format!("{prefix}-{}", self.unix_time_millis()))
    }
}
