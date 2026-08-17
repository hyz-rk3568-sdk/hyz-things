use super::{
    management::wait_for_interface_presence,
    process::{LinuxRouterPlatform, Tool},
    storage,
    system::{
        chain_output_is_exact, exact_chain_references, expected_router_chain_rules,
        forward_hook_order_is_exact, input_hook_order_is_exact, normalized_chain_rules,
        owned_forward_hook_is_exact, owned_hook_is_exact,
    },
};
use crate::{
    application::ports::PlatformError,
    domain::network::{
        NetworkAction, ETHERNET_LAN_INTERFACE, LAN_ADDRESS, LAN_BRIDGE, LAN_MEMBER,
        ROUTER_FILTER_CHAIN, ROUTER_INPUT_CHAIN, ROUTER_NAT_CHAIN,
    },
};
use std::{fs, time::Duration};

#[derive(Default)]
struct RouterFirewallInstall {
    input_hook_created: bool,
    filter_chain_created: bool,
    filter_hook_created: bool,
    nat_chain_created: bool,
    nat_hook_created: bool,
}

impl LinuxRouterPlatform {
    pub(crate) fn apply_network_action(&self, action: &NetworkAction) -> Result<(), PlatformError> {
        match action {
            NetworkAction::EnsureOwnedBridge { token } => self.ensure_bridge(token),
            NetworkAction::RemoveOwnedBridge { token } => self.remove_owned_bridge(token),
            NetworkAction::ConfigureBridge => {
                self.ip(&[
                    "link",
                    "set",
                    "dev",
                    LAN_BRIDGE,
                    "type",
                    "bridge",
                    "stp_state",
                    "0",
                    "forward_delay",
                    "0",
                ])?;
                self.ip(&["link", "set", "dev", LAN_BRIDGE, "up"])
            }
            NetworkAction::SetBridgeDown => self.ip(&["link", "set", "dev", LAN_BRIDGE, "down"]),
            NetworkAction::AssignLanAddress => {
                self.ip(&["address", "replace", LAN_ADDRESS, "dev", LAN_BRIDGE])
            }
            NetworkAction::RemoveLanAddress => {
                self.ip(&["address", "del", LAN_ADDRESS, "dev", LAN_BRIDGE])
            }
            NetworkAction::AttachAp => self.attach_ap(),
            NetworkAction::DetachAp => self.detach_ap(),
            NetworkAction::AttachEthernetLan => self.attach_lan_member(ETHERNET_LAN_INTERFACE),
            NetworkAction::DetachEthernetLan => self.detach_lan_member(ETHERNET_LAN_INTERFACE),
            NetworkAction::EnsureManagementServices => self.ensure_management_services(),
            NetworkAction::StopManagementServices => self.stop_owned_management_services(),
            NetworkAction::WaitForWanRoute => self.wait_for_sta_route(Duration::from_secs(30)),
            NetworkAction::CaptureIpv4Forwarding => self.capture_forwarding(),
            NetworkAction::EnableIpv4Forwarding => self.enable_forwarding(),
            NetworkAction::DisableIpv4Forwarding => self.disable_forwarding(),
            NetworkAction::InstallRouterFirewall { token, wan_set } => {
                self.install_router_firewall(token, *wan_set)
            }
            NetworkAction::ReconfigureRouterFirewall {
                token,
                previous_wan_set,
                wan_set,
            } => self.reconfigure_router_firewall(token, *previous_wan_set, *wan_set),
            NetworkAction::RemoveRouterFirewall { token } => {
                self.remove_router_firewall(token, self.observe_router_wan_set(token)?)
            }
            NetworkAction::RestoreIpv4Forwarding => self.restore_forwarding(),
        }
    }

    fn ip(&self, args: &[&str]) -> Result<(), PlatformError> {
        let args = args
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        self.run(Tool::Ip, &args).map(|_| ())
    }

    fn iptables(&self, args: &[&str]) -> Result<(), PlatformError> {
        let args = args
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        self.run(Tool::Iptables, &args).map(|_| ())
    }

