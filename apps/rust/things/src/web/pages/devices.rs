use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, PartialEq)]
struct DeviceView {
    mac: String,
    name: String,
    lease_address: Option<String>,
    associated: Option<bool>,
    policy: DevicePolicyDto,
    configured: bool,
}

#[derive(Properties, PartialEq)]
pub(crate) struct DevicesPageProps {
    pub(crate) state: UseReducerHandle<AppState>,
    pub(crate) route: PortalRoute,
    pub(crate) route_state: UseStateHandle<PortalRoute>,
}

fn display_name_for(snapshot: &DevicePolicySnapshotDto, mac: &str) -> String {
    let configured = snapshot
        .config
        .entries
        .iter()
        .find(|entry| entry.mac.eq_ignore_ascii_case(mac));
    if let Some(label) = configured
        .map(|entry| entry.label.trim())
        .filter(|label| !label.is_empty())
    {
        return label.to_owned();
    }
    if let Some(hostname) = snapshot
        .clients
        .iter()
        .find(|client| client.mac.eq_ignore_ascii_case(mac))
        .and_then(|client| client.hostname.as_deref())
        .filter(|hostname| !hostname.is_empty())
    {
        return hostname.to_owned();
    }
    if let Some(name) = snapshot
        .activity
        .as_ref()
        .and_then(|activity| {
            activity
                .records
                .iter()
                .find(|record| record.mac.eq_ignore_ascii_case(mac))
        })
        .map(|record| record.device_name.as_str())
        .filter(|name| !name.is_empty() && !name.eq_ignore_ascii_case(mac))
    {
        return name.to_owned();
    }
    mac.to_owned()
}

fn merged_devices(snapshot: &DevicePolicySnapshotDto) -> Vec<DeviceView> {
    let mut devices = BTreeMap::<String, DeviceView>::new();
    for entry in &snapshot.config.entries {
        let mac = entry.mac.to_ascii_lowercase();
        devices.insert(
            mac.clone(),
            DeviceView {
                name: display_name_for(snapshot, &mac),
                mac,
                lease_address: None,
                associated: None,
                policy: entry.policy,
                configured: true,
            },
        );
    }
    for client in &snapshot.clients {
        let mac = client.mac.to_ascii_lowercase();
        let configured = snapshot
            .config
            .entries
            .iter()
            .any(|entry| entry.mac.eq_ignore_ascii_case(&mac));
        devices
            .entry(mac.clone())
            .and_modify(|device| {
                device.name = display_name_for(snapshot, &mac);
                device.lease_address = client.lease_address.clone();
                device.associated = Some(client.associated);
                device.policy = client.policy;
                device.configured = configured;
            })
            .or_insert_with(|| DeviceView {
                name: display_name_for(snapshot, &mac),
                mac,
                lease_address: client.lease_address.clone(),
                associated: Some(client.associated),
                policy: client.policy,
                configured,
            });
    }
    if let Some(activity) = &snapshot.activity {
        for record in &activity.records {
            let mac = record.mac.to_ascii_lowercase();
            devices.entry(mac.clone()).or_insert_with(|| DeviceView {
                name: display_name_for(snapshot, &mac),
                mac,
                lease_address: Some(record.source_address.clone()),
                associated: None,
                policy: DevicePolicyDto::Proxy,
                configured: false,
            });
        }
    }
    devices.into_values().collect()
}

fn policy_label(policy: DevicePolicyDto) -> &'static str {
    match policy {
        DevicePolicyDto::Direct => "直连",
        DevicePolicyDto::Proxy => "按规则分流",
    }
}

