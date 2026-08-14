#![cfg(feature = "native")]

use hyz_router::{
    application::{
        ports::{
            ClockPort, LifecycleLease, PlatformError, SystemProbePort, TailscalePlatformPort,
            TailscaleProbePort,
        },
        reconcile::{tailscale_bootstrap_plan, tailscale_plan},
        status::tailscale_status_component_from_observed,
        tailscale::{TailscaleApplication, TailscaleReconcileState},
    },
    domain::{
        network::{NetworkObserved, OwnedResource, Probe},
        proxy::{ProxyFeaturesV1, ProxyObserved},
        status::{
            ComponentState, TailscaleErrorCategory, TailscaleExplicitProxyPath,
            TailscaleProxyFallback,
        },
        tailscale::{
            TailscaleAction, TailscaleBackendState, TailscaleConnectionKind, TailscaleDesired,
            TailscaleEnvironment, TailscaleLoginUrl, TailscaleMode, TailscaleObserved,
            TailscalePreferences, TailscaleProcessState, TailscaleReadiness,
            TAILSCALE_CGNAT_SUBNET, TAILSCALE_FORWARD_CHAIN, TAILSCALE_INPUT_CHAIN,
            TAILSCALE_INTERFACE, TAILSCALE_LAN_ROUTE, TAILSCALE_MANAGEMENT_HTTP_PORT,
            TAILSCALE_NAT_CHAIN, TAILSCALE_UDP_PORT,
        },
    },
};
use std::{collections::VecDeque, net::Ipv4Addr, sync::Mutex};

fn owned(token: &str) -> Probe<OwnedResource> {
    Probe::Known(OwnedResource::Owned {
        token: token.to_owned(),
    })
}

fn stopped(mode: Option<TailscaleMode>) -> TailscaleObserved {
    TailscaleObserved {
        persisted_mode: Probe::Known(mode),
        backend_state: Probe::Known(TailscaleBackendState::Stopped),
        environment: Probe::Known(TailscaleEnvironment::Direct),
        process: Probe::Known(TailscaleProcessState::Absent),
        socket: Probe::Known(OwnedResource::Absent),
        interface: Probe::Known(OwnedResource::Absent),
        authenticated: Probe::Known(false),
        ipv4: Probe::Known(None),
        preferences: Probe::Known(TailscalePreferences::FIXED),
        route_advertised: Probe::Known(false),
        router_firewall: Probe::Known(OwnedResource::Absent),
        subnet_firewall: Probe::Known(OwnedResource::Absent),
        management_listener: Probe::Known(OwnedResource::Absent),
        management_listener_ipv4: Probe::Known(None),
        connection: Probe::Known(TailscaleConnectionKind::Unknown),
    }
}

fn running_without_surface(mode: Option<TailscaleMode>) -> TailscaleObserved {
    let mut observed = stopped(mode);
    observed.backend_state = Probe::Known(TailscaleBackendState::Running);
    observed.process = Probe::Known(TailscaleProcessState::OwnedLive {
        token: "process-old".to_owned(),
    });
    observed.socket = owned("process-old");
    observed.interface = owned("process-old");
    observed.authenticated = Probe::Known(true);
    observed.ipv4 = Probe::Known(Some(Ipv4Addr::new(100, 64, 0, 7)));
    observed
}

fn exited_owned(mode: Option<TailscaleMode>) -> TailscaleObserved {
    let mut observed = stopped(mode);
    observed.process = Probe::Known(TailscaleProcessState::OwnedExited {
        token: "process-old".to_owned(),
    });
    observed.socket = owned("process-old");
    observed.interface = owned("process-old");
    observed
}

fn needs_login(mode: Option<TailscaleMode>) -> TailscaleObserved {
    let mut observed = running_without_surface(mode);
    observed.backend_state = Probe::Known(TailscaleBackendState::NeedsLogin);
    observed.authenticated = Probe::Known(false);
    observed.ipv4 = Probe::Known(None);
    observed
}

