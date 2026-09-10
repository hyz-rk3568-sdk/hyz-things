use super::*;

pub(crate) fn render_overview(state: &UseReducerHandle<AppState>) -> Html {
    html! {
        <>
            if let Some(snapshot) = &state.snapshot {
                {render_health_summary(snapshot)}
                {render_issues(snapshot)}
                {render_topology(snapshot)}
                <div class={VIEW_HEADING}>
                    <div><p class={EYEBROW}>{"DETAILS"}</p><h2 class={SECTION_TITLE}>{"运行详情"}</h2></div>
                    <span class={SECTION_META}>{"保留最近一次成功快照"}</span>
                </div>
                {render_dashboard(snapshot)}
                if let Some(panel) = &state.panel {
                    {render_proxy_groups_read_only(&panel.panel.proxy_groups)}
                }
            } else if state.loading {
                <LoadingState title_id="overview-loading-title" title="正在加载状态" />
            } else {
                <EmptyState
                    title_id="overview-empty-title"
                    title="暂时无法读取状态"
                    message="面板会自动重试，无需刷新页面。"
                />
            }
            <CustomCountdownPanel />
            <ExamCountdownPanel />
        </>
    }
}

pub(crate) fn subscription_state_label(state: SubscriptionStateDto) -> &'static str {
    match state {
        SubscriptionStateDto::Idle => "空闲",
        SubscriptionStateDto::Fetching => "更新中",
        SubscriptionStateDto::Active => "已生效",
        SubscriptionStateDto::Failed => "更新失败",
    }
}

pub(crate) fn subscription_tone(state: SubscriptionStateDto) -> &'static str {
    match state {
        SubscriptionStateDto::Active => "text-success",
        SubscriptionStateDto::Fetching => "text-warning",
        SubscriptionStateDto::Failed => "text-error",
        SubscriptionStateDto::Idle => "text-base-content/60",
    }
}

pub(crate) fn current_time() -> String {
    let value = js_sys::Date::new_0()
        .to_time_string()
        .as_string()
        .unwrap_or_default();
    value.get(0..8).unwrap_or(value.as_str()).to_owned()
}

pub(crate) fn deployment_time_label(unix_ms: Option<u64>) -> String {
    const MAX_DATE_MILLIS: u64 = 8_640_000_000_000_000;
    let Some(unix_ms) = unix_ms.filter(|value| *value <= MAX_DATE_MILLIS) else {
        return "未记录".to_owned();
    };
    js_sys::Date::new(&JsValue::from_f64(unix_ms as f64))
        .to_iso_string()
        .as_string()
        .unwrap_or_else(|| "未记录".to_owned())
}

pub(crate) fn overall_status(state: &AppState) -> (&'static str, Tone) {
    match (&state.snapshot, &state.poll_error) {
        (None, Some(_)) => ("连接失败", Tone::Bad),
        (None, None) => ("正在获取", Tone::Neutral),
        (Some(_), Some(_)) => ("数据已降级", Tone::Warn),
        (Some(snapshot), None) if snapshot.state == SnapshotState::Degraded => {
            ("部分异常", Tone::Warn)
        }
        (Some(_), None) => ("运行正常", Tone::Good),
    }
}

pub(crate) fn render_notice(state: &AppState) -> Html {
    match (&state.poll_error, &state.snapshot) {
        (Some(error), Some(_)) => html! {
            <div class={classes!(NOTICE, "alert-warning", "border-warning/20")} role="alert">
                <strong>{"刷新失败，正在展示最近一次数据"}</strong><span class={NOTICE_COPY}>{error}</span>
            </div>
        },
        (Some(error), None) => html! {
            <div class={classes!(NOTICE, "alert-error", "border-error/20")} role="alert">
                <strong>{"状态服务不可用"}</strong><span class={NOTICE_COPY}>{error}</span>
            </div>
        },
        _ => Html::default(),
    }
}