fn activity_for_device(snapshot: &DevicePolicySnapshotDto, mac: &str) -> Vec<ActivityRecordDto> {
    let mut seen = BTreeSet::new();
    let mut records = snapshot
        .activity
        .as_ref()
        .map(|activity| {
            activity
                .records
                .iter()
                .filter(|record| record.mac.eq_ignore_ascii_case(mac))
                .filter(|record| seen.insert(record.connection_id.clone()))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    records.sort_by_key(|record| std::cmp::Reverse(record.last_seen_unix_ms));
    records
}

fn traffic_summary(records: &[ActivityRecordDto]) -> (u64, u64) {
    records.iter().fold((0_u64, 0_u64), |(up, down), record| {
        (
            up.saturating_add(record.upload_bytes),
            down.saturating_add(record.download_bytes),
        )
    })
}

fn device_open_id(mac: &str) -> String {
    format!("device-open-{}", mac.replace(':', "-"))
}

fn restore_list_focus(scroll_y: f64, opener_id: String) {
    spawn_local(async move {
        TimeoutFuture::new(0).await;
        if let Some(window) = web_sys::window() {
            window.scroll_to_with_x_and_y(0.0, scroll_y);
            if !opener_id.is_empty() {
                if let Some(element) = window
                    .document()
                    .and_then(|document| document.get_element_by_id(&opener_id))
                    .and_then(|element| element.dyn_into::<HtmlElement>().ok())
                {
                    let _ = element.focus();
                }
            }
        }
    });
}

#[function_component(DevicesPage)]
pub(crate) fn devices_page(props: &DevicesPageProps) -> Html {
    let filter_input = use_state(|| props.route.device_filter.clone().unwrap_or_default());
    let manual_mac = use_node_ref();
    let manual_label = use_node_ref();
    let close_button = use_node_ref();
    let list_scroll_y = use_state(|| 0.0_f64);
    let opener_id = use_state(String::new);

    {
        let filter_input = filter_input.clone();
        let route_filter = props.route.device_filter.clone();
        use_effect_with(route_filter, move |filter| {
            filter_input.set(filter.clone().unwrap_or_default());
            || ()
        });
    }
    {
        let close_button = close_button.clone();
        let selected = props.route.device_id.clone();
        use_effect_with(selected, move |selected| {
            if selected.is_some() {
                reveal_and_focus_after_render(close_button.clone(), close_button.clone());
            }
            || ()
        });
    }
    {
        let route_state = props.route_state.clone();
        let selected = props.route.device_id.clone();
        let filter = props.route.device_filter.clone();
        let scroll = *list_scroll_y;
        let opener = (*opener_id).clone();
        use_effect_with(selected.clone(), move |selected| {
            let active = selected.is_some();
            let closure = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
                if active && is_escape_key(&event) {
                    event.prevent_default();
                    navigate_route(
                        &route_state,
                        PortalRoute::devices(None, filter.clone()),
                        false,
                    );
                    restore_list_focus(scroll, opener.clone());
                }
            });
            if active {
                if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                    let _ = document.add_event_listener_with_callback(
                        "keydown",
                        closure.as_ref().unchecked_ref(),
                    );
                }
            }
            move || {
                if active {
                    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                        let _ = document.remove_event_listener_with_callback(
                            "keydown",
                            closure.as_ref().unchecked_ref(),
                        );
                    }
                }
            }
        });
    }

    let retry = {
        let state = props.state.clone();
        Callback::from(move |_| dispatch_devices_refresh(state.clone()))
    };
    let on_filter_input = {
        let value = filter_input.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            value.set(input.value());
        })
    };
    let apply_filter = {
        let route_state = props.route_state.clone();
        let value = filter_input.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let value = value.trim().to_owned();
            navigate_route(
                &route_state,
                PortalRoute::devices(None, (!value.is_empty()).then_some(value)),
                false,
            );
        })
    };
    let clear_filter = {
        let route_state = props.route_state.clone();
        let value = filter_input.clone();
        Callback::from(move |_| {
            value.set(String::new());
            navigate_route(&route_state, PortalRoute::devices(None, None), false);
        })
    };

    let csrf = props
        .state
        .panel
        .as_ref()
        .map(|panel| panel.csrf_token.clone())
        .unwrap_or_default();
    let snapshot = props.state.device_policies.as_ref();
    let devices = snapshot.map(merged_devices).unwrap_or_default();
    let filter = props.route.device_filter.as_deref().unwrap_or("").to_lowercase();
    let visible_devices = devices
        .iter()
        .filter(|device| {
            filter.is_empty()
                || device.name.to_lowercase().contains(&filter)
                || device.mac.contains(&filter)
                || device
                    .lease_address
                    .as_deref()
                    .is_some_and(|address| address.to_lowercase().contains(&filter))
        })
        .cloned()
        .collect::<Vec<_>>();

    let add_manual = if let Some(snapshot) = snapshot {
        let state = props.state.clone();
        let csrf = csrf.clone();
        let mac_ref = manual_mac.clone();
        let label_ref = manual_label.clone();
        let generation = snapshot.config.generation;
        let configured = snapshot.config.entries.clone();
        Some(Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let (Some(mac), Some(label)) = (
                mac_ref.cast::<HtmlInputElement>(),
                label_ref.cast::<HtmlInputElement>(),
            ) else {
                return;
            };
            let canonical = mac.value().to_ascii_lowercase();
            let mut entries = configured.clone();
            entries.retain(|entry| !entry.mac.eq_ignore_ascii_case(&canonical));
            entries.push(DevicePolicyEntryDto {
                mac: canonical,
                label: label.value().trim().to_owned(),
                policy: DevicePolicyDto::Proxy,
            });
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, entries);
        }))
    } else {
        None
    };

    let selected_device = props
        .route
        .device_id
        .as_ref()
        .and_then(|selected| devices.iter().find(|device| &device.mac == selected))
        .cloned();
    let close_detail = {
        let route_state = props.route_state.clone();
        let filter = props.route.device_filter.clone();
        let scroll = *list_scroll_y;
        let opener = (*opener_id).clone();
        Callback::from(move |_| {
            navigate_route(
                &route_state,
                PortalRoute::devices(None, filter.clone()),
                false,
            );
            restore_list_focus(scroll, opener.clone());
        })
    };

    html! {
        <>
            <SectionCard title_id="devices-title">
                <PageHeader title_id="devices-title" eyebrow="LAN DEVICES" title="设备">
                    <span class={SECTION_META}>{props.state.device_meta.status_text()}</span>
                </PageHeader>
                if let Some(notice) = &props.state.device_notice {
                    <FeedbackState message={AttrValue::from(notice.clone())} />
                }
                if let Some(error) = &props.state.device_meta.error {
                    <div class="grid gap-2">
                        <ErrorState message={format!("设备状态刷新失败，当前显示上次成功数据：{error}")} />
                        <div class={BUTTON_ROW}><button class={BUTTON} type="button" onclick={retry.clone()} disabled={props.state.device_meta.loading}>{"重试设备状态"}</button></div>
                    </div>
                }
                <form class="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto_auto]" onsubmit={apply_filter} role="search">
                    <label class={FIELD}><span class={FIELD_LABEL}>{"筛选设备"}</span><input class={INPUT} value={(*filter_input).clone()} oninput={on_filter_input} placeholder="名称、MAC 或 IP" /></label>
                    <button class={BUTTON_PRIMARY} type="submit">{"应用筛选"}</button>
                    <button class={BUTTON} type="button" onclick={clear_filter}>{"清除筛选"}</button>
                </form>
                if snapshot.is_none() && props.state.device_meta.loading {
                    <LoadingState title_id="devices-loading-title" title="正在读取设备" />
                } else if snapshot.is_none() {
                    <EmptyState title_id="devices-empty-title" title="暂时无法读取设备" message="可重试设备状态；其他页面不受影响。" />
                } else if visible_devices.is_empty() {
                    <div class={SETTINGS_EMPTY} role="status">{if filter.is_empty() { "当前没有已发现或已配置设备" } else { "没有匹配的设备" }}</div>
                } else {
                    <div class="grid gap-3" role="list" aria-label="LAN 设备">
                        {for visible_devices.iter().map(|device| {
                            let route_state = props.route_state.clone();
                            let filter = props.route.device_filter.clone();
                            let mac = device.mac.clone();
                            let open_id = device_open_id(&mac);
                            let opener_id = opener_id.clone();
                            let list_scroll_y = list_scroll_y.clone();
                            let open_id_for_click = open_id.clone();
                            let open = Callback::from(move |_| {
                                let scroll_y = web_sys::window().and_then(|window| window.scroll_y().ok()).unwrap_or(0.0);
                                list_scroll_y.set(scroll_y);
                                opener_id.set(open_id_for_click.clone());
                                navigate_route(&route_state, PortalRoute::devices(Some(mac.clone()), filter.clone()), false);
                            });
                            let presence = match device.associated {
                                Some(true) => "在线",
                                Some(false) => "离线",
                                None => "当前未发现",
                            };
                            html! {
                                <article class={INNER_CARD} role="listitem" aria-label={format!("设备 {}", device.name)} key={device.mac.clone()}>
                                    <div class="flex min-w-0 flex-wrap items-start justify-between gap-3">
                                        <div class="min-w-0">
                                            <strong class="block truncate">{&device.name}</strong>
                                            <small class="block break-all font-mono text-base-content/65">{&device.mac}</small>
                                            <small class={HELP_TEXT}>{format!("{} · {} · {}", device.lease_address.as_deref().unwrap_or("无租约 IP"), presence, policy_label(device.policy))}</small>
                                        </div>
                                        <button id={open_id} class={BUTTON} type="button" onclick={open} aria-label={format!("打开设备 {}", device.name)}>{"查看详情"}</button>
                                    </div>
                                </article>
                            }
                        })}
                    </div>
                }
                if let (Some(snapshot), Some(add_manual)) = (snapshot, add_manual) {
                    <article class={INNER_CARD} aria-labelledby="manual-device-title">
                        <div class={CONTROL_TITLE}><h3 id="manual-device-title" class={CONTROL_HEADING}>{"手工添加设备"}</h3><span class={CONTROL_META}>{format!("配置代次 {}", snapshot.config.generation)}</span></div>
                        <form class={FORM_GRID_COMPACT} onsubmit={add_manual} autocomplete="off">
                            <label class={FIELD}><span class={FIELD_LABEL}>{"MAC"}</span><input class={INPUT} ref={manual_mac} required=true placeholder="02:00:00:00:00:01" maxlength="17" autocapitalize="none" spellcheck="false" /></label>
                            <label class={FIELD}><span class={FIELD_LABEL}>{"显示名"}</span><input class={INPUT} ref={manual_label} maxlength="32" /></label>
                            <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="submit" disabled={props.state.device_busy || csrf.is_empty()}>{"添加为按规则分流"}</button></div>
                        </form>
                    </article>
                }
                <small class={HELP_TEXT}>{"设备流量仅统计 Mihomo 可观测的 LAN 连接与有限历史；没有记录不代表设备没有访问互联网。手机随机 MAC 变化后会被识别为新设备。"}</small>
            </SectionCard>
            if let Some(selected_mac) = props.route.device_id.as_ref() {
                <DeviceDetail
                    key={selected_mac.clone()}
                    state={props.state.clone()}
                    snapshot={snapshot.cloned()}
                    device={selected_device}
                    selected_mac={selected_mac.clone()}
                    csrf={csrf.clone()}
                    close_button={close_button.clone()}
                    onclose={close_detail}
                />
            }
        </>
    }
}

