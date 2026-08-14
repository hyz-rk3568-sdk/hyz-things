#![cfg(feature = "native")]

use hyz_router::{
    adapters::outbound::system::forward_hook_order_is_exact,
    application::{
        ports::{ClockPort, LifecycleLease, PlatformError, RouterPlatformPort, SystemProbePort},
        reconcile::{forwarding_plan, management_plan, network_plan, proxy_plan},
        router::RouterApplication,
    },
    domain::{
        network::{NetworkAction, NetworkDesired, NetworkObserved, OwnedResource, Probe},
        proxy::{ProxyAction, ProxyDesired, ProxyFeaturesV1, ProxyObserved},
    },
};
use std::{collections::VecDeque, sync::Mutex};

fn network(forwarding: bool, firewall: OwnedResource) -> NetworkObserved {
    NetworkObserved {
        bridge: Probe::Known(OwnedResource::Owned {
            token: "bridge-old".to_owned(),
        }),
        bridge_up: Probe::Known(true),
        lan_address_present: Probe::Known(true),
        ap_attached: Probe::Known(true),
        management_services_healthy: Probe::Known(true),
        wan_default_route_present: Probe::Known(true),
        ipv4_forwarding: Probe::Known(forwarding),
        previous_ipv4_forwarding: Probe::Known(Some(false)),
        router_firewall: Probe::Known(firewall),
    }
}

fn stopped_proxy() -> ProxyObserved {
    ProxyObserved {
        persisted_features: Probe::Known(ProxyFeaturesV1::disabled()),
        process_identity_valid: Probe::Known(false),
        watcher_identity_valid: Probe::Known(false),
        runtime_config_valid: Probe::Known(false),
        mixed_port_ready: Probe::Known(false),
        tun_interface_present: Probe::Known(false),
        tun_firewall: Probe::Known(OwnedResource::Absent),
        policy_rule_present: Probe::Known(false),
        policy_route_present: Probe::Known(false),
        interception_entry_present: Probe::Known(false),
        ordinary_nat_confirmed: Probe::Known(true),
        active_direct_macs: Probe::Known(Default::default()),
    }
}

fn ready_proxy(features: ProxyFeaturesV1) -> ProxyObserved {
    let tun = features.lan_tun_enabled;
    let core = features.mihomo_required();
    ProxyObserved {
        persisted_features: Probe::Known(features),
        process_identity_valid: Probe::Known(core),
        watcher_identity_valid: Probe::Known(tun),
        runtime_config_valid: Probe::Known(core),
        mixed_port_ready: Probe::Known(core),
        tun_interface_present: Probe::Known(tun),
        tun_firewall: Probe::Known(if tun {
            OwnedResource::Owned {
                token: "proxy-old".to_owned(),
            }
        } else {
            OwnedResource::Absent
        }),
        policy_rule_present: Probe::Known(tun),
        policy_route_present: Probe::Known(tun),
        interception_entry_present: Probe::Known(tun),
        ordinary_nat_confirmed: Probe::Known(!tun),
        active_direct_macs: Probe::Known(Default::default()),
    }
}

#[test]
fn cold_management_start_initializes_ap_services_before_bridge_attachment() {
    let mut observed = network(false, OwnedResource::Absent);
    observed.bridge = Probe::Known(OwnedResource::Absent);
    observed.bridge_up = Probe::Known(false);
    observed.lan_address_present = Probe::Known(false);
    observed.ap_attached = Probe::Known(false);
    observed.management_services_healthy = Probe::Known(false);
    observed.wan_default_route_present = Probe::Known(false);

    assert_eq!(
        network_plan(&NetworkDesired::management_only(), &observed, "new").unwrap(),
        vec![
            NetworkAction::EnsureOwnedBridge {
                token: "new".to_owned(),
            },
            NetworkAction::ConfigureBridge,
            NetworkAction::AssignLanAddress,
            NetworkAction::EnsureManagementServices,
            NetworkAction::AttachAp,
        ]
    );
}

