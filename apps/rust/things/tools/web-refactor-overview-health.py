#!/usr/bin/env python3
"""Tighten Overview health semantics after the information-priority migration.

A successful component probe is not enough to call a capability healthy. Keep
unknown runtime fields neutral, surface explicit mismatches as warnings/failures,
and add deterministic unit coverage for the semantic boundary.
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
OVERVIEW = WEB / "pages/overview.rs"
UI = WEB / "ui.rs"

HELPERS = r'''pub(crate) fn component_runtime_tone(state: ComponentState, runtime: Tone) -> Tone {
    match state {
        ComponentState::Unavailable => Tone::Bad,
        ComponentState::Degraded => {
            if matches!(runtime, Tone::Bad) {
                Tone::Bad
            } else {
                Tone::Warn
            }
        }
        ComponentState::Available => runtime,
    }
}

pub(crate) fn lan_health_tone(
    component: &Component<hyz_things::domain::status::RouterStatus>,
) -> Tone {
    use hyz_things::domain::status::LinkState;

    let runtime = component.data.as_ref().map_or(Tone::Neutral, |router| {
        if router.lan_present == Some(false)
            || router.ap_attached_to_lan == Some(false)
            || router.ap_state == Some(LinkState::Down)
        {
            Tone::Bad
        } else if router.ap_state == Some(LinkState::Connecting) {
            Tone::Warn
        } else if router.lan_present == Some(true)
            && router.lan_address.is_some()
            && router.ap_attached_to_lan == Some(true)
            && router.ap_state == Some(LinkState::Up)
        {
            Tone::Good
        } else {
            Tone::Neutral
        }
    });
    component_runtime_tone(component.state, runtime)
}

fn aggregate_runtime_tones(tones: [Tone; 3]) -> Tone {
    if tones.iter().any(|tone| matches!(tone, Tone::Bad)) {
        Tone::Bad
    } else if tones.iter().any(|tone| matches!(tone, Tone::Warn)) {
        Tone::Warn
    } else if tones.iter().any(|tone| matches!(tone, Tone::Neutral)) {
        Tone::Neutral
    } else {
        Tone::Good
    }
}

fn mihomo_health_tone(status: &ProxyStatus) -> Tone {
    let resources = [
        status.mihomo.process,
        status.mihomo.runtime_config,
        status.mihomo.mixed_port,
    ];
    let Some(required) = status.mihomo.configured_required else {
        return Tone::Neutral;
    };
    if resources
        .iter()
        .any(|state| matches!(state, ProxyResourceState::NotReady))
    {
        return Tone::Warn;
    }
    if resources
        .iter()
        .any(|state| matches!(state, ProxyResourceState::Unknown))
    {
        return Tone::Neutral;
    }
    match (required, resources) {
        (true, [ProxyResourceState::Ready, ProxyResourceState::Ready, ProxyResourceState::Ready])
        | (
            false,
            [
                ProxyResourceState::Absent,
                ProxyResourceState::Absent,
                ProxyResourceState::Absent,
            ],
        ) => Tone::Good,
        _ => Tone::Warn,
    }
}

fn lan_tun_health_tone(status: &ProxyStatus) -> Tone {
    match (status.lan_tun.desired, status.lan_tun.effective) {
        (Some(true), LanTunEffective::Ready)
        | (Some(false), LanTunEffective::OrdinaryNat) => Tone::Good,
        (Some(true), LanTunEffective::OrdinaryNat | LanTunEffective::NotConfirmed)
        | (Some(false), LanTunEffective::Ready) => Tone::Warn,
        (Some(false), LanTunEffective::NotConfirmed) | (None, _) => Tone::Neutral,
    }
}

fn local_system_proxy_health_tone(status: &ProxyStatus) -> Tone {
    match (
        status.local_system_proxy.desired,
        status.local_system_proxy.effective,
    ) {
        (Some(true), LocalSystemProxyEffective::Ready)
        | (Some(false), LocalSystemProxyEffective::Disabled) => Tone::Good,
        (Some(true), LocalSystemProxyEffective::Disabled | LocalSystemProxyEffective::NotConfirmed)
        | (Some(false), LocalSystemProxyEffective::Ready | LocalSystemProxyEffective::NotConfirmed) => {
            Tone::Warn
        }
        (None, _) => Tone::Neutral,
    }
}

pub(crate) fn proxy_health_tone(component: &Component<ProxyStatus>) -> Tone {
    let runtime = component.data.as_ref().map_or(Tone::Neutral, |status| {
        aggregate_runtime_tones([
            mihomo_health_tone(status),
            lan_tun_health_tone(status),
            local_system_proxy_health_tone(status),
        ])
    });
    component_runtime_tone(component.state, runtime)
}

fn tailscale_runtime_tone(status: &TailscaleStatus) -> Tone {
    use hyz_things::domain::status::TailscaleRouteApproval;

    if status.error_category.is_some() {
        return Tone::Warn;
    }
    let Some(desired) = status.desired_mode else {
        return Tone::Neutral;
    };
    match desired {
        TailscaleMode::Disabled => {
            if status.effective_mode == Some(TailscaleMode::Disabled)
                && status.backend_state == TailscaleBackendState::Stopped
            {
                Tone::Good
            } else if status.effective_mode.is_none()
                || status.backend_state == TailscaleBackendState::Unknown
            {
                Tone::Neutral
            } else {
                Tone::Warn
            }
        }
        TailscaleMode::RouterOnly => {
            if status.effective_mode == Some(TailscaleMode::RouterOnly)
                && status.backend_state == TailscaleBackendState::Running
                && status.authenticated == Some(true)
            {
                Tone::Good
            } else if status
                .effective_mode
                .is_some_and(|mode| mode != TailscaleMode::RouterOnly)
                || matches!(
                    status.backend_state,
                    TailscaleBackendState::Stopped | TailscaleBackendState::NeedsLogin
                )
                || status.authenticated == Some(false)
            {
                Tone::Warn
            } else {
                Tone::Neutral
            }
        }
        TailscaleMode::LanSubnetAccess => {
            if status.effective_mode == Some(TailscaleMode::LanSubnetAccess)
                && status.backend_state == TailscaleBackendState::Running
                && status.authenticated == Some(true)
                && status.route_advertised == Some(true)
                && status.local_firewall_ready == Some(true)
                && status.route_approval == TailscaleRouteApproval::Approved
            {
                Tone::Good
            } else if status
                .effective_mode
                .is_some_and(|mode| mode != TailscaleMode::LanSubnetAccess)
                || matches!(
                    status.backend_state,
                    TailscaleBackendState::Stopped | TailscaleBackendState::NeedsLogin
                )
                || status.authenticated == Some(false)
                || status.route_advertised == Some(false)
                || status.local_firewall_ready == Some(false)
                || status.route_approval
                    == TailscaleRouteApproval::UnknownExternalApprovalRequired
            {
                Tone::Warn
            } else {
                Tone::Neutral
            }
        }
    }
}

pub(crate) fn tailscale_health_tone(component: &Component<TailscaleStatus>) -> Tone {
    let runtime = component
        .data
        .as_ref()
        .map_or(Tone::Neutral, tailscale_runtime_tone);
    component_runtime_tone(component.state, runtime)
}

'''

TESTS = r'''#[cfg(test)]
mod health_tests {
    use super::*;
    use hyz_things::domain::status::{
        Component, Issue, LanTunStatus, LinkState, LocalSystemProxyStatus, MihomoCoreStatus,
        RouterStatus, TailscaleConnectionStatus, TailscaleRouteApproval,
    };

    fn ready_lan() -> RouterStatus {
        RouterStatus {
            ap_state: Some(LinkState::Up),
            lan_present: Some(true),
            lan_address: Some("192.168.8.1/24".to_owned()),
            ap_attached_to_lan: Some(true),
            ..RouterStatus::default()
        }
    }

    fn stopped_proxy() -> ProxyStatus {
        ProxyStatus {
            configured: true,
            mihomo: MihomoCoreStatus {
                configured_required: Some(false),
                process: ProxyResourceState::Absent,
                runtime_config: ProxyResourceState::Absent,
                mixed_port: ProxyResourceState::Absent,
            },
            lan_tun: LanTunStatus {
                desired: Some(false),
                effective: LanTunEffective::OrdinaryNat,
                ordinary_nat_fallback: Some(true),
            },
            local_system_proxy: LocalSystemProxyStatus {
                desired: Some(false),
                effective: LocalSystemProxyEffective::Disabled,
            },
        }
    }

    fn disabled_tailscale() -> TailscaleStatus {
        TailscaleStatus {
            desired_mode: Some(TailscaleMode::Disabled),
            effective_mode: Some(TailscaleMode::Disabled),
            backend_state: TailscaleBackendState::Stopped,
            authenticated: Some(true),
            ipv4: None,
            route_advertised: None,
            local_firewall_ready: None,
            route_approval: TailscaleRouteApproval::Approved,
            connection: TailscaleConnectionStatus {
                kind: TailscaleConnectionType::Unknown,
                derp_region: None,
            },
            error_category: None,
        }
    }

    #[test]
    fn lan_health_never_promotes_unknown_runtime_to_good() {
        let unknown = Component::available(RouterStatus::default());
        assert!(matches!(lan_health_tone(&unknown), Tone::Neutral));

        let ready = Component::available(ready_lan());
        assert!(matches!(lan_health_tone(&ready), Tone::Good));

        let mut down = ready_lan();
        down.ap_state = Some(LinkState::Down);
        assert!(matches!(
            lan_health_tone(&Component::available(down)),
            Tone::Bad
        ));
    }

    #[test]
    fn proxy_health_requires_three_known_consistent_runtime_states() {
        let ready = stopped_proxy();
        assert!(matches!(
            proxy_health_tone(&Component::available(ready.clone())),
            Tone::Good
        ));

        let mut unknown = ready.clone();
        unknown.mihomo.runtime_config = ProxyResourceState::Unknown;
        assert!(matches!(
            proxy_health_tone(&Component::available(unknown)),
            Tone::Neutral
        ));

        let mut degraded = ready;
        degraded.lan_tun.desired = Some(true);
        assert!(matches!(
            proxy_health_tone(&Component::available(degraded)),
            Tone::Warn
        ));
    }

    #[test]
    fn tailscale_health_requires_confirmed_mode_and_lan_route_readiness() {
        let disabled = disabled_tailscale();
        assert!(matches!(
            tailscale_health_tone(&Component::available(disabled.clone())),
            Tone::Good
        ));

        let mut lan = disabled;
        lan.desired_mode = Some(TailscaleMode::LanSubnetAccess);
        lan.effective_mode = Some(TailscaleMode::LanSubnetAccess);
        lan.backend_state = TailscaleBackendState::Running;
        lan.authenticated = Some(true);
        lan.route_advertised = Some(true);
        lan.local_firewall_ready = Some(true);
        assert!(matches!(
            tailscale_health_tone(&Component::available(lan.clone())),
            Tone::Good
        ));

        lan.route_approval = TailscaleRouteApproval::UnknownExternalApprovalRequired;
        assert!(matches!(
            tailscale_health_tone(&Component::available(lan.clone())),
            Tone::Warn
        ));

        lan.route_approval = TailscaleRouteApproval::Approved;
        lan.route_advertised = None;
        assert!(matches!(
            tailscale_health_tone(&Component::available(lan)),
            Tone::Neutral
        ));
    }

    #[test]
    fn degraded_component_cannot_render_as_healthy() {
        let degraded = Component::degraded(stopped_proxy(), Issue::new("probe", "probe degraded"));
        assert!(matches!(proxy_health_tone(&degraded), Tone::Warn));
    }
}
'''


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if new in text:
        return text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def migrate_overview_health() -> None:
    text = OVERVIEW.read_text()
    if "pub(crate) fn lan_health_tone(" not in text:
        anchor = "pub(crate) fn health_status_label(tone: Tone) -> &'static str {\n"
        if anchor not in text:
            raise SystemExit("Overview health-label anchor missing")
        text = text.replace(anchor, HELPERS + anchor, 1)

    text = replace_once(
        text,
        "    let lan_tone = component_tone(&snapshot.router);",
        "    let lan_tone = lan_health_tone(&snapshot.router);",
        "LAN health tone",
    )
    text = replace_once(
        text,
        "    let proxy_tone = component_tone(&snapshot.proxy);",
        "    let proxy_tone = proxy_health_tone(&snapshot.proxy);",
        "Proxy health tone",
    )
    text = replace_once(
        text,
        "    let tailscale_tone = component_tone(&snapshot.tailscale);",
        "    let tailscale_tone = tailscale_health_tone(&snapshot.tailscale);",
        "Tailscale health tone",
    )

    if "mod health_tests" not in text:
        text = text.rstrip() + "\n\n" + TESTS + "\n"
    OVERVIEW.write_text(text)


def cleanup_legacy_navigation_tokens() -> None:
    lines = UI.read_text().splitlines()
    obsolete = (
        "pub const WORKSPACE_TABS:",
        "pub const WORKSPACE_TAB:",
        "pub const WORKSPACE_TAB_ACTIVE:",
    )
    filtered = [line for line in lines if not line.startswith(obsolete)]
    UI.write_text("\n".join(filtered) + "\n")


def validate() -> None:
    overview = OVERVIEW.read_text()
    required = (
        "lan_health_tone(&snapshot.router)",
        "proxy_health_tone(&snapshot.proxy)",
        "tailscale_health_tone(&snapshot.tailscale)",
        "TailscaleRouteApproval::Approved",
        "proxy_health_requires_three_known_consistent_runtime_states",
        "tailscale_health_requires_confirmed_mode_and_lan_route_readiness",
    )
    missing = [token for token in required if token not in overview]
    if missing:
        raise SystemExit(f"Overview semantic health migration incomplete: {missing}")

    ui = UI.read_text()
    leaked = [
        token
        for token in ("WORKSPACE_TABS", "WORKSPACE_TAB", "WORKSPACE_TAB_ACTIVE")
        if token in ui
    ]
    if leaked:
        raise SystemExit(f"obsolete navigation tokens remain: {leaked}")


migrate_overview_health()
cleanup_legacy_navigation_tokens()
validate()
print("web refactor Overview semantic health: ready")
