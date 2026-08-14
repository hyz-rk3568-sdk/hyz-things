use super::{
    paths::{
        MIHOMO_CANDIDATE_CONFIG, MIHOMO_CONTROLLER_ADDRESS, MIHOMO_CONTROLLER_SECRET,
        MIHOMO_DATA_DIR, MIHOMO_FEATURES_FILE, MIHOMO_RUNTIME_CONFIG, MIHOMO_SOURCE_CONFIG,
        MIHOMO_STATE_DIR, MIHOMO_TUN_IDENTITY,
    },
    process::{
        mihomo_process_holds_tun, mihomo_process_owns_tcp_listener, LinuxRouterPlatform, Tool,
    },
    storage,
    system::{
        chain_output_is_exact, exact_chain_references, exact_default_gateway,
        expected_chain_rules_with_direct, expected_interception_rule, forward_hook_order_is_exact,
        normalized_chain_rules, owned_forward_hook_is_exact, parse_direct_mac_rules,
        policy_route_state_is_exact, policy_rule_state_is_exact,
    },
};
use crate::{
    application::ports::{CoreIdentity, PlatformError},
    domain::{
        network::{LAN_BRIDGE, LAN_SUBNET},
        proxy::{
            ProxyAction, ProxyFeaturesV1, CONTROLLED_LOCAL_MIXED, CONTROLLED_TUN_DISABLED,
            CONTROLLED_TUN_ENABLED, MIHOMO_FILTER_CHAIN, MIHOMO_MANGLE_CHAIN, MIHOMO_MARK,
            MIHOMO_MIXED_ADDRESS, MIHOMO_ROUTE_TABLE, MIHOMO_RULE_PRIORITY, MIHOMO_TUN_INTERFACE,
        },
    },
};
use std::{
    fs,
    net::{SocketAddr, TcpStream},
    thread,
    time::Duration,
};

const MAX_CONFIG_SIZE: usize = 4 * 1024 * 1024;

impl LinuxRouterPlatform {
    pub(crate) fn apply_proxy_action(&self, action: &ProxyAction) -> Result<(), PlatformError> {
        match action {
            ProxyAction::RemoveInterceptionEntry { token } => self.remove_interception_entry(token),
            ProxyAction::RemovePolicyRule => self.remove_policy_rule(),
            ProxyAction::RemovePolicyRoute => self.remove_policy_route(),
            ProxyAction::RemoveTunForwardHook { token } => self.remove_tun_hook(token),
            ProxyAction::RemoveTunChains { token } => self.remove_tun_chains(token),
            ProxyAction::StopWatcher => self.stop_mihomo_watcher(),
            ProxyAction::StopCore => self.stop_mihomo(),
            ProxyAction::RemoveRuntimeState => self.remove_runtime_state(),
            ProxyAction::WriteRuntimeConfig { lan_tun_enabled } => {
                self.write_runtime_config(*lan_tun_enabled)
            }
            ProxyAction::ValidateRuntimeConfig => self.validate_mihomo_config(),
            ProxyAction::StartCore => self.start_mihomo(),
            ProxyAction::WaitForMixedPort => self.wait_for_mixed_port(),
            ProxyAction::WaitForTunInterface { token } => self.wait_for_tun(token),
            ProxyAction::CreateTunChains { token, direct_macs } => {
                self.create_tun_chains(token, direct_macs)
            }
            ProxyAction::InstallTunForwardHook { token } => self.install_tun_hook(token),
            ProxyAction::InstallPolicyRoute => self.install_policy_route(),
            ProxyAction::InstallPolicyRule => {
                self.proxy_ip(&[
                    "-4",
                    "rule",
                    "add",
                    "priority",
                    &MIHOMO_RULE_PRIORITY.to_string(),
                    "fwmark",
                    MIHOMO_MARK,
                    "lookup",
                    &MIHOMO_ROUTE_TABLE.to_string(),
                ])?;
                let rp_filter = format!("/proc/sys/net/ipv4/conf/{MIHOMO_TUN_INTERFACE}/rp_filter");
                if std::fs::metadata(&rp_filter)
                    .map(|metadata| !metadata.permissions().readonly())
                    .unwrap_or(false)
                {
                    let _ = std::fs::write(rp_filter, b"2\n");
                }
                Ok(())
            }
            ProxyAction::InstallInterceptionEntry { token } => {
                storage::validate_token(token)?;
                self.require_owned_mihomo_tun(Some(token))?;
                self.proxy_iptables(&[
                    "-w",
                    "-t",
                    "mangle",
                    "-I",
                    "PREROUTING",
                    "1",
                    "-i",
                    LAN_BRIDGE,
                    "-s",
                    LAN_SUBNET,
                    "-m",
                    "comment",
                    "--comment",
                    token,
                    "-j",
                    MIHOMO_MANGLE_CHAIN,
                ])
            }
            ProxyAction::StartWatcher => self.start_mihomo_watcher(),
            ProxyAction::WaitForWatcher => self.wait_for_mihomo_watcher(),
            ProxyAction::CommitFeatures { features } => self.write_features(*features),
            ProxyAction::RestorePersistedFeatures { features } => self.write_features(*features),
        }
    }

    fn proxy_ip(&self, args: &[&str]) -> Result<(), PlatformError> {
        self.run(Tool::Ip, &strings(args)).map(|_| ())
    }

    fn proxy_iptables(&self, args: &[&str]) -> Result<(), PlatformError> {
        self.run(Tool::Iptables, &strings(args)).map(|_| ())
    }

