use crate::{
    application::ports::PlatformError,
    domain::network_config::{
        ApConfig, NetworkConfigSummary, NetworkConfigV1, PendingNetworkConfigSummary, StaConfig,
        WifiCountry, WifiPassphrase, WifiSsid,
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
    fn begin_sta_candidate(&self) -> Result<NetworkConfigV1, PlatformError>;
    fn apply_sta_candidate(&self, candidate: &StaConfig) -> Result<(), PlatformError>;
    fn sta_candidate_ready(&self, candidate: &StaConfig) -> Result<bool, PlatformError>;
    fn commit_sta_candidate(
        &self,
        candidate: StaConfig,
    ) -> Result<NetworkConfigSummary, PlatformError>;
    fn rollback_sta_candidate(&self, committed: &NetworkConfigV1) -> Result<(), PlatformError>;
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
        let committed = self.platform.begin_sta_candidate()?;
        if let Err(error) = self.platform.apply_sta_candidate(&candidate) {
            return Err(self.rollback_sta_failure(error, &committed));
        }
        match self.platform.sta_candidate_ready(&candidate) {
            Ok(true) => {}
            Ok(false) => {
                return Err(self.rollback_sta_failure(
                    PlatformError::InvalidState("STA candidate did not become ready".to_owned()),
                    &committed,
                ));
            }
            Err(error) => return Err(self.rollback_sta_failure(error, &committed)),
        }
        // Persistence is deliberately last: an applied candidate is not committed until readiness
        // has been positively observed. A failed commit restores the old runtime configuration.
        match self.platform.commit_sta_candidate(candidate) {
            Ok(summary) => Ok(summary),
            Err(error) => Err(self.rollback_sta_failure(error, &committed)),
        }
    }

    fn rollback_sta_failure(
        &self,
        primary: PlatformError,
        committed: &NetworkConfigV1,
    ) -> PlatformError {
        match self.platform.rollback_sta_candidate(committed) {
            Ok(()) => primary,
            Err(rollback) => PlatformError::InvalidState(format!(
                "STA transaction failed: {primary}; committed network restoration also failed: {rollback}"
            )),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::network_config::{WifiCountry, NETWORK_CONFIG_VERSION};
    use std::sync::Mutex;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FailurePoint {
        None,
        Apply,
        ReadinessFalse,
        ReadinessError,
        Commit,
    }

    struct FakeWifiPlatform {
        failure: FailurePoint,
        rollback_fails: bool,
        calls: Mutex<Vec<&'static str>>,
    }

    impl FakeWifiPlatform {
        fn new(failure: FailurePoint, rollback_fails: bool) -> Self {
            Self {
                failure,
                rollback_fails,
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

    fn committed_record() -> NetworkConfigV1 {
        let passphrase = WifiPassphrase::new("committed-password").unwrap();
        NetworkConfigV1::new(
            ApConfig::from_passphrase(
                WifiSsid::new("router-ap").unwrap(),
                &passphrase,
                WifiCountry::Cn,
            ),
            StaConfig::from_passphrase(WifiSsid::new("committed").unwrap(), &passphrase),
        )
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

        fn begin_sta_candidate(&self) -> Result<NetworkConfigV1, PlatformError> {
            self.record("begin");
            Ok(committed_record())
        }

        fn apply_sta_candidate(&self, _candidate: &StaConfig) -> Result<(), PlatformError> {
            self.record("apply");
            if self.failure == FailurePoint::Apply {
                return Err(PlatformError::CommandFailed("candidate apply".to_owned()));
            }
            Ok(())
        }

        fn sta_candidate_ready(&self, _candidate: &StaConfig) -> Result<bool, PlatformError> {
            self.record("ready");
            match self.failure {
                FailurePoint::ReadinessFalse => Ok(false),
                FailurePoint::ReadinessError => {
                    Err(PlatformError::ProbeFailed("candidate readiness".to_owned()))
                }
                _ => Ok(true),
            }
        }

        fn commit_sta_candidate(
            &self,
            candidate: StaConfig,
        ) -> Result<NetworkConfigSummary, PlatformError> {
            self.record("commit");
            if self.failure == FailurePoint::Commit {
                return Err(PlatformError::Io("candidate commit".to_owned()));
            }
            Ok(summary(candidate.ssid))
        }

        fn rollback_sta_candidate(&self, committed: &NetworkConfigV1) -> Result<(), PlatformError> {
            self.record("rollback");
            assert_eq!(committed.sta.ssid.as_str(), "committed");
            if self.rollback_fails {
                return Err(PlatformError::UnsafeToCutOver(
                    "committed restoration".to_owned(),
                ));
            }
            Ok(())
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
    fn every_sta_transaction_failure_rolls_back_exactly_once() {
        let cases = [
            (FailurePoint::Apply, vec!["begin", "apply", "rollback"]),
            (
                FailurePoint::ReadinessFalse,
                vec!["begin", "apply", "ready", "rollback"],
            ),
            (
                FailurePoint::ReadinessError,
                vec!["begin", "apply", "ready", "rollback"],
            ),
            (
                FailurePoint::Commit,
                vec!["begin", "apply", "ready", "commit", "rollback"],
            ),
        ];
        for (failure, expected_calls) in cases {
            let platform = FakeWifiPlatform::new(failure, false);
            assert!(WifiApplication::new(&platform)
                .apply_sta(request())
                .is_err());
            assert_eq!(platform.calls(), expected_calls);
        }
    }

    #[test]
    fn successful_sta_transaction_commits_without_rollback() {
        let platform = FakeWifiPlatform::new(FailurePoint::None, false);
        let result = WifiApplication::new(&platform)
            .apply_sta(request())
            .unwrap();
        assert_eq!(result.sta_ssid.as_str(), "candidate");
        assert_eq!(platform.calls(), vec!["begin", "apply", "ready", "commit"]);
    }

    #[test]
    fn rollback_failure_preserves_both_errors() {
        let platform = FakeWifiPlatform::new(FailurePoint::Apply, true);
        let error = WifiApplication::new(&platform)
            .apply_sta(request())
            .unwrap_err();
        assert_eq!(platform.calls(), vec!["begin", "apply", "rollback"]);
        assert_eq!(
            error,
            PlatformError::InvalidState(
                "STA transaction failed: command failed: candidate apply; committed network restoration also failed: unsafe to cut over: committed restoration".to_owned()
            )
        );
    }
}
