pub mod admin;
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

pub use process::{LinuxMihomoFailOpenPlatform, LinuxRouterPlatform};
