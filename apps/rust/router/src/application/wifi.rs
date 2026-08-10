use crate::{
    application::ports::PlatformError,
    domain::network_config::{
        ApConfig, NetworkConfigSummary, PendingNetworkConfigSummary, StaConfig, WifiCountry,
        WifiPassphrase, WifiSsid,
    },
};
use serde::{Deserialize, Serialize};

pub const AP_CONFIRM_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaCandidateRequest {
    pub ssid: WifiSsid,
    pub passphrase: WifiPassphrase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApPrepareRequest {
    pub ssid: WifiSsid,
    pub passphrase: WifiPassphrase,
    pub country: WifiCountry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WifiScanEntry {
    pub ssid: WifiSsid,
    pub bssid: String,
    pub frequency_mhz: u16,
    pub signal_dbm: i16,
    pub secured: bool,
}

pub trait WifiPlatformPort: Send + Sync {
    fn recover_interrupted_ap_transaction(&self) -> Result<(), PlatformError>;
    fn scan(&self) -> Result<Vec<WifiScanEntry>, PlatformError>;
    fn committed_config(&self) -> Result<NetworkConfigSummary, PlatformError>;
    fn apply_sta_candidate(&self, candidate: &StaConfig) -> Result<(), PlatformError>;
    fn sta_candidate_ready(&self, candidate: &StaConfig) -> Result<bool, PlatformError>;
    fn commit_sta_candidate(
        &self,
        candidate: StaConfig,
    ) -> Result<NetworkConfigSummary, PlatformError>;
    fn rollback_sta_candidate(&self) -> Result<(), PlatformError>;
    fn prepare_ap_candidate(
        &self,
        candidate: ApConfig,
        staged_at_unix_ms: u64,
    ) -> Result<PendingNetworkConfigSummary, PlatformError>;
    fn apply_ap_candidate(
        &self,
        applied_at_unix_ms: u64,
    ) -> Result<PendingNetworkConfigSummary, PlatformError>;
    fn confirm_ap_candidate(&self) -> Result<NetworkConfigSummary, PlatformError>;
    fn cancel_ap_candidate(&self) -> Result<NetworkConfigSummary, PlatformError>;
    fn pending_ap_candidate(&self) -> Result<Option<PendingNetworkConfigSummary>, PlatformError>;
    fn ap_candidate_applied(&self) -> Result<bool, PlatformError>;
}

pub struct WifiApplication<'a, P: WifiPlatformPort> {
    platform: &'a P,
}

impl<'a, P: WifiPlatformPort> WifiApplication<'a, P> {
    pub const fn new(platform: &'a P) -> Self {
        Self { platform }
    }

    pub fn recover(&self) -> Result<(), PlatformError> {
        self.platform.recover_interrupted_ap_transaction()
    }

    pub fn scan(&self) -> Result<Vec<WifiScanEntry>, PlatformError> {
        self.platform.scan()
    }

    pub fn committed(&self) -> Result<NetworkConfigSummary, PlatformError> {
        self.platform.committed_config()
    }

    pub fn pending(&self) -> Result<Option<PendingNetworkConfigSummary>, PlatformError> {
        self.platform.pending_ap_candidate()
    }

    pub fn apply_sta(
        &self,
        request: StaCandidateRequest,
    ) -> Result<NetworkConfigSummary, PlatformError> {
        let candidate = StaConfig::from_passphrase(request.ssid, &request.passphrase);
        if let Err(error) = self.platform.apply_sta_candidate(&candidate) {
            let _ = self.platform.rollback_sta_candidate();
            return Err(error);
        }
        match self.platform.sta_candidate_ready(&candidate) {
            Ok(true) => {}
            Ok(false) => {
                let rollback = self.platform.rollback_sta_candidate();
                return Err(rollback.err().unwrap_or_else(|| {
                    PlatformError::InvalidState(
                        "STA candidate did not become ready; restored committed configuration"
                            .to_owned(),
                    )
                }));
            }
            Err(error) => {
                let _ = self.platform.rollback_sta_candidate();
                return Err(error);
            }
        }
        // Persistence is deliberately last: an applied candidate is not committed until readiness
        // has been positively observed. A failed commit restores the old runtime configuration.
        match self.platform.commit_sta_candidate(candidate) {
            Ok(summary) => Ok(summary),
            Err(error) => {
                let _ = self.platform.rollback_sta_candidate();
                Err(error)
            }
        }
    }

    pub fn prepare_ap(
        &self,
        request: ApPrepareRequest,
        staged_at_unix_ms: u64,
    ) -> Result<PendingNetworkConfigSummary, PlatformError> {
        let candidate =
            ApConfig::from_passphrase(request.ssid, &request.passphrase, request.country);
        self.platform
            .prepare_ap_candidate(candidate, staged_at_unix_ms)
    }

    pub fn apply_ap(
        &self,
        applied_at_unix_ms: u64,
    ) -> Result<PendingNetworkConfigSummary, PlatformError> {
        self.platform.apply_ap_candidate(applied_at_unix_ms)
    }

    pub fn confirm_ap(&self) -> Result<NetworkConfigSummary, PlatformError> {
        self.platform.confirm_ap_candidate()
    }

    pub fn cancel_ap(&self) -> Result<NetworkConfigSummary, PlatformError> {
        self.platform.cancel_ap_candidate()
    }

    pub fn expire_ap(&self, now_unix_ms: u64) -> Result<bool, PlatformError> {
        if !self.platform.ap_candidate_applied()? {
            return Ok(false);
        }
        let Some(pending) = self.platform.pending_ap_candidate()? else {
            return Ok(false);
        };
        let deadline = pending
            .staged_at_unix_ms
            .saturating_add(AP_CONFIRM_TIMEOUT_SECS * 1_000);
        if now_unix_ms < deadline {
            return Ok(false);
        }
        self.platform.cancel_ap_candidate()?;
        Ok(true)
    }
}