pub(crate) fn render_topology(snapshot: &StatusSnapshot) -> Html {
    let router = snapshot.router.data.as_ref();
    let proxy = snapshot.proxy.data.as_ref();
    let tailscale = snapshot.tailscale.data.as_ref();
    let active_uplink = router.and_then(|value| value.active_uplink);
    let ethernet = router.and_then(|value| value.ethernet.as_ref());
    let wifi = router.and_then(|value| value.wifi.as_ref());
    let active_status = active_uplink.and_then(|uplink| match uplink {
        UplinkId::Ethernet => ethernet,
        UplinkId::Wifi => wifi,
    });
    let internet_tone = internet_health_tone(router);
    let internet_detail = active_uplink.map_or_else(
        || "没有确认的活动上行".to_owned(),
        |uplink| {
            let metric = active_status
                .and_then(|status| status.default_route_metric)
                .map_or_else(|| "未知".to_owned(), |metric| metric.to_string());
            format!("活动 {} · 默认路由 metric {metric}", uplink_label(uplink))
        },
    );
    let router_detail = router.map_or_else(
        || "等待转发状态".to_owned(),
        |router| {
            format!(
                "活动 {} · 转发 {} · NAT {}",
                router.active_uplink.map(uplink_label).unwrap_or(MISSING),
                router
                    .ipv4_forwarding
                    .map(format_bool)
                    .unwrap_or_else(missing),
                router
                    .masquerade_enabled
                    .map(format_bool)
                    .unwrap_or_else(missing)
            )
        },
    );
    let ap_detail = router.map_or_else(
        || "等待 LAN 状态".to_owned(),
        |router| {
            format!(
                "{} · {} 台客户端",
                router.lan_address.as_deref().unwrap_or(MISSING),
                router.ap_client_count.unwrap_or(0)
            )
        },
    );
    let proxy_detail = proxy.map_or_else(
        || "代理状态不可用".to_owned(),
        |proxy| {
            format!(
                "Core {} · LAN {}",
                mihomo_core_status_label(proxy),
                lan_tun_status_label(proxy)
            )
        },
    );
    let tailscale_detail = tailscale.map_or_else(
        || "Tailscale 状态不可用".to_owned(),
        tailscale_status_detail,
    );

    html! {
        <section class={TOPOLOGY} aria-labelledby="topology-title">
            <PageHeader title_id="topology-title" eyebrow="PATH" title="网络拓扑" centered=true>
                <span class={SECTION_META}>{"Ethernet 优先，Wi-Fi 保持备用；下游同时经过 Router/NAT 与代理运行时"}</span>
            </PageHeader>
            <div class={TOPOLOGY_FLOW}>
                {topology_node("WAN", "互联网", internet_detail, internet_tone)}
                {topology_link(internet_tone)}
                <div class={TOPOLOGY_UPLINKS}>
                    <div class={TOPOLOGY_UPLINKS_HEAD}><span>{"上游双链路"}</span><span>{"Ethernet 优先 · Wi-Fi fallback"}</span></div>
                    {topology_uplink_node("ETH", "Ethernet WAN", uplink_status_detail(ethernet), uplink_tone(ethernet), active_uplink == Some(UplinkId::Ethernet))}
                    {topology_uplink_node("STA", "Wi-Fi WAN", wifi_uplink_status_detail(router), uplink_tone(wifi), active_uplink == Some(UplinkId::Wifi))}
                </div>
                {topology_link(internet_tone)}
                {topology_node("RTR", "Router / NAT", router_detail, component_tone(&snapshot.router))}
                {topology_link(component_tone(&snapshot.router))}
                {topology_node("LAN", "AP / LAN", ap_detail, component_tone(&snapshot.router))}
            </div>
            <div class={TOPOLOGY_BRANCH_STACK}>
                <div class={TOPOLOGY_PROXY_ROW}>
                    <span class={TOPOLOGY_BRANCH} aria-hidden="true">{"↳"}</span>
                    {topology_node("PX", "Mihomo / 代理数据面", proxy_detail, component_tone(&snapshot.proxy))}
                </div>
                <div class={TOPOLOGY_PROXY_ROW}>
                    <span class={TOPOLOGY_BRANCH} aria-hidden="true">{"↳"}</span>
                    {topology_node("TS", "Tailscale", tailscale_detail, component_tone(&snapshot.tailscale))}
                </div>
            </div>
        </section>
    }
}

