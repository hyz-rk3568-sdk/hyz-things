//! Panel (LCD + proxy group) DTOs; shared with the portal via `hyz-contract`.
//! `PanelBootstrap` is web-side (carries the CSRF token) and stays here until
//! the portal package takes over HTTP.

pub use hyz_contract::panel::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelBootstrap {
    pub csrf_token: String,
    pub panel: PanelSnapshot,
}
