#[path = "tailscale_reconcile.rs"]
mod tailscale_reconcile;
pub use tailscale_reconcile::{
    tailscale_bootstrap_plan, tailscale_plan, tailscale_router_only_runtime_plan,
    tailscale_shutdown_plan,
};

use crate::{
    application::ports::PlatformError,
    domain::{
        network::{
            ForwardingDesired, NetworkAction, NetworkDesired, NetworkObserved, OwnedResource, Probe,
        },
        proxy::{ProxyAction, ProxyDesired, ProxyObserved},
    },
};

pub fn management_plan(
    observed: &NetworkObserved,
    token: &str,
) -> Result<Vec<NetworkAction>, PlatformError> {
    let mut actions = Vec::new();
    match &observed.bridge {
        Probe::Known(OwnedResource::Absent) => actions.push(NetworkAction::EnsureOwnedBridge {
            token: token.to_owned(),
        }),
        Probe::Known(OwnedResource::Owned { .. }) => {}
        Probe::Known(OwnedResource::Foreign) => {
            return Err(PlatformError::Conflict(
                "br-lan exists without matching ownership".to_owned(),
            ));
        }
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "bridge ownership is unknown: {reason}"
            )));
        }
    }
    if observed.bridge_up != Probe::Known(true) {
        actions.push(NetworkAction::ConfigureBridge);
    }
    if observed.lan_address_present != Probe::Known(true) {
        actions.push(NetworkAction::AssignLanAddress);
    }
    match &observed.management_services_healthy {
        Probe::Known(true) => {}
        Probe::Known(false) => actions.push(NetworkAction::EnsureManagementServices),
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "management service health is unknown: {reason}"
            )));
        }
    }
    // Each downstream member has an independent lifecycle. Repairing one member must not
    // detach or recreate the bridge or the other member.
    if observed.ethernet_lan_attached != Probe::Known(true) {
        actions.push(NetworkAction::AttachEthernetLan);
    }
    if observed.ap_attached != Probe::Known(true) {
        actions.push(NetworkAction::AttachAp);
    }
    Ok(actions)
}

pub fn forwarding_plan(
    desired: &NetworkDesired,
    observed: &NetworkObserved,
    token: &str,
) -> Result<Vec<NetworkAction>, PlatformError> {
    let mut actions = Vec::new();
    match desired.forwarding {
        ForwardingDesired::Enabled => {
            let wan_set = match observed.router_wan_set() {
                Probe::Known(Some(wan_set)) => wan_set,
                Probe::Known(None) => {
                    return Err(PlatformError::UnsafeToCutOver(
                        "no fully confirmed WAN uplink is available for router forwarding"
                            .to_owned(),
                    ));
                }
                Probe::Unknown(reason) => {
                    return Err(PlatformError::ProbeFailed(format!(
                        "WAN readiness is unknown: {reason}"
                    )));
                }
            };
            push_forwarding_capture(&mut actions, &observed.previous_ipv4_forwarding)?;
            match &observed.router_firewall {
                Probe::Known(OwnedResource::Absent) => {
                    actions.push(NetworkAction::InstallRouterFirewall {
                        token: token.to_owned(),
                        wan_set,
                    });
                }
                Probe::Known(OwnedResource::Owned { token }) => match observed.firewall_wan_set {
                    Probe::Known(Some(installed)) if installed == wan_set => {}
                    Probe::Known(Some(installed)) => {
                        actions.push(NetworkAction::ReconfigureRouterFirewall {
                            token: token.clone(),
                            previous_wan_set: installed,
                            wan_set,
                        })
                    }
                    Probe::Known(None) => {
                        return Err(PlatformError::Conflict(
                            "owned router firewall has no observed WAN rule set".to_owned(),
                        ));
                    }
                    Probe::Unknown(ref reason) => {
                        return Err(PlatformError::ProbeFailed(format!(
                            "router firewall WAN set is unknown: {reason}"
                        )));
                    }
                },
                Probe::Known(OwnedResource::Foreign) => {
                    return Err(PlatformError::Conflict(
                        "router firewall chains are not owned".to_owned(),
                    ));
                }
                Probe::Unknown(reason) => {
                    return Err(PlatformError::ProbeFailed(format!(
                        "router firewall ownership is unknown: {reason}"
                    )));
                }
            }
            // Forwarding is the commit point: restrictive chains and hooks exist first.
            if observed.ipv4_forwarding != Probe::Known(true) {
                actions.push(NetworkAction::EnableIpv4Forwarding);
            }
        }
        ForwardingDesired::Disabled => {
            let firewall_token = match &observed.router_firewall {
                Probe::Known(OwnedResource::Owned { token }) => Some(token.clone()),
                Probe::Known(OwnedResource::Absent) => None,
                Probe::Known(OwnedResource::Foreign) => {
                    return Err(PlatformError::Conflict(
                        "refusing to remove foreign router firewall chains".to_owned(),
                    ));
                }
                Probe::Unknown(reason) => {
                    return Err(PlatformError::ProbeFailed(format!(
                        "cannot prove forwarding cleanup is safe: {reason}"
                    )));
                }
            };
            if firewall_token.is_some() || observed.ipv4_forwarding != Probe::Known(false) {
                push_forwarding_capture(&mut actions, &observed.previous_ipv4_forwarding)?;
                actions.push(NetworkAction::DisableIpv4Forwarding);
            }
            if let Some(token) = firewall_token {
                actions.push(NetworkAction::RemoveRouterFirewall { token });
            }
        }
    }
    Ok(actions)
}

