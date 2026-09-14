use super::*;
use hyz_things::domain::device_policy::LanDeviceMac;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AppPage {
    Overview,
    Study,
    Network,
    Proxy,
    Devices,
    Tailscale,
    Camera,
    Apps,
    System,
}

impl AppPage {
    pub(crate) const ALL: [Self; 9] = [
        Self::Overview,
        Self::Study,
        Self::Network,
        Self::Proxy,
        Self::Devices,
        Self::Tailscale,
        Self::Camera,
        Self::Apps,
        Self::System,
    ];

    pub(crate) const fn slug(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Study => "study",
            Self::Network => "network",
            Self::Proxy => "proxy",
            Self::Devices => "devices",
            Self::Tailscale => "tailscale",
            Self::Camera => "camera",
            Self::Apps => "apps",
            Self::System => "system",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Overview => "总览",
            Self::Study => "学习",
            Self::Network => "网络",
            Self::Proxy => "代理",
            Self::Devices => "设备",
            Self::Tailscale => "Tailscale",
            Self::Camera => "摄像头",
            Self::Apps => "应用",
            Self::System => "系统",
        }
    }

    pub(crate) const fn tab_id(self) -> &'static str {
        match self {
            Self::Overview => "app-overview-tab",
            Self::Study => "app-study-tab",
            Self::Network => "app-network-tab",
            Self::Proxy => "app-proxy-tab",
            Self::Devices => "app-devices-tab",
            Self::Tailscale => "app-tailscale-tab",
            Self::Camera => "app-camera-tab",
            Self::Apps => "app-apps-tab",
            Self::System => "app-system-tab",
        }
    }

    pub(crate) const fn panel_id(self) -> &'static str {
        match self {
            Self::Overview => "app-overview-panel",
            Self::Study => "app-study-panel",
            Self::Network => "app-network-panel",
            Self::Proxy => "app-proxy-panel",
            Self::Devices => "app-devices-panel",
            Self::Tailscale => "app-tailscale-panel",
            Self::Camera => "app-camera-panel",
            Self::Apps => "app-apps-panel",
            Self::System => "app-system-panel",
        }
    }

    pub(crate) const fn protected(self) -> bool {
        matches!(self, Self::Network | Self::Proxy | Self::Devices | Self::Tailscale)
    }

    pub(crate) const fn next(self) -> Option<Self> {
        match self {
            Self::Overview => Some(Self::Study),
            Self::Study => Some(Self::Network),
            Self::Network => Some(Self::Proxy),
            Self::Proxy => Some(Self::Devices),
            Self::Devices => Some(Self::Tailscale),
            Self::Tailscale => Some(Self::Camera),
            Self::Camera => Some(Self::Apps),
            Self::Apps => Some(Self::System),
            Self::System => None,
        }
    }

    pub(crate) const fn previous(self) -> Option<Self> {
        match self {
            Self::Overview => None,
            Self::Study => Some(Self::Overview),
            Self::Network => Some(Self::Study),
            Self::Proxy => Some(Self::Network),
            Self::Devices => Some(Self::Proxy),
            Self::Tailscale => Some(Self::Devices),
            Self::Camera => Some(Self::Tailscale),
            Self::Apps => Some(Self::Camera),
            Self::System => Some(Self::Apps),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct PortalRoute {
    pub(crate) page: AppPage,
    pub(crate) device_id: Option<String>,
    pub(crate) device_filter: Option<String>,
}

impl PortalRoute {
    pub(crate) fn for_page(page: AppPage) -> Self {
        Self {
            page,
            device_id: None,
            device_filter: None,
        }
    }

    pub(crate) fn devices(device_id: Option<String>, device_filter: Option<String>) -> Self {
        Self {
            page: AppPage::Devices,
            device_id,
            device_filter,
        }
    }

    pub(crate) fn hash(&self) -> String {
        let mut hash = format!("#/{}", self.page.slug());
        if self.page == AppPage::Devices {
            if let Some(device_id) = &self.device_id {
                hash.push('/');
                hash.push_str(device_id);
            }
            if let Some(filter) = self.device_filter.as_deref().filter(|value| !value.is_empty()) {
                let query = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("q", filter)
                    .finish();
                hash.push('?');
                hash.push_str(&query);
            }
        }
        hash
    }

    pub(crate) fn parse(hash: &str) -> (Self, bool) {
        let raw = hash.trim();
        let raw = raw.strip_prefix('#').unwrap_or(raw);
        let raw = raw.strip_prefix('/').unwrap_or(raw);
        let (path, query) = raw.split_once('?').unwrap_or((raw, ""));
        let parts = path
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();

        let mut route = match parts.as_slice() {
            [] | ["overview"] => Self::for_page(AppPage::Overview),
            ["study"] => Self::for_page(AppPage::Study),
            ["network"] => Self::for_page(AppPage::Network),
            ["proxy"] => Self::for_page(AppPage::Proxy),
            ["devices"] | ["activity"] => Self::for_page(AppPage::Devices),
            ["devices", device_id] => match canonical_device_id(device_id) {
                Some(device_id) => Self::devices(Some(device_id), None),
                None => Self::for_page(AppPage::Overview),
            },
            ["tailscale"] => Self::for_page(AppPage::Tailscale),
            ["camera"] => Self::for_page(AppPage::Camera),
            ["apps"] => Self::for_page(AppPage::Apps),
            ["system"] => Self::for_page(AppPage::System),
            _ => Self::for_page(AppPage::Overview),
        };

        if route.page == AppPage::Devices {
            route.device_filter = url::form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "q")
                .map(|(_, value)| value.into_owned())
                .filter(|value| {
                    !value.is_empty()
                        && value.len() <= 64
                        && !value.chars().any(char::is_control)
                });
        }

        let canonical = route.hash();
        let incoming = if hash.starts_with('#') {
            hash.to_owned()
        } else if hash.starts_with('/') {
            format!("#{hash}")
        } else if hash.is_empty() {
            String::new()
        } else {
            format!("#/{hash}")
        };
        (route, incoming == canonical)
    }
}

fn canonical_device_id(value: &str) -> Option<String> {
    value.parse::<LanDeviceMac>().ok().map(|mac| mac.to_string())
}

pub(crate) fn current_route() -> (PortalRoute, bool) {
    let hash = web_sys::window()
        .and_then(|window| window.location().hash().ok())
        .unwrap_or_default();
    PortalRoute::parse(&hash)
}

pub(crate) fn navigate_route(
    selected: &UseStateHandle<PortalRoute>,
    route: PortalRoute,
    replace: bool,
) {
    if **selected == route && !replace {
        return;
    }
    if let Some(window) = web_sys::window() {
        if let Ok(history) = window.history() {
            let hash = route.hash();
            if replace {
                let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&hash));
            } else {
                let _ = history.push_state_with_url(&JsValue::NULL, "", Some(&hash));
            }
        }
    }
    selected.set(route);
}

