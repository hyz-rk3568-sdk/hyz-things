//! Panel (LCD + proxy group) control DTOs.

use serde::{Deserialize, Serialize};

use super::status::Component;

pub const MAX_CONTROL_NAME_BYTES: usize = 192;
pub const MAX_PROXY_GROUPS: usize = 128;
pub const MAX_PROXY_OPTIONS: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayStatus {
    pub enabled: bool,
    pub brightness: u16,
    pub actual_brightness: u16,
    pub max_brightness: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayRequest {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxySelectionRequest {
    pub group: String,
    pub proxy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyDelayRequest {
    pub proxy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyDelayRefreshRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyDelayResult {
    pub proxy: String,
    pub delay_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyGroupKind {
    Selector,
    UrlTest,
    Fallback,
    LoadBalance,
    Relay,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyOption {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alive: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyGroup {
    pub name: String,
    pub kind: ProxyGroupKind,
    pub selectable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    pub options: Vec<ProxyOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelSnapshot {
    pub display: Component<DisplayStatus>,
    pub proxy_groups: Component<Vec<ProxyGroup>>,
}

pub fn valid_control_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CONTROL_NAME_BYTES
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_names_are_bounded_and_reject_control_characters() {
        assert!(valid_control_name("节点 🇯🇵"));
        assert!(!valid_control_name(""));
        assert!(!valid_control_name("node\nsecret"));
        assert!(!valid_control_name(&"x".repeat(MAX_CONTROL_NAME_BYTES + 1)));
    }
}
