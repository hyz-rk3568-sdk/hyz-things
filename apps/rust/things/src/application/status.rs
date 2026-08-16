//! Portal status: the aggregated router snapshot fetched over the control
//! protocol instead of in-process probes.

use std::sync::Arc;

use hyz_contract::{
    router::{ControlOperation, ControlResult},
    status::{Component, Issue, SnapshotState, StatusSnapshot},
};

use super::ports::PortalControlHandler;

#[derive(Clone)]
pub struct PortalStatus {
    control: Arc<dyn PortalControlHandler>,
}

impl PortalStatus {
    pub fn new(control: Arc<dyn PortalControlHandler>) -> Self {
        Self { control }
    }

    pub async fn execute(&self) -> StatusSnapshot {
        match self.control.handle(ControlOperation::Status {}).await {
            Ok(ControlResult::Status { snapshot }) => *snapshot,
            Ok(_) => degraded_snapshot("status contract mismatch"),
            Err(error) => degraded_snapshot(&format!("router status unavailable: {error}")),
        }
    }
}

fn degraded_snapshot(message: &str) -> StatusSnapshot {
    StatusSnapshot {
        state: SnapshotState::Degraded,
        observed_at_unix_ms: 0,
        router: Component::unavailable(Issue::new("router_unavailable", message)),
        proxy: Component::unavailable(Issue::new("router_unavailable", message)),
        tailscale: Component::unavailable(Issue::new("router_unavailable", message)),
        system: Component::unavailable(Issue::new("router_unavailable", message)),
    }
}
