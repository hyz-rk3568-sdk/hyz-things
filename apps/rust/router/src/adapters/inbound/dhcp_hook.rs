use crate::{
    application::{
        dhcp::{
            DhcpEvent, DhcpGeneration, DhcpLease, DhcpTransition, DhcpUplink, DHCP_GENERATION_ENV,
            DHCP_HOOK_ROLE_ENV, DHCP_UPLINK_ENV,
        },
        ports::PlatformError,
    },
    domain::network::{ETHERNET_WAN_INTERFACE, WIFI_WAN_INTERFACE},
};
use std::net::Ipv4Addr;

const DHCP_ACTIONS: [&str; 5] = ["bound", "renew", "deconfig", "leasefail", "nak"];

/// Recognizes and parses the argv/environment shape used when udhcpc invokes this ELF via `-s`.
/// Dispatch remains the composition root's responsibility, so this role never constructs an
/// outbound adapter or mutates networking directly.
pub fn try_event(args: &[String]) -> Option<Result<DhcpEvent, PlatformError>> {
    let [action] = args else {
        return None;
    };
    if !DHCP_ACTIONS.contains(&action.as_str())
        || std::env::var(DHCP_HOOK_ROLE_ENV).ok().as_deref() != Some("v1")
    {
        return None;
    }
    Some(parse_dhcp_event(action, |name| std::env::var(name).ok()))
}

pub fn parse_dhcp_event(
    action: &str,
    get: impl Fn(&str) -> Option<String>,
) -> Result<DhcpEvent, PlatformError> {
    let generation =
        DhcpGeneration::new(get(DHCP_GENERATION_ENV).ok_or_else(|| {
            PlatformError::InvalidState("DHCP hook generation is absent".to_owned())
        })?)
        .map_err(|message| PlatformError::InvalidState(message.to_owned()))?;
    let uplink = match get(DHCP_UPLINK_ENV).as_deref() {
        Some("wifi") => DhcpUplink::Wifi,
        Some("ethernet") => DhcpUplink::Ethernet,
        _ => {
            return Err(PlatformError::InvalidState(
                "DHCP hook uplink is absent or invalid".to_owned(),
            ))
        }
    };
    let expected_interface = match uplink {
        DhcpUplink::Ethernet => ETHERNET_WAN_INTERFACE,
        DhcpUplink::Wifi => WIFI_WAN_INTERFACE,
    };
    if get("interface").as_deref() != Some(expected_interface) {
        return Err(PlatformError::InvalidState(
            "DHCP hook interface does not match the managed uplink".to_owned(),
        ));
    }
    let transition = match action {
        "deconfig" => DhcpTransition::Deconfig,
        "leasefail" | "nak" => DhcpTransition::NoChange,
        "bound" | "renew" => {
            let address = parse_ipv4(get("ip"), "DHCP address")?;
            let prefix =
                netmask_prefix(&get("subnet").unwrap_or_else(|| "255.255.255.0".to_owned()))?;
            let broadcast = get("broadcast")
                .map(|value| value.parse::<Ipv4Addr>())
                .transpose()
                .map_err(|_| PlatformError::InvalidState("invalid DHCP broadcast".to_owned()))?;
            let routers = parse_ipv4_list(get("router"), "DHCP router")?;
            let dns = parse_ipv4_list(get("dns"), "DHCP resolver")?;
            let static_routes = parse_static_routes(get("staticroutes"))?;
            let search_value = get("search").or_else(|| get("domain")).unwrap_or_default();
            let search = search_value
                .split_whitespace()
                .map(validate_domain)
                .collect::<Result<Vec<_>, _>>()?;
            if routers.is_empty() && static_routes.is_empty() {
                return Err(PlatformError::InvalidState(
                    "DHCP lease has no usable route".to_owned(),
                ));
            }
            DhcpTransition::Lease {
                lease: DhcpLease {
                    address,
                    prefix,
                    broadcast,
                    routers,
                    static_routes,
                    dns,
                    search,
                },
            }
        }
        _ => {
            return Err(PlatformError::InvalidState(
                "unsupported DHCP hook action".to_owned(),
            ))
        }
    };
    Ok(DhcpEvent::new(uplink, generation, transition))
}

