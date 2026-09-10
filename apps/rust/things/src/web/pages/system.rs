use super::*;

pub(crate) fn render_dashboard(snapshot: &StatusSnapshot) -> Html {
    let router = &snapshot.router;
    let proxy = &snapshot.proxy;
    let system = &snapshot.system;
    let router_data = router.data.as_ref();
    let router_rows = vec![
        ("活动上行", active_uplink_detail(router_data)),
        (
            "Ethernet WAN",
            uplink_status_detail(router_data.and_then(|v| v.ethernet.as_ref())),
        ),
        ("Wi-Fi WAN", wifi_uplink_status_detail(router_data)),
        ("活动解析器", active_resolver_detail(router_data)),
        (
            "LAN 地址",
            router_data
                .and_then(|v| v.lan_address.clone())
                .unwrap_or_else(missing),
        ),
        (
            "AP 客户端",
            router_data
                .and_then(|v| v.ap_client_count)
                .map(|v| format!("{v} 台"))
                .unwrap_or_else(missing),
        ),
        (
            "IPv4 转发",
            router_data
                .and_then(|v| v.ipv4_forwarding)
                .map(format_bool)
                .unwrap_or_else(missing),
        ),
        (
            "IPv4 NAT",
            router_data
                .and_then(|v| v.masquerade_enabled)
                .map(format_bool)
                .unwrap_or_else(missing),
        ),
    ];
    let proxy_rows = vec![
        (
            "Mihomo core",
            proxy
                .data
                .as_ref()
                .map(mihomo_core_status_label)
                .unwrap_or_else(missing),
        ),
        (
            "运行配置",
            proxy
                .data
                .as_ref()
                .map(|v| proxy_resource_label(v.mihomo.runtime_config).to_owned())
                .unwrap_or_else(missing),
        ),
        (
            "本机 mixed port",
            proxy
                .data
                .as_ref()
                .map(|v| proxy_resource_label(v.mihomo.mixed_port).to_owned())
                .unwrap_or_else(missing),
        ),
        (
            "LAN TUN",
            proxy
                .data
                .as_ref()
                .map(lan_tun_status_label)
                .unwrap_or_else(missing),
        ),
        (
            "本机系统代理",
            proxy
                .data
                .as_ref()
                .map(local_system_proxy_status_label)
                .unwrap_or_else(missing),
        ),
    ];
    let system_rows = vec![
        (
            "运行时间",
            system
                .data
                .as_ref()
                .and_then(|v| v.uptime_seconds)
                .map(format_uptime)
                .unwrap_or_else(missing),
        ),
        (
            "CPU 温度",
            system
                .data
                .as_ref()
                .and_then(|v| v.cpu_temperature_millidegrees)
                .map(|v| format!("{:.1} °C", v as f64 / 1_000.0))
                .unwrap_or_else(missing),
        ),
        (
            "网络接口",
            system
                .data
                .as_ref()
                .map(|v| format!("{} 个", v.interfaces.len()))
                .unwrap_or_else(missing),
        ),
        (
            "WAN 总接收",
            wan_traffic(system)
                .map(|v| format_bytes(v.0))
                .unwrap_or_else(missing),
        ),
        (
            "WAN 总发送",
            wan_traffic(system)
                .map(|v| format_bytes(v.1))
                .unwrap_or_else(missing),
        ),
    ];

    html! {
        <section class={DASHBOARD} aria-labelledby="dashboard-title">
            <h2 id="dashboard-title" class="sr-only">{"路由器状态"}</h2>
            if router.data.is_some() {
                {status_card("路由 / LAN", "NET", component_card_status(router), router_rows, "wan")}
            }
            if proxy.data.is_some() {
                {status_card("代理状态", "PX", component_card_status(proxy), proxy_rows, "proxy")}
            }
            if system.data.is_some() {
                {status_card("系统 / 流量", "SYS", component_card_status(system), system_rows, "system")}
            }
        </section>
    }
}

