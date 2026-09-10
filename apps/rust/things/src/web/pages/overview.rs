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

    let lan_tone = component_tone(&snapshot.router);
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

    let proxy_tone = component_tone(&snapshot.proxy);
    let proxy_value = proxy
        .map(mihomo_core_status_label)
        .unwrap_or_else(|| "代理状态不可用".to_owned());
    let proxy_meta = proxy
        .map(|proxy| format!("LAN TUN · {}", lan_tun_status_label(proxy)))
        .unwrap_or_else(|| "LAN TUN 状态不可用".to_owned());

    let tailscale_tone = component_tone(&snapshot.tailscale);
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
