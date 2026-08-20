use crate::{
    application::{
        ports::{
            ClockPort, PlatformError, RouterPlatformPort, SubscriptionSourcePort,
            SubscriptionStorePort, SubscriptionTransportPort, SystemProbePort,
            TailscalePlatformPort, TailscaleProbePort,
        },
        proxy::ProxyFeatureCoordinator,
    },
    domain::{
        network::Probe,
        proxy::{ProxyDesired, ProxyFeaturesV1},
        subscription::{
            parse_mihomo_subscription, GenerationId, SubscriptionStatus, SubscriptionSummary,
            SubscriptionUrl,
        },
    },
};

pub struct SubscriptionRuntimePorts<'a> {
    pub platform: &'a dyn RouterPlatformPort,
    pub probe: &'a dyn SystemProbePort,
    pub tailscale: &'a dyn TailscalePlatformPort,
    pub tailscale_probe: &'a dyn TailscaleProbePort,
    pub clock: &'a dyn ClockPort,
}

pub struct SubscriptionApplication<'a> {
    store: &'a dyn SubscriptionStorePort,
    transport: &'a dyn SubscriptionTransportPort,
    source: &'a dyn SubscriptionSourcePort,
    platform: &'a dyn RouterPlatformPort,
    probe: &'a dyn SystemProbePort,
    tailscale: &'a dyn TailscalePlatformPort,
    tailscale_probe: &'a dyn TailscaleProbePort,
    clock: &'a dyn ClockPort,
}

impl<'a> SubscriptionApplication<'a> {
    pub fn new(
        store: &'a dyn SubscriptionStorePort,
        transport: &'a dyn SubscriptionTransportPort,
        source: &'a dyn SubscriptionSourcePort,
        ports: SubscriptionRuntimePorts<'a>,
    ) -> Self {
        Self {
            store,
            transport,
            source,
            platform: ports.platform,
            probe: ports.probe,
            tailscale: ports.tailscale,
            tailscale_probe: ports.tailscale_probe,
            clock: ports.clock,
        }
    }

    pub fn summary(&self) -> Result<SubscriptionSummary, PlatformError> {
        let configured = self.store.load_url()?.is_some();
        let status = self.store.load_subscription_status()?;
        Ok(crate::domain::subscription::summary_from_status(
            configured,
            status.as_ref(),
        ))
    }

    pub fn refresh(&self) -> Result<SubscriptionSummary, PlatformError> {
        let url = self.store.load_url()?.ok_or_else(|| {
            PlatformError::InvalidState("subscription URL is not configured".to_owned())
        })?;
        self.refresh_with_url(&url, false, None)
    }

    pub fn replace_url_and_refresh(
        &self,
        value: String,
    ) -> Result<SubscriptionSummary, PlatformError> {
        let url = SubscriptionUrl::parse(value).map_err(|error| {
            PlatformError::InvalidState(format!("subscription URL was rejected: {error}"))
        })?;
        let previous_url = self.store.load_url()?;
        self.refresh_with_url(&url, true, previous_url.as_ref())
    }

    fn refresh_with_url(
        &self,
        url: &SubscriptionUrl,
        replace_url: bool,
        previous_url: Option<&SubscriptionUrl>,
    ) -> Result<SubscriptionSummary, PlatformError> {
        self.store
            .store_subscription_status(&SubscriptionStatus::Fetching)?;
        let result = self.refresh_transaction(url, replace_url, previous_url);
        match result {
            Ok(generation) => {
                self.store
                    .store_subscription_status(&SubscriptionStatus::Active(generation))?;
                self.summary()
            }
            Err(error) => {
                let failed = SubscriptionStatus::failed("manual refresh failed")
                    .expect("static subscription failure status is valid");
                let _ = self.store.store_subscription_status(&failed);
                Err(error)
            }
        }
    }

