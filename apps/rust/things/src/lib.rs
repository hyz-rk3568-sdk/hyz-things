//! hyz-things: the management-plane web portal for the hyz-things device.
//!
//! This package is the LAN-facing "hyz things" portal: it owns all HTTP
//! (LAN and Tailscale exact listeners), administrator authentication and
//! sessions, the embedded Yew SPA and the camera signaling client. It talks
//! to the headless `hyz-router` core and to `hyz-camera` through the root-only
//! versioned Unix control protocols defined in `hyz-contract`.
//!
//! The `native` feature gates the adapters and application use cases (they
//! depend on tokio/Unix sockets/argon2/axum); the `web` feature builds only
//! `domain` plus the Yew SPA, so the wasm target stays free of tokio/mio.

#[cfg(feature = "native")]
pub mod adapters;
#[cfg(feature = "native")]
pub mod application;
pub mod domain;