    fn ensure_bridge(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        if fs::symlink_metadata(format!("/sys/class/net/{LAN_BRIDGE}")).is_ok() {
            return Err(PlatformError::Conflict(
                "br-lan appeared after probe; refusing takeover".to_owned(),
            ));
        }
        self.ip(&["link", "add", "name", LAN_BRIDGE, "type", "bridge"])?;
        let result = (|| {
            let ifindex = fs::read_to_string(format!("/sys/class/net/{LAN_BRIDGE}/ifindex"))
                .map_err(|error| {
                    PlatformError::ProbeFailed(format!("read bridge ifindex: {error}"))
                })?;
            let ifindex = ifindex.trim().parse::<u32>().map_err(|_| {
                PlatformError::ProbeFailed("bridge ifindex was malformed".to_owned())
            })?;
            storage::atomic_write_private(
                storage::BRIDGE_OWNER,
                format!("{token}\n{ifindex}\n").as_bytes(),
            )
        })();
        if result.is_err() {
            let _ = self.ip(&["link", "delete", "dev", LAN_BRIDGE, "type", "bridge"]);
        }
        result
    }

    pub(crate) fn attach_ap(&self) -> Result<(), PlatformError> {
        self.attach_lan_member(LAN_MEMBER)
    }

    fn attach_lan_member(&self, interface: &'static str) -> Result<(), PlatformError> {
        wait_for_interface_presence(interface, Duration::from_secs(20))?;
        self.ip(&["link", "set", "dev", interface, "master", LAN_BRIDGE])
    }

    pub(crate) fn detach_ap(&self) -> Result<(), PlatformError> {
        self.detach_lan_member(LAN_MEMBER)
    }

