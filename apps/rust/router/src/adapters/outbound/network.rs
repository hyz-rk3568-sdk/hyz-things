use super::{
    management::wait_for_interface_presence,
    process::{LinuxRouterPlatform, Tool},
    storage,
    system::{
        chain_output_is_exact, exact_chain_references, expected_chain_rules,
        forward_hook_order_is_exact, normalized_chain_rules, owned_forward_hook_is_exact,
    },
};
use crate::{
    application::ports::PlatformError,
    domain::network::{
        NetworkAction, LAN_ADDRESS, LAN_BRIDGE, LAN_MEMBER, LAN_SUBNET, ROUTER_FILTER_CHAIN,
        ROUTER_NAT_CHAIN, WAN_INTERFACE,
    },
};
use std::{fs, time::Duration};

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
            NetworkAction::EnsureManagementServices => self.ensure_management_services(),
            NetworkAction::StopManagementServices => self.stop_owned_management_services(),
            NetworkAction::WaitForWanRoute => self.wait_for_sta_route(Duration::from_secs(30)),
            NetworkAction::CaptureIpv4Forwarding => self.capture_forwarding(),
            NetworkAction::EnableIpv4Forwarding => self.enable_forwarding(),
            NetworkAction::DisableIpv4Forwarding => self.disable_forwarding(),
            NetworkAction::InstallRouterFirewall { token } => self.install_router_firewall(token),
            NetworkAction::RemoveRouterFirewall { token } => self.remove_router_firewall(token),
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
        wait_for_interface_presence(LAN_MEMBER, Duration::from_secs(20))?;
        self.ip(&["link", "set", "dev", LAN_MEMBER, "master", LAN_BRIDGE])
    }

    pub(crate) fn detach_ap(&self) -> Result<(), PlatformError> {
        let master = match fs::read_link(format!("/sys/class/net/{LAN_MEMBER}/master")) {
            Ok(master) => master,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "read AP bridge master: {error}"
                )))
            }
        };
        if master.file_name().and_then(|name| name.to_str()) != Some(LAN_BRIDGE) {
            return Err(PlatformError::Conflict(
                "AP master changed before rollback; refusing detach".to_owned(),
            ));
        }
        self.ip(&["link", "set", "dev", LAN_MEMBER, "nomaster"])
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

    fn install_router_firewall(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        let filter_hook_comment = token.to_owned();
        let nat_hook_comment = token.to_owned();
        let forward = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "FORWARD"]),
            )?
            .stdout;
        let owned_hook = |marker: &str, chain: &str| -> Result<bool, PlatformError> {
            let token = storage::read_private_small_optional(marker, 128)?;
            let references = exact_chain_references(&forward, chain).ok_or_else(|| {
                PlatformError::ProbeFailed(format!("cannot parse {chain} FORWARD references"))
            })?;
            match token {
                Some(token) => {
                    let token = token.trim();
                    storage::validate_token(token)?;
                    let expected = strings(&[
                        "-A",
                        "FORWARD",
                        "-m",
                        "comment",
                        "--comment",
                        token,
                        "-j",
                        chain,
                    ]);
                    if references != [expected] {
                        return Err(PlatformError::Conflict(format!(
                            "{chain} FORWARD hook is not exact and unique"
                        )));
                    }
                    Ok(true)
                }
                None if references.is_empty() => Ok(false),
                None => Err(PlatformError::Conflict(format!(
                    "unowned {chain} FORWARD references make router insertion unsafe"
                ))),
            }
        };
        let mihomo_present = owned_hook(
            storage::TUN_FIREWALL_OWNER,
            crate::domain::proxy::MIHOMO_FILTER_CHAIN,
        )?;
        let tailscale_present = owned_hook(
            storage::TAILSCALE_FIREWALL_OWNER,
            crate::domain::tailscale::TAILSCALE_FORWARD_CHAIN,
        )?;
        if !forward_hook_order_is_exact(&forward, mihomo_present, tailscale_present, false) {
            return Err(PlatformError::Conflict(
                "managed FORWARD hooks are not in Mihomo, Tailscale order".to_owned(),
            ));
        }
        let router_position = 1 + usize::from(mihomo_present) + usize::from(tailscale_present);
        self.iptables(&["-w", "-t", "filter", "-N", ROUTER_FILTER_CHAIN])?;
        let mut filter_hook_created = false;
        let mut nat_chain_created = false;
        let mut nat_hook_created = false;
        let result = (|| {
            self.iptables(&[
                "-w",
                "-t",
                "filter",
                "-A",
                ROUTER_FILTER_CHAIN,
                "-m",
                "comment",
                "--comment",
                token,
            ])?;
            self.iptables(&[
                "-w",
                "-t",
                "filter",
                "-A",
                ROUTER_FILTER_CHAIN,
                "-i",
                LAN_BRIDGE,
                "-s",
                LAN_SUBNET,
                "-o",
                WAN_INTERFACE,
                "-m",
                "conntrack",
                "--ctstate",
                "NEW,ESTABLISHED,RELATED",
                "-j",
                "ACCEPT",
            ])?;
            self.iptables(&[
                "-w",
                "-t",
                "filter",
                "-A",
                ROUTER_FILTER_CHAIN,
                "-i",
                WAN_INTERFACE,
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
            self.iptables(&[
                "-w",
                "-t",
                "filter",
                "-A",
                ROUTER_FILTER_CHAIN,
                "-i",
                WAN_INTERFACE,
                "-o",
                LAN_BRIDGE,
                "-j",
                "DROP",
            ])?;
            self.iptables(&[
                "-w",
                "-t",
                "filter",
                "-A",
                ROUTER_FILTER_CHAIN,
                "-i",
                LAN_BRIDGE,
                "-j",
                "DROP",
            ])?;
            self.iptables(&[
                "-w",
                "-t",
                "filter",
                "-I",
                "FORWARD",
                &router_position.to_string(),
                "-m",
                "comment",
                "--comment",
                &filter_hook_comment,
                "-j",
                ROUTER_FILTER_CHAIN,
            ])?;
            filter_hook_created = true;
            self.iptables(&["-w", "-t", "nat", "-N", ROUTER_NAT_CHAIN])?;
            nat_chain_created = true;
            self.iptables(&[
                "-w",
                "-t",
                "nat",
                "-A",
                ROUTER_NAT_CHAIN,
                "-m",
                "comment",
                "--comment",
                token,
            ])?;
            self.iptables(&[
                "-w",
                "-t",
                "nat",
                "-A",
                ROUTER_NAT_CHAIN,
                "-s",
                LAN_SUBNET,
                "-o",
                WAN_INTERFACE,
                "-j",
                "MASQUERADE",
            ])?;
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
                &nat_hook_comment,
                "-j",
                ROUTER_NAT_CHAIN,
            ])?;
            nat_hook_created = true;
            storage::atomic_write_private(
                storage::ROUTER_FIREWALL_OWNER,
                format!("{token}\n").as_bytes(),
            )
        })();
        if result.is_err() {
            self.rollback_created_router_firewall(
                token,
                filter_hook_created,
                nat_chain_created,
                nat_hook_created,
            );
        }
        result
    }

    fn rollback_created_router_firewall(
        &self,
        token: &str,
        filter_hook_created: bool,
        nat_chain_created: bool,
        nat_hook_created: bool,
    ) {
        if nat_hook_created {
            let _ = self.iptables(&[
                "-w",
                "-t",
                "nat",
                "-D",
                "POSTROUTING",
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                ROUTER_NAT_CHAIN,
            ]);
        }
        if nat_chain_created {
            self.rollback_created_chain("nat", ROUTER_NAT_CHAIN, token);
        }
        if filter_hook_created {
            let _ = self.iptables(&[
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
                ROUTER_FILTER_CHAIN,
            ]);
        }
        self.rollback_created_chain("filter", ROUTER_FILTER_CHAIN, token);
    }

    fn rollback_created_chain(&self, table: &str, chain: &str, token: &str) {
        for rule in expected_chain_rules(chain, token, None).into_iter().rev() {
            let mut args = strings(&["-w", "-t", table]);
            args.push("-D".to_owned());
            args.push(chain.to_owned());
            args.extend(rule.into_iter().skip(2));
            let _ = self.run(Tool::Iptables, &args);
        }
        // Never flush during partial-install rollback. -X succeeds only if no modified or
        // foreign rules/references remain.
        let _ = self.iptables(&["-w", "-t", table, "-X", chain]);
    }

    fn remove_router_firewall(&self, token: &str) -> Result<(), PlatformError> {
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
        let filter = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", ROUTER_FILTER_CHAIN]),
            )?
            .stdout;
        let nat = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "nat", "-S", ROUTER_NAT_CHAIN]),
            )?
            .stdout;
        if !chain_output_is_exact(
            &filter,
            ROUTER_FILTER_CHAIN,
            &expected_chain_rules(ROUTER_FILTER_CHAIN, token, None),
        ) || !chain_output_is_exact(
            &nat,
            ROUTER_NAT_CHAIN,
            &expected_chain_rules(ROUTER_NAT_CHAIN, token, None),
        ) {
            return Err(PlatformError::Conflict(
                "live router chain bodies do not exactly match the owned installer rules"
                    .to_owned(),
            ));
        }
        self.verify_router_hooks(token)?;
        // Remove NAT first so any failure leaves the restrictive FORWARD hook in place.
        self.delete_router_hooks(token)?;

        let filter = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", ROUTER_FILTER_CHAIN]),
            )?
            .stdout;
        let nat = self
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "nat", "-S", ROUTER_NAT_CHAIN]),
            )?
            .stdout;
        if !chain_output_is_exact(
            &filter,
            ROUTER_FILTER_CHAIN,
            &expected_chain_rules(ROUTER_FILTER_CHAIN, token, None),
        ) || !chain_output_is_exact(
            &nat,
            ROUTER_NAT_CHAIN,
            &expected_chain_rules(ROUTER_NAT_CHAIN, token, None),
        ) || !self
            .router_references("filter", ROUTER_FILTER_CHAIN)?
            .is_empty()
            || !self.router_references("nat", ROUTER_NAT_CHAIN)?.is_empty()
        {
            return Err(PlatformError::Conflict(
                "router chains changed or gained references after hook deletion".to_owned(),
            ));
        }
        self.iptables(&["-w", "-t", "filter", "-F", ROUTER_FILTER_CHAIN])?;
        self.iptables(&["-w", "-t", "filter", "-X", ROUTER_FILTER_CHAIN])?;
        self.iptables(&["-w", "-t", "nat", "-F", ROUTER_NAT_CHAIN])?;
        self.iptables(&["-w", "-t", "nat", "-X", ROUTER_NAT_CHAIN])?;
        storage::remove_file_durable(storage::ROUTER_FIREWALL_OWNER)
    }

    fn router_references(
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

    fn verify_router_hooks(&self, token: &str) -> Result<(), PlatformError> {
        let filter_hook = strings(&[
            "-A",
            "FORWARD",
            "-m",
            "comment",
            "--comment",
            token,
            "-j",
            ROUTER_FILTER_CHAIN,
        ]);
        let nat_hook = strings(&[
            "-A",
            "POSTROUTING",
            "-m",
            "comment",
            "--comment",
            token,
            "-j",
            ROUTER_NAT_CHAIN,
        ]);
        if self.router_references("filter", ROUTER_FILTER_CHAIN)? != [filter_hook.clone()]
            || self.router_references("nat", ROUTER_NAT_CHAIN)? != [nat_hook.clone()]
        {
            return Err(PlatformError::Conflict(
                "router hooks are not exact unique owned references".to_owned(),
            ));
        }
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
        let mihomo_present = owned_forward_hook_is_exact(
            &forward,
            storage::TUN_FIREWALL_OWNER,
            crate::domain::proxy::MIHOMO_FILTER_CHAIN,
        )?;
        let tailscale_present = owned_forward_hook_is_exact(
            &forward,
            storage::TAILSCALE_FIREWALL_OWNER,
            crate::domain::tailscale::TAILSCALE_FORWARD_CHAIN,
        )?;
        if !forward_hook_order_is_exact(&forward, mihomo_present, tailscale_present, true) {
            return Err(PlatformError::Conflict(
                "FORWARD hooks are not in exact Mihomo, Tailscale, router order".to_owned(),
            ));
        }
        let postrouting = normalized_chain_rules(&postrouting, "POSTROUTING").ok_or_else(|| {
            PlatformError::ProbeFailed("cannot parse POSTROUTING rules".to_owned())
        })?;
        if postrouting.first() != Some(&nat_hook) {
            return Err(PlatformError::Conflict(
                "router hooks are not in exact installer order".to_owned(),
            ));
        }
        Ok(())
    }

    fn delete_router_hooks(&self, token: &str) -> Result<(), PlatformError> {
        self.iptables(&[
            "-w",
            "-t",
            "nat",
            "-D",
            "POSTROUTING",
            "-m",
            "comment",
            "--comment",
            token,
            "-j",
            ROUTER_NAT_CHAIN,
        ])?;
        self.iptables(&[
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
            ROUTER_FILTER_CHAIN,
        ])
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