pub(crate) fn render_display_control(
    state: &UseReducerHandle<AppState>,
    brightness: UseStateHandle<u16>,
) -> Html {
    let Some(bootstrap) = state.panel.as_ref() else {
        return Html::default();
    };
    let csrf = bootstrap.csrf_token.clone();
    let busy = state.display_busy;
    let display = bootstrap.panel.display.data.as_ref();
    let max_brightness = display.map_or(255, |display| display.max_brightness.max(1));
    let display_label = display.map_or_else(
        || "显示状态不可用".to_owned(),
        |display| {
            if display.enabled {
                format!(
                    "已点亮 · {} / {}",
                    display.actual_brightness, display.max_brightness
                )
            } else {
                "默认黑屏 · 背光已关闭".to_owned()
            }
        },
    );

    let on_brightness = {
        let brightness = brightness.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            if let Ok(value) = input.value().parse::<u16>() {
                brightness.set(value.max(1));
            }
        })
    };
    let display_on = {
        let state = state.clone();
        let csrf = csrf.clone();
        let brightness = *brightness;
        Callback::from(move |_| {
            dispatch_control(
                state.clone(),
                ControlArea::Display,
                DISPLAY_ENDPOINT,
                csrf.clone(),
                DisplayRequest {
                    enabled: true,
                    brightness: Some(brightness),
                },
                "背光已开启".to_owned(),
            )
        })
    };
    let display_off = {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| {
            dispatch_control(
                state.clone(),
                ControlArea::Display,
                DISPLAY_ENDPOINT,
                csrf.clone(),
                DisplayRequest {
                    enabled: false,
                    brightness: None,
                },
                "背光已关闭".to_owned(),
            )
        })
    };

    html! {
        if display.is_some() {
            <SectionCard title_id="display-controls-title" busy={Some(busy)}>
                <PageHeader title_id="display-controls-title" eyebrow="QUICK CONTROL" title="设备快捷控制">
                    <span class={SECTION_META}>{"仅限管理 LAN · 同源令牌保护"}</span>
                </PageHeader>
                <FeedbackState
                    message={AttrValue::from(state.display_notice.clone().unwrap_or_else(|| "等待操作".to_owned()))}
                    hidden={state.display_notice.is_none()}
                />
                <article class={INNER_CARD} aria-labelledby="display-control-title">
                    <div class={CONTROL_TITLE}><h3 id="display-control-title" class={CONTROL_HEADING}>{"LCD 背光"}</h3><span class={CONTROL_META}>{display_label}</span></div>
                    <label class={RANGE_LABEL} for="brightness"><span>{"点亮亮度"}</span><strong>{*brightness}</strong></label>
                    <input class={RANGE} id="brightness" type="range" min="1" max={max_brightness.to_string()} value={(*brightness).min(max_brightness).to_string()} oninput={on_brightness} disabled={busy} />
                    <div class={BUTTON_ROW}><button class={BUTTON_PRIMARY} type="button" onclick={display_on} disabled={busy}>{"点亮"}</button><button class={BUTTON} type="button" onclick={display_off} disabled={busy}>{"黑屏"}</button></div>
                    <small class={HELP_TEXT}>{"黑屏会将 PWM 亮度设为 0；面板 5V 是共享电源，无法单独物理断开。"}</small>
                </article>
            </SectionCard>
        }
    }
}

pub(crate) fn render_issues(snapshot: &StatusSnapshot) -> Html {
    let issues: Vec<&str> = [
        snapshot.router.issue.as_ref(),
        snapshot.proxy.issue.as_ref(),
        snapshot.tailscale.issue.as_ref(),
        snapshot.system.issue.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(|issue| issue.message.as_str())
    .collect();
    if issues.is_empty() {
        Html::default()
    } else {
        html! {
            <section class={WARNINGS} aria-labelledby="issues-title">
                <div>
                    <strong id="issues-title">{"状态提示"}</strong>
                    <ul class={WARNINGS_LIST}>{for issues.into_iter().map(|issue| html! { <li>{issue}</li> })}</ul>
                </div>
            </section>
        }
    }
}

pub(crate) fn status_card(
    title: &'static str,
    icon: &'static str,
    status: (&'static str, Tone),
    rows: Vec<(&'static str, String)>,
    accent: &'static str,
) -> Html {
    let (card_accent, icon_accent) = match accent {
        "wan" => (ACCENT_WAN, ICON_WAN),
        "proxy" => (ACCENT_PROXY, ICON_PROXY),
        _ => (ACCENT_SYSTEM, ICON_SYSTEM),
    };
    html! {
        <article class={classes!(STATUS_CARD, card_accent)}>
            <div class={STATUS_CARD_BODY}>
                <div class={STATUS_CARD_HEAD}>
                    <span class={classes!(STATUS_ICON, icon_accent)} aria-hidden="true">{icon}</span>
                    <h2 class={STATUS_TITLE}>{title}</h2>
                    <StatusBadge label={status.0} tone={classes!(status.1.class())} />
                </div>
                <dl class={METRIC_LIST}>{for rows.into_iter().map(|(label, value)| html! { <div class={METRIC}><dt class={METRIC_LABEL}>{label}</dt><dd class={METRIC_VALUE} title={value.clone()}>{value}</dd></div> })}</dl>
            </div>
        </article>
    }
}

pub(crate) fn component_card_status<T>(component: &Component<T>) -> (&'static str, Tone) {
    match component.state {
        ComponentState::Available => ("正常", Tone::Good),
        ComponentState::Degraded => ("降级", Tone::Warn),
        ComponentState::Unavailable => ("不可用", Tone::Bad),
    }
}

pub(crate) fn tailscale_mode_label(mode: TailscaleMode) -> &'static str {
    match mode {
        TailscaleMode::Disabled => "已停用",
        TailscaleMode::RouterOnly => "RouterOnly（仅路由器）",
        TailscaleMode::LanSubnetAccess => "LAN Access",
    }
}

pub(crate) fn tailscale_error_label(category: TailscaleErrorCategory) -> &'static str {
    match category {
        TailscaleErrorCategory::ProbeFailed => "状态探测失败",
        TailscaleErrorCategory::Conflict => "状态冲突",
        TailscaleErrorCategory::NotReady => "尚未就绪",
        TailscaleErrorCategory::OperationFailed => "操作失败",
    }
}

pub(crate) fn proxy_resource_label(state: ProxyResourceState) -> &'static str {
    match state {
        ProxyResourceState::Ready => "就绪",
        ProxyResourceState::Absent => "已停止",
        ProxyResourceState::NotReady => "未就绪",
        ProxyResourceState::Unknown => "未知",
    }
}