pub(crate) fn topology_node(
    icon: &'static str,
    title: &'static str,
    detail: String,
    tone: Tone,
) -> Html {
    html! {
        <article class={TOPOLOGY_NODE}>
            <span class={classes!(TOPOLOGY_ICON, tone.class())} aria-hidden="true">{icon}</span>
            <span class={TOPOLOGY_COPY}><strong class={TOPOLOGY_TITLE}>{title}</strong><small class={TOPOLOGY_DETAIL} title={detail.clone()}>{detail}</small></span>
        </article>
    }
}

pub(crate) fn topology_uplink_node(
    icon: &'static str,
    title: &'static str,
    detail: String,
    tone: Tone,
    active: bool,
) -> Html {
    html! {
        <article class={TOPOLOGY_NODE}>
            <span class={classes!(TOPOLOGY_ICON, tone.class())} aria-hidden="true">{icon}</span>
            <span class={TOPOLOGY_COPY}>
                <span class="flex min-w-0 items-center gap-2">
                    <strong class={classes!(TOPOLOGY_TITLE, "truncate")}>{title}</strong>
                    if active {
                        <span class={TOPOLOGY_UPLINK_BADGE}>{"主用"}</span>
                    }
                </span>
                <small class={TOPOLOGY_DETAIL} title={detail.clone()}>{detail}</small>
            </span>
        </article>
    }
}

pub(crate) fn topology_link(tone: Tone) -> Html {
    html! { <span class={classes!(TOPOLOGY_LINK, tone.class())} aria-hidden="true">{"→"}</span> }
}

pub(crate) fn component_tone<T>(component: &Component<T>) -> Tone {
    match component.state {
        ComponentState::Available => Tone::Good,
        ComponentState::Degraded => Tone::Warn,
        ComponentState::Unavailable => Tone::Bad,
    }
}

pub(crate) fn uplink_tone(status: Option<&UplinkStatus>) -> Tone {
    let Some(status) = status else {
        return Tone::Neutral;
    };
    if status.link_up == Some(false) {
        Tone::Bad
    } else if status.session_up == Some(false)
        || status.address_present == Some(false)
        || status.default_route_present == Some(false)
    {
        Tone::Warn
    } else if status.link_up == Some(true)
        && status.session_up == Some(true)
        && status.address_present == Some(true)
        && status.default_route_present == Some(true)
        && status.gateway.is_some()
    {
        Tone::Good
    } else {
        Tone::Warn
    }
}

pub(crate) fn uplink_status_detail(status: Option<&UplinkStatus>) -> String {
    let Some(status) = status else {
        return "状态不可用".to_owned();
    };
    let link = match status.link_up {
        Some(true) => "链路正常",
        Some(false) => "链路断开",
        None => "链路未知",
    };
    let dhcp = match status.session_up {
        Some(true) => "DHCP 已建立",
        Some(false) => "DHCP 未建立",
        None => "DHCP 未知",
    };
    let address = status.address.as_deref().unwrap_or(MISSING);
    let gateway = status
        .gateway
        .map(|value| value.to_string())
        .unwrap_or_else(missing);
    let route = status
        .default_route_metric
        .map(|metric| format!("默认路由 metric {metric}"))
        .unwrap_or_else(|| "默认路由未确认".to_owned());
    format!("{link} · {dhcp} · IPv4 {address} · 网关 {gateway} · {route}")
}

pub(crate) fn wifi_uplink_status_detail(
    router: Option<&hyz_things::domain::status::RouterStatus>,
) -> String {
    let detail = uplink_status_detail(router.and_then(|router| router.wifi.as_ref()));
    let ssid = router
        .and_then(|router| router.sta_ssid.as_deref())
        .unwrap_or(MISSING);
    let signal = router
        .and_then(|router| router.sta_signal_dbm)
        .map(|value| format!(" · 信号 {value} dBm"))
        .unwrap_or_default();
    format!("{detail} · SSID {ssid}{signal}")
}

pub(crate) fn uplink_label(uplink: UplinkId) -> &'static str {
    match uplink {
        UplinkId::Ethernet => "Ethernet",
        UplinkId::Wifi => "Wi-Fi",
    }
}