fn push_forwarding_capture(
    actions: &mut Vec<NetworkAction>,
    previous: &Probe<Option<bool>>,
) -> Result<(), PlatformError> {
    match previous {
        Probe::Known(Some(_)) => Ok(()),
        Probe::Known(None) => {
            actions.push(NetworkAction::CaptureIpv4Forwarding);
            Ok(())
        }
        Probe::Unknown(reason) => Err(PlatformError::ProbeFailed(format!(
            "ip_forward ownership is unknown: {reason}"
        ))),
    }
}

/// Convenience planner for callers that already hold a post-management observation.
pub fn network_plan(
    desired: &NetworkDesired,
    observed: &NetworkObserved,
    token: &str,
) -> Result<Vec<NetworkAction>, PlatformError> {
    let mut actions = management_plan(observed, token)?;
    actions.extend(forwarding_plan(desired, observed, token)?);
    Ok(actions)
}

pub fn proxy_plan(
    desired: &ProxyDesired,
    observed: &ProxyObserved,
    network: &NetworkObserved,
    token: &str,
) -> Result<Vec<ProxyAction>, PlatformError> {
    let active_gateway = network.active_uplink_observation();
    let tun_gateway_matches = !desired.lan_tun_enabled
        || matches!(
            (&active_gateway, &observed.tun_active_uplink),
            (Probe::Known(Some(active)), Probe::Known(Some(current))) if active == current
        );
    if observed.ready_for(desired) && tun_gateway_matches {
        return Ok(Vec::new());
    }
    let current = match &observed.persisted_features {
        Probe::Known(features) if features.supported() => *features,
        Probe::Known(_) => {
            return Err(PlatformError::ProbeFailed(
                "persisted proxy feature version is unsupported".to_owned(),
            ))
        }
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "persisted proxy features are unknown: {reason}"
            )))
        }
    };
    require_known_proxy_ownership(observed)?;
    let (tun_token, capture_tun) = tun_reconcile_token(observed, token)?;
    let mut actions = Vec::new();
    let direct_mac_change = match &observed.active_direct_macs {
        Probe::Known(active) => active != &desired.direct_macs,
        Probe::Unknown(reason) if current.lan_tun_enabled || desired.lan_tun_enabled => {
            return Err(PlatformError::ProbeFailed(format!(
                "active device policy is unknown: {reason}"
            )))
        }
        Probe::Unknown(_) => false,
    };
    let runtime_change = current.lan_tun_enabled != desired.lan_tun_enabled
        || observed.runtime_config_valid != Probe::Known(true);
    let tun_change = runtime_change || direct_mac_change || !tun_gateway_matches;

    // A pure device-policy change on an already-ready LAN TUN must not tear down the whole data
    // plane. The TUN is up and owned with every resource present except the live direct-MAC set;
    // refresh only those rules in place, keeping the interception entry, watcher and core live.
    if desired.lan_tun_enabled
        && !runtime_change
        && direct_mac_change
        && observed.lan_tun_ready_except_direct_macs()
        && network.ready_for(&crate::domain::network::NetworkDesired::forwarding())
    {
        actions.push(ProxyAction::RefreshTunDirectMacs {
            direct_macs: desired.direct_macs.clone(),
        });
        actions.push(ProxyAction::CommitFeatures {
            features: desired.features(),
        });
        return Ok(actions);
    }

    if tun_change || !desired.lan_tun_enabled {
        if observed.watcher_identity_valid == Probe::Known(true) {
            actions.push(ProxyAction::StopWatcher);
        }
        actions.extend(cleanup_tun_plan(observed)?);
    }
    if observed.process_identity_valid == Probe::Known(true) && runtime_change {
        actions.push(ProxyAction::StopCore);
    }

    if !desired.mihomo_required() {
        if observed.process_identity_valid == Probe::Known(true) && !runtime_change {
            actions.push(ProxyAction::StopCore);
        }
        actions.push(ProxyAction::RemoveRuntimeState);
        actions.push(ProxyAction::CommitFeatures {
            features: desired.features(),
        });
        return Ok(actions);
    }

    let core_needs_start = observed.process_identity_valid == Probe::Known(false) || runtime_change;
    if core_needs_start {
        actions.extend([
            ProxyAction::WriteRuntimeConfig {
                lan_tun_enabled: desired.lan_tun_enabled,
            },
            ProxyAction::ValidateRuntimeConfig,
            ProxyAction::StartCore,
            ProxyAction::WaitForMixedPort,
        ]);
    } else if observed.mixed_port_ready != Probe::Known(true) {
        return Err(PlatformError::UnsafeToCutOver(
            "owned Mihomo core is live but its fixed mixed port is not ready".to_owned(),
        ));
    }

    if desired.lan_tun_enabled {
        if !network.ready_for(&crate::domain::network::NetworkDesired::forwarding()) {
            return Err(PlatformError::UnsafeToCutOver(
                "LAN TUN requires confirmed management LAN, WAN route, forwarding and owned NAT"
                    .to_owned(),
            ));
        }
        let active = match active_gateway {
            Probe::Known(Some(active)) => active,
            Probe::Known(None) => {
                return Err(PlatformError::UnsafeToCutOver(
                    "LAN TUN requires an active DHCP-owned IPv4 gateway".to_owned(),
                ))
            }
            Probe::Unknown(reason) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "active uplink gateway is unknown: {reason}"
                )))
            }
        };
        if !observed.lan_tun_ready(&desired.direct_macs) || tun_change {
            if capture_tun {
                actions.push(ProxyAction::WaitForTunInterface {
                    token: tun_token.clone(),
                });
            }
            actions.extend([
                ProxyAction::CreateTunChains {
                    token: tun_token.clone(),
                    direct_macs: desired.direct_macs.clone(),
                    active,
                },
                ProxyAction::InstallTunForwardHook {
                    token: tun_token.clone(),
                },
                ProxyAction::InstallPolicyRoute,
                ProxyAction::InstallPolicyRule,
                ProxyAction::InstallInterceptionEntry {
                    token: tun_token.clone(),
                },
                ProxyAction::StartWatcher,
                ProxyAction::WaitForWatcher,
            ]);
        }
    }
    actions.push(ProxyAction::CommitFeatures {
        features: desired.features(),
    });
    Ok(actions)
}

