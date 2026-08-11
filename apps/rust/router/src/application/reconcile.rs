use crate::{
    application::ports::PlatformError,
    domain::{
        network::{
            ForwardingDesired, NetworkAction, NetworkDesired, NetworkObserved, OwnedResource, Probe,
        },
        proxy::{ProxyAction, ProxyDesired, ProxyMode, ProxyObserved},
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
    // RTL8852BS rejects bridge attachment while p2p0 is still a down, uninitialized AP
    // interface. Management startup raises p2p0 and lets hostapd enter AP mode first.
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
            if observed.wan_default_route_present != Probe::Known(true) {
                return Err(PlatformError::UnsafeToCutOver(
                    "wlan0 has no confirmed DHCP-owned default route with metric 600".to_owned(),
                ));
            }
            push_forwarding_capture(&mut actions, &observed.previous_ipv4_forwarding)?;
            match &observed.router_firewall {
                Probe::Known(OwnedResource::Absent) => {
                    actions.push(NetworkAction::InstallRouterFirewall {
                        token: token.to_owned(),
                    });
                }
                Probe::Known(OwnedResource::Owned { .. }) => {}
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
    if observed.ready_for(desired) {
        return Ok(Vec::new());
    }
    if matches!(&observed.persisted_mode, Probe::Unknown(_)) {
        return Err(PlatformError::ProbeFailed(
            "persisted proxy mode is unknown; rollback cannot be guaranteed".to_owned(),
        ));
    }
    let mut actions = Vec::new();
    if observed.watcher_identity_valid == Probe::Known(true) {
        actions.push(ProxyAction::StopWatcher);
    } else if observed.watcher_identity_valid != Probe::Known(false) {
        return Err(PlatformError::ProbeFailed(
            "Mihomo watcher identity is unknown".to_owned(),
        ));
    }
    actions.extend(cleanup_tun_plan(observed)?);
    if observed.process_identity_valid == Probe::Known(true) {
        actions.push(ProxyAction::StopCore);
    } else if !matches!(observed.process_identity_valid, Probe::Known(false)) {
        return Err(PlatformError::ProbeFailed(
            "Mihomo process identity is unknown".to_owned(),
        ));
    }

    match desired.mode {
        ProxyMode::Disabled => {
            actions.push(ProxyAction::CommitMode {
                mode: ProxyMode::Disabled,
            });
        }
        ProxyMode::Explicit => {
            actions.extend([
                ProxyAction::WriteRuntimeConfig {
                    mode: ProxyMode::Explicit,
                },
                ProxyAction::ValidateRuntimeConfig,
                ProxyAction::StartCore,
                ProxyAction::CommitMode {
                    mode: ProxyMode::Explicit,
                },
            ]);
        }
        ProxyMode::Tun => {
            if !network.ready_for(&crate::domain::network::NetworkDesired::forwarding()) {
                return Err(PlatformError::UnsafeToCutOver(
                    "TUN requires confirmed management LAN, WAN route, forwarding and owned NAT"
                        .to_owned(),
                ));
            }
            actions.extend([
                ProxyAction::WriteRuntimeConfig {
                    mode: ProxyMode::Tun,
                },
                ProxyAction::ValidateRuntimeConfig,
                ProxyAction::StartCore,
                ProxyAction::WaitForTunInterface,
                ProxyAction::CreateTunChains {
                    token: token.to_owned(),
                    direct_macs: desired.direct_macs.clone(),
                },
                ProxyAction::InstallTunForwardHook {
                    token: token.to_owned(),
                },
                ProxyAction::InstallPolicyRoute,
                ProxyAction::InstallPolicyRule,
                // Commit point for packet interception is intentionally last.
                ProxyAction::InstallInterceptionEntry {
                    token: token.to_owned(),
                },
                ProxyAction::StartWatcher,
                ProxyAction::WaitForWatcher,
                // Persistent desired mode is committed only after data-plane commit.
                ProxyAction::CommitMode {
                    mode: ProxyMode::Tun,
                },
            ]);
        }
    }
    Ok(actions)
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
        && observed.management_services_healthy == Probe::Known(false)
        && observed.wan_default_route_present == Probe::Known(false)
        && observed.ipv4_forwarding == Probe::Known(forwarding_target)
        && observed.previous_ipv4_forwarding == Probe::Known(None)
        && observed.router_firewall == Probe::Known(OwnedResource::Absent)
}

fn cleanup_tun_plan(observed: &ProxyObserved) -> Result<Vec<ProxyAction>, PlatformError> {
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