pub(crate) fn active_uplink_detail(
    router: Option<&hyz_things::domain::status::RouterStatus>,
) -> String {
    router
        .and_then(|router| router.active_uplink)
        .map(|uplink| {
            let metric = router
                .and_then(|router| match uplink {
                    UplinkId::Ethernet => router.ethernet.as_ref(),
                    UplinkId::Wifi => router.wifi.as_ref(),
                })
                .and_then(|status| status.default_route_metric)
                .map_or_else(|| "未知".to_owned(), |metric| metric.to_string());
            format!("{} · metric {metric}", uplink_label(uplink))
        })
        .unwrap_or_else(|| "未确认".to_owned())
}

pub(crate) fn active_resolver_detail(
    router: Option<&hyz_things::domain::status::RouterStatus>,
) -> String {
    router
        .and_then(|router| router.active_resolver.as_ref())
        .map(|resolver| {
            let nameservers = resolver
                .nameservers
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} · {nameservers}", uplink_label(resolver.uplink))
        })
        .unwrap_or_else(|| "未确认".to_owned())
}

pub(crate) fn internet_health_tone(
    router: Option<&hyz_things::domain::status::RouterStatus>,
) -> Tone {
    let Some(router) = router else {
        return Tone::Neutral;
    };
    let ethernet = router.ethernet.as_ref();
    let wifi = router.wifi.as_ref();
    if let Some(active) = router.active_uplink {
        return match active {
            UplinkId::Ethernet => uplink_tone(ethernet),
            UplinkId::Wifi => uplink_tone(wifi),
        };
    }
    if [ethernet, wifi]
        .into_iter()
        .flatten()
        .any(|status| status.session_up == Some(true))
    {
        Tone::Warn
    } else if [ethernet, wifi]
        .into_iter()
        .flatten()
        .all(|status| status.link_up == Some(false))
    {
        Tone::Bad
    } else {
        Tone::Neutral
    }
}

pub(crate) fn component_runtime_tone(state: ComponentState, runtime: Tone) -> Tone {
    match state {
        ComponentState::Unavailable => Tone::Bad,
        ComponentState::Degraded => {
            if matches!(runtime, Tone::Bad) {
                Tone::Bad
            } else {
                Tone::Warn
            }
        }
        ComponentState::Available => runtime,
    }
}

pub(crate) fn lan_health_tone(
    component: &Component<hyz_things::domain::status::RouterStatus>,
) -> Tone {
    use hyz_things::domain::status::LinkState;

    let runtime = component.data.as_ref().map_or(Tone::Neutral, |router| {
        if router.lan_present == Some(false)
            || router.ap_attached_to_lan == Some(false)
            || router.ap_state == Some(LinkState::Down)
        {
            Tone::Bad
        } else if router.ap_state == Some(LinkState::Connecting) {
            Tone::Warn
        } else if router.lan_present == Some(true)
            && router.lan_address.is_some()
            && router.ap_attached_to_lan == Some(true)
            && router.ap_state == Some(LinkState::Up)
        {
            Tone::Good
        } else {
            Tone::Neutral
        }
    });
    component_runtime_tone(component.state, runtime)
}

fn aggregate_runtime_tones(tones: [Tone; 3]) -> Tone {
    if tones.iter().any(|tone| matches!(tone, Tone::Bad)) {
        Tone::Bad
    } else if tones.iter().any(|tone| matches!(tone, Tone::Warn)) {
        Tone::Warn
    } else if tones.iter().any(|tone| matches!(tone, Tone::Neutral)) {
        Tone::Neutral
    } else {
        Tone::Good
    }
}

fn mihomo_health_tone(status: &ProxyStatus) -> Tone {
    let resources = [
        status.mihomo.process,
        status.mihomo.runtime_config,
        status.mihomo.mixed_port,
    ];
    let Some(required) = status.mihomo.configured_required else {
        return Tone::Neutral;
    };
    if resources
        .iter()
        .any(|state| matches!(state, ProxyResourceState::NotReady))
    {
        return Tone::Warn;
    }
    if resources
        .iter()
        .any(|state| matches!(state, ProxyResourceState::Unknown))
    {
        return Tone::Neutral;
    }
    match (required, resources) {
        (
            true,
            [ProxyResourceState::Ready, ProxyResourceState::Ready, ProxyResourceState::Ready],
        )
        | (
            false,
            [ProxyResourceState::Absent, ProxyResourceState::Absent, ProxyResourceState::Absent],
        ) => Tone::Good,
        _ => Tone::Warn,
    }
}