    fn refresh_transaction(
        &self,
        url: &SubscriptionUrl,
        replace_url: bool,
        previous_url: Option<&SubscriptionUrl>,
    ) -> Result<GenerationId, PlatformError> {
        let fetched = self.transport.fetch(url)?;
        let subscription = parse_mihomo_subscription(&fetched).map_err(|error| {
            PlatformError::InvalidState(format!("subscription provider was rejected: {error}"))
        })?;
        let observed = self.probe.observe_proxy()?;
        let features = match observed.persisted_features {
            Probe::Known(features) if features.supported() => features,
            Probe::Known(_) => {
                return Err(PlatformError::ProbeFailed(
                    "persisted proxy feature version is unsupported".to_owned(),
                ))
            }
            Probe::Unknown(_) => {
                return Err(PlatformError::ProbeFailed(
                    "persisted proxy features are unknown".to_owned(),
                ))
            }
        };
        let direct_macs = match observed.active_direct_macs.clone() {
            Probe::Known(macs) => macs,
            Probe::Unknown(reason) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "active device policy is unknown: {reason}"
                )))
            }
        };
        let old_source = self.source.load_source()?;
        let candidate =
            self.source
                .prepare_candidate(&old_source, &subscription, features.lan_tun_enabled)?;
        let generation = GenerationId::parse(format!("g-{}", self.clock.unix_time_millis()))
            .map_err(|_| {
                PlatformError::InvalidState("could not allocate subscription generation".to_owned())
            })?;
        self.store.stage_generation(&generation, &subscription)?;

        if !features.mihomo_required() {
            let mut url_committed = false;
            let cutover = (|| {
                self.source.store_source(&candidate)?;
                if replace_url {
                    self.store.store_url(url)?;
                    url_committed = true;
                }
                self.store.activate_generation(&generation)
            })();
            if let Err(cutover_error) = cutover {
                let restore_source = self.source.store_source(&old_source);
                let restore_url = if url_committed {
                    self.restore_url(previous_url)
                } else {
                    Ok(())
                };
                return Err(Self::rollback_error(
                    cutover_error,
                    restore_source,
                    restore_url,
                ));
            }
            return Ok(generation);
        }

        let proxy = || {
            ProxyFeatureCoordinator::new(
                self.platform,
                self.probe,
                self.tailscale,
                self.tailscale_probe,
                self.clock,
            )
        };
        proxy().reconcile_runtime_preserving_features(&ProxyDesired {
            lan_tun_enabled: false,
            tailscale_explicit_proxy_enabled: false,
            direct_macs: direct_macs.clone(),
        })?;
        let mut url_committed = false;
        let cutover = (|| {
            self.source.store_source(&candidate)?;
            proxy()
                .reconcile_runtime_preserving_features(&ProxyDesired {
                    lan_tun_enabled: features.lan_tun_enabled,
                    tailscale_explicit_proxy_enabled: features.tailscale_explicit_proxy_enabled,
                    direct_macs: direct_macs.clone(),
                })
                .map(|_| ())?;
            if replace_url {
                self.store.store_url(url)?;
                url_committed = true;
            }
            self.store.activate_generation(&generation)
        })();
        if let Err(cutover_error) = cutover {
            let restore = self.restore_live_source(&old_source, features, &direct_macs);
            let restore_url = if url_committed {
                self.restore_url(previous_url)
            } else {
                Ok(())
            };
            return Err(Self::rollback_error(cutover_error, restore, restore_url));
        }
        Ok(generation)
    }

    fn restore_url(&self, previous_url: Option<&SubscriptionUrl>) -> Result<(), PlatformError> {
        match previous_url {
            Some(url) => self.store.store_url(url),
            None => self.store.clear_url(),
        }
    }

    fn rollback_error(
        primary: PlatformError,
        restore_live: Result<(), PlatformError>,
        restore_url: Result<(), PlatformError>,
    ) -> PlatformError {
        match (restore_live, restore_url) {
            (Ok(()), Ok(())) => primary,
            (Err(restore), Ok(())) | (Ok(()), Err(restore)) => {
                PlatformError::UnsafeToCutOver(format!(
                    "subscription cutover failed and rollback also failed: {restore}"
                ))
            }
            (Err(restore_live), Err(restore_url)) => PlatformError::UnsafeToCutOver(format!(
                "subscription cutover failed and rollback also failed: live={restore_live}; url={restore_url}"
            )),
        }
    }

    fn restore_live_source(
        &self,
        old_source: &[u8],
        features: ProxyFeaturesV1,
        direct_macs: &std::collections::BTreeSet<crate::domain::device_policy::LanDeviceMac>,
    ) -> Result<(), PlatformError> {
        ProxyFeatureCoordinator::new(
            self.platform,
            self.probe,
            self.tailscale,
            self.tailscale_probe,
            self.clock,
        )
        .reconcile_runtime_preserving_features(&ProxyDesired {
            lan_tun_enabled: false,
            tailscale_explicit_proxy_enabled: false,
            direct_macs: direct_macs.clone(),
        })?;
        self.source.store_source(old_source)?;
        ProxyFeatureCoordinator::new(
            self.platform,
            self.probe,
            self.tailscale,
            self.tailscale_probe,
            self.clock,
        )
        .reconcile_runtime_preserving_features(&ProxyDesired {
            lan_tun_enabled: features.lan_tun_enabled,
            tailscale_explicit_proxy_enabled: features.tailscale_explicit_proxy_enabled,
            direct_macs: direct_macs.clone(),
        })
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::ports::LifecycleLease,
        domain::{
            network::{NetworkAction, NetworkObserved},
            proxy::{ProxyAction, ProxyObserved},
            subscription::ValidatedSubscription,
            tailscale::{TailscaleAction, TailscaleLoginUrl, TailscaleObserved},
        },
    };
    use std::sync::Mutex;

    fn unexpected<T>() -> Result<T, PlatformError> {
        Err(PlatformError::InvalidState(
            "unexpected test call".to_owned(),
        ))
    }

    #[derive(Default)]
    struct FakeStore {
        url: Mutex<Option<String>>,
        status: Mutex<Option<SubscriptionStatus>>,
    }

    impl SubscriptionStorePort for FakeStore {
        fn store_url(&self, url: &SubscriptionUrl) -> Result<(), PlatformError> {
            let mut bytes = Vec::new();
            url.write_secret(&mut bytes)
                .map_err(|_| PlatformError::InvalidState("test URL encode failed".to_owned()))?;
            *self.url.lock().unwrap() =
                Some(String::from_utf8(bytes).map_err(|_| {
                    PlatformError::InvalidState("test URL was not UTF-8".to_owned())
                })?);
            Ok(())
        }

        fn clear_url(&self) -> Result<(), PlatformError> {
            *self.url.lock().unwrap() = None;
            Ok(())
        }

        fn load_url(&self) -> Result<Option<SubscriptionUrl>, PlatformError> {
            self.url
                .lock()
                .unwrap()
                .as_ref()
                .map(|value| SubscriptionUrl::parse(value.clone()))
                .transpose()
                .map_err(|_| PlatformError::InvalidState("test URL was invalid".to_owned()))
        }

        fn store_subscription_status(
            &self,
            status: &SubscriptionStatus,
        ) -> Result<(), PlatformError> {
            *self.status.lock().unwrap() = Some(status.clone());
            Ok(())
        }

        fn load_subscription_status(&self) -> Result<Option<SubscriptionStatus>, PlatformError> {
            Ok(self.status.lock().unwrap().clone())
        }

        fn stage_generation(
            &self,
            _generation: &GenerationId,
            _subscription: &ValidatedSubscription,
        ) -> Result<(), PlatformError> {
            unexpected()
        }

        fn activate_generation(&self, _generation: &GenerationId) -> Result<(), PlatformError> {
            unexpected()
        }
    }

    struct FailingTransport;

    impl SubscriptionTransportPort for FailingTransport {
        fn fetch(&self, _url: &SubscriptionUrl) -> Result<Vec<u8>, PlatformError> {
            Err(PlatformError::Io("fetch failed".to_owned()))
        }
    }

    struct NeverCalledSource;

    impl SubscriptionSourcePort for NeverCalledSource {
        fn load_source(&self) -> Result<Vec<u8>, PlatformError> {
            unexpected()
        }

        fn prepare_candidate(
            &self,
            _current_source: &[u8],
            _subscription: &ValidatedSubscription,
            _lan_tun_enabled: bool,
        ) -> Result<Vec<u8>, PlatformError> {
            unexpected()
        }

        fn store_source(&self, _source: &[u8]) -> Result<(), PlatformError> {
            unexpected()
        }
    }

    struct NeverCalledRouter;

    impl RouterPlatformPort for NeverCalledRouter {
        fn acquire_lifecycle_lock(&self) -> Result<LifecycleLease, PlatformError> {
            unexpected()
        }

        fn release_lifecycle_lock(&self, _lease: &LifecycleLease) -> Result<(), PlatformError> {
            unexpected()
        }

        fn apply_network(&self, _action: &NetworkAction) -> Result<(), PlatformError> {
            unexpected()
        }

        fn apply_proxy(&self, _action: &ProxyAction) -> Result<(), PlatformError> {
            unexpected()
        }
    }

    struct NeverCalledProbe;

    impl SystemProbePort for NeverCalledProbe {
        fn observe_network(&self) -> Result<NetworkObserved, PlatformError> {
            unexpected()
        }

        fn observe_proxy(&self) -> Result<ProxyObserved, PlatformError> {
            unexpected()
        }
    }

    struct NeverCalledTailscale;

    impl TailscalePlatformPort for NeverCalledTailscale {
        fn acquire_tailscale_lock(&self) -> Result<LifecycleLease, PlatformError> {
            unexpected()
        }

        fn release_tailscale_lock(&self, _lease: &LifecycleLease) -> Result<(), PlatformError> {
            unexpected()
        }

        fn apply_tailscale(&self, _action: &TailscaleAction) -> Result<(), PlatformError> {
            unexpected()
        }

        fn request_login(&self) -> Result<TailscaleLoginUrl, PlatformError> {
            unexpected()
        }

        fn logout(&self) -> Result<(), PlatformError> {
            unexpected()
        }
    }

    struct NeverCalledTailscaleProbe;

    impl TailscaleProbePort for NeverCalledTailscaleProbe {
        fn observe_tailscale(&self) -> Result<TailscaleObserved, PlatformError> {
            unexpected()
        }

        fn probe_explicit_proxy_path(&self) -> Result<bool, PlatformError> {
            unexpected()
        }
    }

    struct FixedClock;

    impl ClockPort for FixedClock {
        fn unix_time_millis(&self) -> u64 {
            1
        }
    }

    #[test]
    fn failed_url_replacement_preserves_last_known_good_url() {
        let old_url = "https://1.1.1.1/old".to_owned();
        let store = FakeStore {
            url: Mutex::new(Some(old_url.clone())),
            ..Default::default()
        };
        let router = NeverCalledRouter;
        let probe = NeverCalledProbe;
        let tailscale = NeverCalledTailscale;
        let tailscale_probe = NeverCalledTailscaleProbe;
        let clock = FixedClock;
        let application = SubscriptionApplication::new(
            &store,
            &FailingTransport,
            &NeverCalledSource,
            SubscriptionRuntimePorts {
                platform: &router,
                probe: &probe,
                tailscale: &tailscale,
                tailscale_probe: &tailscale_probe,
                clock: &clock,
            },
        );

        let result = application.replace_url_and_refresh("https://1.1.1.2/new".to_owned());

        assert_eq!(result, Err(PlatformError::Io("fetch failed".to_owned())));
        assert_eq!(store.url.lock().unwrap().as_deref(), Some(old_url.as_str()));
        assert_eq!(
            store.load_subscription_status().unwrap(),
            Some(SubscriptionStatus::Failed(
                "manual refresh failed".to_owned()
            ))
        );
    }
}
