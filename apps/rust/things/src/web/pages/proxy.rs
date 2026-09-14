use super::*;

const PRIMARY_PROXY_GROUP: &str = "HYZ-PROXY";

fn current_proxy_summary(groups: &[ProxyGroup]) -> String {
    groups
        .iter()
        .find(|group| group.name == PRIMARY_PROXY_GROUP)
        .or_else(|| groups.iter().find(|group| group.selectable))
        .and_then(|group| group.selected.as_deref())
        .unwrap_or(MISSING)
        .to_owned()
}

fn dispatch_proxy_selection(
    state: UseReducerHandle<AppState>,
    csrf: String,
    group_name: String,
    proxy: String,
    mut groups: Vec<ProxyGroup>,
) {
    state.dispatch(Action::ControlStarted(ControlArea::Nodes));
    spawn_local(async move {
        let result = post_json(
            PROXY_SELECTION_ENDPOINT,
            &csrf,
            &ProxySelectionRequest {
                group: group_name.clone(),
                proxy: proxy.clone(),
            },
            "代理节点切换",
        )
        .await;
        match result {
            Ok(_) => {
                if let Some(group) = groups.iter_mut().find(|group| group.name == group_name) {
                    group.selected = Some(proxy);
                }
                state.dispatch(Action::ProxyDelaysFinished(Ok(groups)));
                state.dispatch(Action::ControlFinished(
                    ControlArea::Nodes,
                    Ok("代理节点已切换".to_owned()),
                ));
            }
            Err(error) => {
                state.dispatch(Action::ControlFinished(ControlArea::Nodes, Err(error)));
            }
        }
    });
}

#[derive(Properties, PartialEq)]
struct SubscriptionSettingsProps {
    state: UseReducerHandle<AppState>,
    csrf: String,
}

#[function_component(SubscriptionSettings)]
fn subscription_settings(props: &SubscriptionSettingsProps) -> Html {
    let subscription_url = use_node_ref();
    let busy = props.state.subscription_busy;
    let save_subscription = {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let input = subscription_url.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let Some(input) = input.cast::<HtmlInputElement>() else {
                return;
            };
            let url = input.value();
            input.set_value("");
            dispatch_subscription_source(state.clone(), csrf.clone(), url);
        })
    };
    let refresh_subscription = {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |_| dispatch_subscription_refresh(state.clone(), csrf.clone()))
    };
    let retry_subscription = {
        let state = props.state.clone();
        Callback::from(move |_| dispatch_subscription_refresh_read(state.clone()))
    };

    html! {
        <article class={INNER_CARD} aria-labelledby="proxy-subscription-title">
            <div class={CONTROL_TITLE}>
                <h3 id="proxy-subscription-title" class={CONTROL_HEADING}>{"代理订阅"}</h3>
                <span class={CONTROL_META}>{format!("来源只写 · {}", props.state.subscription_meta.status_text())}</span>
            </div>
            if let Some(subscription) = &props.state.subscription {
                <div class={SUMMARY}>
                    <span>{if subscription.configured { "已配置" } else { "未配置" }}</span>
                    <strong class={subscription_tone(subscription.state)}>{subscription_state_label(subscription.state)}</strong>
                </div>
            }
            if let Some(notice) = &props.state.subscription_notice {
                <FeedbackState message={AttrValue::from(notice.clone())} />
            }
            if let Some(error) = &props.state.subscription_meta.error {
                <div class="grid gap-2">
                    <ErrorState message={format!("订阅状态读取失败，其他代理控制仍可使用：{error}")} />
                    <div class={BUTTON_ROW}><button class={BUTTON} type="button" onclick={retry_subscription} disabled={props.state.subscription_meta.loading}>{"重试订阅状态"}</button></div>
                </div>
            }
            <form class={FORM_GRID_COMPACT} onsubmit={save_subscription} autocomplete="off">
                <label class={FIELD}>
                    <span class={FIELD_LABEL}>{"订阅 URL"}</span>
                    <input class={INPUT} ref={subscription_url} type="url" required=true placeholder="https://…" autocomplete="off" autocapitalize="none" spellcheck="false" aria-describedby="proxy-subscription-secret-note" />
                </label>
                <small id="proxy-subscription-secret-note" class={HELP_TEXT}>{"已保存的 URL 永不回显；输入只用于本次提交。"}</small>
                <div class={FORM_ACTIONS}>
                    <button class={BUTTON_PRIMARY} type="submit" disabled={busy || props.csrf.is_empty()}>{"保存并立即更新"}</button>
                    <button class={BUTTON} type="button" onclick={refresh_subscription} disabled={busy || !props.state.subscription.as_ref().is_some_and(|value| value.configured)}>{"手动刷新"}</button>
                </div>
            </form>
        </article>
    }
}