#[test]
fn forwarding_off_removes_only_data_plane_in_cleanup_order() {
    let observed = network(
        true,
        OwnedResource::Owned {
            token: "router-old".to_owned(),
        },
    );
    let actions = network_plan(&NetworkDesired::management_only(), &observed, "new")
        .expect("safe forwarding-only stop plan");
    assert_eq!(
        actions,
        vec![
            NetworkAction::DisableIpv4Forwarding,
            NetworkAction::RemoveRouterFirewall {
                token: "router-old".to_owned(),
            },
        ]
    );
    assert!(!actions.iter().any(|action| matches!(
        action,
        NetworkAction::EnsureOwnedBridge { .. }
            | NetworkAction::ConfigureBridge
            | NetworkAction::AssignLanAddress
            | NetworkAction::AttachAp
            | NetworkAction::EnsureManagementServices
    )));
}

#[test]
fn management_only_disable_does_not_require_a_wan_route() {
    let mut observed = network(
        true,
        OwnedResource::Owned {
            token: "router-old".to_owned(),
        },
    );
    observed.wan_default_route_present = Probe::Known(false);
    assert_eq!(
        forwarding_plan(&NetworkDesired::management_only(), &observed, "new")
            .expect("route-independent management-only plan"),
        vec![
            NetworkAction::DisableIpv4Forwarding,
            NetworkAction::RemoveRouterFirewall {
                token: "router-old".to_owned(),
            },
        ]
    );
}

#[test]
fn management_only_disables_forwarding_even_when_firewall_is_absent() {
    let observed = network(true, OwnedResource::Absent);
    assert_eq!(
        forwarding_plan(&NetworkDesired::management_only(), &observed, "new")
            .expect("forwarding must be disabled"),
        vec![NetworkAction::DisableIpv4Forwarding]
    );
}

#[test]
fn management_only_reconfirms_forwarding_before_removing_owned_firewall() {
    let observed = network(
        false,
        OwnedResource::Owned {
            token: "router-old".to_owned(),
        },
    );
    assert_eq!(
        forwarding_plan(&NetworkDesired::management_only(), &observed, "new")
            .expect("owned firewall cleanup"),
        vec![
            NetworkAction::DisableIpv4Forwarding,
            NetworkAction::RemoveRouterFirewall {
                token: "router-old".to_owned(),
            },
        ]
    );
}

#[test]
fn forwarding_captures_global_switch_before_data_plane_mutation() {
    let mut observed = network(false, OwnedResource::Absent);
    observed.previous_ipv4_forwarding = Probe::Known(None);
    assert_eq!(
        forwarding_plan(&NetworkDesired::forwarding(), &observed, "new").unwrap(),
        vec![
            NetworkAction::CaptureIpv4Forwarding,
            NetworkAction::InstallRouterFirewall {
                token: "new".to_owned(),
            },
            NetworkAction::EnableIpv4Forwarding,
        ]
    );
}

#[test]
fn disabled_readiness_requires_forwarding_false_and_firewall_absent() {
    let desired = NetworkDesired::management_only();
    assert!(!network(true, OwnedResource::Absent).ready_for(&desired));
    assert!(!network(
        false,
        OwnedResource::Owned {
            token: "router-old".to_owned(),
        },
    )
    .ready_for(&desired));
    assert!(network(false, OwnedResource::Absent).ready_for(&desired));
}

#[test]
fn foreign_bridge_is_neither_ready_nor_mutated() {
    let mut observed = network(false, OwnedResource::Absent);
    observed.bridge = Probe::Known(OwnedResource::Foreign);

    assert!(!observed.management_ready());
    assert!(matches!(
        management_plan(&observed, "new"),
        Err(PlatformError::Conflict(_))
    ));
}

#[test]
fn foreign_firewall_is_neither_ready_nor_removed() {
    let observed = network(false, OwnedResource::Foreign);

    assert!(!observed.ready_for(&NetworkDesired::management_only()));
    assert!(matches!(
        forwarding_plan(&NetworkDesired::management_only(), &observed, "new"),
        Err(PlatformError::Conflict(_))
    ));
}

