use super::{
    paths::{
        MIHOMO_CANDIDATE_CONFIG, MIHOMO_CONTROLLER_ADDRESS, MIHOMO_CONTROLLER_SECRET,
        MIHOMO_DATA_DIR, MIHOMO_RUNTIME_CONFIG, MIHOMO_SOURCE_CONFIG, MIHOMO_STATE_DIR,
    },
    process::{LinuxRouterPlatform, Tool},
    storage,
    system::{
        chain_output_is_exact, exact_chain_references, exact_default_gateway,
        expected_chain_rules_with_direct, expected_interception_rule, normalized_chain_rules,
        parse_direct_mac_rules, policy_route_state_is_exact, policy_rule_state_is_exact,
    },
};
use crate::{
    application::ports::PlatformError,
    domain::{
        network::{LAN_BRIDGE, LAN_SUBNET},
        proxy::{
            ProxyAction, ProxyMode, CONTROLLED_TUN_DISABLED, CONTROLLED_TUN_ENABLED,
            MIHOMO_FILTER_CHAIN, MIHOMO_MANGLE_CHAIN, MIHOMO_MARK, MIHOMO_ROUTE_TABLE,
            MIHOMO_RULE_PRIORITY, MIHOMO_TUN_INTERFACE,
        },
    },
};
use std::{fs, thread, time::Duration};

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
            ProxyAction::WriteRuntimeConfig { mode } => self.write_runtime_config(*mode),
            ProxyAction::ValidateRuntimeConfig => self.validate_mihomo_config(),
            ProxyAction::StartCore => self.start_mihomo(),
            ProxyAction::WaitForTunInterface => self.wait_for_tun(),
            ProxyAction::CreateTunChains { token, direct_macs } => {
                self.create_tun_chains(token, direct_macs)
            }
            ProxyAction::InstallTunForwardHook { token } => self.install_tun_hook(token),
            ProxyAction::InstallPolicyRoute => self.proxy_ip(&[
                "-4",
                "route",
                "add",
                "default",
                "dev",
                MIHOMO_TUN_INTERFACE,
                "table",
                &MIHOMO_ROUTE_TABLE.to_string(),
            ]),
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
            ProxyAction::CommitMode { mode } => self.commit_mode(*mode),
            ProxyAction::RestorePersistedMode { mode } => self.restore_persisted_mode(*mode),
        }
    }

    fn proxy_ip(&self, args: &[&str]) -> Result<(), PlatformError> {
        self.run(Tool::Ip, &strings(args)).map(|_| ())
    }

    fn proxy_iptables(&self, args: &[&str]) -> Result<(), PlatformError> {
        self.run(Tool::Iptables, &strings(args)).map(|_| ())
    }

    fn write_runtime_config(&self, mode: ProxyMode) -> Result<(), PlatformError> {
        storage::ensure_private_dir(MIHOMO_DATA_DIR)?;
        storage::ensure_private_dir(MIHOMO_STATE_DIR)?;
        let source = storage::read_private_small_optional(MIHOMO_SOURCE_CONFIG, MAX_CONFIG_SIZE)?
            .ok_or_else(|| {
            PlatformError::InvalidState("Mihomo source config is absent".to_owned())
        })?;
        validate_source_config(&source)?;
        let controller_secret = new_controller_secret()?;
        let runtime = render_runtime_config(&source, mode, &controller_secret)?;
        storage::atomic_write_private(MIHOMO_CONTROLLER_SECRET, controller_secret.as_bytes())?;
        storage::atomic_write_private(MIHOMO_RUNTIME_CONFIG, runtime.as_bytes())
    }

    pub(crate) fn validate_subscription_candidate(
        &self,
        source: &[u8],
        mode: ProxyMode,
    ) -> Result<(), PlatformError> {
        let source = std::str::from_utf8(source).map_err(|_| {
            PlatformError::InvalidState("candidate source config is not UTF-8".to_owned())
        })?;
        let validation_mode = match mode {
            ProxyMode::Disabled => ProxyMode::Explicit,
            mode => mode,
        };
        let runtime = render_runtime_config(source, validation_mode, "candidate-validation-only")?;
        storage::atomic_write_private(MIHOMO_CANDIDATE_CONFIG, runtime.as_bytes())?;
        let result = self.validate_mihomo_config_at(MIHOMO_CANDIDATE_CONFIG);
        let cleanup = storage::remove_file_durable(MIHOMO_CANDIDATE_CONFIG);
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn wait_for_tun(&self) -> Result<(), PlatformError> {
        for _ in 0..60 {
            if fs::metadata(format!("/sys/class/net/{MIHOMO_TUN_INTERFACE}/tun_flags")).is_ok() {
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
        if normalized_chain_rules(&forward, "FORWARD").and_then(|rules| rules.first().cloned())
            != Some(expected)
        {
            return Err(PlatformError::Conflict(
                "TUN FORWARD hook is not first".to_owned(),
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

    fn commit_mode(&self, mode: ProxyMode) -> Result<(), PlatformError> {
        self.update_persisted_mode(Some(mode))
    }

    fn restore_persisted_mode(&self, mode: Option<ProxyMode>) -> Result<(), PlatformError> {
        self.update_persisted_mode(mode)
    }

    fn update_persisted_mode(&self, mode: Option<ProxyMode>) -> Result<(), PlatformError> {
        let previous_mode = storage::read_private_small_optional(storage::MODE_FILE, 32)?;
        let previous_disabled = storage::read_private_small_optional(storage::DISABLED_MARKER, 32)?;
        let result = self.write_persisted_mode(mode);
        if result.is_err() {
            let _ = restore_optional_file(storage::MODE_FILE, previous_mode.as_deref());
            let _ = restore_optional_file(storage::DISABLED_MARKER, previous_disabled.as_deref());
        }
        result
    }

    fn write_persisted_mode(&self, mode: Option<ProxyMode>) -> Result<(), PlatformError> {
        match mode {
            Some(ProxyMode::Disabled) => {
                storage::atomic_write_private(storage::DISABLED_MARKER, b"")
            }
            Some(mode @ (ProxyMode::Explicit | ProxyMode::Tun)) => {
                storage::atomic_write_private(
                    storage::MODE_FILE,
                    format!("{}\n", mode.as_str()).as_bytes(),
                )?;
                remove_optional_file(storage::DISABLED_MARKER, "disabled marker")
            }
            None => {
                remove_optional_file(storage::MODE_FILE, "proxy mode")?;
                remove_optional_file(storage::DISABLED_MARKER, "disabled marker")
            }
        }
    }
}

fn remove_optional_file(path: &str, _label: &str) -> Result<(), PlatformError> {
    storage::remove_file_durable(path)
}

fn restore_optional_file(path: &str, contents: Option<&str>) -> Result<(), PlatformError> {
    match contents {
        Some(contents) => storage::atomic_write_private(path, contents.as_bytes()),
        None => remove_optional_file(path, path),
    }
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
    mode: ProxyMode,
    controller_secret: &str,
) -> Result<String, PlatformError> {
    validate_source_config(source)?;
    let controlled = match mode {
        ProxyMode::Tun => CONTROLLED_TUN_ENABLED,
        ProxyMode::Explicit => CONTROLLED_TUN_DISABLED,
        ProxyMode::Disabled => {
            return Err(PlatformError::InvalidState(
                "disabled mode has no runtime config".to_owned(),
            ))
        }
    };
    let mut runtime = replace_top_level_tun_blocks(source, controlled);
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
        if controlled_key_appears(source, key) {
            return Err(PlatformError::InvalidState(format!(
                "Mihomo source config may not set controlled key {key}"
            )));
        }
    }
    let allow_lan = source
        .lines()
        .filter(|line| safe_scalar_line(line, "allow-lan", "true"))
        .count();
    let bind_address = source
        .lines()
        .filter(|line| safe_scalar_line(line, "bind-address", "192.168.8.1"))
        .count();
    if allow_lan != 1 || bind_address != 1 {
        return Err(PlatformError::InvalidState(
            "Mihomo requires exactly one top-level allow-lan: true and bind-address: 192.168.8.1"
                .to_owned(),
        ));
    }
    Ok(())
}

fn safe_scalar_line(line: &str, key: &str, value: &str) -> bool {
    let Some(rest) = line
        .strip_prefix(key)
        .and_then(|rest| rest.strip_prefix(':'))
    else {
        return false;
    };
    let rest = rest.trim_start_matches(' ');
    let Some(rest) = rest.strip_prefix(value) else {
        return false;
    };
    rest.is_empty()
        || rest.starts_with(' ') && (rest.trim().is_empty() || rest.trim_start().starts_with('#'))
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

fn controlled_key_appears(source: &str, key: &str) -> bool {
    let needle = format!("{key}:");
    source.lines().any(|line| {
        if line.trim_start().starts_with('#') {
            return false;
        }
        let normalized = line
            .chars()
            .filter(|character| !character.is_whitespace() && !matches!(character, '\'' | '"'))
            .collect::<String>();
        normalized.contains(&needle)
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

    const SOURCE: &str = "allow-lan: true # managed LAN\nbind-address: 192.168.8.1\nport: 7890\ntun:\n  enable: maybe\n  nested:\n    value: 1\n# consumed with tun block\nmode: rule\ntun: { enable: true }\n  child: true\ndns:\n  enable: true\n";

    #[test]
    fn runtime_config_golden_removes_every_plain_top_level_tun_block() {
        validate_source_config(SOURCE).expect("safe source");
        assert_eq!(
            replace_top_level_tun_blocks(SOURCE, CONTROLLED_TUN_DISABLED),
            "allow-lan: true # managed LAN\nbind-address: 192.168.8.1\nport: 7890\nmode: rule\ndns:\n  enable: true\n\ntun:\n  enable: false\n"
        );
        assert_eq!(
            replace_top_level_tun_blocks(SOURCE, CONTROLLED_TUN_ENABLED),
            "allow-lan: true # managed LAN\nbind-address: 192.168.8.1\nport: 7890\nmode: rule\ndns:\n  enable: true\n\ntun:\n  enable: true\n  stack: system\n  device: hyz-mihomo\n  auto-route: false\n  auto-redirect: false\n  auto-detect-interface: false\n  strict-route: false\n  dns-hijack: []\n  mtu: 1500\n"
        );
    }

    #[test]
    fn source_safety_is_exact_and_rejects_unsafe_bytes_and_placeholders() {
        for unsafe_source in [
            "allow-lan: false\nbind-address: 192.168.8.1\n",
            " allow-lan: true\nbind-address: 192.168.8.1\n",
            "allow-lan: true\nallow-lan: true\nbind-address: 192.168.8.1\n",
            "allow-lan: true\nbind-address: 0.0.0.0\n",
            "allow-lan:\ttrue\nbind-address: 192.168.8.1\n",
            "allow-lan: true\r\nbind-address: 192.168.8.1\n",
            "allow-lan: true\nbind-address: 192.168.8.1\npassword: CHANGE_ME_secret\n",
            "allow-lan: true\nbind-address: 192.168.8.1\nexternal-controller: 0.0.0.0:9090\n",
            "allow-lan: true\nbind-address: 192.168.8.1\n{\"external-controller\": 0.0.0.0:9090}\n",
            "allow-lan: true\nbind-address: 192.168.8.1\nsecret: exposed\n",
            "allow-lan: true\nbind-address: 192.168.8.1\0\n",
        ] {
            assert!(
                validate_source_config(unsafe_source).is_err(),
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