fn lan_tun_health_tone(status: &ProxyStatus) -> Tone {
    match (status.lan_tun.desired, status.lan_tun.effective) {
        (Some(true), LanTunEffective::Ready) | (Some(false), LanTunEffective::OrdinaryNat) => {
            Tone::Good
        }
        (Some(true), LanTunEffective::OrdinaryNat | LanTunEffective::NotConfirmed)
        | (Some(false), LanTunEffective::Ready) => Tone::Warn,
        (Some(false), LanTunEffective::NotConfirmed) | (None, _) => Tone::Neutral,
    }
}

fn local_system_proxy_health_tone(status: &ProxyStatus) -> Tone {
    match (
        status.local_system_proxy.desired,
        status.local_system_proxy.effective,
    ) {
        (Some(true), LocalSystemProxyEffective::Ready)
        | (Some(false), LocalSystemProxyEffective::Disabled) => Tone::Good,
        (
            Some(true),
            LocalSystemProxyEffective::Disabled | LocalSystemProxyEffective::NotConfirmed,
        )
        | (
            Some(false),
            LocalSystemProxyEffective::Ready | LocalSystemProxyEffective::NotConfirmed,
        ) => Tone::Warn,
        (None, _) => Tone::Neutral,
    }
}

pub(crate) fn proxy_health_tone(component: &Component<ProxyStatus>) -> Tone {
    let runtime = component.data.as_ref().map_or(Tone::Neutral, |status| {
        aggregate_runtime_tones([
            mihomo_health_tone(status),
            lan_tun_health_tone(status),
            local_system_proxy_health_tone(status),
        ])
    });
    component_runtime_tone(component.state, runtime)
}

fn tailscale_runtime_tone(status: &TailscaleStatus) -> Tone {
    use hyz_things::domain::status::TailscaleRouteApproval;

    if status.error_category.is_some() {
        return Tone::Warn;
    }
    let Some(desired) = status.desired_mode else {
        return Tone::Neutral;
    };
    match desired {
        TailscaleMode::Disabled => {
            if status.effective_mode == Some(TailscaleMode::Disabled)
                && status.backend_state == TailscaleBackendState::Stopped
            {
                Tone::Good
            } else if status.effective_mode.is_none()
                || status.backend_state == TailscaleBackendState::Unknown
            {
                Tone::Neutral
            } else {
                Tone::Warn
            }
        }
        TailscaleMode::RouterOnly => {
            if status.effective_mode == Some(TailscaleMode::RouterOnly)
                && status.backend_state == TailscaleBackendState::Running
                && status.authenticated == Some(true)
            {
                Tone::Good
            } else if status
                .effective_mode
                .is_some_and(|mode| mode != TailscaleMode::RouterOnly)
                || matches!(
                    status.backend_state,
                    TailscaleBackendState::Stopped | TailscaleBackendState::NeedsLogin
                )
                || status.authenticated == Some(false)
            {
                Tone::Warn
            } else {
                Tone::Neutral
            }
        }
        TailscaleMode::LanSubnetAccess => {
            if status.effective_mode == Some(TailscaleMode::LanSubnetAccess)
                && status.backend_state == TailscaleBackendState::Running
                && status.authenticated == Some(true)
                && status.route_advertised == Some(true)
                && status.local_firewall_ready == Some(true)
                && status.route_approval == TailscaleRouteApproval::Approved
            {
                Tone::Good
            } else if status
                .effective_mode
                .is_some_and(|mode| mode != TailscaleMode::LanSubnetAccess)
                || matches!(
                    status.backend_state,
                    TailscaleBackendState::Stopped | TailscaleBackendState::NeedsLogin
                )
                || status.authenticated == Some(false)
                || status.route_advertised == Some(false)
                || status.local_firewall_ready == Some(false)
                || status.route_approval == TailscaleRouteApproval::UnknownExternalApprovalRequired
            {
                Tone::Warn
            } else {
                Tone::Neutral
            }
        }
    }
}

