use crate::{
    application::ports::PlatformError,
    domain::network_config::{
        ApConfig, NetworkConfigSummary, PendingNetworkConfigSummary, StaConfig,
        PRODUCT_WIFI_COUNTRY,
    },
};

pub use hyz_contract::wifi::{ApPrepareRequest, StaCandidateRequest, WifiScanEntry};

pub const AP_CONFIRM_TIMEOUT_SECS: u64 = 120;

pub trait WifiPlatformPort: Send + Sync {
    fn recover_interrupted_ap_transaction(&self) -> Result<(), PlatformError>;
    fn scan(&self) -> Result<Vec<WifiScanEntry>, PlatformError>;
    fn committed_config(&self) -> Result<NetworkConfigSummary, PlatformError>;
    /// Persists `candidate` as the committed STA and applies it by reusing the proven cold-start
    /// management sequence. No live-swap rollback journal is created: the management AP stays
    /// fail-open, so a failed upstream remains reachable from the portal for a retry.
    fn apply_sta(&self, candidate: &StaConfig) -> Result<NetworkConfigSummary, PlatformError>;
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

    pub fn pending_status(
        &self,
    ) -> Result<(Option<PendingNetworkConfigSummary>, bool), PlatformError> {
        Ok((
            self.platform.pending_ap_candidate()?,
            self.platform.ap_candidate_applied()?,
        ))
    }

    pub fn apply_sta(
        &self,
        request: StaCandidateRequest,
    ) -> Result<NetworkConfigSummary, PlatformError> {
        let candidate = StaConfig::from_passphrase(request.ssid, &request.passphrase);
        self.platform.apply_sta(&candidate)
    }

    pub fn prepare_ap(
        &self,
        request: ApPrepareRequest,
        staged_at_unix_ms: u64,
    ) -> Result<PendingNetworkConfigSummary, PlatformError> {
        if request.country != PRODUCT_WIFI_COUNTRY {
            return Err(PlatformError::Conflict(
                "product Wi-Fi country is fixed to CN".to_owned(),
            ));
        }
        let candidate =
            ApConfig::from_passphrase(request.ssid, &request.passphrase, PRODUCT_WIFI_COUNTRY);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::network_config::{
        WifiCountry, WifiPassphrase, WifiSsid, NETWORK_CONFIG_VERSION,
    };
    use std::sync::Mutex;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FailurePoint {
        None,
        Apply,
    }

    struct FakeWifiPlatform {
        failure: FailurePoint,
        calls: Mutex<Vec<&'static str>>,
    }

    impl FakeWifiPlatform {
        fn new(failure: FailurePoint) -> Self {
            Self {
                failure,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn record(&self, call: &'static str) {
            self.calls.lock().unwrap().push(call);
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().unwrap().clone()
        }
    }

    fn summary(sta_ssid: WifiSsid) -> NetworkConfigSummary {
        NetworkConfigSummary {
            version: NETWORK_CONFIG_VERSION,
            ap_ssid: WifiSsid::new("router-ap").unwrap(),
            sta_ssid,
            country: WifiCountry::Cn,
        }
    }

    fn request() -> StaCandidateRequest {
        StaCandidateRequest {
            ssid: WifiSsid::new("candidate").unwrap(),
            passphrase: WifiPassphrase::new("candidate-password").unwrap(),
        }
    }

    impl WifiPlatformPort for FakeWifiPlatform {
        fn recover_interrupted_ap_transaction(&self) -> Result<(), PlatformError> {
            unreachable!()
        }

        fn scan(&self) -> Result<Vec<WifiScanEntry>, PlatformError> {
            unreachable!()
        }

        fn committed_config(&self) -> Result<NetworkConfigSummary, PlatformError> {
            unreachable!()
        }

        fn apply_sta(&self, candidate: &StaConfig) -> Result<NetworkConfigSummary, PlatformError> {
            self.record("apply");
            if self.failure == FailurePoint::Apply {
                return Err(PlatformError::CommandFailed("candidate apply".to_owned()));
            }
            Ok(summary(candidate.ssid.clone()))
        }

        fn prepare_ap_candidate(
            &self,
            _candidate: ApConfig,
            _staged_at_unix_ms: u64,
        ) -> Result<PendingNetworkConfigSummary, PlatformError> {
            unreachable!()
        }

        fn apply_ap_candidate(
            &self,
            _applied_at_unix_ms: u64,
        ) -> Result<PendingNetworkConfigSummary, PlatformError> {
            unreachable!()
        }

        fn confirm_ap_candidate(&self) -> Result<NetworkConfigSummary, PlatformError> {
            unreachable!()
        }

        fn cancel_ap_candidate(&self) -> Result<NetworkConfigSummary, PlatformError> {
            unreachable!()
        }

        fn pending_ap_candidate(
            &self,
        ) -> Result<Option<PendingNetworkConfigSummary>, PlatformError> {
            unreachable!()
        }

        fn ap_candidate_applied(&self) -> Result<bool, PlatformError> {
            unreachable!()
        }
    }

    #[test]
    fn prepare_ap_rejects_non_cn_country() {
        let platform = FakeWifiPlatform::new(FailurePoint::None);
        let request = ApPrepareRequest {
            ssid: WifiSsid::new("candidate-ap").unwrap(),
            passphrase: WifiPassphrase::new("candidate-password").unwrap(),
            country: WifiCountry::Us,
        };
        let error = WifiApplication::new(&platform)
            .prepare_ap(request, 1_000)
            .expect_err("non-CN AP country must be rejected");
        assert_eq!(
            error,
            PlatformError::Conflict("product Wi-Fi country is fixed to CN".to_owned())
        );
        assert!(platform.calls().is_empty());
    }

    #[test]
    fn sta_apply_commits_directly_without_candidate_flow() {
        let platform = FakeWifiPlatform::new(FailurePoint::None);
        let result = WifiApplication::new(&platform)
            .apply_sta(request())
            .unwrap();
        assert_eq!(result.sta_ssid.as_str(), "candidate");
        assert_eq!(platform.calls(), vec!["apply"]);
    }

    #[test]
    fn sta_apply_propagates_platform_failure_without_rollback() {
        let platform = FakeWifiPlatform::new(FailurePoint::Apply);
        let error = WifiApplication::new(&platform)
            .apply_sta(request())
            .unwrap_err();
        assert_eq!(
            error,
            PlatformError::CommandFailed("candidate apply".to_owned())
        );
        assert_eq!(platform.calls(), vec!["apply"]);
    }
}