    fn detach_lan_member(&self, interface: &str) -> Result<(), PlatformError> {
        let master = match fs::read_link(format!("/sys/class/net/{interface}/master")) {
            Ok(master) => master,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "read bridge master for {interface}: {error}"
                )))
            }
        };
        if master.file_name().and_then(|name| name.to_str()) != Some(LAN_BRIDGE) {
            return Err(PlatformError::Conflict(format!(
                "{interface} master changed before rollback; refusing detach"
            )));
        }
        self.ip(&["link", "set", "dev", interface, "nomaster"])
    }

    fn remove_owned_bridge(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        let marker =
            storage::read_small_optional(storage::BRIDGE_OWNER, 256)?.ok_or_else(|| {
                PlatformError::Conflict("bridge ownership marker is absent".to_owned())
            })?;
        let mut lines = marker.lines();
        if lines.next() != Some(token) {
            return Err(PlatformError::Conflict(
                "bridge ownership token does not match".to_owned(),
            ));
        }
        let expected_ifindex = lines.next().and_then(|value| value.parse::<u32>().ok());
        let actual_ifindex = fs::read_to_string(format!("/sys/class/net/{LAN_BRIDGE}/ifindex"))
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok());
        if expected_ifindex.is_none() || expected_ifindex != actual_ifindex {
            return Err(PlatformError::Conflict(
                "bridge identity changed before rollback".to_owned(),
            ));
        }
        self.ip(&["link", "delete", "dev", LAN_BRIDGE, "type", "bridge"])?;
        fs::remove_file(storage::BRIDGE_OWNER)
            .map_err(|error| PlatformError::Io(format!("remove bridge marker: {error}")))
    }

    fn capture_forwarding(&self) -> Result<(), PlatformError> {
        if storage::read_small_optional(storage::PREVIOUS_FORWARDING, 8)?.is_some() {
            return Ok(());
        }
        let current = fs::read_to_string("/proc/sys/net/ipv4/ip_forward")
            .map_err(|error| PlatformError::ProbeFailed(format!("read ip_forward: {error}")))?;
        let current = current.trim();
        if current != "0" && current != "1" {
            return Err(PlatformError::InvalidState(
                "ip_forward was neither zero nor one".to_owned(),
            ));
        }
        storage::atomic_write_private(
            storage::PREVIOUS_FORWARDING,
            format!("{current}\n").as_bytes(),
        )
    }

    fn enable_forwarding(&self) -> Result<(), PlatformError> {
        if storage::read_small_optional(storage::PREVIOUS_FORWARDING, 8)?.is_none() {
            return Err(PlatformError::UnsafeToCutOver(
                "ip_forward ownership was not captured before enable".to_owned(),
            ));
        }
        fs::write("/proc/sys/net/ipv4/ip_forward", b"1\n")
            .map_err(|error| PlatformError::Io(format!("enable ip_forward: {error}")))
    }

    fn disable_forwarding(&self) -> Result<(), PlatformError> {
        if storage::read_small_optional(storage::PREVIOUS_FORWARDING, 8)?.is_none() {
            return Err(PlatformError::UnsafeToCutOver(
                "ip_forward ownership was not captured before disable".to_owned(),
            ));
        }
        fs::write("/proc/sys/net/ipv4/ip_forward", b"0\n")
            .map_err(|error| PlatformError::Io(format!("disable ip_forward: {error}")))?;
        let current = fs::read_to_string("/proc/sys/net/ipv4/ip_forward")
            .map_err(|error| PlatformError::ProbeFailed(format!("probe ip_forward: {error}")))?;
        if current.trim() != "0" {
            return Err(PlatformError::InvalidState(
                "ip_forward was not zero after disable".to_owned(),
            ));
        }
        Ok(())
    }

    fn restore_forwarding(&self) -> Result<(), PlatformError> {
        let Some(previous) = storage::read_small_optional(storage::PREVIOUS_FORWARDING, 8)? else {
            return Err(PlatformError::UnsafeToCutOver(
                "previous ip_forward value is unavailable".to_owned(),
            ));
        };
        let previous = previous.trim();
        if previous != "0" && previous != "1" {
            return Err(PlatformError::InvalidState(
                "saved ip_forward value is invalid".to_owned(),
            ));
        }
        fs::write("/proc/sys/net/ipv4/ip_forward", format!("{previous}\n"))
            .map_err(|error| PlatformError::Io(format!("restore ip_forward: {error}")))?;
        fs::remove_file(storage::PREVIOUS_FORWARDING)
            .map_err(|error| PlatformError::Io(format!("remove forwarding marker: {error}")))
    }

    fn install_router_firewall(
        &self,
        token: &str,
        wan_set: crate::domain::network::RouterWanSet,
    ) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        if storage::read_private_small_optional(storage::ROUTER_FIREWALL_OWNER, 128)?.is_some() {
            return Err(PlatformError::Conflict(
                "router firewall owner marker already exists".to_owned(),
            ));
        }
        self.ensure_router_hook_preconditions()?;

        let mut installed = RouterFirewallInstall::default();
        let result = (|| {
            self.iptables(&["-w", "-t", "filter", "-N", ROUTER_INPUT_CHAIN])?;
            for rule in expected_router_chain_rules(ROUTER_INPUT_CHAIN, token, wan_set) {
                self.append_chain_rule("filter", ROUTER_INPUT_CHAIN, &rule)?;
            }
            self.iptables(&[
                "-w",
                "-t",
                "filter",
                "-I",
                "INPUT",
                &self.router_input_position()?.to_string(),
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                ROUTER_INPUT_CHAIN,
            ])?;
            installed.input_hook_created = true;

            self.iptables(&["-w", "-t", "filter", "-N", ROUTER_FILTER_CHAIN])?;
            installed.filter_chain_created = true;
            for rule in expected_router_chain_rules(ROUTER_FILTER_CHAIN, token, wan_set) {
                self.append_chain_rule("filter", ROUTER_FILTER_CHAIN, &rule)?;
            }
            self.iptables(&[
                "-w",
                "-t",
                "filter",
                "-I",
                "FORWARD",
                &self.router_forward_position()?.to_string(),
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                ROUTER_FILTER_CHAIN,
            ])?;
            installed.filter_hook_created = true;

            self.iptables(&["-w", "-t", "nat", "-N", ROUTER_NAT_CHAIN])?;
            installed.nat_chain_created = true;
            for rule in expected_router_chain_rules(ROUTER_NAT_CHAIN, token, wan_set) {
                self.append_chain_rule("nat", ROUTER_NAT_CHAIN, &rule)?;
            }
            self.iptables(&[
                "-w",
                "-t",
                "nat",
                "-I",
                "POSTROUTING",
                "1",
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                ROUTER_NAT_CHAIN,
            ])?;
            installed.nat_hook_created = true;
            storage::atomic_write_private(
                storage::ROUTER_FIREWALL_OWNER,
                format!("{token}\n").as_bytes(),
            )
        })();
        if result.is_err() {
            self.rollback_created_router_firewall(token, wan_set, &installed);
        }
        result
    }

    fn reconfigure_router_firewall(
        &self,
        token: &str,
        previous_wan_set: crate::domain::network::RouterWanSet,
        wan_set: crate::domain::network::RouterWanSet,
    ) -> Result<(), PlatformError> {
        if previous_wan_set == wan_set {
            return Ok(());
        }
        self.verify_router_firewall(token, previous_wan_set)?;
        self.replace_router_chain_rules(
            "filter",
            ROUTER_FILTER_CHAIN,
            token,
            previous_wan_set,
            wan_set,
        )?;
        if let Err(error) = self.replace_router_chain_rules(
            "nat",
            ROUTER_NAT_CHAIN,
            token,
            previous_wan_set,
            wan_set,
        ) {
            return match self.replace_router_chain_rules(
                "filter",
                ROUTER_FILTER_CHAIN,
                token,
                wan_set,
                previous_wan_set,
            ) {
                Ok(()) => Err(error),
                Err(rollback) => Err(PlatformError::InvalidState(format!(
                    "router firewall NAT reconfiguration failed: {error}; exact FORWARD rollback failed: {rollback}"
                ))),
            };
        }
        Ok(())
    }

    fn replace_router_chain_rules(
        &self,
        table: &str,
        chain: &str,
        token: &str,
        previous_wan_set: crate::domain::network::RouterWanSet,
        wan_set: crate::domain::network::RouterWanSet,
    ) -> Result<(), PlatformError> {
        let previous = expected_router_chain_rules(chain, token, previous_wan_set);
        let next = expected_router_chain_rules(chain, token, wan_set);
        if previous.len() != next.len() || previous.first() != next.first() {
            return Err(PlatformError::InvalidState(
                "router firewall chains must keep an exact fixed replacement shape".to_owned(),
            ));
        }
        let mut replaced = Vec::new();
        for (index, (previous_rule, next_rule)) in previous.iter().zip(&next).enumerate().skip(1) {
            if previous_rule == next_rule {
                continue;
            }
            let mut args = strings(&["-w", "-t", table, "-R", chain, &(index + 1).to_string()]);
            args.extend(next_rule.iter().skip(2).cloned());
            if let Err(error) = self.run(Tool::Iptables, &args) {
                let rollback = replaced.into_iter().rev().try_for_each(
                    |(position, rule): (usize, &Vec<String>)| {
                        let mut rollback =
                            strings(&["-w", "-t", table, "-R", chain, &(position + 1).to_string()]);
                        rollback.extend(rule.iter().skip(2).cloned());
                        self.run(Tool::Iptables, &rollback).map(|_| ())
                    },
                );
                return match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(PlatformError::InvalidState(format!(
                        "router firewall {chain} replacement failed: {error}; exact rollback failed: {rollback}"
                    ))),
                };
            }
            replaced.push((index, previous_rule));
        }
        Ok(())
    }

    fn ensure_router_hook_preconditions(&self) -> Result<(), PlatformError> {
        let forward = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "FORWARD"]),
            )?
            .stdout;
        let input = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "INPUT"]),
            )?
            .stdout;
        let router_forward = exact_chain_references(&forward, ROUTER_FILTER_CHAIN);
        let router_input = exact_chain_references(&input, ROUTER_INPUT_CHAIN);
        if !router_forward.is_some_and(|rules| rules.is_empty())
            || !router_input.is_some_and(|rules| rules.is_empty())
        {
            return Err(PlatformError::Conflict(
                "router firewall references already exist or are unparseable".to_owned(),
            ));
        }
        let mihomo = owned_forward_hook_is_exact(
            &forward,
            storage::TUN_FIREWALL_OWNER,
            crate::domain::proxy::MIHOMO_FILTER_CHAIN,
        )?;
        let tailscale = owned_forward_hook_is_exact(
            &forward,
            storage::TAILSCALE_FIREWALL_OWNER,
            crate::domain::tailscale::TAILSCALE_FORWARD_CHAIN,
        )?;
        if !forward_hook_order_is_exact(&forward, mihomo, tailscale, false) {
            return Err(PlatformError::Conflict(
                "managed FORWARD hooks are not in exact Mihomo, Tailscale order".to_owned(),
            ));
        }
        let tailscale_input = owned_hook_is_exact(
            &input,
            "INPUT",
            storage::TAILSCALE_FIREWALL_OWNER,
            crate::domain::tailscale::TAILSCALE_INPUT_CHAIN,
        )?;
        if !input_hook_order_is_exact(&input, tailscale_input, false) {
            return Err(PlatformError::Conflict(
                "managed INPUT hooks are not in exact Tailscale, router order".to_owned(),
            ));
        }
        Ok(())
    }

    fn router_forward_position(&self) -> Result<String, PlatformError> {
        let forward = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "FORWARD"]),
            )?
            .stdout;
        let mihomo = owned_forward_hook_is_exact(
            &forward,
            storage::TUN_FIREWALL_OWNER,
            crate::domain::proxy::MIHOMO_FILTER_CHAIN,
        )?;
        let tailscale = owned_forward_hook_is_exact(
            &forward,
            storage::TAILSCALE_FIREWALL_OWNER,
            crate::domain::tailscale::TAILSCALE_FORWARD_CHAIN,
        )?;
        Ok((1 + usize::from(mihomo) + usize::from(tailscale)).to_string())
    }

    fn router_input_position(&self) -> Result<String, PlatformError> {
        let input = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "INPUT"]),
            )?
            .stdout;
        let tailscale = owned_hook_is_exact(
            &input,
            "INPUT",
            storage::TAILSCALE_FIREWALL_OWNER,
            crate::domain::tailscale::TAILSCALE_INPUT_CHAIN,
        )?;
        Ok((1 + usize::from(tailscale)).to_string())
    }

    fn append_chain_rule(
        &self,
        table: &str,
        chain: &str,
        rule: &[String],
    ) -> Result<(), PlatformError> {
        let mut args = strings(&["-w", "-t", table, "-A", chain]);
        args.extend(rule.iter().cloned());
        self.run(Tool::Iptables, &args).map(|_| ())
    }

    fn rollback_created_router_firewall(
        &self,
        token: &str,
        wan_set: crate::domain::network::RouterWanSet,
        installed: &RouterFirewallInstall,
    ) {
        if installed.nat_hook_created {
            let _ = self.delete_router_hook("nat", "POSTROUTING", token, ROUTER_NAT_CHAIN);
        }
        if installed.nat_chain_created {
            self.rollback_created_chain("nat", ROUTER_NAT_CHAIN, token, wan_set);
        }
        if installed.filter_hook_created {
            let _ = self.delete_router_hook("filter", "FORWARD", token, ROUTER_FILTER_CHAIN);
        }
        if installed.filter_chain_created {
            self.rollback_created_chain("filter", ROUTER_FILTER_CHAIN, token, wan_set);
        }
        if installed.input_hook_created {
            let _ = self.delete_router_hook("filter", "INPUT", token, ROUTER_INPUT_CHAIN);
        }
        self.rollback_created_chain("filter", ROUTER_INPUT_CHAIN, token, wan_set);
    }

    fn rollback_created_chain(
        &self,
        table: &str,
        chain: &str,
        token: &str,
        wan_set: crate::domain::network::RouterWanSet,
    ) {
        for rule in expected_router_chain_rules(chain, token, wan_set)
            .into_iter()
            .rev()
        {
            let mut args = strings(&["-w", "-t", table, "-D", chain]);
            args.extend(rule);
            let _ = self.run(Tool::Iptables, &args);
        }
        let _ = self.iptables(&["-w", "-t", table, "-X", chain]);
    }

    fn remove_router_firewall(
        &self,
        token: &str,
        wan_set: crate::domain::network::RouterWanSet,
    ) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        let marker = storage::read_private_small_optional(storage::ROUTER_FIREWALL_OWNER, 128)?
            .ok_or_else(|| {
                PlatformError::Conflict("router ownership marker is absent".to_owned())
            })?;
        if marker.trim() != token {
            return Err(PlatformError::Conflict(
                "router ownership token does not match".to_owned(),
            ));
        }
        self.verify_router_firewall(token, wan_set)?;
        self.delete_router_hook("nat", "POSTROUTING", token, ROUTER_NAT_CHAIN)?;
        self.delete_router_hook("filter", "FORWARD", token, ROUTER_FILTER_CHAIN)?;
        self.delete_router_hook("filter", "INPUT", token, ROUTER_INPUT_CHAIN)?;
        self.remove_exact_router_chain("filter", ROUTER_INPUT_CHAIN, token, wan_set)?;
        self.remove_exact_router_chain("filter", ROUTER_FILTER_CHAIN, token, wan_set)?;
        self.remove_exact_router_chain("nat", ROUTER_NAT_CHAIN, token, wan_set)?;
        storage::remove_file_durable(storage::ROUTER_FIREWALL_OWNER)
    }

    fn remove_exact_router_chain(
        &self,
        table: &str,
        chain: &str,
        token: &str,
        wan_set: crate::domain::network::RouterWanSet,
    ) -> Result<(), PlatformError> {
        let output = self
            .run(Tool::Iptables, &strings(&["-w", "-t", table, "-S", chain]))?
            .stdout;
        let expected = expected_router_chain_rules(chain, token, wan_set);
        if !chain_output_is_exact(&output, chain, &expected) {
            return Err(PlatformError::Conflict(format!(
                "live {table}/{chain} body does not exactly match the owned installer rules"
            )));
        }
        self.iptables(&["-w", "-t", table, "-F", chain])?;
        self.iptables(&["-w", "-t", table, "-X", chain])
    }

    fn verify_router_firewall(
        &self,
        token: &str,
        wan_set: crate::domain::network::RouterWanSet,
    ) -> Result<(), PlatformError> {
        for (table, chain) in [
            ("filter", ROUTER_INPUT_CHAIN),
            ("filter", ROUTER_FILTER_CHAIN),
            ("nat", ROUTER_NAT_CHAIN),
        ] {
            let output = self
                .run(Tool::Iptables, &strings(&["-w", "-t", table, "-S", chain]))?
                .stdout;
            if !chain_output_is_exact(
                &output,
                chain,
                &expected_router_chain_rules(chain, token, wan_set),
            ) {
                return Err(PlatformError::Conflict(format!(
                    "live {table}/{chain} body does not exactly match the owned installer rules"
                )));
            }
        }
        let input = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "INPUT"]),
            )?
            .stdout;
        let forward = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "FORWARD"]),
            )?
            .stdout;
        let postrouting = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "nat", "-S", "POSTROUTING"]),
            )?
            .stdout;
        self.verify_exact_router_hook(&input, "INPUT", token, ROUTER_INPUT_CHAIN, 1)?;
        self.verify_exact_router_hook(&forward, "FORWARD", token, ROUTER_FILTER_CHAIN, 1)?;
        self.verify_exact_router_hook(&postrouting, "POSTROUTING", token, ROUTER_NAT_CHAIN, 0)?;
        let mihomo = owned_forward_hook_is_exact(
            &forward,
            storage::TUN_FIREWALL_OWNER,
            crate::domain::proxy::MIHOMO_FILTER_CHAIN,
        )?;
        let tailscale_forward = owned_forward_hook_is_exact(
            &forward,
            storage::TAILSCALE_FIREWALL_OWNER,
            crate::domain::tailscale::TAILSCALE_FORWARD_CHAIN,
        )?;
        let tailscale_input = owned_hook_is_exact(
            &input,
            "INPUT",
            storage::TAILSCALE_FIREWALL_OWNER,
            crate::domain::tailscale::TAILSCALE_INPUT_CHAIN,
        )?;
        if !forward_hook_order_is_exact(&forward, mihomo, tailscale_forward, true)
            || !input_hook_order_is_exact(&input, tailscale_input, true)
        {
            return Err(PlatformError::Conflict(
                "router hook order is not exact".to_owned(),
            ));
        }
        Ok(())
    }

    fn verify_exact_router_hook(
        &self,
        output: &str,
        parent: &str,
        token: &str,
        chain: &str,
        position_after_tailscale: usize,
    ) -> Result<(), PlatformError> {
        let expected = strings(&[
            "-A",
            parent,
            "-m",
            "comment",
            "--comment",
            token,
            "-j",
            chain,
        ]);
        if exact_chain_references(output, chain) != Some(vec![expected.clone()]) {
            return Err(PlatformError::Conflict(format!(
                "{chain} hook is not exact and unique"
            )));
        }
        if parent == "POSTROUTING" {
            if normalized_chain_rules(output, parent).and_then(|rules| rules.first().cloned())
                != Some(expected)
            {
                return Err(PlatformError::Conflict(
                    "router NAT hook is not first".to_owned(),
                ));
            }
        } else if parent == "INPUT" {
            let tailscale = owned_hook_is_exact(
                output,
                "INPUT",
                storage::TAILSCALE_FIREWALL_OWNER,
                crate::domain::tailscale::TAILSCALE_INPUT_CHAIN,
            )?;
            let expected_position = position_after_tailscale - 1 + usize::from(tailscale);
            if normalized_chain_rules(output, parent)
                .and_then(|rules| rules.get(expected_position).cloned())
                != Some(expected)
            {
                return Err(PlatformError::Conflict(
                    "router INPUT hook is not after Tailscale".to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn delete_router_hook(
        &self,
        table: &str,
        parent: &str,
        token: &str,
        chain: &str,
    ) -> Result<(), PlatformError> {
        self.iptables(&[
            "-w",
            "-t",
            table,
            "-D",
            parent,
            "-m",
            "comment",
            "--comment",
            token,
            "-j",
            chain,
        ])
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