#[test]
fn stale_or_unknown_route_and_ownership_cannot_enable_forwarding() {
    let mut unknown_route = network(false, OwnedResource::Absent);
    unknown_route.wan_default_route_present = Probe::Unknown("stale route record".to_owned());
    assert!(!unknown_route.ready_for(&NetworkDesired::forwarding()));
    assert!(matches!(
        forwarding_plan(&NetworkDesired::forwarding(), &unknown_route, "new"),
        Err(PlatformError::UnsafeToCutOver(_))
    ));

    let mut unknown_firewall = network(true, OwnedResource::Absent);
    unknown_firewall.router_firewall = Probe::Unknown("stale firewall marker".to_owned());
    assert!(!unknown_firewall.ready_for(&NetworkDesired::forwarding()));
    assert!(matches!(
        forwarding_plan(&NetworkDesired::forwarding(), &unknown_firewall, "new"),
        Err(PlatformError::ProbeFailed(_))
    ));
}

#[test]
fn lan_tun_plan_is_interception_last_and_feature_commit_last() {
    let actions = proxy_plan(
        &ProxyDesired {
            lan_tun_enabled: true,
            tailscale_explicit_proxy_enabled: false,
            direct_macs: Default::default(),
        },
        &stopped_proxy(),
        &network(
            true,
            OwnedResource::Owned {
                token: "router".to_owned(),
            },
        ),
        "new",
    )
    .expect("LAN TUN plan");
    assert!(matches!(
        actions.first(),
        Some(ProxyAction::WriteRuntimeConfig {
            lan_tun_enabled: true
        })
    ));
    assert!(
        matches!(actions.get(actions.len() - 4), Some(ProxyAction::InstallInterceptionEntry { token }) if token == "new")
    );
    assert_eq!(
        &actions[actions.len() - 3..],
        &[
            ProxyAction::StartWatcher,
            ProxyAction::WaitForWatcher,
            ProxyAction::CommitFeatures {
                features: ProxyFeaturesV1::new(true, false)
            },
        ]
    );
}

#[test]
fn tailscale_only_plan_starts_core_without_lan_resources() {
    let actions = proxy_plan(
        &ProxyDesired {
            lan_tun_enabled: false,
            tailscale_explicit_proxy_enabled: true,
            direct_macs: Default::default(),
        },
        &stopped_proxy(),
        &network(
            true,
            OwnedResource::Owned {
                token: "router".to_owned(),
            },
        ),
        "new",
    )
    .unwrap();
    assert!(actions.contains(&ProxyAction::WaitForMixedPort));
    assert!(!actions.iter().any(|action| matches!(
        action,
        ProxyAction::CreateTunChains { .. } | ProxyAction::InstallInterceptionEntry { .. }
    )));
    assert_eq!(
        actions.last(),
        Some(&ProxyAction::CommitFeatures {
            features: ProxyFeaturesV1::new(false, true),
        })
    );
}

