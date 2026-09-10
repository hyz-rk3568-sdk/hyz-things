use super::*;

pub(super) fn render_proxy_control(state: &UseReducerHandle<AppState>) -> Html {
    let Some(bootstrap) = state.panel.as_ref() else {
        return Html::default();
    };
    let csrf = bootstrap.csrf_token.clone();
    let proxy = state
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.proxy.data.as_ref());
    let lan_desired = proxy.and_then(|status| status.lan_tun.desired);
    let local_system_proxy_desired = proxy.and_then(|status| status.local_system_proxy.desired);
    let lan_status = proxy.map_or_else(|| "未知".to_owned(), lan_tun_status_label);
    let local_system_proxy_status = proxy.map_or_else(
        || "未知 · 未确认".to_owned(),
        local_system_proxy_status_label,
    );
    let mihomo_status = proxy.map_or_else(|| "未知".to_owned(), mihomo_core_status_label);
    let selected_node = bootstrap
        .panel
        .proxy_groups
        .data
        .as_ref()
        .and_then(|groups| groups.iter().find_map(|group| group.selected.as_deref()))
        .unwrap_or(MISSING);
    let toggle_lan = {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let enabled = input.checked();
            dispatch_control(
                state.clone(),
                ControlArea::LanTun,
                PROXY_LAN_TUN_ENDPOINT,
                csrf.clone(),
                ProxyFeatureRequestDto { enabled },
                if enabled {
                    "LAN 透明代理已启用"
                } else {
                    "LAN 透明代理已关闭，普通 NAT 保持可用"
                }
                .to_owned(),
            );
        })
    };
    let toggle_local_system_proxy = {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let enabled = input.checked();
            dispatch_control(
                state.clone(),
                ControlArea::LocalSystemProxy,
                PROXY_LOCAL_SYSTEM_ENDPOINT,
                csrf.clone(),
                ProxyFeatureRequestDto { enabled },
                if enabled {
                    "本机系统代理已启用"
                } else {
                    "本机系统代理已关闭，普通本机 HTTP/HTTPS 连接不使用该显式代理"
                }
                .to_owned(),
            );
        })
    };

    html! {
        <section class={classes!(SECTION, "gap-6")} aria-labelledby="proxy-controls-title">
            <div class={SECTION_HEAD}>
                <div><p class={EYEBROW}>{"PROXY"}</p><h2 id="proxy-controls-title" class={SECTION_TITLE}>{"代理设置"}</h2></div>
                <span class={SECTION_META}>{"两个数据面独立切换，共享 Mihomo core"}</span>
            </div>
            <article class={INNER_CARD} aria-labelledby="proxy-features-title">
                <div class={CONTROL_TITLE}>
                    <h3 id="proxy-features-title" class={CONTROL_HEADING}>{"代理能力"}</h3>
                    <span class={CONTROL_META}>{format!("Mihomo core：{mihomo_status}")}</span>
                </div>
                <div class="grid gap-3">
                    <label class="flex min-w-0 items-center justify-between gap-4 rounded-box border border-base-content/10 bg-base-200/40 p-4">
                        <span class="grid min-w-0 gap-1">
                            <strong class={CONTROL_HEADING}>{"LAN 透明代理"}</strong>
                            <small class={HELP_TEXT}>{"通过 Mihomo TUN 接管来自 192.168.8.0/24 的下游流量；关闭后使用普通 NAT。"}</small>
                            <span class={CONTROL_META}>{lan_status}</span>
                        </span>
                        <input class="toggle toggle-primary shrink-0" type="checkbox" role="switch" aria-label="LAN 透明代理" checked={lan_desired == Some(true)} onchange={toggle_lan} disabled={state.lan_tun_busy || lan_desired.is_none()} />
                    </label>
                    if let Some(notice) = &state.lan_tun_notice {
                        <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
                    }
                    <label class="flex min-w-0 items-center justify-between gap-4 rounded-box border border-base-content/10 bg-base-200/40 p-4">
                        <span class="grid min-w-0 gap-1">
                            <strong class={CONTROL_HEADING}>{"本机系统代理"}</strong>
                            <small class={HELP_TEXT}>{"为本机 HTTP/HTTPS 显式代理；使用 Mihomo 127.0.0.1:7890，不接管所有本机流量。"}</small>
                            <span class={CONTROL_META}>{local_system_proxy_status}</span>
                        </span>
                        <input class="toggle toggle-secondary shrink-0" type="checkbox" role="switch" aria-label="本机系统代理" checked={local_system_proxy_desired == Some(true)} onchange={toggle_local_system_proxy} disabled={state.local_system_proxy_busy || local_system_proxy_desired.is_none()} />
                    </label>
                    if let Some(notice) = &state.local_system_proxy_notice {
                        <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
                    }
                </div>
                <div class={SUMMARY}><span>{"当前代理节点"}</span><strong>{selected_node}</strong></div>
                <small class={HELP_TEXT}>{"停用后保留订阅配置；普通 NAT 在路由启用时保持可用；浏览器不能直连 Mihomo Controller。"}</small>
            </article>
            if let Some(notice) = &state.node_notice {
                <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
            }
            <div class={PROXY_GROUPS} aria-busy={state.node_busy.to_string()}>
                {render_proxy_groups(&bootstrap.panel.proxy_groups, state, &csrf, state.node_busy)}
            </div>
        </section>
    }
}