    fn write_runtime_config(&self, lan_tun_enabled: bool) -> Result<(), PlatformError> {
        storage::ensure_private_dir(MIHOMO_DATA_DIR)?;
        storage::ensure_private_dir(MIHOMO_STATE_DIR)?;
        let source = storage::read_private_small_optional(MIHOMO_SOURCE_CONFIG, MAX_CONFIG_SIZE)?
            .ok_or_else(|| {
            PlatformError::InvalidState("Mihomo source config is absent".to_owned())
        })?;
        let source = migrate_legacy_persisted_source(&source);
        let controller_secret = new_controller_secret()?;
        let runtime = render_runtime_config(&source, lan_tun_enabled, &controller_secret)?;
        storage::atomic_write_private(MIHOMO_CONTROLLER_SECRET, controller_secret.as_bytes())?;
        storage::atomic_write_private(MIHOMO_RUNTIME_CONFIG, runtime.as_bytes())
    }

    pub(crate) fn validate_subscription_candidate(
        &self,
        source: &[u8],
        lan_tun_enabled: bool,
    ) -> Result<(), PlatformError> {
        let source = std::str::from_utf8(source).map_err(|_| {
            PlatformError::InvalidState("candidate source config is not UTF-8".to_owned())
        })?;
        let runtime = render_runtime_config(source, lan_tun_enabled, "candidate-validation-only")?;
        storage::atomic_write_private(MIHOMO_CANDIDATE_CONFIG, runtime.as_bytes())?;
        let result = self.validate_mihomo_config_at(MIHOMO_CANDIDATE_CONFIG);
        let cleanup = storage::remove_file_durable(MIHOMO_CANDIDATE_CONFIG);
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn wait_for_mixed_port(&self) -> Result<(), PlatformError> {
        let address: SocketAddr = MIHOMO_MIXED_ADDRESS.parse().expect("fixed mixed address");
        let identity = self.mihomo_identity()?.ok_or_else(|| {
            PlatformError::InvalidState(
                "Mihomo identity is absent before waiting for its fixed mixed port".to_owned(),
            )
        })?;
        for _ in 0..60 {
            if self.mihomo_identity()?.as_ref() != Some(&identity) {
                return Err(PlatformError::Conflict(
                    "Mihomo identity changed while waiting for its fixed mixed port".to_owned(),
                ));
            }
            if TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_ok() {
                if !mihomo_process_owns_tcp_listener(identity.core, address)? {
                    return Err(PlatformError::Conflict(
                        "fixed mixed port listener is not owned by the exact Mihomo process"
                            .to_owned(),
                    ));
                }
                if self.mihomo_identity()?.as_ref() != Some(&identity) {
                    return Err(PlatformError::Conflict(
                        "Mihomo identity changed after mixed-port ownership confirmation"
                            .to_owned(),
                    ));
                }
                return Ok(());
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(PlatformError::UnsafeToCutOver(
            "Mihomo fixed loopback mixed port did not become ready".to_owned(),
        ))
    }

    fn wait_for_tun(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        if self.read_mihomo_tun_identity()?.is_some() {
            return Err(PlatformError::Conflict(
                "Mihomo TUN identity already exists before ownership capture".to_owned(),
            ));
        }
        let core = self.mihomo_identity()?.ok_or_else(|| {
            PlatformError::InvalidState(
                "cannot capture Mihomo TUN ownership without exact core identity".to_owned(),
            )
        })?;
        for _ in 0..60 {
            if let Some(ifindex) = mihomo_tun_ifindex()? {
                if self.mihomo_identity()?.as_ref() != Some(&core)
                    || !mihomo_process_holds_tun(core.core)?
                {
                    return Err(PlatformError::Conflict(
                        "Mihomo core does not hold the observed TUN interface".to_owned(),
                    ));
                }
                let identity = MihomoTunIdentity {
                    token: token.to_owned(),
                    core: core.core,
                    ifindex,
                };
                storage::atomic_write_private(
                    MIHOMO_TUN_IDENTITY,
                    identity.serialize().as_bytes(),
                )?;
                if self.mihomo_identity()?.as_ref() != Some(&core)
                    || !mihomo_process_holds_tun(core.core)?
                    || mihomo_tun_ifindex()? != Some(ifindex)
                    || self.read_mihomo_tun_identity()?.as_ref() != Some(&identity)
                {
                    return Err(PlatformError::UnsafeToCutOver(
                        "Mihomo TUN ownership changed after identity capture".to_owned(),
                    ));
                }
                return Ok(());
            }
            if self.mihomo_identity()?.is_none() {
                return Err(PlatformError::InvalidState(
                    "Mihomo exited before creating its TUN interface".to_owned(),
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(PlatformError::UnsafeToCutOver(
            "Mihomo TUN interface did not become ready".to_owned(),
        ))
    }

    fn require_owned_mihomo_tun(
        &self,
        expected_token: Option<&str>,
    ) -> Result<MihomoTunIdentity, PlatformError> {
        let identity = self.read_mihomo_tun_identity()?.ok_or_else(|| {
            PlatformError::Conflict("Mihomo TUN ownership identity is absent".to_owned())
        })?;
        if expected_token.is_some_and(|token| token != identity.token) {
            return Err(PlatformError::Conflict(
                "Mihomo TUN ownership token changed".to_owned(),
            ));
        }
        let core = self.mihomo_identity()?.ok_or_else(|| {
            PlatformError::Conflict("Mihomo TUN has no exact live core identity".to_owned())
        })?;
        if core.core != identity.core
            || !mihomo_process_holds_tun(core.core)?
            || mihomo_tun_ifindex()? != Some(identity.ifindex)
        {
            return Err(PlatformError::Conflict(
                "Mihomo TUN interface is stale, replaced, or foreign".to_owned(),
            ));
        }
        Ok(identity)
    }

    fn install_policy_route(&self) -> Result<(), PlatformError> {
        self.require_owned_mihomo_tun(None)?;
        self.proxy_ip(&[
            "-4",
            "route",
            "add",
            "default",
            "dev",
            MIHOMO_TUN_INTERFACE,
            "table",
            &MIHOMO_ROUTE_TABLE.to_string(),
        ])
    }

    fn remove_policy_rule(&self) -> Result<(), PlatformError> {
        let output = self
            .run(Tool::Ip, &strings(&["-4", "rule", "show"]))?
            .stdout;
        if !policy_rule_state_is_exact(&output) {
            return Err(PlatformError::Conflict(
                "policy rule is absent, duplicate, or conflicts at the managed priority".to_owned(),
            ));
        }
        self.proxy_ip(&[
            "-4",
            "rule",
            "del",
            "priority",
            &MIHOMO_RULE_PRIORITY.to_string(),
            "fwmark",
            MIHOMO_MARK,
            "lookup",
            &MIHOMO_ROUTE_TABLE.to_string(),
        ])
    }

    fn remove_policy_route(&self) -> Result<(), PlatformError> {
        let output = self
            .run(
                Tool::Ip,
                &strings(&[
                    "-4",
                    "route",
                    "show",
                    "table",
                    &MIHOMO_ROUTE_TABLE.to_string(),
                ]),
            )?
            .stdout;
        if !policy_route_state_is_exact(&output) {
            return Err(PlatformError::Conflict(
                "policy table is absent, ambiguous, or contains extra routes".to_owned(),
            ));
        }
        self.proxy_ip(&[
            "-4",
            "route",
            "del",
            "default",
            "dev",
            MIHOMO_TUN_INTERFACE,
            "table",
            &MIHOMO_ROUTE_TABLE.to_string(),
        ])
    }

    fn tun_gateway(&self) -> Result<String, PlatformError> {
        let routes = self
            .run(
                Tool::Ip,
                &strings(&[
                    "-4",
                    "route",
                    "show",
                    "default",
                    "dev",
                    crate::domain::network::WAN_INTERFACE,
                ]),
            )?
            .stdout;
        exact_default_gateway(&routes)
            .map(str::to_owned)
            .ok_or_else(|| {
                PlatformError::Conflict(
                    "wlan0 default gateway is not exactly and uniquely identifiable".to_owned(),
                )
            })
    }

    fn verify_tun_chain_bodies(&self, token: &str) -> Result<(), PlatformError> {
        let gateway = self.tun_gateway()?;
        for (table, chain, gateway) in [
            ("filter", MIHOMO_FILTER_CHAIN, None),
            ("mangle", MIHOMO_MANGLE_CHAIN, Some(gateway.as_str())),
        ] {
            let output = self
                .run(Tool::Iptables, &strings(&["-w", "-t", table, "-S", chain]))?
                .stdout;
            let direct_macs = if chain == MIHOMO_MANGLE_CHAIN {
                let parsed = parse_direct_mac_rules(&output, chain).ok_or_else(|| {
                    PlatformError::Conflict("live direct MAC rules are malformed".to_owned())
                })?;
                if !self
                    .allowed_direct_mac_sets()?
                    .iter()
                    .any(|allowed| allowed == &parsed)
                {
                    return Err(PlatformError::Conflict(
                        "live direct MAC rules do not match committed or pending policy".to_owned(),
                    ));
                }
                parsed
            } else {
                std::collections::BTreeSet::new()
            };
            let expected = expected_chain_rules_with_direct(chain, token, gateway, &direct_macs);
            if !chain_output_is_exact(&output, chain, &expected) {
                return Err(PlatformError::Conflict(format!(
                    "live {table}/{chain} body is not the exact owned installer body"
                )));
            }
        }
        Ok(())
    }

    fn table_references(
        &self,
        table: &str,
        chain: &str,
    ) -> Result<Vec<Vec<String>>, PlatformError> {
        let output = self
            .run(Tool::Iptables, &strings(&["-w", "-t", table, "-S"]))?
            .stdout;
        exact_chain_references(&output, chain).ok_or_else(|| {
            PlatformError::ProbeFailed(format!("cannot parse {table} table references"))
        })
    }

    fn remove_interception_entry(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        self.verify_tun_chain_bodies(token)?;
        self.verify_tun_forward_hook(token)?;
        let expected = expected_interception_rule(token);
        let references = self.table_references("mangle", MIHOMO_MANGLE_CHAIN)?;
        if references != [expected.clone()] {
            return Err(PlatformError::Conflict(
                "TUN interception hook is not the exact unique owned reference".to_owned(),
            ));
        }
        let prerouting = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "mangle", "-S", "PREROUTING"]),
            )?
            .stdout;
        if normalized_chain_rules(&prerouting, "PREROUTING")
            .and_then(|rules| rules.first().cloned())
            != Some(expected.clone())
        {
            return Err(PlatformError::Conflict(
                "TUN interception hook is not first".to_owned(),
            ));
        }
        self.proxy_iptables(&[
            "-w",
            "-t",
            "mangle",
            "-D",
            "PREROUTING",
            "-i",
            LAN_BRIDGE,
            "-s",
            LAN_SUBNET,
            "-m",
            "comment",
            "--comment",
            token,
            "-j",
            MIHOMO_MANGLE_CHAIN,
        ])
    }

    fn verify_tun_forward_hook(&self, token: &str) -> Result<(), PlatformError> {
        let expected = strings(&[
            "-A",
            "FORWARD",
            "-m",
            "comment",
            "--comment",
            token,
            "-j",
            MIHOMO_FILTER_CHAIN,
        ]);
        if self.table_references("filter", MIHOMO_FILTER_CHAIN)? != [expected.clone()] {
            return Err(PlatformError::Conflict(
                "TUN FORWARD hook is not the exact unique owned reference".to_owned(),
            ));
        }
        let forward = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "FORWARD"]),
            )?
            .stdout;
        let tailscale_present = owned_forward_hook_is_exact(
            &forward,
            storage::TAILSCALE_FIREWALL_OWNER,
            crate::domain::tailscale::TAILSCALE_FORWARD_CHAIN,
        )?;
        let router_present = owned_forward_hook_is_exact(
            &forward,
            storage::ROUTER_FIREWALL_OWNER,
            crate::domain::network::ROUTER_FILTER_CHAIN,
        )?;
        if !forward_hook_order_is_exact(&forward, true, tailscale_present, router_present) {
            return Err(PlatformError::Conflict(
                "FORWARD hooks are not in exact Mihomo, Tailscale, router order".to_owned(),
            ));
        }
        Ok(())
    }

    fn remove_tun_hook(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        self.verify_tun_chain_bodies(token)?;
        if !self
            .table_references("mangle", MIHOMO_MANGLE_CHAIN)?
            .is_empty()
        {
            return Err(PlatformError::Conflict(
                "TUN mangle chain still has references".to_owned(),
            ));
        }
        self.verify_tun_forward_hook(token)?;
        self.proxy_iptables(&[
            "-w",
            "-t",
            "filter",
            "-D",
            "FORWARD",
            "-m",
            "comment",
            "--comment",
            token,
            "-j",
            MIHOMO_FILTER_CHAIN,
        ])
    }

    fn create_tun_chains(
        &self,
        token: &str,
        direct_macs: &std::collections::BTreeSet<crate::domain::device_policy::LanDeviceMac>,
    ) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        let route = self
            .run(
                Tool::Ip,
                &strings(&[
                    "-4",
                    "route",
                    "show",
                    "default",
                    "dev",
                    crate::domain::network::WAN_INTERFACE,
                ]),
            )?
            .stdout;
        let gateway = exact_default_gateway(&route).ok_or_else(|| {
            PlatformError::UnsafeToCutOver(
                "wlan0 default gateway is not exactly and uniquely explicit".to_owned(),
            )
        })?;
        self.proxy_iptables(&["-w", "-t", "mangle", "-N", MIHOMO_MANGLE_CHAIN])?;
        let mut filter_created = false;
        let result = (|| {
            self.proxy_iptables(&[
                "-w",
                "-t",
                "mangle",
                "-A",
                MIHOMO_MANGLE_CHAIN,
                "-m",
                "comment",
                "--comment",
                token,
            ])?;
            self.proxy_iptables(&["-w", "-t", "filter", "-N", MIHOMO_FILTER_CHAIN])?;
            filter_created = true;
            self.proxy_iptables(&[
                "-w",
                "-t",
                "filter",
                "-A",
                MIHOMO_FILTER_CHAIN,
                "-m",
                "comment",
                "--comment",
                token,
            ])?;
            for subnet in [
                LAN_SUBNET,
                "0.0.0.0/8",
                "10.0.0.0/8",
                "100.64.0.0/10",
                "127.0.0.0/8",
                "169.254.0.0/16",
                "172.16.0.0/12",
                "192.0.0.0/24",
                "192.0.2.0/24",
                "192.168.0.0/16",
                "198.18.0.0/15",
                "198.51.100.0/24",
                "203.0.113.0/24",
                "224.0.0.0/4",
                "240.0.0.0/4",
            ] {
                self.proxy_iptables(&[
                    "-w",
                    "-t",
                    "mangle",
                    "-A",
                    MIHOMO_MANGLE_CHAIN,
                    "-d",
                    subnet,
                    "-j",
                    "RETURN",
                ])?;
            }
            let gateway_host = format!("{gateway}/32");
            self.proxy_iptables(&[
                "-w",
                "-t",
                "mangle",
                "-A",
                MIHOMO_MANGLE_CHAIN,
                "-d",
                &gateway_host,
                "-j",
                "RETURN",
            ])?;
            self.proxy_iptables(&[
                "-w",
                "-t",
                "mangle",
                "-A",
                MIHOMO_MANGLE_CHAIN,
                "-p",
                "udp",
                "--dport",
                "67:68",
                "-j",
                "RETURN",
            ])?;
            for protocol in ["tcp", "udp"] {
                self.proxy_iptables(&[
                    "-w",
                    "-t",
                    "mangle",
                    "-A",
                    MIHOMO_MANGLE_CHAIN,
                    "-p",
                    protocol,
                    "--dport",
                    "53",
                    "-j",
                    "RETURN",
                ])?;
            }
            for mac in direct_macs {
                let mac = mac.to_string();
                self.proxy_iptables(&[
                    "-w",
                    "-t",
                    "mangle",
                    "-A",
                    MIHOMO_MANGLE_CHAIN,
                    "-m",
                    "mac",
                    "--mac-source",
                    &mac,
                    "-j",
                    "RETURN",
                ])?;
            }
            for protocol in ["tcp", "udp"] {
                self.proxy_iptables(&[
                    "-w",
                    "-t",
                    "mangle",
                    "-A",
                    MIHOMO_MANGLE_CHAIN,
                    "-p",
                    protocol,
                    "-j",
                    "MARK",
                    "--set-xmark",
                    MIHOMO_MARK,
                ])?;
                self.proxy_iptables(&[
                    "-w",
                    "-t",
                    "filter",
                    "-A",
                    MIHOMO_FILTER_CHAIN,
                    "-i",
                    LAN_BRIDGE,
                    "-o",
                    MIHOMO_TUN_INTERFACE,
                    "-s",
                    LAN_SUBNET,
                    "-p",
                    protocol,
                    "-j",
                    "ACCEPT",
                ])?;
            }
            self.proxy_iptables(&[
                "-w",
                "-t",
                "filter",
                "-A",
                MIHOMO_FILTER_CHAIN,
                "-i",
                MIHOMO_TUN_INTERFACE,
                "-o",
                LAN_BRIDGE,
                "-d",
                LAN_SUBNET,
                "-m",
                "conntrack",
                "--ctstate",
                "ESTABLISHED,RELATED",
                "-j",
                "ACCEPT",
            ])?;
            self.proxy_iptables(&[
                "-w",
                "-t",
                "filter",
                "-A",
                MIHOMO_FILTER_CHAIN,
                "-j",
                "RETURN",
            ])?;
            storage::atomic_write_private(
                storage::TUN_FIREWALL_OWNER,
                format!("{token}\n").as_bytes(),
            )
        })();
        if result.is_err() {
            self.rollback_created_tun_chains(token, gateway, filter_created, direct_macs);
        }
        result
    }