#[derive(Properties, PartialEq)]
struct DeviceDetailProps {
    state: UseReducerHandle<AppState>,
    snapshot: Option<DevicePolicySnapshotDto>,
    device: Option<DeviceView>,
    selected_mac: String,
    csrf: String,
    close_button: NodeRef,
    onclose: Callback<MouseEvent>,
}

#[function_component(DeviceDetail)]
fn device_detail(props: &DeviceDetailProps) -> Html {
    let persisted = props.snapshot.as_ref().and_then(|snapshot| {
        snapshot
            .config
            .entries
            .iter()
            .find(|entry| entry.mac.eq_ignore_ascii_case(&props.selected_mac))
    });
    let initial_label = persisted
        .map(|entry| entry.label.clone())
        .filter(|label| !label.is_empty())
        .or_else(|| {
            props.snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .clients
                    .iter()
                    .find(|client| client.mac.eq_ignore_ascii_case(&props.selected_mac))
                    .and_then(|client| client.hostname.clone())
            })
        })
        .unwrap_or_default();
    let initial_policy = props
        .device
        .as_ref()
        .map(|device| device.policy)
        .or_else(|| persisted.map(|entry| entry.policy))
        .unwrap_or(DevicePolicyDto::Proxy);
    let label = use_state(|| initial_label);
    let policy = use_state(|| initial_policy);
    let on_label = {
        let label = label.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            label.set(input.value());
        })
    };
    let on_policy = {
        let policy = policy.clone();
        Callback::from(move |event: Event| {
            let select: HtmlSelectElement = event.target_unchecked_into();
            policy.set(if select.value() == "direct" {
                DevicePolicyDto::Direct
            } else {
                DevicePolicyDto::Proxy
            });
        })
    };
    let save = props.snapshot.as_ref().map(|snapshot| {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let mac = props.selected_mac.clone();
        let generation = snapshot.config.generation;
        let configured = snapshot.config.entries.clone();
        let label = label.clone();
        let policy = policy.clone();
        Callback::from(move |_| {
            let mut entries = configured.clone();
            entries.retain(|entry| !entry.mac.eq_ignore_ascii_case(&mac));
            let saved_label = label.trim().to_owned();
            if *policy == DevicePolicyDto::Direct || !saved_label.is_empty() {
                entries.push(DevicePolicyEntryDto {
                    mac: mac.clone(),
                    label: saved_label,
                    policy: *policy,
                });
            }
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, entries);
        })
    });
    let clear = props.snapshot.as_ref().and_then(|snapshot| {
        persisted?;
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let mac = props.selected_mac.clone();
        let generation = snapshot.config.generation;
        let configured = snapshot.config.entries.clone();
        Some(Callback::from(move |_| {
            let mut entries = configured.clone();
            entries.retain(|entry| !entry.mac.eq_ignore_ascii_case(&mac));
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, entries);
        }))
    });
    let records = props
        .snapshot
        .as_ref()
        .map(|snapshot| activity_for_device(snapshot, &props.selected_mac))
        .unwrap_or_default();
    let (upload, download) = traffic_summary(&records);
    let observed_at = props
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.activity.as_ref())
        .map(|activity| deployment_time_label(Some(activity.observed_at_unix_ms)))
        .unwrap_or_else(|| "未观测".to_owned());
    let runtime = props.snapshot.as_ref().map_or_else(
        || "状态未知".to_owned(),
        |snapshot| {
            if snapshot.effective {
                format!("当前观测策略 {} · 配置代次生效状态待确认", policy_label(initial_policy))
            } else {
                "策略已保存，等待启用 TUN".to_owned()
            }
        },
    );
    let title = props
        .device
        .as_ref()
        .map(|device| device.name.clone())
        .unwrap_or_else(|| props.selected_mac.clone());

    html! {
        <div class="fixed inset-0 z-50 bg-base-300/40 md:flex md:justify-end" data-swipe-ignore="true">
            <section class="h-full w-full overflow-y-auto bg-base-100 p-4 shadow-2xl md:w-[min(42rem,92vw)] md:p-6" role="dialog" aria-modal="true" aria-labelledby="device-detail-title">
                <div class="grid gap-5">
                    <div class="flex items-start justify-between gap-3">
                        <div class="min-w-0"><p class={EYEBROW}>{"DEVICE DETAIL"}</p><h2 id="device-detail-title" class={SECTION_TITLE}>{title}</h2><p class="break-all font-mono text-xs text-base-content/65">{&props.selected_mac}</p></div>
                        <button ref={props.close_button.clone()} class={BUTTON} type="button" onclick={props.onclose.clone()} aria-label="关闭设备详情">{"关闭"}</button>
                    </div>
                    if props.device.is_none() {
                        <div class={RISK_NOTE} role="status">{"当前未发现该设备；若它有已保存配置，配置仍保留。"}</div>
                    } else if let Some(device) = &props.device {
                        <div class={SUMMARY}><span>{match device.associated { Some(true) => "在线", Some(false) => "离线", None => "当前在线状态未知" }}</span><strong>{device.lease_address.as_deref().unwrap_or("无租约 IP")}</strong></div>
                    }
                    <article class={INNER_CARD} aria-labelledby="device-policy-detail-title">
                        <div class={CONTROL_TITLE}><h3 id="device-policy-detail-title" class={CONTROL_HEADING}>{"代理策略"}</h3><span class={CONTROL_META}>{runtime}</span></div>
                        if let Some(snapshot) = &props.snapshot {
                            <div class={FORM_GRID_COMPACT}>
                                <label class={FIELD}><span class={FIELD_LABEL}>{"显示名"}</span><input class={INPUT} value={(*label).clone()} oninput={on_label} maxlength="32" aria-label={format!("{} 显示名", props.selected_mac)} /></label>
                                <label class={FIELD}><span class={FIELD_LABEL}>{"路由策略"}</span><select class={SELECT} value={if *policy == DevicePolicyDto::Direct { "direct" } else { "proxy" }} onchange={on_policy} aria-label={format!("{} 代理策略", props.selected_mac)}><option value="proxy">{"按规则分流"}</option><option value="direct">{"直连"}</option></select></label>
                                <small class={HELP_TEXT}>{format!("当前配置代次 {}。保存成功只证明配置已持久化；接口未提供“该代次已应用”的确认字段。", snapshot.config.generation)}</small>
                                <div class={FORM_ACTIONS}>
                                    if let Some(save) = save { <button class={BUTTON_PRIMARY} type="button" onclick={save} disabled={props.state.device_busy || props.csrf.is_empty()}>{"保存设备设置"}</button> }
                                    if let Some(clear) = clear { <button class={BUTTON_GHOST} type="button" onclick={clear} disabled={props.state.device_busy || props.csrf.is_empty()}>{"清除名称和自定义策略"}</button> }
                                </div>
                            </div>
                        } else {
                            <div class={SETTINGS_EMPTY} role="status">{"设备策略暂不可用"}</div>
                        }
                    </article>
                    <article class={INNER_CARD} aria-labelledby="device-traffic-title">
                        <div class={CONTROL_TITLE}><h3 id="device-traffic-title" class={CONTROL_HEADING}>{"Mihomo 可观测流量"}</h3><span class={CONTROL_META}>{format!("数据时间 {observed_at}")}</span></div>
                        if records.is_empty() {
                            <div class={SETTINGS_EMPTY} role="status">{"尚未观测到 Mihomo LAN 流量；这不代表设备没有上网。"}</div>
                        } else {
                            <div class={SUMMARY}><span>{format!("{} 条去重连接", records.len())}</span><strong>{format!("↑ {} · ↓ {}", format_bytes(upload), format_bytes(download))}</strong></div>
                            <div class="grid gap-2">
                                {for records.iter().map(|record| {
                                    let egress = if record.chains.is_empty() { "未报告".to_owned() } else { record.chains.join(" → ") };
                                    html! {
                                        <article class="grid gap-1 rounded-box border border-base-content/10 p-3" data-activity-mac={record.mac.clone()} key={record.connection_id.clone()}>
                                            <div class="flex flex-wrap items-center justify-between gap-2"><strong>{format!("{}:{}", record.target, record.destination_port)}</strong><span class={CONTROL_META}>{if record.active { "当前连接" } else { "最近连接" }}</span></div>
                                            <small class={HELP_TEXT}>{format!("{} · {} · 出口 {}", record.network.label(), record.rule, egress)}</small>
                                            <small class={HELP_TEXT}>{format!("↑ {} · ↓ {}", format_bytes(record.upload_bytes), format_bytes(record.download_bytes))}</small>
                                        </article>
                                    }
                                })}
                            </div>
                        }
                    </article>
                </div>
            </section>
        </div>
    }
}