pub(crate) fn tailscale_health_tone(component: &Component<TailscaleStatus>) -> Tone {
    let runtime = component
        .data
        .as_ref()
        .map_or(Tone::Neutral, tailscale_runtime_tone);
    component_runtime_tone(component.state, runtime)
}

pub(crate) fn health_status_label(tone: Tone) -> &'static str {
    match tone {
        Tone::Good => "正常",
        Tone::Warn => "需检查",
        Tone::Bad => "不可用",
        Tone::Neutral => "未知",
    }
}

pub(crate) fn render_health_summary(snapshot: &StatusSnapshot) -> Html {
    let router = snapshot.router.data.as_ref();
    let proxy = snapshot.proxy.data.as_ref();
    let tailscale = snapshot.tailscale.data.as_ref();

    let internet_tone = internet_health_tone(router);
    let internet_value = active_uplink_detail(router);
    let internet_meta = router
        .and_then(|router| router.active_resolver.as_ref())
        .map(|resolver| format!("DNS {}", uplink_label(resolver.uplink)))
        .unwrap_or_else(|| "默认路由 / DNS 尚未确认".to_owned());

    let lan_tone = lan_health_tone(&snapshot.router);
    let lan_value = router.map_or_else(
        || "LAN 状态不可用".to_owned(),
        |router| {
            format!(
                "{} · {} 台客户端",
                router.lan_address.as_deref().unwrap_or(MISSING),
                router.ap_client_count.unwrap_or(0)
            )
        },
    );
    let lan_meta = router
        .and_then(|router| router.sta_ssid.as_deref())
        .map_or_else(
            || "AP / LAN 与上游 Wi-Fi".to_owned(),
            |ssid| format!("上游 Wi-Fi · {ssid}"),
        );

    let proxy_tone = proxy_health_tone(&snapshot.proxy);
    let proxy_value = proxy
        .map(mihomo_core_status_label)
        .unwrap_or_else(|| "代理状态不可用".to_owned());
    let proxy_meta = proxy
        .map(|proxy| format!("LAN TUN · {}", lan_tun_status_label(proxy)))
        .unwrap_or_else(|| "LAN TUN 状态不可用".to_owned());

    let tailscale_tone = tailscale_health_tone(&snapshot.tailscale);
    let tailscale_value = tailscale
        .map(tailscale_status_detail)
        .unwrap_or_else(|| "Tailscale 状态不可用".to_owned());

    html! {
        <section class={SECTION} aria-labelledby="overview-health-title">
            <PageHeader
                title_id="overview-health-title"
                eyebrow="HEALTH"
                title="核心健康状态"
            >
                <span class={SECTION_META}>{"先确认 WAN、LAN、Proxy 与 Tailscale，再查看拓扑和运行详情"}</span>
            </PageHeader>
            <div class={KPI_GRID} aria-label="核心网络健康">
                <MetricCard
                    label="Internet / WAN"
                    value={internet_value}
                    meta={internet_meta}
                    status={Some(AttrValue::from(health_status_label(internet_tone)))}
                    status_tone={classes!(internet_tone.class())}
                />
                <MetricCard
                    label="LAN / Wi-Fi"
                    value={lan_value}
                    meta={lan_meta}
                    status={Some(AttrValue::from(health_status_label(lan_tone)))}
                    status_tone={classes!(lan_tone.class())}
                />
                <MetricCard
                    label="Proxy"
                    value={proxy_value}
                    meta={proxy_meta}
                    status={Some(AttrValue::from(health_status_label(proxy_tone)))}
                    status_tone={classes!(proxy_tone.class())}
                />
                <MetricCard
                    label="Tailscale"
                    value={tailscale_value}
                    meta="独立覆盖网络状态"
                    status={Some(AttrValue::from(health_status_label(tailscale_tone)))}
                    status_tone={classes!(tailscale_tone.class())}
                />
            </div>
        </section>
    }
}

#[cfg(test)]
mod health_tests {
    use super::*;
    use hyz_things::domain::status::{
        Component, Issue, LanTunStatus, LinkState, LocalSystemProxyStatus, MihomoCoreStatus,
        RouterStatus, TailscaleConnectionStatus, TailscaleRouteApproval,
    };