fn router_only(mode: Option<TailscaleMode>) -> TailscaleObserved {
    let mut observed = running_without_surface(mode);
    observed.router_firewall = owned("router-old");
    observed.management_listener = owned("listener-old");
    observed.management_listener_ipv4 = Probe::Known(Some(Ipv4Addr::new(100, 64, 0, 7)));
    observed
}

fn lan_access(mode: Option<TailscaleMode>) -> TailscaleObserved {
    let mut observed = router_only(mode);
    observed.route_advertised = Probe::Known(true);
    observed.subnet_firewall = owned("subnet-old");
    observed
}

fn network_ready() -> NetworkObserved {
    NetworkObserved {
        bridge: owned("router"),
        bridge_up: Probe::Known(true),
        lan_address_present: Probe::Known(true),
        ap_attached: Probe::Known(true),
        management_services_healthy: Probe::Known(true),
        wan_default_route_present: Probe::Known(true),
        ipv4_forwarding: Probe::Known(true),
        previous_ipv4_forwarding: Probe::Known(Some(false)),
        router_firewall: owned("router"),
    }
}

fn network_unready() -> NetworkObserved {
    let mut observed = network_ready();
    observed.ipv4_forwarding = Probe::Known(false);
    observed
}

fn stopped_proxy() -> ProxyObserved {
    ProxyObserved {
        persisted_features: Probe::Known(ProxyFeaturesV1::disabled()),
        process_identity_valid: Probe::Known(false),
        watcher_identity_valid: Probe::Known(false),
        runtime_config_valid: Probe::Known(false),
        mixed_port_ready: Probe::Known(false),
        tun_interface: Probe::Known(OwnedResource::Absent),
        tun_firewall: Probe::Known(OwnedResource::Absent),
        policy_rule_present: Probe::Known(false),
        policy_route_present: Probe::Known(false),
        interception_entry_present: Probe::Known(false),
        ordinary_nat_confirmed: Probe::Known(true),
        active_direct_macs: Probe::Known(Default::default()),
    }
}

#[test]
fn fixed_domain_contract_and_connection_kind_do_not_change_readiness() {
    assert_eq!(TAILSCALE_INTERFACE, "tailscale0");
    assert_eq!(TAILSCALE_LAN_ROUTE, "192.168.8.0/24");
    assert_eq!(TAILSCALE_CGNAT_SUBNET, "100.64.0.0/10");
    assert_eq!(TAILSCALE_UDP_PORT, 41_641);
    assert_eq!(TAILSCALE_MANAGEMENT_HTTP_PORT, 8080);
    assert_eq!(TAILSCALE_INPUT_CHAIN, "HYZ_TS_INPUT");
    assert_eq!(TAILSCALE_FORWARD_CHAIN, "HYZ_TS_FWD");
    assert_eq!(TAILSCALE_NAT_CHAIN, "HYZ_TS_NAT");

    let desired = TailscaleDesired::router_only();
    let mut observed = router_only(Some(TailscaleMode::RouterOnly));
    observed.connection = Probe::Known(TailscaleConnectionKind::Direct);
    assert!(observed.ready_for(&desired, false));
    observed.connection = Probe::Known(TailscaleConnectionKind::Derp("sfo".to_owned()));
    assert!(observed.ready_for(&desired, false));
    observed.connection = Probe::Unknown("connection telemetry unavailable".to_owned());
    assert!(observed.ready_for(&desired, false));
}

#[test]
fn bootstrap_starts_exact_owned_backend_before_any_remote_surface() {
    assert_eq!(
        tailscale_bootstrap_plan(&stopped(Some(TailscaleMode::Disabled)), "new").unwrap(),
        vec![
            TailscaleAction::StartBackend {
                token: "new".to_owned(),
                environment: TailscaleEnvironment::Direct,
            },
            TailscaleAction::WaitForBackend,
            TailscaleAction::SetFixedPreferences,
        ]
    );
}