pub(crate) fn app_nav_button(
    candidate: AppPage,
    current: AppPage,
    selected: UseStateHandle<PortalRoute>,
) -> Html {
    let active = candidate == current;
    let onclick = Callback::from(move |_| {
        navigate_route(&selected, PortalRoute::for_page(candidate), false)
    });
    html! {
        <button
            id={candidate.tab_id()}
            class={classes!(PORTAL_TAB, active.then_some(PORTAL_TAB_ACTIVE))}
            type="button"
            aria-pressed={active.to_string()}
            aria-controls={candidate.panel_id()}
            onclick={onclick}
        >
            {candidate.label()}
        </button>
    }
}

pub(crate) const PORTAL_SWIPE_THRESHOLD_PX: i32 = 48;

pub(crate) fn app_page_for_swipe(
    current: AppPage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
) -> Option<AppPage> {
    let horizontal = end_x - start_x;
    let vertical = end_y - start_y;
    if horizontal.abs() < PORTAL_SWIPE_THRESHOLD_PX || horizontal.abs() <= vertical.abs() {
        return None;
    }
    if horizontal < 0 {
        current.next()
    } else {
        current.previous()
    }
}

pub(crate) fn swipe_start_allowed(event: &PointerEvent) -> bool {
    let Some(mut element) = event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
    else {
        return true;
    };
    loop {
        if matches!(
            element.tag_name().as_str(),
            "A" | "AUDIO" | "BUTTON" | "INPUT" | "SELECT" | "TEXTAREA" | "VIDEO"
        ) || element.get_attribute("data-swipe-ignore").is_some()
        {
            return false;
        }
        let Some(parent) = element.parent_element() else {
            return true;
        };
        element = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_routes_canonicalize_legacy_and_invalid_hashes() {
        let (devices, canonical) = PortalRoute::parse("#/activity");
        assert_eq!(devices.page, AppPage::Devices);
        assert!(!canonical);
        assert_eq!(devices.hash(), "#/devices");

        let (detail, canonical) = PortalRoute::parse("#/devices/02:00:00:00:00:0A?q=phone");
        assert_eq!(detail.device_id.as_deref(), Some("02:00:00:00:00:0a"));
        assert_eq!(detail.device_filter.as_deref(), Some("phone"));
        assert!(!canonical);
        assert_eq!(detail.hash(), "#/devices/02:00:00:00:00:0a?q=phone");

        let (invalid, canonical) = PortalRoute::parse("#/not-a-page");
        assert_eq!(invalid.page, AppPage::Overview);
        assert!(!canonical);
    }

    #[test]
    fn study_and_devices_are_first_class_pages() {
        assert!(AppPage::ALL.contains(&AppPage::Study));
        assert!(AppPage::ALL.contains(&AppPage::Devices));
        assert_eq!(AppPage::Devices.label(), "设备");
        assert_eq!(AppPage::Proxy.next(), Some(AppPage::Devices));
        assert_eq!(AppPage::Devices.previous(), Some(AppPage::Proxy));
    }
}