#[test]
fn proxy_planner_covers_shared_core_transition_matrix() {
    let forwarding = network(
        true,
        OwnedResource::Owned {
            token: "router".to_owned(),
        },
    );
    let desired = |lan_tun_enabled, tailscale_explicit_proxy_enabled| ProxyDesired {
        lan_tun_enabled,
        tailscale_explicit_proxy_enabled,
        direct_macs: Default::default(),
    };

    let lan_to_both = proxy_plan(
        &desired(true, true),
        &ready_proxy(ProxyFeaturesV1::new(true, false)),
        &forwarding,
        "new",
    )
    .unwrap();
    assert_eq!(
        lan_to_both,
        vec![ProxyAction::CommitFeatures {
            features: ProxyFeaturesV1::new(true, true),
        }]
    );

    let both_to_lan = proxy_plan(
        &desired(true, false),
        &ready_proxy(ProxyFeaturesV1::new(true, true)),
        &forwarding,
        "new",
    )
    .unwrap();
    assert_eq!(
        both_to_lan,
        vec![ProxyAction::CommitFeatures {
            features: ProxyFeaturesV1::new(true, false),
        }]
    );
    assert!(!both_to_lan.contains(&ProxyAction::StopCore));

    let tailscale_to_both = proxy_plan(
        &desired(true, true),
        &ready_proxy(ProxyFeaturesV1::new(false, true)),
        &forwarding,
        "new",
    )
    .unwrap();
    assert!(tailscale_to_both.contains(&ProxyAction::StopCore));
    assert!(tailscale_to_both.contains(&ProxyAction::WaitForMixedPort));
    assert!(tailscale_to_both
        .iter()
        .any(|action| matches!(action, ProxyAction::InstallInterceptionEntry { .. })));

    let both_to_tailscale = proxy_plan(
        &desired(false, true),
        &ready_proxy(ProxyFeaturesV1::new(true, true)),
        &forwarding,
        "new",
    )
    .unwrap();
    assert!(matches!(
        both_to_tailscale.first(),
        Some(ProxyAction::StopWatcher)
    ));
    assert!(both_to_tailscale.contains(&ProxyAction::StopCore));
    assert!(both_to_tailscale.contains(&ProxyAction::WaitForMixedPort));
    assert_eq!(
        both_to_tailscale.last(),
        Some(&ProxyAction::CommitFeatures {
            features: ProxyFeaturesV1::new(false, true),
        })
    );

    for current in [
        ProxyFeaturesV1::new(true, false),
        ProxyFeaturesV1::new(false, true),
        ProxyFeaturesV1::new(true, true),
    ] {
        let actions = proxy_plan(
            &desired(false, false),
            &ready_proxy(current),
            &forwarding,
            "new",
        )
        .unwrap();
        assert!(actions.contains(&ProxyAction::StopCore));
        assert_eq!(
            actions.last(),
            Some(&ProxyAction::CommitFeatures {
                features: ProxyFeaturesV1::disabled(),
            })
        );
    }
}

#[test]
fn device_policy_change_rebuilds_only_lan_tun_resources_and_keeps_shared_core() {
    let mut observed = ready_proxy(ProxyFeaturesV1::new(true, true));
    observed.active_direct_macs = Probe::Known(Default::default());
    let desired_mac = "02:00:00:00:00:01".parse().unwrap();
    let actions = proxy_plan(
        &ProxyDesired {
            lan_tun_enabled: true,
            tailscale_explicit_proxy_enabled: true,
            direct_macs: [desired_mac].into_iter().collect(),
        },
        &observed,
        &network(
            true,
            OwnedResource::Owned {
                token: "router".to_owned(),
            },
        ),
        "new",
    )
    .unwrap();
    assert!(matches!(actions.first(), Some(ProxyAction::StopWatcher)));
    assert!(actions
        .iter()
        .any(|action| matches!(action, ProxyAction::CreateTunChains { .. })));
    assert!(!actions.contains(&ProxyAction::StopCore));
}

#[test]
fn proxy_planner_rejects_lan_commit_without_ordinary_forwarding_but_allows_tailscale_core() {
    let management_only = network(false, OwnedResource::Absent);
    assert!(matches!(
        proxy_plan(
            &ProxyDesired {
                lan_tun_enabled: true,
                tailscale_explicit_proxy_enabled: false,
                direct_macs: Default::default(),
            },
            &stopped_proxy(),
            &management_only,
            "new",
        ),
        Err(PlatformError::UnsafeToCutOver(_))
    ));
    assert!(proxy_plan(
        &ProxyDesired {
            lan_tun_enabled: false,
            tailscale_explicit_proxy_enabled: true,
            direct_macs: Default::default(),
        },
        &stopped_proxy(),
        &management_only,
        "new",
    )
    .is_ok());
}

