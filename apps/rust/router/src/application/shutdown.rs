use crate::{
    application::{
        ports::{PlatformError, RouterPlatformPort, SystemProbePort},
        reconcile::{
            network_shutdown_ready, proxy_shutdown_ready, shutdown_forwarding_target,
            shutdown_network_plan, shutdown_proxy_plan,
        },
    },
    domain::{network::NetworkAction, proxy::ProxyAction},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownResult {
    pub proxy_actions_applied: usize,
    pub network_actions_applied: usize,
}

/// Tears down only runtime-owned state. Persisted router/proxy intent and userdata are preserved so
/// the next daemon start can restore the selected mode.
pub struct ShutdownApplication<'a> {
    platform: &'a dyn RouterPlatformPort,
    probe: &'a dyn SystemProbePort,
}

impl<'a> ShutdownApplication<'a> {
    pub fn new(platform: &'a dyn RouterPlatformPort, probe: &'a dyn SystemProbePort) -> Self {
        Self { platform, probe }
    }

    pub fn execute(&self) -> Result<ShutdownResult, PlatformError> {
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = (|| {
            let proxy = self.probe.observe_proxy()?;
            let proxy_actions = shutdown_proxy_plan(&proxy)?;
            apply_proxy_actions(self.platform, &proxy_actions)?;

            // Proxy interception and its core disappear before ordinary forwarding and management
            // services. This keeps fail-open behavior valid throughout teardown.
            let network = self.probe.observe_network()?;
            let forwarding_target = shutdown_forwarding_target(&network)?;
            let network_actions = shutdown_network_plan(&network)?;
            apply_network_actions(self.platform, &network_actions)?;

            let final_proxy = self.probe.observe_proxy()?;
            let final_network = self.probe.observe_network()?;
            if !proxy_shutdown_ready(&final_proxy)
                || !network_shutdown_ready(&final_network, forwarding_target)
            {
                return Err(PlatformError::UnsafeToCutOver(
                    "runtime teardown completed but strict shutdown readiness failed".to_owned(),
                ));
            }
            Ok(ShutdownResult {
                proxy_actions_applied: proxy_actions.len(),
                network_actions_applied: network_actions.len(),
            })
        })();
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(result), Ok(())) => Ok(result),
        }
    }
}

fn apply_proxy_actions(
    platform: &dyn RouterPlatformPort,
    actions: &[ProxyAction],
) -> Result<(), PlatformError> {
    for action in actions {
        platform.apply_proxy(action).map_err(|error| {
            PlatformError::CommandFailed(format!(
                "shutdown proxy action {action:?} failed: {error}"
            ))
        })?;
    }
    Ok(())
}

