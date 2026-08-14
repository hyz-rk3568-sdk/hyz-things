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
        Ok(SubscriptionSummary::from_status(
            configured,
            status.as_ref(),
        ))
    }

    pub fn set_url(&self, value: String) -> Result<SubscriptionSummary, PlatformError> {
        let url = SubscriptionUrl::parse(value).map_err(|error| {
            PlatformError::InvalidState(format!("subscription URL was rejected: {error}"))
        })?;
        self.store.store_url(&url)?;
        self.store
            .store_subscription_status(&SubscriptionStatus::Idle)?;
        self.summary()
    }

    pub fn refresh(&self) -> Result<SubscriptionSummary, PlatformError> {
        self.store
            .store_subscription_status(&SubscriptionStatus::Fetching)?;
        let result = self.refresh_transaction();
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

    fn refresh_transaction(&self) -> Result<GenerationId, PlatformError> {
        let url = self.store.load_url()?.ok_or_else(|| {
            PlatformError::InvalidState("subscription URL is not configured".to_owned())
        })?;
        let fetched = self.transport.fetch(&url)?;
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
            self.source.store_source(&candidate)?;
            if let Err(error) = self.store.activate_generation(&generation) {
                return match self.source.store_source(&old_source) {
                    Ok(()) => Err(error),
                    Err(restore_error) => Err(PlatformError::UnsafeToCutOver(format!(
                        "disabled subscription commit failed and old source could not be restored: {restore_error}"
                    ))),
                };
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
        proxy().reconcile(&ProxyDesired {
            lan_tun_enabled: false,
            tailscale_explicit_proxy_enabled: false,
            direct_macs: direct_macs.clone(),
        })?;
        let cutover = self
            .source
            .store_source(&candidate)
            .and_then(|()| {
                proxy()
                    .reconcile(&ProxyDesired {
                        lan_tun_enabled: features.lan_tun_enabled,
                        tailscale_explicit_proxy_enabled: features.tailscale_explicit_proxy_enabled,
                        direct_macs: direct_macs.clone(),
                    })
                    .map(|_| ())
            })
            .and_then(|()| self.store.activate_generation(&generation));
        if let Err(cutover_error) = cutover {
            let restore = self.restore_live_source(&old_source, features, &direct_macs);
            return match restore {
                Ok(()) => Err(cutover_error),
                Err(restore_error) => Err(PlatformError::UnsafeToCutOver(format!(
                    "subscription cutover failed and old live configuration could not be restored: {restore_error}"
                ))),
            };
        }
        Ok(generation)
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
        .reconcile(&ProxyDesired {
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
        .reconcile(&ProxyDesired {
            lan_tun_enabled: features.lan_tun_enabled,
            tailscale_explicit_proxy_enabled: features.tailscale_explicit_proxy_enabled,
            direct_macs: direct_macs.clone(),
        })
        .map(|_| ())
    }
}