    fn rollback_created_tun_chains(
        &self,
        token: &str,
        gateway: &str,
        filter_created: bool,
        direct_macs: &std::collections::BTreeSet<crate::domain::device_policy::LanDeviceMac>,
    ) {
        let remove_rules = |table: &str, chain: &str, gateway: Option<&str>| {
            for rule in expected_chain_rules_with_direct(chain, token, gateway, direct_macs)
                .into_iter()
                .rev()
            {
                let mut args = strings(&["-w", "-t", table]);
                args.push("-D".to_owned());
                args.push(chain.to_owned());
                args.extend(rule.into_iter().skip(2));
                let _ = self.run(Tool::Iptables, &args);
            }
            let _ = self.proxy_iptables(&["-w", "-t", table, "-X", chain]);
        };
        if filter_created {
            remove_rules("filter", MIHOMO_FILTER_CHAIN, None);
        }
        remove_rules("mangle", MIHOMO_MANGLE_CHAIN, Some(gateway));
    }

    fn install_tun_hook(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        let forward = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "FORWARD"]),
            )?
            .stdout;
        if !exact_chain_references(&forward, MIHOMO_FILTER_CHAIN)
            .is_some_and(|references| references.is_empty())
        {
            return Err(PlatformError::Conflict(
                "Mihomo FORWARD hook already exists or is unparseable".to_owned(),
            ));
        }
        let tailscale_present = owned_forward_hook_is_exact(
            &forward,
            storage::TAILSCALE_FIREWALL_OWNER,
            crate::domain::tailscale::TAILSCALE_FORWARD_CHAIN,
        )?;
        let router_present = owned_forward_hook_is_exact(
            &forward,
            storage::ROUTER_FIREWALL_OWNER,
            crate::domain::network::ROUTER_FILTER_CHAIN,
        )?;
        if !forward_hook_order_is_exact(&forward, false, tailscale_present, router_present) {
            return Err(PlatformError::Conflict(
                "existing FORWARD hooks are not in Tailscale, router order".to_owned(),
            ));
        }
        self.proxy_iptables(&[
            "-w",
            "-t",
            "filter",
            "-I",
            "FORWARD",
            "1",
            "-m",
            "comment",
            "--comment",
            token,
            "-j",
            MIHOMO_FILTER_CHAIN,
        ])
    }

    fn remove_tun_chains(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        let marker = storage::read_private_small_optional(storage::TUN_FIREWALL_OWNER, 128)?
            .ok_or_else(|| PlatformError::Conflict("TUN ownership marker is absent".to_owned()))?;
        if marker.trim() != token {
            return Err(PlatformError::Conflict(
                "TUN ownership token does not match".to_owned(),
            ));
        }
        // Both exact hooks were deleted by earlier actions. Reprobe the complete, gateway-
        // dependent bodies and all table references before any flush.
        self.verify_tun_chain_bodies(token)?;
        if !self
            .table_references("filter", MIHOMO_FILTER_CHAIN)?
            .is_empty()
            || !self
                .table_references("mangle", MIHOMO_MANGLE_CHAIN)?
                .is_empty()
        {
            return Err(PlatformError::Conflict(
                "TUN chains gained or retained references after hook deletion".to_owned(),
            ));
        }
        self.proxy_iptables(&["-w", "-t", "mangle", "-F", MIHOMO_MANGLE_CHAIN])?;
        self.proxy_iptables(&["-w", "-t", "mangle", "-X", MIHOMO_MANGLE_CHAIN])?;
        self.proxy_iptables(&["-w", "-t", "filter", "-F", MIHOMO_FILTER_CHAIN])?;
        self.proxy_iptables(&["-w", "-t", "filter", "-X", MIHOMO_FILTER_CHAIN])?;
        storage::remove_file_durable(storage::TUN_FIREWALL_OWNER)
    }

    fn write_features(&self, features: ProxyFeaturesV1) -> Result<(), PlatformError> {
        if !features.supported() {
            return Err(PlatformError::InvalidState(
                "unsupported proxy feature version".to_owned(),
            ));
        }
        storage::ensure_private_dir(MIHOMO_DATA_DIR)?;
        let json = serde_json::to_vec(&features).map_err(|error| {
            PlatformError::InvalidState(format!("serialize proxy features: {error}"))
        })?;
        storage::atomic_write_private(MIHOMO_FEATURES_FILE, &json)
    }

    fn remove_runtime_state(&self) -> Result<(), PlatformError> {
        for path in [MIHOMO_RUNTIME_CONFIG, MIHOMO_CONTROLLER_SECRET] {
            storage::remove_file_durable(path)?;
        }
        Ok(())
    }
}