fn tun_reconcile_token(
    observed: &ProxyObserved,
    fresh_token: &str,
) -> Result<(String, bool), PlatformError> {
    match (&observed.tun_interface, &observed.tun_firewall) {
        (
            Probe::Known(OwnedResource::Owned { token: interface }),
            Probe::Known(OwnedResource::Owned { token: firewall }),
        ) if interface == firewall => Ok((interface.clone(), false)),
        (
            Probe::Known(OwnedResource::Owned { token: interface }),
            Probe::Known(OwnedResource::Absent),
        ) => Ok((interface.clone(), false)),
        (Probe::Known(OwnedResource::Absent), _) => Ok((fresh_token.to_owned(), true)),
        (Probe::Known(OwnedResource::Owned { .. }), Probe::Known(OwnedResource::Owned { .. })) => {
            Err(PlatformError::Conflict(
                "Mihomo TUN and firewall ownership tokens disagree".to_owned(),
            ))
        }
        (Probe::Known(OwnedResource::Owned { .. }), Probe::Known(OwnedResource::Foreign)) => {
            Err(PlatformError::Conflict(
                "refusing to reconcile an owned Mihomo TUN beside foreign chains".to_owned(),
            ))
        }
        (Probe::Known(OwnedResource::Owned { .. }), Probe::Unknown(reason)) => Err(
            PlatformError::ProbeFailed(format!("Mihomo firewall ownership is unknown: {reason}")),
        ),
        (Probe::Known(OwnedResource::Foreign), _) => Err(PlatformError::Conflict(
            "refusing to reconcile a foreign Mihomo TUN interface".to_owned(),
        )),
        (Probe::Unknown(reason), _) => Err(PlatformError::ProbeFailed(format!(
            "Mihomo TUN interface ownership is unknown: {reason}"
        ))),
    }
}

