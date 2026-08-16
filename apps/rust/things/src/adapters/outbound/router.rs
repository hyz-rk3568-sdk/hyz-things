//! Typed client for the headless router core's root-only control socket.

use hyz_contract::router::{ControlOperation, ControlResult};

use crate::application::ports::PortalControlHandler;

/// Production portal control client: each mutation/read is one versioned
/// typed frame on `/run/hyz-router/control.sock`. The router core remains the
/// sole authority; the portal only relays typed operations and enforces its
/// own administrator sessions on top.
pub struct RouterControlClient;

#[async_trait::async_trait]
impl PortalControlHandler for RouterControlClient {
    async fn handle(&self, operation: ControlOperation) -> Result<ControlResult, String> {
        hyz_contract::client::request(operation)
            .await
            .map_err(|error| error.to_string())
    }
}
