use super::*;

pub(crate) const STATUS_ENDPOINT: &str = "/api/v1/status";

pub(crate) const PANEL_ENDPOINT: &str = "/api/v1/panel";

pub(crate) const DISPLAY_ENDPOINT: &str = "/api/v1/control/display";

pub(crate) async fn fetch_dashboard() -> Result<(StatusSnapshot, PanelBootstrap), String> {
    let status = fetch_json::<StatusSnapshot>(STATUS_ENDPOINT, "状态").await?;
    let panel = fetch_json::<PanelBootstrap>(PANEL_ENDPOINT, "控制面").await?;
    Ok((status, panel))
}