#[test]
fn exited_owned_backend_is_cleaned_before_restart_without_becoming_foreign() {
    assert_eq!(
        tailscale_bootstrap_plan(&exited_owned(Some(TailscaleMode::RouterOnly)), "new",).unwrap(),
        vec![
            TailscaleAction::StopBackend {
                token: "process-old".to_owned(),
            },
            TailscaleAction::StartBackend {
                token: "new".to_owned(),
                environment: TailscaleEnvironment::Direct,
            },
            TailscaleAction::WaitForBackend,
            TailscaleAction::SetFixedPreferences,
        ]
    );

    assert_eq!(
        hyz_router::application::reconcile::tailscale_shutdown_plan(&exited_owned(Some(
            TailscaleMode::RouterOnly,
        )))
        .unwrap(),
        vec![TailscaleAction::StopBackend {
            token: "process-old".to_owned(),
        }]
    );
}

#[test]
fn persisted_lan_access_router_only_installs_lan_path_when_router_becomes_ready() {
    assert_eq!(
        tailscale_plan(
            &TailscaleDesired::lan_subnet_access(),
            &router_only(Some(TailscaleMode::LanSubnetAccess)),
            &network_ready(),
            "new",
        )
        .unwrap(),
        vec![
            TailscaleAction::AdvertiseLanRoute,
            TailscaleAction::InstallSubnetFirewall {
                token: "new".to_owned(),
            },
            TailscaleAction::CommitDesiredMode {
                mode: TailscaleMode::LanSubnetAccess,
            },
        ]
    );
}

#[test]
fn router_only_to_lan_access_gates_route_and_firewall_on_router_readiness() {
    let desired = TailscaleDesired::lan_subnet_access();
    assert_eq!(
        tailscale_plan(
            &desired,
            &router_only(Some(TailscaleMode::RouterOnly)),
            &network_ready(),
            "new",
        )
        .unwrap(),
        vec![
            TailscaleAction::AdvertiseLanRoute,
            TailscaleAction::InstallSubnetFirewall {
                token: "new".to_owned(),
            },
            TailscaleAction::CommitDesiredMode {
                mode: TailscaleMode::LanSubnetAccess,
            },
        ]
    );
}

#[test]
fn tailscale_ipv4_change_stops_the_old_exact_listener_before_binding_the_new_one() {
    let mut observed = router_only(Some(TailscaleMode::RouterOnly));
    observed.management_listener_ipv4 = Probe::Known(Some(Ipv4Addr::new(100, 64, 0, 6)));

    assert_eq!(
        tailscale_plan(
            &TailscaleDesired::router_only(),
            &observed,
            &network_ready(),
            "new",
        )
        .unwrap(),
        vec![
            TailscaleAction::StopManagementListener {
                token: "listener-old".to_owned(),
            },
            TailscaleAction::StartManagementListener {
                token: "new".to_owned(),
                ipv4: Ipv4Addr::new(100, 64, 0, 7),
            },
            TailscaleAction::CommitDesiredMode {
                mode: TailscaleMode::RouterOnly,
            },
        ]
    );
}

#[test]
fn lan_access_to_router_only_closes_forwarding_before_committing_mode() {
    assert_eq!(
        tailscale_plan(
            &TailscaleDesired::router_only(),
            &lan_access(Some(TailscaleMode::LanSubnetAccess)),
            &network_ready(),
            "new",
        )
        .unwrap(),
        vec![
            TailscaleAction::RemoveSubnetFirewall {
                token: "subnet-old".to_owned(),
            },
            TailscaleAction::ClearAdvertisedRoute,
            TailscaleAction::CommitDesiredMode {
                mode: TailscaleMode::RouterOnly,
            },
        ]
    );
}

#[test]
fn disable_closes_lan_and_management_surfaces_before_stopping_process() {
    assert_eq!(
        tailscale_plan(
            &TailscaleDesired::disabled(),
            &lan_access(Some(TailscaleMode::LanSubnetAccess)),
            &network_ready(),
            "unused",
        )
        .unwrap(),
        vec![
            TailscaleAction::RemoveSubnetFirewall {
                token: "subnet-old".to_owned(),
            },
            TailscaleAction::ClearAdvertisedRoute,
            TailscaleAction::StopManagementListener {
                token: "listener-old".to_owned(),
            },
            TailscaleAction::RemoveRouterFirewall {
                token: "router-old".to_owned(),
            },
            TailscaleAction::StopBackend {
                token: "process-old".to_owned(),
            },
            TailscaleAction::CommitDesiredMode {
                mode: TailscaleMode::Disabled,
            },
        ]
    );
}