pub(crate) fn render_proxy_control(state: &UseReducerHandle<AppState>) -> Html {
    let Some(bootstrap) = state.panel.as_ref() else {
        return html! {
            <SectionCard title_id="proxy-controls-title">
                <PageHeader title_id="proxy-controls-title" eyebrow="PROXY" title="代理设置" />
                <div class={SETTINGS_EMPTY} role="status">{"正在读取代理控制面…"}</div>
            </SectionCard>
        };
    };
    let csrf = bootstrap.csrf_token.clone();
    let proxy = state.snapshot.as_ref().and_then(|snapshot| snapshot.proxy.data.as_ref());
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
        .as_deref()
        .map(current_proxy_summary)
        .unwrap_or_else(|| MISSING.to_owned());
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
                if enabled { "LAN 透明代理已启用" } else { "LAN 透明代理已关闭，普通 NAT 保持可用" }.to_owned(),
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
                if enabled { "本机系统代理已启用" } else { "本机系统代理已关闭，普通本机 HTTP/HTTPS 连接不使用该显式代理" }.to_owned(),
            );
        })
    };
    let retry_status = {
        let state = state.clone();
        Callback::from(move |_| dispatch_status_refresh(state.clone()))
    };
    let retry_panel = {
        let state = state.clone();
        Callback::from(move |_| dispatch_panel_refresh(state.clone()))
    };

    html! {
        <SectionCard title_id="proxy-controls-title" extra_class={classes!("gap-6")}>
            <PageHeader title_id="proxy-controls-title" eyebrow="PROXY" title="代理设置">
                <span class={SECTION_META}>{format!("状态 {} · 控制面 {}", state.status_meta.status_text(), state.panel_meta.status_text())}</span>
            </PageHeader>
            if let Some(error) = &state.status_meta.error {
                <div class="grid gap-2"><ErrorState message={format!("代理运行状态刷新失败，保留最近数据：{error}")} /><div class={BUTTON_ROW}><button class={BUTTON} type="button" onclick={retry_status} disabled={state.status_meta.loading}>{"重试运行状态"}</button></div></div>
            }
            if let Some(error) = &state.panel_meta.error {
                <div class="grid gap-2"><ErrorState message={format!("代理控制面刷新失败，保留最近数据：{error}")} /><div class={BUTTON_ROW}><button class={BUTTON} type="button" onclick={retry_panel} disabled={state.panel_meta.loading}>{"重试控制面"}</button></div></div>
            }
            <article class={INNER_CARD} aria-labelledby="proxy-features-title">
                <div class={CONTROL_TITLE}>
                    <h3 id="proxy-features-title" class={CONTROL_HEADING}>{"代理能力"}</h3>
                    <span class={CONTROL_META}>{format!("Mihomo core：{mihomo_status}")}</span>
                </div>
                <div class="grid gap-3">
                    <label class="flex min-w-0 items-center justify-between gap-4 rounded-box border border-base-content/10 bg-base-200/40 p-4">
                        <span class="grid min-w-0 gap-1"><strong class={CONTROL_HEADING}>{"LAN 透明代理"}</strong><small class={HELP_TEXT}>{"通过 Mihomo TUN 接管来自 192.168.8.0/24 的下游流量；关闭后使用普通 NAT。"}</small><span class={CONTROL_META}>{lan_status}</span></span>
                        <input class="toggle toggle-primary shrink-0" type="checkbox" role="switch" aria-label="LAN 透明代理" checked={lan_desired == Some(true)} onchange={toggle_lan} disabled={state.lan_tun_busy || lan_desired.is_none()} />
                    </label>
                    if let Some(notice) = &state.lan_tun_notice { <FeedbackState message={AttrValue::from(notice.clone())} /> }
                    <label class="flex min-w-0 items-center justify-between gap-4 rounded-box border border-base-content/10 bg-base-200/40 p-4">
                        <span class="grid min-w-0 gap-1"><strong class={CONTROL_HEADING}>{"本机系统代理"}</strong><small class={HELP_TEXT}>{"为本机 HTTP/HTTPS 显式代理；使用 Mihomo 127.0.0.1:7890，不接管所有本机流量。"}</small><span class={CONTROL_META}>{local_system_proxy_status}</span></span>
                        <input class="toggle toggle-secondary shrink-0" type="checkbox" role="switch" aria-label="本机系统代理" checked={local_system_proxy_desired == Some(true)} onchange={toggle_local_system_proxy} disabled={state.local_system_proxy_busy || local_system_proxy_desired.is_none()} />
                    </label>
                    if let Some(notice) = &state.local_system_proxy_notice { <FeedbackState message={AttrValue::from(notice.clone())} /> }
                </div>
                <div class={SUMMARY}><span>{"当前代理节点"}</span><strong>{selected_node}</strong></div>
                <small class={HELP_TEXT}>{"停用后保留订阅配置；普通 NAT 在路由启用时保持可用；浏览器不能直连 Mihomo Controller。"}</small>
            </article>
            <SubscriptionSettings state={state.clone()} csrf={csrf.clone()} />
            if let Some(notice) = &state.node_notice { <FeedbackState message={AttrValue::from(notice.clone())} /> }
            <div class={PROXY_GROUPS} aria-busy={state.node_busy.to_string()}>{render_proxy_groups(&bootstrap.panel.proxy_groups, state, &csrf, state.node_busy)}</div>
        </SectionCard>
    }
}