fn require_known_proxy_ownership(observed: &ProxyObserved) -> Result<(), PlatformError> {
    for (label, probe) in [
        ("Mihomo process", &observed.process_identity_valid),
        ("Mihomo watcher", &observed.watcher_identity_valid),
    ] {
        if let Probe::Unknown(reason) = probe {
            return Err(PlatformError::ProbeFailed(format!(
                "{label} identity is unknown: {reason}"
            )));
        }
    }
    match &observed.tun_interface {
        Probe::Known(OwnedResource::Absent | OwnedResource::Owned { .. }) => {}
        Probe::Known(OwnedResource::Foreign) => {
            return Err(PlatformError::Conflict(
                "refusing foreign hyz-mihomo interface".to_owned(),
            ))
        }
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "Mihomo TUN interface ownership is unknown: {reason}"
            )))
        }
    }
    Ok(())
}

pub fn fail_open_plan(observed: &ProxyObserved) -> Result<Vec<ProxyAction>, PlatformError> {
    cleanup_tun_plan(observed)
}

pub fn shutdown_proxy_plan(observed: &ProxyObserved) -> Result<Vec<ProxyAction>, PlatformError> {
    let mut actions = Vec::new();
    match &observed.watcher_identity_valid {
        Probe::Known(true) => actions.push(ProxyAction::StopWatcher),
        Probe::Known(false) => {}
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "cannot prove watcher shutdown is safe: {reason}"
            )));
        }
    }
    actions.extend(cleanup_tun_plan(observed)?);
    match &observed.process_identity_valid {
        Probe::Known(true) => actions.push(ProxyAction::StopCore),
        Probe::Known(false) => {}
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "cannot prove Mihomo shutdown is safe: {reason}"
            )));
        }
    }
    Ok(actions)
}