const MAX_TUN_IDENTITY_SIZE: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MihomoTunIdentity {
    pub token: String,
    pub core: CoreIdentity,
    pub ifindex: u32,
}

impl MihomoTunIdentity {
    fn serialize(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n",
            self.token, self.core.pid, self.core.start_time, self.ifindex
        )
    }

    fn parse(record: &str) -> Result<Self, PlatformError> {
        let mut lines = record.lines();
        let token = lines
            .next()
            .ok_or_else(|| PlatformError::InvalidState("Mihomo TUN token is absent".to_owned()))?
            .to_owned();
        storage::validate_token(&token)?;
        let pid = parse_nonzero_u32(lines.next(), "Mihomo TUN core PID")?;
        let start_time = parse_nonzero_u64(lines.next(), "Mihomo TUN core start time")?;
        let ifindex = parse_nonzero_u32(lines.next(), "Mihomo TUN ifindex")?;
        if lines.next().is_some() {
            return Err(PlatformError::InvalidState(
                "Mihomo TUN identity has unexpected fields".to_owned(),
            ));
        }
        Ok(Self {
            token,
            core: CoreIdentity { pid, start_time },
            ifindex,
        })
    }
}

impl LinuxRouterPlatform {
    pub(crate) fn read_mihomo_tun_identity(
        &self,
    ) -> Result<Option<MihomoTunIdentity>, PlatformError> {
        let Some(record) =
            storage::read_private_small_optional(MIHOMO_TUN_IDENTITY, MAX_TUN_IDENTITY_SIZE)?
        else {
            return Ok(None);
        };
        MihomoTunIdentity::parse(&record).map(Some)
    }
}