struct Fake {
    observations: Mutex<VecDeque<NetworkObserved>>,
    actions: Mutex<Vec<NetworkAction>>,
    releases: Mutex<usize>,
    lifecycle: Mutex<Vec<&'static str>>,
}

impl Fake {
    fn with_observations(observations: Vec<NetworkObserved>) -> Self {
        Self {
            observations: Mutex::new(observations.into()),
            actions: Mutex::new(Vec::new()),
            releases: Mutex::new(0),
            lifecycle: Mutex::new(Vec::new()),
        }
    }
}

impl RouterPlatformPort for Fake {
    fn acquire_lifecycle_lock(&self) -> Result<LifecycleLease, PlatformError> {
        self.lifecycle.lock().expect("lifecycle").push("lock");
        Ok(LifecycleLease {
            path: "/run/fake.lock",
            identity: "fake".to_owned(),
            directory_device: 0,
            directory_inode: 0,
        })
    }

    fn release_lifecycle_lock(&self, _: &LifecycleLease) -> Result<(), PlatformError> {
        self.lifecycle.lock().expect("lifecycle").push("release");
        *self.releases.lock().expect("release lock") += 1;
        Ok(())
    }

    fn apply_network(&self, action: &NetworkAction) -> Result<(), PlatformError> {
        self.actions
            .lock()
            .expect("actions lock")
            .push(action.clone());
        Ok(())
    }

    fn apply_proxy(&self, _: &ProxyAction) -> Result<(), PlatformError> {
        unreachable!("router test does not apply proxy actions")
    }
}

impl SystemProbePort for Fake {
    fn observe_network(&self) -> Result<NetworkObserved, PlatformError> {
        self.observations
            .lock()
            .expect("observations lock")
            .pop_front()
            .ok_or_else(|| PlatformError::ProbeFailed("no fake observation".to_owned()))
    }

    fn observe_proxy(&self) -> Result<ProxyObserved, PlatformError> {
        Ok(stopped_proxy())
    }
}

impl ClockPort for Fake {
    fn unix_time_millis(&self) -> u64 {
        42
    }
}

#[test]
fn fake_port_characterizes_enable_order_and_reprobes_before_ready() {
    let initial = network(false, OwnedResource::Absent);
    let final_state = network(
        true,
        OwnedResource::Owned {
            token: "hyz-router-42".to_owned(),
        },
    );
    let fake = Fake::with_observations(vec![initial.clone(), initial, final_state]);
    let result = RouterApplication::new(&fake, &fake, &fake)
        .reconcile(&NetworkDesired::forwarding())
        .expect("reconcile");
    assert_eq!(result.actions_applied, 2);
    assert_eq!(
        *fake.actions.lock().expect("actions"),
        vec![
            NetworkAction::InstallRouterFirewall {
                token: "hyz-router-42".to_owned(),
            },
            NetworkAction::EnableIpv4Forwarding,
        ]
    );
    assert_eq!(*fake.releases.lock().expect("releases"), 1);
}

#[test]
fn fake_port_disables_forwarding_before_removing_firewall() {
    let initial = network(
        true,
        OwnedResource::Owned {
            token: "router-old".to_owned(),
        },
    );
    let final_state = network(false, OwnedResource::Absent);
    let fake = Fake::with_observations(vec![initial.clone(), initial, final_state]);
    let result = RouterApplication::new(&fake, &fake, &fake)
        .reconcile(&NetworkDesired::management_only())
        .expect("management-only reconcile");
    assert_eq!(result.actions_applied, 2);
    assert_eq!(
        *fake.actions.lock().expect("actions"),
        vec![
            NetworkAction::DisableIpv4Forwarding,
            NetworkAction::RemoveRouterFirewall {
                token: "router-old".to_owned(),
            },
        ]
    );
    assert_eq!(*fake.releases.lock().expect("releases"), 1);
}