#[test]
fn unready_or_unknown_router_falls_back_to_confirmed_router_only() {
    let desired = TailscaleDesired::lan_subnet_access();
    let expected = vec![TailscaleAction::CommitDesiredMode {
        mode: TailscaleMode::LanSubnetAccess,
    }];
    assert_eq!(
        tailscale_plan(
            &desired,
            &router_only(Some(TailscaleMode::RouterOnly)),
            &network_unready(),
            "new",
        )
        .unwrap(),
        expected
    );

    let mut unknown = network_ready();
    unknown.router_firewall = Probe::Unknown("stale owner record".to_owned());
    assert_eq!(
        tailscale_plan(
            &desired,
            &router_only(Some(TailscaleMode::RouterOnly)),
            &unknown,
            "new",
        )
        .unwrap(),
        expected
    );

    let mut effective = router_only(Some(TailscaleMode::LanSubnetAccess));
    effective.connection = Probe::Known(TailscaleConnectionKind::PeerRelay);
    assert_eq!(
        effective.readiness(&desired, false),
        TailscaleReadiness::Ready {
            effective_mode: TailscaleMode::RouterOnly,
        }
    );
}

#[test]
fn desired_lan_access_effective_router_only_is_typed_degraded_status() {
    let component = tailscale_status_component_from_observed(
        &router_only(Some(TailscaleMode::LanSubnetAccess)),
        &Probe::Known(ProxyFeaturesV1::disabled()),
        false,
        &Probe::Known(false),
    );
    assert_eq!(component.state, ComponentState::Degraded);
    assert_eq!(
        component.data.unwrap().error_category,
        Some(TailscaleErrorCategory::NotReady)
    );
}

#[test]
fn explicit_proxy_status_distinguishes_ready_direct_fallback_and_unknown_desired() {
    let direct = router_only(Some(TailscaleMode::RouterOnly));
    let restored = tailscale_status_component_from_observed(
        &direct,
        &Probe::Known(ProxyFeaturesV1::new(false, true)),
        true,
        &Probe::Known(false),
    );
    let restored = restored.data.unwrap();
    assert_eq!(restored.explicit_proxy_desired, Some(true));
    assert_eq!(
        restored.proxy_fallback,
        TailscaleProxyFallback::DirectRestored
    );

    let mut proxied = direct.clone();
    proxied.environment = Probe::Known(TailscaleEnvironment::MihomoExplicit);
    let ready = tailscale_status_component_from_observed(
        &proxied,
        &Probe::Known(ProxyFeaturesV1::new(false, true)),
        true,
        &Probe::Known(true),
    );
    assert_eq!(ready.state, ComponentState::Available);
    let ready = ready.data.unwrap();
    assert_eq!(ready.proxy_fallback, TailscaleProxyFallback::NotNeeded);
    assert_eq!(ready.explicit_proxy_path, TailscaleExplicitProxyPath::Ready);

    let unavailable = tailscale_status_component_from_observed(
        &proxied,
        &Probe::Known(ProxyFeaturesV1::new(false, true)),
        true,
        &Probe::Known(false),
    );
    assert_eq!(unavailable.state, ComponentState::Degraded);
    assert_eq!(
        unavailable.issue.as_ref().map(|issue| issue.code.as_str()),
        Some("tailscale_proxy_path_unavailable")
    );
    let unavailable = unavailable.data.unwrap();
    assert_eq!(
        unavailable.explicit_proxy_path,
        TailscaleExplicitProxyPath::Unavailable
    );
    assert_eq!(
        unavailable.error_category,
        Some(TailscaleErrorCategory::NotReady)
    );
    assert_eq!(
        unavailable.proxy_fallback,
        TailscaleProxyFallback::NotConfirmed
    );

    let path_unknown = tailscale_status_component_from_observed(
        &proxied,
        &Probe::Known(ProxyFeaturesV1::new(false, true)),
        true,
        &Probe::Unknown("fixed path probe failed".to_owned()),
    );
    assert_eq!(path_unknown.state, ComponentState::Degraded);
    let path_unknown = path_unknown.data.unwrap();
    assert_eq!(
        path_unknown.explicit_proxy_path,
        TailscaleExplicitProxyPath::Unknown
    );
    assert_eq!(
        path_unknown.error_category,
        Some(TailscaleErrorCategory::ProbeFailed)
    );

    let unknown = tailscale_status_component_from_observed(
        &proxied,
        &Probe::Unknown("features unreadable".to_owned()),
        true,
        &Probe::Known(true),
    );
    assert_eq!(unknown.state, ComponentState::Degraded);
    assert_eq!(unknown.data.unwrap().explicit_proxy_desired, None);
}