pub(super) fn render_proxy_groups_read_only(component: &Component<Vec<ProxyGroup>>) -> Html {
    let Some(groups) = component.data.as_ref() else {
        return Html::default();
    };
    if groups.is_empty() {
        return Html::default();
    }
    html! {
        <section class={SECTION} aria-labelledby="proxy-readonly-title">
            <div class={SECTION_HEAD}>
                <div><p class={EYEBROW}>{"PROXY STATUS"}</p><h2 id="proxy-readonly-title" class={SECTION_TITLE}>{"当前代理与延迟"}</h2></div>
                <span class={SECTION_META}>{"只读 · 修改需管理员登录"}</span>
            </div>
            <div class={PROXY_GROUPS}>
                {for groups.iter().map(|group| {
                    let selected = group.selected.as_deref().unwrap_or(MISSING);
                    html! {
                        <article class={PROXY_GROUP} key={group.name.clone()}>
                            <div class={PROXY_NAME_WRAP}>
                                <div><h3 class={PROXY_NAME}>{&group.name}</h3><small class={HELP_TEXT}>{format!("当前选择 · {selected}")}</small></div>
                                <span class={PROXY_KIND}>{group_kind_label(group)}</span>
                            </div>
                            <dl class={METRIC_LIST}>
                                {for group.options.iter().map(|option| {
                                    let state = option.delay_ms.map_or_else(
                                        || if option.alive == Some(false) { "超时".to_owned() } else { "未测速".to_owned() },
                                        |delay| format!("{delay} ms"),
                                    );
                                    let name = option.region.as_ref().map_or_else(
                                        || option.name.clone(),
                                        |region| format!("{} · {region}", option.name),
                                    );
                                    html! {
                                        <div class={METRIC} key={option.name.clone()}>
                                            <dt class={METRIC_LABEL}>{name}</dt>
                                            <dd class={METRIC_VALUE}>{if group.selected.as_deref() == Some(option.name.as_str()) { format!("当前 · {state}") } else { state }}</dd>
                                        </div>
                                    }
                                })}
                            </dl>
                        </article>
                    }
                })}
            </div>
        </section>
    }
}

pub(super) fn render_proxy_groups(
    component: &Component<Vec<ProxyGroup>>,
    state: &UseReducerHandle<AppState>,
    csrf: &str,
    busy: bool,
) -> Html {
    let Some(groups) = component.data.as_ref() else {
        return Html::default();
    };
    if groups.is_empty() {
        return Html::default();
    }
    let refresh_delays = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| dispatch_delay_refresh(state.clone(), csrf.clone()))
    };
    html! {
        <>
            <div class={PROXY_TOOLBAR}>
                <span>{"进入页面时自动测速一次"}</span>
                <button class={BUTTON} type="button" onclick={refresh_delays} disabled={busy}>{if busy { "测速中…" } else { "重新测速全部节点" }}</button>
            </div>
            {for groups.iter().map(|group| {
                let group_name = group.name.clone();
                let selection_state = state.clone();
                let selection_csrf = csrf.to_owned();
                let on_selection = Callback::from(move |event: Event| {
                    let select: HtmlSelectElement = event.target_unchecked_into();
                    dispatch_control(
                        selection_state.clone(),
                        ControlArea::Nodes,
                        PROXY_SELECTION_ENDPOINT,
                        selection_csrf.clone(),
                        ProxySelectionRequest { group: group_name.clone(), proxy: select.value() },
                        "代理节点已切换".to_owned(),
                    );
                });
                let selected = group.selected.clone();
                html! {
                    <article class={PROXY_GROUP} key={group.name.clone()}>
                        <div class={PROXY_NAME_WRAP}><h3 class={PROXY_NAME}>{&group.name}</h3><span class={PROXY_KIND}>{group_kind_label(group)}</span></div>
                        <select class={SELECT} value={selected.clone().unwrap_or_default()} onchange={on_selection} disabled={busy || !group.selectable} aria-label={format!("{} 节点", group.name)}>
                            {for group.options.iter().map(|option| {
                                let delay = option
                                    .delay_ms
                                    .map(|delay| format!("{delay} ms"))
                                    .or_else(|| (option.alive == Some(false)).then(|| "超时".to_owned()));
                                let details = [option.region.clone(), delay].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                                let label = if details.is_empty() { option.name.clone() } else { format!("{} · {details}", option.name) };
                                html! { <option key={option.name.clone()} value={option.name.clone()} selected={group.selected.as_deref() == Some(option.name.as_str())}>{label}</option> }
                            })}
                        </select>
                    </article>
                }
            })}
        </>
    }
}

pub(super) fn group_kind_label(group: &ProxyGroup) -> &'static str {
    use hyz_things::domain::panel::ProxyGroupKind;
    match group.kind {
        ProxyGroupKind::Selector => "手动选择",
        ProxyGroupKind::UrlTest => "自动测速",
        ProxyGroupKind::Fallback => "故障转移",
        ProxyGroupKind::LoadBalance => "负载均衡",
        ProxyGroupKind::Relay => "链式代理",
        ProxyGroupKind::Other => "代理组",
    }
}