pub fn shutdown_network_plan(
    observed: &NetworkObserved,
) -> Result<Vec<NetworkAction>, PlatformError> {
    let mut actions = forwarding_plan(&NetworkDesired::management_only(), observed, "unused")?;
    let restore_forwarding = observed.previous_ipv4_forwarding == Probe::Known(Some(false))
        || observed.previous_ipv4_forwarding == Probe::Known(Some(true))
        || actions.contains(&NetworkAction::CaptureIpv4Forwarding);

    let bridge_token = match &observed.bridge {
        Probe::Known(OwnedResource::Owned { token }) => Some(token.clone()),
        Probe::Known(OwnedResource::Absent) => None,
        Probe::Known(OwnedResource::Foreign) => {
            return Err(PlatformError::Conflict(
                "refusing to remove a foreign br-lan during shutdown".to_owned(),
            ));
        }
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "cannot prove bridge shutdown is safe: {reason}"
            )));
        }
    };

    push_boolean_cleanup(
        &mut actions,
        &observed.ethernet_lan_attached,
        NetworkAction::DetachEthernetLan,
        "Ethernet LAN attachment",
    )?;
    push_boolean_cleanup(
        &mut actions,
        &observed.ap_attached,
        NetworkAction::DetachAp,
        "AP attachment",
    )?;
    // RTL8852BS must be detached while hostapd still keeps p2p0 in AP mode. Stopping hostapd
    // first implicitly removes the master and leaves a later exact detach unable to complete.
    actions.push(NetworkAction::StopManagementServices);
    push_boolean_cleanup(
        &mut actions,
        &observed.lan_address_present,
        NetworkAction::RemoveLanAddress,
        "LAN address",
    )?;
    push_boolean_cleanup(
        &mut actions,
        &observed.bridge_up,
        NetworkAction::SetBridgeDown,
        "bridge link state",
    )?;
    if let Some(token) = bridge_token {
        actions.push(NetworkAction::RemoveOwnedBridge { token });
    } else if actions.iter().any(|action| {
        matches!(
            action,
            NetworkAction::DetachAp
                | NetworkAction::RemoveLanAddress
                | NetworkAction::SetBridgeDown
        )
    }) {
        return Err(PlatformError::Conflict(
            "bridge-owned resources exist while br-lan is absent".to_owned(),
        ));
    }
    if restore_forwarding {
        actions.push(NetworkAction::RestoreIpv4Forwarding);
    }
    Ok(actions)
}

fn push_boolean_cleanup(
    actions: &mut Vec<NetworkAction>,
    observed: &Probe<bool>,
    action: NetworkAction,
    label: &str,
) -> Result<(), PlatformError> {
    match observed {
        Probe::Known(true) => actions.push(action),
        Probe::Known(false) => {}
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "cannot prove {label} cleanup is safe: {reason}"
            )));
        }
    }
    Ok(())
}

pub fn proxy_shutdown_ready(observed: &ProxyObserved) -> bool {
    observed.process_identity_valid == Probe::Known(false)
        && observed.watcher_identity_valid == Probe::Known(false)
        && observed.tun_resources_absent()
}

pub fn shutdown_forwarding_target(observed: &NetworkObserved) -> Result<bool, PlatformError> {
    match (
        &observed.previous_ipv4_forwarding,
        &observed.ipv4_forwarding,
    ) {
        (Probe::Known(Some(previous)), _) => Ok(*previous),
        (Probe::Known(None), Probe::Known(current)) => Ok(*current),
        (Probe::Unknown(reason), _) => Err(PlatformError::ProbeFailed(format!(
            "cannot determine pre-daemon ip_forward value: {reason}"
        ))),
        (_, Probe::Unknown(reason)) => Err(PlatformError::ProbeFailed(format!(
            "cannot determine current ip_forward value: {reason}"
        ))),
    }
}

pub fn network_shutdown_ready(observed: &NetworkObserved, forwarding_target: bool) -> bool {
    observed.bridge == Probe::Known(OwnedResource::Absent)
        && observed.bridge_up == Probe::Known(false)
        && observed.lan_address_present == Probe::Known(false)
        && observed.ap_attached == Probe::Known(false)
        && observed.ethernet_lan_attached == Probe::Known(false)
        && observed.management_services_healthy == Probe::Known(false)
        && observed.active_uplink_probe() == Probe::Known(None)
        && observed.ipv4_forwarding == Probe::Known(forwarding_target)
        && observed.previous_ipv4_forwarding == Probe::Known(None)
        && observed.router_firewall == Probe::Known(OwnedResource::Absent)
}