#[test]
fn foreign_unknown_and_partial_runtime_state_are_never_mutated() {
    let mut foreign = router_only(Some(TailscaleMode::RouterOnly));
    foreign.subnet_firewall = Probe::Known(OwnedResource::Foreign);
    assert!(matches!(
        tailscale_plan(
            &TailscaleDesired::disabled(),
            &foreign,
            &network_ready(),
            "new"
        ),
        Err(PlatformError::Conflict(_))
    ));

    let mut foreign_listener = router_only(Some(TailscaleMode::RouterOnly));
    foreign_listener.management_listener = Probe::Known(OwnedResource::Foreign);
    foreign_listener.management_listener_ipv4 =
        Probe::Unknown("foreign Tailscale listener address is not trusted".to_owned());
    assert!(matches!(
        tailscale_plan(
            &TailscaleDesired::disabled(),
            &foreign_listener,
            &network_ready(),
            "new"
        ),
        Err(PlatformError::Conflict(_))
    ));

    let mut unknown_listener = router_only(Some(TailscaleMode::RouterOnly));
    unknown_listener.management_listener =
        Probe::Unknown("Tailscale listener ownership is unconfirmed".to_owned());
    assert!(matches!(
        tailscale_plan(
            &TailscaleDesired::disabled(),
            &unknown_listener,
            &network_ready(),
            "new"
        ),
        Err(PlatformError::ProbeFailed(_))
    ));

    let mut stale_with_replaced_socket = exited_owned(Some(TailscaleMode::RouterOnly));
    stale_with_replaced_socket.socket = Probe::Known(OwnedResource::Foreign);
    assert!(matches!(
        tailscale_bootstrap_plan(&stale_with_replaced_socket, "new"),
        Err(PlatformError::Conflict(_))
    ));

    let mut replaced_process = exited_owned(Some(TailscaleMode::RouterOnly));
    replaced_process.process = Probe::Known(TailscaleProcessState::Foreign);
    assert!(matches!(
        tailscale_bootstrap_plan(&replaced_process, "new"),
        Err(PlatformError::Conflict(_))
    ));

    let unknown = TailscaleObserved::unknown("stale probe");
    assert!(!unknown.ready_for(&TailscaleDesired::disabled(), false));
    assert!(matches!(
        tailscale_bootstrap_plan(&unknown, "new"),
        Err(PlatformError::ProbeFailed(_))
    ));

    let mut partial = stopped(Some(TailscaleMode::Disabled));
    partial.socket = owned("stale-socket");
    assert!(matches!(
        tailscale_bootstrap_plan(&partial, "new"),
        Err(PlatformError::Conflict(_))
    ));
}

struct Fake {
    tailscale_observations: Mutex<VecDeque<TailscaleObserved>>,
    network_observations: Mutex<VecDeque<NetworkObserved>>,
    actions: Mutex<Vec<TailscaleAction>>,
    fail_action: Mutex<Option<TailscaleAction>>,
    login_requests: Mutex<usize>,
    releases: Mutex<usize>,
}