fn parse_ipv4(value: Option<String>, label: &str) -> Result<Ipv4Addr, PlatformError> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| PlatformError::InvalidState(format!("invalid {label}")))
}

fn parse_ipv4_list(value: Option<String>, label: &str) -> Result<Vec<Ipv4Addr>, PlatformError> {
    value
        .unwrap_or_default()
        .split_whitespace()
        .map(|value| {
            value
                .parse()
                .map_err(|_| PlatformError::InvalidState(format!("invalid {label}")))
        })
        .collect()
}

fn parse_static_routes(value: Option<String>) -> Result<Vec<(String, Ipv4Addr)>, PlatformError> {
    let fields = value
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if fields.len() % 2 != 0 {
        return Err(PlatformError::InvalidState(
            "invalid classless static routes".to_owned(),
        ));
    }
    fields
        .chunks_exact(2)
        .map(|pair| {
            validate_ipv4_cidr(&pair[0])?;
            let gateway = pair[1].parse::<Ipv4Addr>().map_err(|_| {
                PlatformError::InvalidState("invalid classless route gateway".to_owned())
            })?;
            Ok((pair[0].clone(), gateway))
        })
        .collect()
}

fn validate_ipv4_cidr(value: &str) -> Result<(), PlatformError> {
    let (address, prefix) = value
        .split_once('/')
        .ok_or_else(|| PlatformError::InvalidState("invalid classless route".to_owned()))?;
    address
        .parse::<Ipv4Addr>()
        .map_err(|_| PlatformError::InvalidState("invalid classless route".to_owned()))?;
    prefix
        .parse::<u8>()
        .ok()
        .filter(|prefix| *prefix <= 32)
        .ok_or_else(|| PlatformError::InvalidState("invalid classless route".to_owned()))?;
    Ok(())
}

pub fn netmask_prefix(value: &str) -> Result<u8, PlatformError> {
    let mask = u32::from(
        value
            .parse::<Ipv4Addr>()
            .map_err(|_| PlatformError::InvalidState("invalid DHCP netmask".to_owned()))?,
    );
    let prefix = mask.leading_ones() as u8;
    let expected = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    if mask != expected {
        return Err(PlatformError::InvalidState(
            "non-contiguous DHCP netmask".to_owned(),
        ));
    }
    Ok(prefix)
}

fn validate_domain(value: &str) -> Result<String, PlatformError> {
    if value.is_empty()
        || value.len() > 253
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(PlatformError::InvalidState(
            "invalid DHCP search domain".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_udhcpc_action_shapes_select_internal_role() {
        assert!(try_event(&[]).is_none());
        assert!(try_event(&["status".to_owned()]).is_none());
        assert!(try_event(&["bound".to_owned()]).is_none());
        assert!(try_event(&["bound".to_owned(), "extra".to_owned()]).is_none());
    }

    #[test]
    fn parser_is_typed_and_preserves_route_inputs() {
        let values = [
            (DHCP_GENERATION_ENV, "dhcp-test-generation"),
            (DHCP_UPLINK_ENV, "wifi"),
            ("interface", "wlan0"),
            ("ip", "192.0.2.5"),
            ("subnet", "255.255.255.0"),
            ("broadcast", "192.0.2.255"),
            ("router", "192.0.2.1"),
            ("dns", "1.1.1.1 8.8.8.8"),
            ("search", "example.test"),
        ];
        let event = parse_dhcp_event("renew", |key| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        })
        .unwrap();
        assert!(matches!(event.transition, DhcpTransition::Lease { .. }));
        assert!(netmask_prefix("255.0.255.0").is_err());
    }

    #[test]
    fn ethernet_hook_is_a_normal_typed_dhcp_event() {
        let values = [
            (DHCP_GENERATION_ENV, "dhcp-test-generation"),
            (DHCP_UPLINK_ENV, "ethernet"),
            ("interface", "eth0"),
            ("ip", "192.0.2.5"),
            ("subnet", "255.255.255.0"),
            ("router", "192.0.2.1"),
        ];
        let event = parse_dhcp_event("bound", |key| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        })
        .expect("Ethernet hook must parse into a dispatchable event");
        assert_eq!(event.uplink, DhcpUplink::Ethernet);
        assert!(matches!(event.transition, DhcpTransition::Lease { .. }));
    }
}