#[test]
fn repeated_recovery_reconcile_does_not_accumulate_resources_or_actions() {
    let dirty = network(
        true,
        OwnedResource::Owned {
            token: "router-old".to_owned(),
        },
    );
    let clean = network(false, OwnedResource::Absent);
    let fake = Fake::with_observations(vec![
        dirty.clone(),
        dirty,
        clean.clone(),
        clean.clone(),
        clean.clone(),
        clean,
    ]);
    let application = RouterApplication::new(&fake, &fake, &fake);

    let recovered = application
        .reconcile(&NetworkDesired::management_only())
        .expect("first recovery reconcile");
    let stable = application
        .reconcile(&NetworkDesired::management_only())
        .expect("idempotent recovery reconcile");

    assert_eq!(recovered.actions_applied, 2);
    assert_eq!(stable.actions_applied, 0);
    assert_eq!(
        *fake.actions.lock().expect("actions"),
        vec![
            NetworkAction::DisableIpv4Forwarding,
            NetworkAction::RemoveRouterFirewall {
                token: "router-old".to_owned(),
            },
        ]
    );
    assert_eq!(*fake.releases.lock().expect("releases"), 2);
}

#[test]
fn cold_start_applies_management_then_reprobes_route_before_forwarding() {
    let mut initial = network(false, OwnedResource::Absent);
    initial.bridge = Probe::Known(OwnedResource::Absent);
    initial.bridge_up = Probe::Known(false);
    initial.lan_address_present = Probe::Known(false);
    initial.ap_attached = Probe::Known(false);
    initial.management_services_healthy = Probe::Known(false);
    initial.wan_default_route_present = Probe::Known(false);

    let mut managed_without_route = network(false, OwnedResource::Absent);
    managed_without_route.wan_default_route_present = Probe::Known(false);
    let route_ready = network(false, OwnedResource::Absent);
    let final_state = network(
        true,
        OwnedResource::Owned {
            token: "hyz-router-42".to_owned(),
        },
    );
    let fake = Fake::with_observations(vec![
        initial,
        managed_without_route,
        route_ready,
        final_state,
    ]);
    let result = RouterApplication::new(&fake, &fake, &fake)
        .reconcile(&NetworkDesired::forwarding())
        .expect("cold-start reconcile");

    assert_eq!(result.actions_applied, 8);
    assert_eq!(
        *fake.actions.lock().expect("actions"),
        vec![
            NetworkAction::EnsureOwnedBridge {
                token: "hyz-router-42".to_owned(),
            },
            NetworkAction::ConfigureBridge,
            NetworkAction::AssignLanAddress,
            NetworkAction::EnsureManagementServices,
            NetworkAction::AttachAp,
            NetworkAction::WaitForWanRoute,
            NetworkAction::InstallRouterFirewall {
                token: "hyz-router-42".to_owned(),
            },
            NetworkAction::EnableIpv4Forwarding,
        ]
    );
    assert_eq!(
        *fake.lifecycle.lock().expect("lifecycle"),
        vec!["lock", "release", "lock", "release"]
    );
}

#[test]
fn route_wait_reacquires_and_rejects_changed_management_state() {
    let mut without_route = network(false, OwnedResource::Absent);
    without_route.wan_default_route_present = Probe::Known(false);
    let mut changed = network(false, OwnedResource::Absent);
    changed.ap_attached = Probe::Known(false);
    let fake = Fake::with_observations(vec![without_route.clone(), without_route, changed]);

    let error = RouterApplication::new(&fake, &fake, &fake)
        .reconcile(&NetworkDesired::forwarding())
        .expect_err("management state changed while lifecycle lock was released");
    assert!(matches!(error, PlatformError::UnsafeToCutOver(_)));
    assert_eq!(
        *fake.actions.lock().expect("actions"),
        vec![NetworkAction::WaitForWanRoute]
    );
    assert_eq!(
        *fake.lifecycle.lock().expect("lifecycle"),
        vec!["lock", "release", "lock", "release"]
    );
}

