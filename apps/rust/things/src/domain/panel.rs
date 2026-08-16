//! Panel (LCD + proxy group) DTOs; shared with the router core via
//! `hyz-contract`. `PanelBootstrap` carries the portal CSRF token.

pub use hyz_contract::panel::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelBootstrap {
    pub csrf_token: String,
    pub panel: PanelSnapshot,
}