pub(crate) fn mihomo_tun_ifindex() -> Result<Option<u32>, PlatformError> {
    let interface = std::path::Path::new("/sys/class/net").join(MIHOMO_TUN_INTERFACE);
    match fs::symlink_metadata(&interface) {
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.file_type().is_dir() => {}
        Ok(_) => {
            return Err(PlatformError::InvalidState(
                "Mihomo TUN sysfs node is not an interface".to_owned(),
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(PlatformError::ProbeFailed(format!(
                "inspect Mihomo TUN interface: {error}"
            )))
        }
    }
    if !interface.join("tun_flags").is_file() {
        return Err(PlatformError::Conflict(
            "same-name hyz-mihomo interface is not a TUN device".to_owned(),
        ));
    }
    let value = fs::read_to_string(interface.join("ifindex"))
        .map_err(|error| PlatformError::ProbeFailed(format!("read Mihomo TUN ifindex: {error}")))?;
    let ifindex = value
        .trim()
        .parse::<u32>()
        .map_err(|_| PlatformError::InvalidState("Mihomo TUN ifindex is malformed".to_owned()))?;
    if ifindex == 0 {
        return Err(PlatformError::InvalidState(
            "Mihomo TUN ifindex is zero".to_owned(),
        ));
    }
    Ok(Some(ifindex))
}

fn parse_nonzero_u32(value: Option<&str>, label: &str) -> Result<u32, PlatformError> {
    let value = value
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| PlatformError::InvalidState(format!("{label} is invalid")))?;
    Ok(value)
}

fn parse_nonzero_u64(value: Option<&str>, label: &str) -> Result<u64, PlatformError> {
    let value = value
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| PlatformError::InvalidState(format!("{label} is invalid")))?;
    Ok(value)
}