#[test]
fn cold_management_only_start_succeeds_without_a_wan_route() {
    let mut initial = network(false, OwnedResource::Absent);
    initial.bridge = Probe::Known(OwnedResource::Absent);
    initial.bridge_up = Probe::Known(false);
    initial.lan_address_present = Probe::Known(false);
    initial.ap_attached = Probe::Known(false);
    initial.management_services_healthy = Probe::Known(false);
    initial.wan_default_route_present = Probe::Known(false);

    let mut managed_offline = network(false, OwnedResource::Absent);
    managed_offline.wan_default_route_present = Probe::Known(false);
    let fake = Fake::with_observations(vec![initial, managed_offline.clone(), managed_offline]);
    let result = RouterApplication::new(&fake, &fake, &fake)
        .reconcile(&NetworkDesired::management_only())
        .expect("offline management startup");

    assert_eq!(result.actions_applied, 5);
    assert!(!fake
        .actions
        .lock()
        .expect("actions")
        .contains(&NetworkAction::WaitForWanRoute));
}

#[test]
fn route_gate_retains_strictly_confirmed_management_state() {
    let mut initial = network(false, OwnedResource::Absent);
    initial.bridge = Probe::Known(OwnedResource::Absent);
    initial.bridge_up = Probe::Known(false);
    initial.lan_address_present = Probe::Known(false);
    initial.ap_attached = Probe::Known(false);
    initial.management_services_healthy = Probe::Known(false);
    initial.wan_default_route_present = Probe::Known(false);

    let mut managed_without_route = network(false, OwnedResource::Absent);
    managed_without_route.wan_default_route_present = Probe::Known(false);
    let fake = Fake::with_observations(vec![
        initial,
        managed_without_route.clone(),
        managed_without_route,
    ]);
    let error = RouterApplication::new(&fake, &fake, &fake)
        .reconcile(&NetworkDesired::forwarding())
        .expect_err("route gate must reject forwarding");
    assert!(matches!(error, PlatformError::UnsafeToCutOver(_)));

    assert_eq!(
        *fake.actions.lock().expect("actions"),
        vec![
            NetworkAction::EnsureOwnedBridge {
                token: "hyz-router-42".to_owned(),
            },
            NetworkAction::ConfigureBridge,
            NetworkAction::AssignLanAddress,
            NetworkAction::EnsureManagementServices,
            NetworkAction::AttachAp,
            NetworkAction::WaitForWanRoute,
        ]
    );
}

#[test]
fn previously_healthy_management_services_are_not_rollback_compensated() {
    let initial = network(false, OwnedResource::Absent);
    let mut managed_without_route = initial.clone();
    managed_without_route.wan_default_route_present = Probe::Known(false);
    let fake = Fake::with_observations(vec![
        initial,
        managed_without_route.clone(),
        managed_without_route,
    ]);
    RouterApplication::new(&fake, &fake, &fake)
        .reconcile(&NetworkDesired::forwarding())
        .expect_err("route gate must reject forwarding");
    assert!(!fake
        .actions
        .lock()
        .expect("actions")
        .contains(&NetworkAction::StopManagementServices));
}

#[test]
fn successful_actions_do_not_create_false_readiness() {
    let still_unready = network(false, OwnedResource::Absent);
    let fake = Fake::with_observations(vec![
        still_unready.clone(),
        still_unready.clone(),
        still_unready,
    ]);
    let error = RouterApplication::new(&fake, &fake, &fake)
        .reconcile(&NetworkDesired::forwarding())
        .expect_err("post-action observation must decide readiness");
    assert!(matches!(error, PlatformError::UnsafeToCutOver(_)));
    assert_eq!(
        *fake.actions.lock().expect("actions"),
        vec![
            NetworkAction::InstallRouterFirewall {
                token: "hyz-router-42".to_owned(),
            },
            NetworkAction::EnableIpv4Forwarding,
            NetworkAction::DisableIpv4Forwarding,
            NetworkAction::RemoveRouterFirewall {
                token: "hyz-router-42".to_owned(),
            },
        ]
    );
    assert_eq!(*fake.releases.lock().expect("releases"), 1);
}