impl Fake {
    fn new(
        tailscale_observations: Vec<TailscaleObserved>,
        network_observations: Vec<NetworkObserved>,
    ) -> Self {
        Self {
            tailscale_observations: Mutex::new(tailscale_observations.into()),
            network_observations: Mutex::new(network_observations.into()),
            actions: Mutex::new(Vec::new()),
            fail_action: Mutex::new(None),
            login_requests: Mutex::new(0),
            releases: Mutex::new(0),
        }
    }
}

impl TailscalePlatformPort for Fake {
    fn acquire_tailscale_lock(&self) -> Result<LifecycleLease, PlatformError> {
        Ok(LifecycleLease {
            path: "/run/fake-tailscale.lock",
            identity: "fake".to_owned(),
            directory_device: 0,
            directory_inode: 0,
        })
    }

    fn release_tailscale_lock(&self, _: &LifecycleLease) -> Result<(), PlatformError> {
        *self.releases.lock().unwrap() += 1;
        Ok(())
    }

    fn apply_tailscale(&self, action: &TailscaleAction) -> Result<(), PlatformError> {
        self.actions.lock().unwrap().push(action.clone());
        if self.fail_action.lock().unwrap().as_ref() == Some(action) {
            return Err(PlatformError::CommandFailed("injected failure".to_owned()));
        }
        Ok(())
    }

    fn request_login(&self) -> Result<TailscaleLoginUrl, PlatformError> {
        *self.login_requests.lock().unwrap() += 1;
        TailscaleLoginUrl::new("https://login.tailscale.com/a/example")
            .ok_or_else(|| PlatformError::InvalidState("invalid fake URL".to_owned()))
    }

    fn logout(&self) -> Result<(), PlatformError> {
        Ok(())
    }
}

impl TailscaleProbePort for Fake {
    fn observe_tailscale(&self) -> Result<TailscaleObserved, PlatformError> {
        self.tailscale_observations
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| PlatformError::ProbeFailed("no Tailscale observation".to_owned()))
    }

    fn probe_explicit_proxy_path(&self) -> Result<bool, PlatformError> {
        Ok(true)
    }
}

