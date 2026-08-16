//! hyz-things: the management-plane web portal for the hyz-things device.
//!
//! This package is the LAN-facing "hyz things" portal: it owns all HTTP
//! (LAN and Tailscale exact listeners), administrator authentication and
//! sessions, the embedded Yew SPA and the camera signaling client. It talks
//! to the headless `hyz-router` core and to `hyz-camera` through the root-only
//! versioned Unix control protocols defined in `hyz-contract`.

pub mod adapters;
pub mod application;
pub mod domain;