    fn ready_lan() -> RouterStatus {
        RouterStatus {
            ap_state: Some(LinkState::Up),
            lan_present: Some(true),
            lan_address: Some("192.168.8.1/24".to_owned()),
            ap_attached_to_lan: Some(true),
            ..RouterStatus::default()
        }
    }

    fn stopped_proxy() -> ProxyStatus {
        ProxyStatus {
            configured: true,
            mihomo: MihomoCoreStatus {
                configured_required: Some(false),
                process: ProxyResourceState::Absent,
                runtime_config: ProxyResourceState::Absent,
                mixed_port: ProxyResourceState::Absent,
            },
            lan_tun: LanTunStatus {
                desired: Some(false),
                effective: LanTunEffective::OrdinaryNat,
                ordinary_nat_fallback: Some(true),
            },
            local_system_proxy: LocalSystemProxyStatus {
                desired: Some(false),
                effective: LocalSystemProxyEffective::Disabled,
            },
        }
    }

    fn disabled_tailscale() -> TailscaleStatus {
        TailscaleStatus {
            desired_mode: Some(TailscaleMode::Disabled),
            effective_mode: Some(TailscaleMode::Disabled),
            backend_state: TailscaleBackendState::Stopped,
            authenticated: Some(true),
            ipv4: None,
            route_advertised: None,
            local_firewall_ready: None,
            route_approval: TailscaleRouteApproval::Approved,
            connection: TailscaleConnectionStatus {
                kind: TailscaleConnectionType::Unknown,
                derp_region: None,
            },
            error_category: None,
        }
    }

    #[test]
    fn lan_health_never_promotes_unknown_runtime_to_good() {
        let unknown = Component::available(RouterStatus::default());
        assert!(matches!(lan_health_tone(&unknown), Tone::Neutral));

        let ready = Component::available(ready_lan());
        assert!(matches!(lan_health_tone(&ready), Tone::Good));

        let mut down = ready_lan();
        down.ap_state = Some(LinkState::Down);
        assert!(matches!(
            lan_health_tone(&Component::available(down)),
            Tone::Bad
        ));
    }

    #[test]
    fn proxy_health_requires_three_known_consistent_runtime_states() {
        let ready = stopped_proxy();
        assert!(matches!(
            proxy_health_tone(&Component::available(ready.clone())),
            Tone::Good
        ));

        let mut unknown = ready.clone();
        unknown.mihomo.runtime_config = ProxyResourceState::Unknown;
        assert!(matches!(
            proxy_health_tone(&Component::available(unknown)),
            Tone::Neutral
        ));

        let mut degraded = ready;
        degraded.lan_tun.desired = Some(true);
        assert!(matches!(
            proxy_health_tone(&Component::available(degraded)),
            Tone::Warn
        ));
    }

    #[test]
    fn tailscale_health_requires_confirmed_mode_and_lan_route_readiness() {
        let disabled = disabled_tailscale();
        assert!(matches!(
            tailscale_health_tone(&Component::available(disabled.clone())),
            Tone::Good
        ));

        let mut lan = disabled;
        lan.desired_mode = Some(TailscaleMode::LanSubnetAccess);
        lan.effective_mode = Some(TailscaleMode::LanSubnetAccess);
        lan.backend_state = TailscaleBackendState::Running;
        lan.authenticated = Some(true);
        lan.route_advertised = Some(true);
        lan.local_firewall_ready = Some(true);
        assert!(matches!(
            tailscale_health_tone(&Component::available(lan.clone())),
            Tone::Good
        ));

        lan.route_approval = TailscaleRouteApproval::UnknownExternalApprovalRequired;
        assert!(matches!(
            tailscale_health_tone(&Component::available(lan.clone())),
            Tone::Warn
        ));

        lan.route_approval = TailscaleRouteApproval::Approved;
        lan.route_advertised = None;
        assert!(matches!(
            tailscale_health_tone(&Component::available(lan)),
            Tone::Neutral
        ));
    }

    #[test]
    fn degraded_component_cannot_render_as_healthy() {
        let degraded = Component::degraded(stopped_proxy(), Issue::new("probe", "probe degraded"));
        assert!(matches!(proxy_health_tone(&degraded), Tone::Warn));
    }
}