fn cleanup_tun_plan(observed: &ProxyObserved) -> Result<Vec<ProxyAction>, PlatformError> {
    match &observed.tun_interface {
        Probe::Known(OwnedResource::Absent | OwnedResource::Owned { .. }) => {}
        Probe::Known(OwnedResource::Foreign) => {
            return Err(PlatformError::Conflict(
                "refusing to route to or clean a foreign hyz-mihomo interface".to_owned(),
            ))
        }
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "cannot prove Mihomo TUN interface cleanup is safe: {reason}"
            )))
        }
    }
    let token = match &observed.tun_firewall {
        Probe::Known(OwnedResource::Owned { token }) => Some(token.clone()),
        Probe::Known(OwnedResource::Absent) => None,
        Probe::Known(OwnedResource::Foreign) => {
            return Err(PlatformError::Conflict(
                "refusing to clean foreign Mihomo chains".to_owned(),
            ));
        }
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "cannot prove TUN cleanup is safe: {reason}"
            )));
        }
    };
    let mut actions = Vec::new();
    if observed.interception_entry_present == Probe::Known(true) {
        let token = token.clone().ok_or_else(|| {
            PlatformError::Conflict("interception exists without owned chains".to_owned())
        })?;
        actions.push(ProxyAction::RemoveInterceptionEntry { token });
    } else if !matches!(observed.interception_entry_present, Probe::Known(false)) {
        return Err(PlatformError::ProbeFailed(
            "interception presence is unknown".to_owned(),
        ));
    }
    if observed.policy_rule_present == Probe::Known(true) {
        actions.push(ProxyAction::RemovePolicyRule);
    } else if !matches!(observed.policy_rule_present, Probe::Known(false)) {
        return Err(PlatformError::ProbeFailed(
            "policy rule presence is unknown".to_owned(),
        ));
    }
    if observed.policy_route_present == Probe::Known(true) {
        actions.push(ProxyAction::RemovePolicyRoute);
    } else if !matches!(observed.policy_route_present, Probe::Known(false)) {
        return Err(PlatformError::ProbeFailed(
            "policy route presence is unknown".to_owned(),
        ));
    }
    if let Some(token) = token {
        actions.push(ProxyAction::RemoveTunForwardHook {
            token: token.clone(),
        });
        actions.push(ProxyAction::RemoveTunChains { token });
    }
    Ok(actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed_with_tun(
        tun_interface: OwnedResource,
        tun_firewall: OwnedResource,
    ) -> ProxyObserved {
        let mut observed = ProxyObserved::unknown("test");
        observed.tun_interface = Probe::Known(tun_interface);
        observed.tun_firewall = Probe::Known(tun_firewall);
        observed
    }

    #[test]
    fn uplink_reconcile_reuses_matching_owned_tun_token() {
        let observed = observed_with_tun(
            OwnedResource::Owned {
                token: "existing".to_owned(),
            },
            OwnedResource::Owned {
                token: "existing".to_owned(),
            },
        );

        assert_eq!(
            tun_reconcile_token(&observed, "fresh").unwrap(),
            ("existing".to_owned(), false)
        );
    }

    #[test]
    fn uplink_reconcile_captures_a_new_token_when_tun_is_absent() {
        let observed = observed_with_tun(OwnedResource::Absent, OwnedResource::Absent);

        assert_eq!(
            tun_reconcile_token(&observed, "fresh").unwrap(),
            ("fresh".to_owned(), true)
        );
    }

    #[test]
    fn uplink_reconcile_rejects_disagreeing_owned_tokens() {
        let observed = observed_with_tun(
            OwnedResource::Owned {
                token: "interface".to_owned(),
            },
            OwnedResource::Owned {
                token: "firewall".to_owned(),
            },
        );

        assert!(tun_reconcile_token(&observed, "fresh").is_err());
    }
}