fn apply_network_actions(
    platform: &dyn RouterPlatformPort,
    actions: &[NetworkAction],
) -> Result<(), PlatformError> {
    for action in actions {
        platform.apply_network(action).map_err(|error| {
            PlatformError::CommandFailed(format!(
                "shutdown network action {action:?} failed: {error}"
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        network::{NetworkObserved, OwnedResource, Probe},
        proxy::{ProxyFeaturesV1, ProxyObserved},
    };

    fn active_proxy() -> ProxyObserved {
        ProxyObserved {
            persisted_features: Probe::Known(ProxyFeaturesV1::new(true, true)),
            process_identity_valid: Probe::Known(true),
            watcher_identity_valid: Probe::Known(true),
            runtime_config_valid: Probe::Known(true),
            mixed_port_ready: Probe::Known(true),
            tun_interface_present: Probe::Known(true),
            tun_firewall: Probe::Known(OwnedResource::Owned {
                token: "proxy-owned".to_owned(),
            }),
            policy_rule_present: Probe::Known(true),
            policy_route_present: Probe::Known(true),
            interception_entry_present: Probe::Known(true),
            ordinary_nat_confirmed: Probe::Known(false),
            active_direct_macs: Probe::Known(Default::default()),
        }
    }

    fn active_network() -> NetworkObserved {
        NetworkObserved {
            bridge: Probe::Known(OwnedResource::Owned {
                token: "router-owned".to_owned(),
            }),
            bridge_up: Probe::Known(true),
            lan_address_present: Probe::Known(true),
            ap_attached: Probe::Known(true),
            management_services_healthy: Probe::Known(true),
            wan_default_route_present: Probe::Known(true),
            ipv4_forwarding: Probe::Known(true),
            previous_ipv4_forwarding: Probe::Known(Some(false)),
            router_firewall: Probe::Known(OwnedResource::Owned {
                token: "router-owned".to_owned(),
            }),
        }
    }

    #[test]
    fn shutdown_is_proxy_first_cleanup_and_never_commits_persisted_mode() {
        let proxy = shutdown_proxy_plan(&active_proxy()).unwrap();
        assert_eq!(
            proxy,
            vec![
                ProxyAction::StopWatcher,
                ProxyAction::RemoveInterceptionEntry {
                    token: "proxy-owned".to_owned(),
                },
                ProxyAction::RemovePolicyRule,
                ProxyAction::RemovePolicyRoute,
                ProxyAction::RemoveTunForwardHook {
                    token: "proxy-owned".to_owned(),
                },
                ProxyAction::RemoveTunChains {
                    token: "proxy-owned".to_owned(),
                },
                ProxyAction::StopCore,
            ]
        );
        assert!(!proxy.iter().any(|action| matches!(
            action,
            ProxyAction::CommitFeatures { .. } | ProxyAction::RestorePersistedFeatures { .. }
        )));

        let network = shutdown_network_plan(&active_network()).unwrap();
        assert_eq!(
            network,
            vec![
                NetworkAction::DisableIpv4Forwarding,
                NetworkAction::RemoveRouterFirewall {
                    token: "router-owned".to_owned(),
                },
                NetworkAction::DetachAp,
                NetworkAction::StopManagementServices,
                NetworkAction::RemoveLanAddress,
                NetworkAction::SetBridgeDown,
                NetworkAction::RemoveOwnedBridge {
                    token: "router-owned".to_owned(),
                },
                NetworkAction::RestoreIpv4Forwarding,
            ]
        );
    }

    #[test]
    fn shutdown_restores_a_preexisting_enabled_forwarding_switch() {
        let mut network = active_network();
        network.previous_ipv4_forwarding = Probe::Known(Some(true));
        assert!(shutdown_forwarding_target(&network).unwrap());
        assert_eq!(
            shutdown_network_plan(&network).unwrap().last(),
            Some(&NetworkAction::RestoreIpv4Forwarding)
        );

        network.bridge = Probe::Known(OwnedResource::Absent);
        network.bridge_up = Probe::Known(false);
        network.lan_address_present = Probe::Known(false);
        network.ap_attached = Probe::Known(false);
        network.management_services_healthy = Probe::Known(false);
        network.wan_default_route_present = Probe::Known(false);
        network.ipv4_forwarding = Probe::Known(true);
        network.previous_ipv4_forwarding = Probe::Known(None);
        network.router_firewall = Probe::Known(OwnedResource::Absent);
        assert!(network_shutdown_ready(&network, true));
    }

    #[test]
    fn strict_shutdown_readiness_rejects_unknown_or_residual_resources() {
        let mut proxy = active_proxy();
        assert!(!proxy_shutdown_ready(&proxy));
        proxy.process_identity_valid = Probe::Known(false);
        proxy.watcher_identity_valid = Probe::Known(false);
        proxy.tun_interface_present = Probe::Known(false);
        proxy.tun_firewall = Probe::Known(OwnedResource::Absent);
        proxy.policy_rule_present = Probe::Known(false);
        proxy.policy_route_present = Probe::Known(false);
        proxy.interception_entry_present = Probe::Known(false);
        assert!(proxy_shutdown_ready(&proxy));

        let mut network = active_network();
        assert!(!network_shutdown_ready(&network, false));
        network.bridge = Probe::Known(OwnedResource::Absent);
        network.bridge_up = Probe::Known(false);
        network.lan_address_present = Probe::Known(false);
        network.ap_attached = Probe::Known(false);
        network.management_services_healthy = Probe::Known(false);
        network.wan_default_route_present = Probe::Known(false);
        network.ipv4_forwarding = Probe::Known(false);
        network.previous_ipv4_forwarding = Probe::Known(None);
        network.router_firewall = Probe::Known(OwnedResource::Absent);
        assert!(network_shutdown_ready(&network, false));
    }
}
