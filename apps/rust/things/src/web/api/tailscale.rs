use super::*;

pub(crate) const TAILSCALE_ENDPOINT: &str = "/api/v1/tailscale";

pub(crate) const TAILSCALE_PEERS_ENDPOINT: &str = "/api/v1/tailscale/peers";

pub(crate) const TAILSCALE_MODE_ENDPOINT: &str = "/api/v1/control/tailscale/mode";

pub(crate) const TAILSCALE_LOGIN_ENDPOINT: &str = "/api/v1/control/tailscale/login";

pub(crate) const TAILSCALE_LOGOUT_ENDPOINT: &str = "/api/v1/control/tailscale/logout";

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TailscaleModeRequestDto {
    pub(crate) mode: TailscaleMode,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TailscaleResponseDto {
    pub(crate) tailscale: TailscaleStatus,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TailscalePeersResponseDto {
    pub(crate) peers: TailscalePeerSnapshot,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TailscaleMutationResponseDto {
    pub(crate) tailscale: TailscaleStatus,
    pub(crate) login_url: Option<String>,
}