fn new_controller_secret() -> Result<String, PlatformError> {
    let mut secret = String::with_capacity(64);
    for _ in 0..2 {
        let uuid = fs::read_to_string("/proc/sys/kernel/random/uuid")
            .map_err(|error| PlatformError::Io(format!("read controller UUID: {error}")))?;
        let part = uuid.trim().replace('-', "");
        if part.len() != 32 || !part.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(PlatformError::InvalidState(
                "kernel controller UUID has invalid framing".to_owned(),
            ));
        }
        secret.push_str(&part);
    }
    Ok(secret)
}

fn render_runtime_config(
    source: &str,
    lan_tun_enabled: bool,
    controller_secret: &str,
) -> Result<String, PlatformError> {
    validate_source_config(source)?;
    let controlled = if lan_tun_enabled {
        CONTROLLED_TUN_ENABLED
    } else {
        CONTROLLED_TUN_DISABLED
    };
    let source = remove_controlled_listener_fields(source);
    let mut runtime = replace_top_level_tun_blocks(&source, controlled);
    runtime.push_str(CONTROLLED_LOCAL_MIXED);
    runtime.push_str(&format!(
        "\nexternal-controller: {MIHOMO_CONTROLLER_ADDRESS}\nsecret: \"{controller_secret}\"\n"
    ));
    Ok(runtime)
}

fn validate_source_config(source: &str) -> Result<(), PlatformError> {
    for (needle, label) in [('\0', "NUL"), ('\r', "carriage return"), ('\t', "tab")] {
        if source.contains(needle) {
            return Err(PlatformError::InvalidState(format!(
                "Mihomo source config contains {label}"
            )));
        }
    }
    if source.contains("CHANGE_ME_") {
        return Err(PlatformError::InvalidState(
            "Mihomo source config contains CHANGE_ME_".to_owned(),
        ));
    }
    for key in [
        "external-controller",
        "external-controller-cors",
        "external-controller-pipe",
        "external-controller-unix",
        "external-ui",
        "external-ui-url",
        "secret",
    ] {
        if controlled_top_level_key_appears(source, key) {
            return Err(PlatformError::InvalidState(format!(
                "Mihomo source config may not set controlled key {key}"
            )));
        }
    }
    for key in [
        "mixed-port",
        "port",
        "socks-port",
        "redir-port",
        "tproxy-port",
        "listeners",
        "authentication",
        "skip-auth-prefixes",
        "lan-allowed-ips",
        "lan-disallowed-ips",
    ] {
        if controlled_top_level_key_appears(source, key) {
            return Err(PlatformError::InvalidState(format!(
                "Mihomo source config may not set controlled listener key {key}"
            )));
        }
    }
    Ok(())
}

