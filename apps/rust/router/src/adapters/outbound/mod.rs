pub mod admin;
pub mod camera;
pub mod device_policy;
pub mod firmware;
pub mod management;
pub mod network;
pub mod network_config;
pub mod panel;
pub mod paths;
pub mod process;
pub mod proxy;
pub mod status;
pub mod storage;
pub mod subscription;
pub mod system;
pub mod tailscale;

pub use process::{LinuxMihomoFailOpenPlatform, LinuxRouterPlatform};
pub use tailscale::LinuxTailscalePlatform;