#[test]
fn feature_readiness_is_independent_but_core_is_shared() {
    let mut observed = stopped_proxy();
    observed.persisted_features = Probe::Known(ProxyFeaturesV1::new(false, true));
    observed.process_identity_valid = Probe::Known(true);
    observed.runtime_config_valid = Probe::Known(true);
    observed.mixed_port_ready = Probe::Known(true);
    assert!(observed.ready_for(&ProxyDesired {
        lan_tun_enabled: false,
        tailscale_explicit_proxy_enabled: true,
        direct_macs: Default::default(),
    }));
    observed.persisted_features = Probe::Known(ProxyFeaturesV1::new(true, true));
    assert!(!observed.ready_for(&ProxyDesired {
        lan_tun_enabled: true,
        tailscale_explicit_proxy_enabled: true,
        direct_macs: Default::default(),
    }));
}

#[test]
fn unknown_probe_fields_are_never_ready() {
    assert!(!NetworkObserved::unknown("probe failed").ready_for(&NetworkDesired::management_only()));
    assert!(
        !ProxyObserved::unknown("probe failed").ready_for(&ProxyDesired {
            lan_tun_enabled: false,
            tailscale_explicit_proxy_enabled: false,
            direct_macs: Default::default(),
        })
    );
}

#[test]
fn shared_forward_hook_order_covers_install_remove_and_restart_combinations() {
    let cases = [
        (false, false, false, ""),
        (false, false, true, "-A FORWARD -j HYZ_ROUTER_FWD\n"),
        (
            true,
            false,
            true,
            "-A FORWARD -j HYZ_MIHOMO_FWD\n-A FORWARD -j HYZ_ROUTER_FWD\n",
        ),
        (
            false,
            true,
            true,
            "-A FORWARD -j HYZ_TS_FWD\n-A FORWARD -j HYZ_ROUTER_FWD\n",
        ),
        (
            true,
            true,
            true,
            "-A FORWARD -j HYZ_MIHOMO_FWD\n-A FORWARD -j HYZ_TS_FWD\n-A FORWARD -j HYZ_ROUTER_FWD\n",
        ),
    ];
    for (mihomo, tailscale, router, rules) in cases {
        assert!(forward_hook_order_is_exact(
            rules, mihomo, tailscale, router
        ));
    }
    for invalid in [
        "-A FORWARD -j HYZ_ROUTER_FWD\n-A FORWARD -j HYZ_TS_FWD\n",
        "-A FORWARD -j HYZ_TS_FWD\n-A FORWARD -j HYZ_MIHOMO_FWD\n-A FORWARD -j HYZ_ROUTER_FWD\n",
        "-A FORWARD -j HYZ_MIHOMO_FWD\n-A FORWARD -j HYZ_TS_FWD\n-A FORWARD -j HYZ_TS_FWD\n-A FORWARD -j HYZ_ROUTER_FWD\n",
    ] {
        assert!(!forward_hook_order_is_exact(invalid, true, true, true));
    }
}

#[test]
fn outbound_sources_do_not_reference_legacy_wrappers_or_shell_eval() {
    let source = [
        include_str!("../src/adapters/outbound/management.rs"),
        include_str!("../src/adapters/outbound/process.rs"),
        include_str!("../src/adapters/outbound/network.rs"),
        include_str!("../src/adapters/outbound/proxy.rs"),
        include_str!("../src/adapters/outbound/tailscale.rs"),
        include_str!("../src/adapters/outbound/system.rs"),
        include_str!("../src/adapters/inbound/dhcp_hook.rs"),
    ]
    .join("\n");
    for legacy in [
        format!("/usr/sbin/{}", "hyz-router"),
        format!("/usr/sbin/{}", "hyz-mihomo"),
    ] {
        assert!(!source.contains(&legacy));
    }
    assert!(!source.contains(&["sh", " -c"].concat()));
    assert!(!source.contains("Command::new(\"/bin/sh\")"));
    assert!(!source.contains("Command::new(\"sh\")"));
}