pub(crate) fn mihomo_core_status_label(status: &hyz_things::domain::status::ProxyStatus) -> String {
    match (
        status.mihomo.configured_required,
        status.mihomo.process,
        status.mihomo.runtime_config,
        status.mihomo.mixed_port,
    ) {
        (
            Some(true),
            ProxyResourceState::Ready,
            ProxyResourceState::Ready,
            ProxyResourceState::Ready,
        ) => "运行中".to_owned(),
        (
            Some(false),
            ProxyResourceState::Absent,
            ProxyResourceState::Absent,
            ProxyResourceState::Absent,
        ) => "已停止".to_owned(),
        (None, _, _, _) | (_, ProxyResourceState::Unknown, _, _) => "未知".to_owned(),
        _ => "已降级 · 未就绪".to_owned(),
    }
}

pub(crate) fn lan_tun_status_label(status: &hyz_things::domain::status::ProxyStatus) -> String {
    match (status.lan_tun.desired, status.lan_tun.effective) {
        (Some(true), LanTunEffective::Ready) => "已启用".to_owned(),
        (Some(true), LanTunEffective::OrdinaryNat) => "已降级 · 普通 NAT".to_owned(),
        (Some(true), LanTunEffective::NotConfirmed) => "已降级 · 未确认".to_owned(),
        (Some(false), LanTunEffective::OrdinaryNat) => "已关闭 · 普通 NAT".to_owned(),
        (Some(false), LanTunEffective::Ready) => "未知 · 状态冲突".to_owned(),
        (Some(false), LanTunEffective::NotConfirmed) | (None, _) => "未知 · 未确认".to_owned(),
    }
}

pub(crate) fn local_system_proxy_status_label(status: &ProxyStatus) -> String {
    match (
        status.local_system_proxy.desired,
        status.local_system_proxy.effective,
    ) {
        (Some(true), LocalSystemProxyEffective::Ready) => "已启用".to_owned(),
        (Some(true), LocalSystemProxyEffective::NotConfirmed)
        | (Some(true), LocalSystemProxyEffective::Disabled) => "已降级 · 未确认".to_owned(),
        (Some(false), LocalSystemProxyEffective::Disabled) => "已关闭".to_owned(),
        (Some(false), _) => "未知 · 状态冲突".to_owned(),
        (None, _) => "未知 · 未确认".to_owned(),
    }
}

pub(crate) fn tailscale_status_detail(status: &TailscaleStatus) -> String {
    let mode = status
        .effective_mode
        .map(tailscale_mode_label)
        .unwrap_or("未知");
    let connection = match status.connection.kind {
        TailscaleConnectionType::Direct => "Direct".to_owned(),
        TailscaleConnectionType::PeerRelay => "Peer relay".to_owned(),
        TailscaleConnectionType::Derp => status
            .connection
            .derp_region
            .as_deref()
            .map_or_else(|| "DERP".to_owned(), |region| format!("DERP {region}")),
        TailscaleConnectionType::Unknown => "连接未知".to_owned(),
    };
    format!("{mode} · {connection}")
}

pub(crate) fn wan_traffic(
    component: &Component<hyz_things::domain::status::SystemStats>,
) -> Option<(u64, u64)> {
    component.data.as_ref().and_then(|stats| {
        let ((rx_bytes, tx_bytes), found) = stats.interfaces.iter().fold(
            ((0_u64, 0_u64), false),
            |((rx_total, tx_total), found), interface| {
                if matches!(interface.name.as_str(), "eth0" | "wlan0") {
                    (
                        (
                            rx_total.saturating_add(interface.rx_bytes),
                            tx_total.saturating_add(interface.tx_bytes),
                        ),
                        true,
                    )
                } else {
                    ((rx_total, tx_total), found)
                }
            },
        );
        found.then_some((rx_bytes, tx_bytes))
    })
}

pub(crate) fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    if days > 0 {
        format!("{days} 天 {hours} 小时")
    } else if hours > 0 {
        format!("{hours} 小时 {minutes} 分")
    } else {
        format!("{minutes} 分钟")
    }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(crate) fn format_bool(value: bool) -> String {
    if value { "是" } else { "否" }.to_owned()
}

pub(crate) fn missing() -> String {
    MISSING.to_owned()
}

pub(crate) fn render_system(
    state: &UseReducerHandle<AppState>,
    brightness: UseStateHandle<u16>,
) -> Html {
    html! {
        <>
            if let Some(snapshot) = &state.snapshot {
                {render_dashboard(snapshot)}
                {render_issues(snapshot)}
            }
            {render_display_control(state, brightness)}
        </>
    }
}
