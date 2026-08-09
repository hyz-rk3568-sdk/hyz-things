use crate::application::ports::PlatformError;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

pub const DHCP_HOOK_ROLE_ENV: &str = "HYZ_ROUTER_INTERNAL_DHCP_HOOK";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DhcpEvent {
    Deconfig,
    Lease { lease: DhcpLease },
    NoChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DhcpLease {
    pub address: Ipv4Addr,
    pub prefix: u8,
    pub broadcast: Option<Ipv4Addr>,
    pub routers: Vec<Ipv4Addr>,
    pub static_routes: Vec<(String, Ipv4Addr)>,
    pub dns: Vec<Ipv4Addr>,
    pub search: Vec<String>,
}

impl DhcpLease {
    pub fn resolver_lines(&self, interface: &str) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.search.is_empty() {
            lines.push(format!("search {} # {interface}", self.search.join(" ")));
        }
        lines.extend(
            self.dns
                .iter()
                .map(|address| format!("nameserver {address} # {interface}")),
        );
        lines
    }
}

pub trait DhcpPlatformPort: Send + Sync {
    fn apply_dhcp_event(&self, event: DhcpEvent) -> Result<(), PlatformError>;
}