impl SystemProbePort for Fake {
    fn observe_network(&self) -> Result<NetworkObserved, PlatformError> {
        self.network_observations
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| PlatformError::ProbeFailed("no network observation".to_owned()))
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
fn persisted_lan_access_reconciles_from_router_only_after_router_readiness() {
    let initial = router_only(Some(TailscaleMode::LanSubnetAccess));
    let mut ready = lan_access(Some(TailscaleMode::LanSubnetAccess));
    ready.subnet_firewall = owned("hyz-tailscale-42");
    let fake = Fake::new(vec![initial, ready.clone(), ready], vec![network_ready()]);

    let result = TailscaleApplication::new(&fake, &fake, &fake, &fake)
        .reconcile(&TailscaleDesired::lan_subnet_access())
        .unwrap();

    assert_eq!(
        result.state,
        TailscaleReconcileState::Ready {
            effective_mode: TailscaleMode::LanSubnetAccess,
        }
    );
    assert_eq!(
        *fake.actions.lock().unwrap(),
        vec![
            TailscaleAction::AdvertiseLanRoute,
            TailscaleAction::InstallSubnetFirewall {
                token: "hyz-tailscale-42".to_owned(),
            },
            TailscaleAction::CommitDesiredMode {
                mode: TailscaleMode::LanSubnetAccess,
            },
        ]
    );
}

#[test]
fn disabled_to_router_only_starts_then_installs_minimum_surface_then_commits() {
    let initial = stopped(Some(TailscaleMode::Disabled));
    let authenticated = running_without_surface(Some(TailscaleMode::Disabled));
    let mut precommit = router_only(Some(TailscaleMode::Disabled));
    precommit.router_firewall = owned("hyz-tailscale-42");
    precommit.management_listener = owned("hyz-tailscale-42");
    precommit.management_listener_ipv4 = Probe::Known(Some(Ipv4Addr::new(100, 64, 0, 7)));
    let mut final_state = precommit.clone();
    final_state.persisted_mode = Probe::Known(Some(TailscaleMode::RouterOnly));
    let fake = Fake::new(
        vec![initial, authenticated, precommit, final_state],
        vec![network_ready()],
    );

    let result = TailscaleApplication::new(&fake, &fake, &fake, &fake)
        .reconcile(&TailscaleDesired::router_only())
        .unwrap();
    assert_eq!(
        result.state,
        TailscaleReconcileState::Ready {
            effective_mode: TailscaleMode::RouterOnly,
        }
    );
    assert_eq!(
        *fake.actions.lock().unwrap(),
        vec![
            TailscaleAction::StartBackend {
                token: "hyz-tailscale-42".to_owned(),
                environment: TailscaleEnvironment::Direct,
            },
            TailscaleAction::WaitForBackend,
            TailscaleAction::SetFixedPreferences,
            TailscaleAction::InstallRouterFirewall {
                token: "hyz-tailscale-42".to_owned(),
            },
            TailscaleAction::StartManagementListener {
                token: "hyz-tailscale-42".to_owned(),
                ipv4: Ipv4Addr::new(100, 64, 0, 7),
            },
            TailscaleAction::CommitDesiredMode {
                mode: TailscaleMode::RouterOnly,
            },
        ]
    );
    assert_eq!(*fake.releases.lock().unwrap(), 1);
}

#[test]
fn unauthenticated_backend_returns_only_transient_login_url_without_lan_actions() {
    let fake = Fake::new(
        vec![
            stopped(Some(TailscaleMode::Disabled)),
            needs_login(Some(TailscaleMode::Disabled)),
        ],
        vec![],
    );
    let result = TailscaleApplication::new(&fake, &fake, &fake, &fake)
        .reconcile(&TailscaleDesired::lan_subnet_access())
        .unwrap();
    match result.state {
        TailscaleReconcileState::NeedsLogin { login_url } => {
            assert_eq!(login_url.as_str(), "https://login.tailscale.com/a/example");
        }
        _ => panic!("expected login result"),
    }
    assert_eq!(*fake.login_requests.lock().unwrap(), 1);
    assert_eq!(
        *fake.actions.lock().unwrap(),
        vec![
            TailscaleAction::StartBackend {
                token: "hyz-tailscale-42".to_owned(),
                environment: TailscaleEnvironment::Direct,
            },
            TailscaleAction::WaitForBackend,
            TailscaleAction::SetFixedPreferences,
            TailscaleAction::CommitDesiredMode {
                mode: TailscaleMode::LanSubnetAccess,
            },
        ]
    );
    assert!(!fake.actions.lock().unwrap().iter().any(|action| matches!(
        action,
        TailscaleAction::AdvertiseLanRoute
            | TailscaleAction::InstallRouterFirewall { .. }
            | TailscaleAction::InstallSubnetFirewall { .. }
            | TailscaleAction::StartManagementListener { .. }
    )));
}

#[test]
fn failed_lan_commit_rolls_back_owned_actions_in_reverse_order() {
    let initial = router_only(Some(TailscaleMode::RouterOnly));
    let mut precommit = lan_access(Some(TailscaleMode::RouterOnly));
    precommit.subnet_firewall = owned("hyz-tailscale-42");
    let fake = Fake::new(vec![initial, precommit], vec![network_ready()]);
    *fake.fail_action.lock().unwrap() = Some(TailscaleAction::CommitDesiredMode {
        mode: TailscaleMode::LanSubnetAccess,
    });

    let error = TailscaleApplication::new(&fake, &fake, &fake, &fake)
        .reconcile(&TailscaleDesired::lan_subnet_access())
        .unwrap_err();
    assert!(matches!(error, PlatformError::CommandFailed(_)));
    assert_eq!(
        *fake.actions.lock().unwrap(),
        vec![
            TailscaleAction::AdvertiseLanRoute,
            TailscaleAction::InstallSubnetFirewall {
                token: "hyz-tailscale-42".to_owned(),
            },
            TailscaleAction::CommitDesiredMode {
                mode: TailscaleMode::LanSubnetAccess,
            },
            TailscaleAction::RemoveSubnetFirewall {
                token: "hyz-tailscale-42".to_owned(),
            },
            TailscaleAction::ClearAdvertisedRoute,
        ]
    );
    assert_eq!(*fake.releases.lock().unwrap(), 1);
}

#[test]
fn failed_restart_after_exited_cleanup_does_not_resurrect_old_backend() {
    let fake = Fake::new(vec![exited_owned(Some(TailscaleMode::RouterOnly))], vec![]);
    *fake.fail_action.lock().unwrap() = Some(TailscaleAction::WaitForBackend);

    let error = TailscaleApplication::new(&fake, &fake, &fake, &fake)
        .reconcile(&TailscaleDesired::router_only())
        .unwrap_err();
    assert!(matches!(error, PlatformError::CommandFailed(_)));
    assert_eq!(
        *fake.actions.lock().unwrap(),
        vec![
            TailscaleAction::StopBackend {
                token: "process-old".to_owned(),
            },
            TailscaleAction::StartBackend {
                token: "hyz-tailscale-42".to_owned(),
                environment: TailscaleEnvironment::Direct,
            },
            TailscaleAction::WaitForBackend,
            TailscaleAction::StopBackend {
                token: "hyz-tailscale-42".to_owned(),
            },
        ]
    );
}

#[test]
fn router_disable_degrades_only_the_runtime_lan_path_and_preserves_desired_mode() {
    let initial = lan_access(Some(TailscaleMode::LanSubnetAccess));
    let final_state = router_only(Some(TailscaleMode::LanSubnetAccess));
    let fake = Fake::new(vec![initial, final_state], vec![]);

    let actions = TailscaleApplication::new(&fake, &fake, &fake, &fake)
        .degrade_to_router_only()
        .unwrap();

    assert_eq!(actions, 2);
    assert_eq!(
        *fake.actions.lock().unwrap(),
        vec![
            TailscaleAction::RemoveSubnetFirewall {
                token: "subnet-old".to_owned(),
            },
            TailscaleAction::ClearAdvertisedRoute,
        ]
    );
}

#[test]
fn shutdown_cleans_an_exact_exited_backend_record_and_owned_nodes() {
    let initial = exited_owned(Some(TailscaleMode::RouterOnly));
    let final_state = stopped(Some(TailscaleMode::RouterOnly));
    let fake = Fake::new(vec![initial, final_state], vec![]);

    let actions = TailscaleApplication::new(&fake, &fake, &fake, &fake)
        .shutdown()
        .unwrap();

    assert_eq!(actions, 1);
    assert_eq!(
        *fake.actions.lock().unwrap(),
        vec![TailscaleAction::StopBackend {
            token: "process-old".to_owned(),
        }]
    );
}

#[test]
fn shutdown_is_tailscale_first_runtime_cleanup_without_desired_mode_commit() {
    let initial = lan_access(Some(TailscaleMode::LanSubnetAccess));
    let final_state = stopped(Some(TailscaleMode::LanSubnetAccess));
    let fake = Fake::new(vec![initial, final_state], vec![]);

    let actions = TailscaleApplication::new(&fake, &fake, &fake, &fake)
        .shutdown()
        .unwrap();

    assert_eq!(actions, 5);
    assert_eq!(
        *fake.actions.lock().unwrap(),
        vec![
            TailscaleAction::RemoveSubnetFirewall {
                token: "subnet-old".to_owned(),
            },
            TailscaleAction::ClearAdvertisedRoute,
            TailscaleAction::StopManagementListener {
                token: "listener-old".to_owned(),
            },
            TailscaleAction::RemoveRouterFirewall {
                token: "router-old".to_owned(),
            },
            TailscaleAction::StopBackend {
                token: "process-old".to_owned(),
            },
        ]
    );
    assert!(!fake.actions.lock().unwrap().iter().any(|action| matches!(
        action,
        TailscaleAction::CommitDesiredMode { .. } | TailscaleAction::RestoreDesiredMode { .. }
    )));
}