pub(crate) fn render_proxy_groups_read_only(component: &Component<Vec<ProxyGroup>>) -> Html {
    let Some(groups) = component.data.as_ref() else { return Html::default(); };
    if groups.is_empty() { return Html::default(); }
    html! {
        <SectionCard title_id="proxy-readonly-title">
            <PageHeader title_id="proxy-readonly-title" eyebrow="PROXY STATUS" title="当前代理与延迟"><span class={SECTION_META}>{"只读 · 修改需管理员登录"}</span></PageHeader>
            <div class={PROXY_GROUPS}>
                {for groups.iter().map(|group| {
                    let selected = group.selected.as_deref().unwrap_or(MISSING);
                    html! {
                        <article class={PROXY_GROUP} key={group.name.clone()}>
                            <div class={PROXY_NAME_WRAP}><div><h3 class={PROXY_NAME}>{&group.name}</h3><small class={HELP_TEXT}>{format!("当前选择 · {selected}")}</small></div><span class={PROXY_KIND}>{group_kind_label(group)}</span></div>
                            <dl class={METRIC_LIST}>
                                {for group.options.iter().map(|option| {
                                    let delay = option.delay_ms.map_or_else(|| if option.alive == Some(false) { "超时".to_owned() } else { "未测速".to_owned() }, |delay| format!("{delay} ms"));
                                    let name = option.region.as_ref().map_or_else(|| option.name.clone(), |region| format!("{} · {region}", option.name));
                                    html! { <div class={METRIC} key={option.name.clone()}><dt class={METRIC_LABEL}>{name}</dt><dd class={METRIC_VALUE}>{if group.selected.as_deref() == Some(option.name.as_str()) { format!("当前 · {delay}") } else { delay }}</dd></div> }
                                })}
                            </dl>
                        </article>
                    }
                })}
            </div>
        </SectionCard>
    }
}

pub(crate) fn render_proxy_groups(
    component: &Component<Vec<ProxyGroup>>,
    state: &UseReducerHandle<AppState>,
    csrf: &str,
    busy: bool,
) -> Html {
    let Some(groups) = component.data.as_ref() else { return Html::default(); };
    if groups.is_empty() { return Html::default(); }
    let refresh_delays = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| dispatch_delay_refresh(state.clone(), csrf.clone()))
    };
    html! {
        <>
            <div class={PROXY_TOOLBAR}><span>{"节点延迟仅在手动操作时测速"}</span><button class={BUTTON} type="button" onclick={refresh_delays} disabled={busy}>{if busy { "测速中…" } else { "测速全部节点" }}</button></div>
            {for groups.iter().map(|group| {
                let group_name = group.name.clone();
                let selection_state = state.clone();
                let selection_csrf = csrf.to_owned();
                let selection_groups = groups.to_vec();
                let on_selection = Callback::from(move |event: Event| {
                    let select: HtmlSelectElement = event.target_unchecked_into();
                    dispatch_proxy_selection(selection_state.clone(), selection_csrf.clone(), group_name.clone(), select.value(), selection_groups.clone());
                });
                html! {
                    <article class={PROXY_GROUP} key={group.name.clone()}>
                        <div class={PROXY_NAME_WRAP}><h3 class={PROXY_NAME}>{&group.name}</h3><span class={PROXY_KIND}>{group_kind_label(group)}</span></div>
                        <select class={SELECT} value={group.selected.clone().unwrap_or_default()} onchange={on_selection} disabled={busy || !group.selectable} aria-label={format!("{} 节点", group.name)}>
                            {for group.options.iter().map(|option| {
                                let delay = option.delay_ms.map(|delay| format!("{delay} ms")).or_else(|| (option.alive == Some(false)).then(|| "超时".to_owned()));
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

pub(crate) fn group_kind_label(group: &ProxyGroup) -> &'static str {
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
