use crate::{
    application::ports::{PanelPlatformPort, PlatformError},
    domain::{
        panel::{
            DisplayRequest, DisplayStatus, PanelSnapshot, ProxyDelayResult, ProxyGroup,
            ProxySelectionRequest,
        },
        status::{Component, Issue},
    },
};

pub struct PanelApplication<'a> {
    platform: &'a dyn PanelPlatformPort,
}

impl<'a> PanelApplication<'a> {
    pub const fn new(platform: &'a dyn PanelPlatformPort) -> Self {
        Self { platform }
    }

    pub fn snapshot(&self) -> PanelSnapshot {
        let display = self
            .platform
            .display_status()
            .map(Component::available)
            .unwrap_or_else(|_| {
                Component::unavailable(Issue::new(
                    "display_unavailable",
                    "Display control is unavailable",
                ))
            });
        let proxy_groups = self
            .platform
            .proxy_groups()
            .map(Component::available)
            .unwrap_or_else(|_| {
                Component::unavailable(Issue::new(
                    "proxy_groups_unavailable",
                    "Mihomo groups are unavailable while the core is stopped or starting",
                ))
            });
        PanelSnapshot {
            display,
            proxy_groups,
        }
    }

    pub fn set_display(&self, request: &DisplayRequest) -> Result<DisplayStatus, PlatformError> {
        self.platform.set_display(request)
    }

    pub fn select_proxy(&self, request: &ProxySelectionRequest) -> Result<(), PlatformError> {
        self.platform.select_proxy(request)
    }

    pub fn measure_proxy_delay(&self, proxy: &str) -> Result<ProxyDelayResult, PlatformError> {
        self.platform.measure_proxy_delay(proxy)
    }

    pub fn proxy_groups(&self) -> Result<Vec<ProxyGroup>, PlatformError> {
        self.platform.proxy_groups()
    }

    pub fn refresh_proxy_delays(&self) -> Result<Vec<ProxyGroup>, PlatformError> {
        self.platform.refresh_proxy_delays()
    }
}