fn migrate_legacy_persisted_source(source: &str) -> String {
    const LEGACY_CONTROLLED_FIELDS: &[&str] = &[
        "mixed-port",
        "allow-lan",
        "bind-address",
        "authentication",
        "skip-auth-prefixes",
        "lan-allowed-ips",
        "lan-disallowed-ips",
    ];
    remove_top_level_fields(source, LEGACY_CONTROLLED_FIELDS)
}

fn remove_controlled_listener_fields(source: &str) -> String {
    const KEYS: &[&str] = &[
        "mixed-port",
        "port",
        "socks-port",
        "redir-port",
        "tproxy-port",
        "allow-lan",
        "bind-address",
        "listeners",
        "authentication",
        "skip-auth-prefixes",
        "lan-allowed-ips",
        "lan-disallowed-ips",
    ];
    remove_top_level_fields(source, KEYS)
}

fn remove_top_level_fields(source: &str, keys: &[&str]) -> String {
    let mut output = String::new();
    let mut skipping = false;
    for line in source.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if keys.iter().any(|key| top_level_plain_key(body, key)) {
            skipping = true;
            continue;
        }
        if skipping {
            let trimmed = body.trim_start();
            if body.is_empty()
                || body.starts_with(char::is_whitespace)
                || body.starts_with('#')
                || trimmed == "-"
                || trimmed.starts_with("- ")
            {
                continue;
            }
            skipping = false;
        }
        output.push_str(line);
    }
    output
}

fn replace_top_level_tun_blocks(source: &str, controlled: &str) -> String {
    let mut output = String::new();
    let mut skipping = false;
    for line in source.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if is_top_level_plain_tun_key(body) {
            skipping = true;
            continue;
        }
        if skipping && (body.is_empty() || body.starts_with(' ') || body.starts_with('#')) {
            continue;
        }
        skipping = false;
        output.push_str(line);
    }
    if !source.ends_with('\n') {
        let final_line = source.rsplit_once('\n').map_or(source, |(_, line)| line);
        if !is_top_level_plain_tun_key(final_line)
            && !(skipping
                && (final_line.is_empty()
                    || final_line.starts_with(' ')
                    || final_line.starts_with('#')))
            && !output.ends_with(final_line)
        {
            output.push_str(final_line);
        }
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(controlled);
    output
}

fn controlled_top_level_key_appears(source: &str, key: &str) -> bool {
    let top_level_indent = source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || matches!(trimmed, "---" | "...") {
                None
            } else {
                Some(line.len() - line.trim_start_matches(' ').len())
            }
        })
        .min();
    let Some(top_level_indent) = top_level_indent else {
        return false;
    };
    let needle = format!("{key}:");
    let sequence_needle = format!("-{needle}");
    let first_flow_needle = format!("{{{needle}");
    let later_flow_needle = format!(",{needle}");
    source.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || matches!(trimmed, "---" | "...") {
            return false;
        }
        let indent = line.len() - line.trim_start_matches(' ').len();
        if indent != top_level_indent {
            return false;
        }
        let normalized = line
            .chars()
            .filter(|character| !character.is_whitespace() && !matches!(character, '\'' | '"'))
            .collect::<String>();
        normalized.starts_with(&needle)
            || normalized.starts_with(&sequence_needle)
            || normalized.contains(&first_flow_needle)
            || normalized.contains(&later_flow_needle)
    })
}

fn is_top_level_plain_tun_key(line: &str) -> bool {
    top_level_plain_key(line, "tun")
}

