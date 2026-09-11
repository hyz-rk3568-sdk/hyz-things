//! Fixed wire contracts shared by the independent hyz-things processes.
//!
//! This crate defines the versioned byte contracts that cross process
//! boundaries:
//!
//! - the `hyz-router` root-only Unix control protocol ([`router`]), plus the
//!   optional pure Unix socket transport in [`client`] (`client` feature);
//! - the `hyz-camera` root-only Unix control protocol ([`camera`]);
//! - the payload DTOs those protocols embed (status, panel, tailscale,
//!   subscription, network config, device policy, activity, Wi-Fi and DHCP requests).
//!
//! The crate must not depend on Axum, Yew, Linux process execution, concrete
//! adapters, or any application-specific behavior. Every type is a serde wire
//! value with its validation/redaction invariants, so both sides of a socket
//! deserialize the same bounded data.
//!
//! Version policy: [`router::PROTOCOL_VERSION`] and [`camera::CONTROL_PROTOCOL_VERSION`]
//! are bumped only on breaking wire changes. Servers accept the current and the
//! previous version (`[current, current - 1]`) to allow rolling application
//! pushes without restarting the stable router core.

pub mod activity;
pub mod admin;
pub mod camera;
pub mod device_policy;
pub mod dhcp;
pub mod network_config;
pub mod ota;
pub mod panel;
pub mod router;
pub mod status;
pub mod subscription;
pub mod tailscale;
pub mod wifi;

#[cfg(feature = "client")]
pub mod client;
