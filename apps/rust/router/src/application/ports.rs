use crate::domain::{
    device_policy::{DevicePolicyConfigV1, LanClientObservation},
    network::{NetworkAction, NetworkObserved},
    panel::{DisplayRequest, DisplayStatus, ProxyDelayResult, ProxyGroup, ProxySelectionRequest},
    proxy::{ProxyAction, ProxyObserved},
    subscription::{GenerationId, SubscriptionStatus, SubscriptionUrl, ValidatedSubscription},
    tailscale::{TailscaleAction, TailscaleLoginUrl, TailscaleObserved, TailscalePeerSnapshot},
};
use std::{error::Error, fmt, net::SocketAddr, time::Duration};

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

pub trait DevicePolicyStorePort: Send + Sync {
    fn load_device_policy(&self) -> Result<DevicePolicyConfigV1, PlatformError>;
    fn load_pending_device_policy(
        &self,
    ) -> Result<Option<(DevicePolicyConfigV1, DevicePolicyConfigV1)>, PlatformError>;
    fn stage_device_policy(
        &self,
        previous: &DevicePolicyConfigV1,
        candidate: &DevicePolicyConfigV1,
    ) -> Result<(), PlatformError>;
    fn commit_device_policy(&self, candidate: &DevicePolicyConfigV1) -> Result<(), PlatformError>;
    fn clear_pending_device_policy(&self) -> Result<(), PlatformError>;
}

pub trait LanClientDiscoveryPort: Send + Sync {
    fn discover_lan_clients(
        &self,
        config: &DevicePolicyConfigV1,
    ) -> Result<Vec<LanClientObservation>, PlatformError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleLease {
    pub path: &'static str,
    pub identity: String,
    pub directory_device: u64,
    pub directory_inode: u64,
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

pub trait TailscalePlatformPort: Send + Sync {
    fn acquire_tailscale_lock(&self) -> Result<LifecycleLease, PlatformError>;
    fn release_tailscale_lock(&self, lease: &LifecycleLease) -> Result<(), PlatformError>;
    fn apply_tailscale(&self, action: &TailscaleAction) -> Result<(), PlatformError>;
    fn request_login(&self) -> Result<TailscaleLoginUrl, PlatformError>;
    fn logout(&self) -> Result<(), PlatformError>;
}

pub trait TailscaleProbePort: Send + Sync {
    fn observe_tailscale(&self) -> Result<TailscaleObserved, PlatformError>;
    fn probe_explicit_proxy_path(&self) -> Result<bool, PlatformError>;
}

pub trait TailnetPeerReadPort: Send + Sync {
    fn read_tailnet_peers(&self) -> Result<TailscalePeerSnapshot, PlatformError>;
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

pub trait SubscriptionResolverPort: Send + Sync {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, PlatformError>;
}

pub trait SubscriptionTransportPort: Send + Sync {
    fn fetch(&self, url: &SubscriptionUrl) -> Result<Vec<u8>, PlatformError>;
}

pub trait SubscriptionStorePort: Send + Sync {
    fn store_url(&self, url: &SubscriptionUrl) -> Result<(), PlatformError>;
    fn clear_url(&self) -> Result<(), PlatformError>;
    fn load_url(&self) -> Result<Option<SubscriptionUrl>, PlatformError>;
    fn store_subscription_status(&self, status: &SubscriptionStatus) -> Result<(), PlatformError>;
    fn load_subscription_status(&self) -> Result<Option<SubscriptionStatus>, PlatformError>;
    fn stage_generation(
        &self,
        generation: &GenerationId,
        subscription: &ValidatedSubscription,
    ) -> Result<(), PlatformError>;
    fn activate_generation(&self, generation: &GenerationId) -> Result<(), PlatformError>;
}

pub trait SubscriptionSourcePort: Send + Sync {
    fn load_source(&self) -> Result<Vec<u8>, PlatformError>;
    fn prepare_candidate(
        &self,
        current_source: &[u8],
        subscription: &ValidatedSubscription,
        lan_tun_enabled: bool,
    ) -> Result<Vec<u8>, PlatformError>;
    fn store_source(&self, source: &[u8]) -> Result<(), PlatformError>;
}

pub trait ClockPort: Send + Sync {
    fn unix_time_millis(&self) -> u64;

    fn ownership_token(&self, prefix: &str) -> Result<String, PlatformError> {
        Ok(format!("{prefix}-{}", self.unix_time_millis()))
    }
}