fn top_level_plain_key(line: &str, key: &str) -> bool {
    !line.starts_with(char::is_whitespace)
        && line
            .strip_prefix(key)
            .is_some_and(|rest| rest.starts_with(':'))
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::outbound::system::expected_chain_rules;

    const SOURCE: &str = "tun:\n  enable: maybe\n  nested:\n    value: 1\n# consumed with tun block\nmode: rule\ntun: { enable: true }\n  child: true\ndns:\n  enable: true\n";

    #[test]
    fn tun_identity_is_exact_and_bound_to_core_and_ifindex() {
        let identity = MihomoTunIdentity {
            token: "hyz-mihomo-test".to_owned(),
            core: CoreIdentity {
                pid: 41,
                start_time: 99,
            },
            ifindex: 7,
        };
        assert_eq!(
            MihomoTunIdentity::parse(&identity.serialize()).unwrap(),
            identity
        );
        assert!(MihomoTunIdentity::parse("hyz-mihomo-test\n41\n99\n0\n").is_err());
        assert!(MihomoTunIdentity::parse("hyz-mihomo-test\n41\n99\n7\nextra\n").is_err());
        assert!(MihomoTunIdentity::parse("foreign token\n41\n99\n7\n").is_err());
    }

    #[test]
    fn runtime_config_fixes_loopback_mixed_port_and_controls_tun() {
        let disabled = render_runtime_config(SOURCE, false, "test-secret").unwrap();
        assert!(disabled.contains("mixed-port: 7890"));
        assert!(disabled.contains("allow-lan: false"));
        assert!(disabled.contains("bind-address: 127.0.0.1"));
        assert!(disabled.contains("authentication: []"));
        assert!(disabled.contains("tun:\n  enable: false"));
        assert!(!disabled.contains("enable: maybe"));

        let enabled = render_runtime_config(SOURCE, true, "test-secret").unwrap();
        assert!(enabled.contains("device: hyz-mihomo"));
        assert!(enabled.contains("auto-route: false"));
    }

    #[test]
    fn source_safety_rejects_controlled_listeners_secrets_and_unsafe_bytes() {
        for unsafe_source in [
            "port: 7890\nmode: rule\n",
            "mixed-port: 7890\nmode: rule\n",
            "mode:\trule\n",
            "mode: rule\r\n",
            "password: CHANGE_ME_secret\n",
            "external-controller: 0.0.0.0:9090\n",
            "authentication:\n- user:password\nmode: rule\n",
            "skip-auth-prefixes: [127.0.0.0/8]\nmode: rule\n",
            "lan-allowed-ips: [0.0.0.0/0]\nmode: rule\n",
            "{\"external-controller\": 0.0.0.0:9090}\n",
            "secret: exposed\n",
            "mode: rule\0\n",
        ] {
            assert!(
                validate_source_config(unsafe_source).is_err(),
                "{unsafe_source:?}"
            );
        }
        let nested_peer_fields = "proxies:\n  - name: example\n    type: socks5\n    server: 192.0.2.1\n    port: 443\n    airport: retained\n";
        validate_source_config(nested_peer_fields).unwrap();

        let caller_listener = "allow-lan: true\nbind-address: 0.0.0.0\nmode: rule\n";
        validate_source_config(caller_listener).unwrap();
        let runtime = render_runtime_config(caller_listener, false, "test-secret").unwrap();
        assert!(!runtime.contains("allow-lan: true"));
        assert!(!runtime.contains("bind-address: 0.0.0.0"));
        assert!(runtime.contains("allow-lan: false"));
        assert!(runtime.contains("bind-address: 127.0.0.1"));
    }

    #[test]
    fn persisted_legacy_listener_scalars_are_removed_without_weakening_candidate_validation() {
        let legacy = "mixed-port: 7890\nallow-lan: true\nbind-address: 0.0.0.0\nauthentication:\n- old-user:old-password\nmode: rule\n";
        assert!(validate_source_config(legacy).is_err());

        let migrated = migrate_legacy_persisted_source(legacy);
        assert_eq!(migrated, "mode: rule\n");
        let runtime = render_runtime_config(&migrated, true, "test-secret").unwrap();
        assert!(runtime.contains("mixed-port: 7890"));
        assert!(runtime.contains("allow-lan: false"));
        assert!(runtime.contains("bind-address: 127.0.0.1"));
        assert!(runtime.contains("authentication: []"));
        assert!(!runtime.contains("old-user"));

        for unsafe_source in [
            "  mixed-port: 7890\n  mode: rule\n",
            "\"mixed-port\": 7890\nmode: rule\n",
            "external-controller: 0.0.0.0:9090\nmode: rule\n",
            "listeners:\n  - name: foreign\n",
        ] {
            let migrated = migrate_legacy_persisted_source(unsafe_source);
            assert!(
                render_runtime_config(&migrated, false, "test-secret").is_err(),
                "{unsafe_source:?}"
            );
        }
    }

    #[test]
    fn chain_rule_golden_has_token_only_sentinel_gateway_and_final_return() {
        let token = "hyz-mihomo-00000000-0000-0000-0000-000000000000";
        assert_eq!(
            expected_interception_rule(token),
            strings(&[
                "-A",
                "PREROUTING",
                "-s",
                LAN_SUBNET,
                "-i",
                LAN_BRIDGE,
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                MIHOMO_MANGLE_CHAIN,
            ])
        );
        let mangle = expected_chain_rules(MIHOMO_MANGLE_CHAIN, token, Some("192.0.2.1"));
        assert_eq!(
            mangle.first(),
            Some(&strings(&[
                "-A",
                MIHOMO_MANGLE_CHAIN,
                "-m",
                "comment",
                "--comment",
                token
            ]))
        );
        assert!(mangle.contains(&strings(&[
            "-A",
            MIHOMO_MANGLE_CHAIN,
            "-d",
            "192.0.2.1/32",
            "-j",
            "RETURN"
        ])));
        let mut output = format!("-N {MIHOMO_MANGLE_CHAIN}\n");
        for rule in &mangle {
            output.push_str(&format!("{}\n", rule.join(" ")));
        }
        assert!(chain_output_is_exact(&output, MIHOMO_MANGLE_CHAIN, &mangle));
        assert!(!chain_output_is_exact(
            &output,
            MIHOMO_MANGLE_CHAIN,
            &expected_chain_rules(MIHOMO_MANGLE_CHAIN, token, Some("192.0.2.2"))
        ));
        let filter = expected_chain_rules(MIHOMO_FILTER_CHAIN, token, None);
        assert_eq!(
            filter.last(),
            Some(&strings(&["-A", MIHOMO_FILTER_CHAIN, "-j", "RETURN"]))
        );
    }

    #[test]
    fn gateway_parser_requires_one_explicit_default_route() {
        assert_eq!(
            exact_default_gateway("default via 192.168.8.254 dev wlan0\n"),
            Some("192.168.8.254")
        );
        assert_eq!(exact_default_gateway("default dev wlan0\n"), None);
        assert_eq!(
            exact_default_gateway(
                "default via 192.168.8.254 dev wlan0\ndefault via 192.168.8.253 dev wlan0\n"
            ),
            None
        );
    }
}
