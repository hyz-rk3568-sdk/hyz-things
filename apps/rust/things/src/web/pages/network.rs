use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct SettingsProps {
    pub(crate) state: UseReducerHandle<AppState>,
    pub(crate) camera_stop_generation: UseStateHandle<u32>,
}

pub(crate) struct ApSettingsRefs<'a> {
    ssid: &'a NodeRef,
    password: &'a NodeRef,
    apply_button: &'a NodeRef,
}

pub(crate) struct ApSettingsActions {
    prepare: Callback<SubmitEvent>,
    apply: Callback<MouseEvent>,
    confirm: Callback<MouseEvent>,
    cancel: Callback<MouseEvent>,
}

#[function_component(Settings)]
pub(crate) fn settings(props: &SettingsProps) -> Html {
    let state = &props.state;
    let login_password = use_node_ref();
    let current_password = use_node_ref();
    let new_password = use_node_ref();
    let confirm_password = use_node_ref();
    let sta_ssid = use_node_ref();
    let sta_password = use_node_ref();
    let sta_toggle = use_node_ref();
    let sta_apply_button = use_node_ref();
    let ap_ssid = use_node_ref();
    let ap_password = use_node_ref();
    let ap_toggle = use_node_ref();
    let ap_apply_button = use_node_ref();
    let network_confirmation_panel = use_node_ref();
    let confirmation_cancel_button = use_node_ref();
    let subscription_url = use_node_ref();
    let login_expanded = use_state(|| false);
    let sta_expanded = use_state(|| false);
    let ap_expanded = use_state(|| false);
    let network_confirmation_open = use_state(|| false);
    let network_apply_intent = use_mut_ref(|| None::<NetworkApplyIntent>);
    let camera_stop_generation = props.camera_stop_generation.clone();

    {
        let network_confirmation_panel = network_confirmation_panel.clone();
        let confirmation_cancel_button = confirmation_cancel_button.clone();
        use_effect_with(*network_confirmation_open, move |open| {
            if *open {
                reveal_and_focus_after_render(
                    network_confirmation_panel,
                    confirmation_cancel_button,
                );
            }
            || ()
        });
    }

    let csrf = state
        .panel
        .as_ref()
        .map(|panel| panel.csrf_token.clone())
        .unwrap_or_default();
    let session = state.session.as_ref();
    let authenticated = session.is_some_and(|session| session.authenticated);
    let must_change = session.is_some_and(|session| session.must_change);
    let busy = state.settings_busy;

    let login = {
        let state = state.clone();
        let csrf = csrf.clone();
        let input = login_password.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let Some(input) = input.cast::<HtmlInputElement>() else {
                return;
            };
            let password = input.value();
            input.set_value("");
            dispatch_auth(
                state.clone(),
                AUTH_LOGIN_ENDPOINT,
                csrf.clone(),
                LoginRequest { password },
                "已登录",
            );
        })
    };
    let logout = {
        let state = state.clone();
        let csrf = csrf.clone();
        let login_expanded = login_expanded.clone();
        let camera_stop_generation = camera_stop_generation.clone();
        Callback::from(move |_| {
            login_expanded.set(false);
            camera_stop_generation.set((*camera_stop_generation).wrapping_add(1));
            dispatch_auth(
                state.clone(),
                AUTH_LOGOUT_ENDPOINT,
                csrf.clone(),
                EmptyRequest {},
                "已退出登录",
            )
        })
    };
    let change_password = {
        let state = state.clone();
        let csrf = csrf.clone();
        let current = current_password.clone();
        let new = new_password.clone();
        let confirm = confirm_password.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let (Some(current), Some(new), Some(confirm)) = (
                current.cast::<HtmlInputElement>(),
                new.cast::<HtmlInputElement>(),
                confirm.cast::<HtmlInputElement>(),
            ) else {
                return;
            };
            let current_value = current.value();
            let new_value = new.value();
            let confirm_value = confirm.value();
            current.set_value("");
            new.set_value("");
            confirm.set_value("");
            if !(12..=1_024).contains(&new_value.len()) {
                state.dispatch(Action::SettingsNotice(
                    "新密码长度必须为 12–1024 字节".to_owned(),
                ));
                return;
            }
            if new_value != confirm_value {
                state.dispatch(Action::SettingsNotice("两次输入的新密码不一致".to_owned()));
                return;
            }
            dispatch_auth(
                state.clone(),
                AUTH_PASSWORD_ENDPOINT,
                csrf.clone(),
                PasswordRequest {
                    current_password: current_value,
                    new_password: new_value,
                },
                "密码已更新",
            );
        })
    };
    let scan = {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| dispatch_scan(state.clone(), csrf.clone()))
    };
    let apply_sta = {
        let intent = network_apply_intent.clone();
        let confirmation_open = network_confirmation_open.clone();
        let ssid = sta_ssid.clone();
        let password = sta_password.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let (Some(ssid), Some(password)) = (
                ssid.cast::<HtmlInputElement>(),
                password.cast::<HtmlInputElement>(),
            ) else {
                return;
            };
            let request = StaRequest {
                ssid: ssid.value(),
                passphrase: password.value(),
            };
            password.set_value("");
            *intent.borrow_mut() = Some(NetworkApplyIntent::Sta(request));
            confirmation_open.set(true);
        })
    };
    let prepare_ap = {
        let state = state.clone();
        let csrf = csrf.clone();
        let ssid = ap_ssid.clone();
        let password = ap_password.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let (Some(ssid), Some(password)) = (
                ssid.cast::<HtmlInputElement>(),
                password.cast::<HtmlInputElement>(),
            ) else {
                return;
            };
            let request = ApRequest {
                ssid: ssid.value(),
                passphrase: password.value(),
                country: "CN".to_owned(),
            };
            password.set_value("");
            dispatch_settings_mutation(
                state.clone(),
                AP_PREPARE_ENDPOINT,
                csrf.clone(),
                request,
                "AP 准备",
                "AP 配置已准备，请确认断线风险后应用",
            );
        })
    };
    let ap_action = |endpoint: &'static str, label: &'static str, success: &'static str| {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| {
            dispatch_settings_mutation(
                state.clone(),
                endpoint,
                csrf.clone(),
                EmptyRequest {},
                label,
                success,
            );
        })
    };
    let confirm_ap = ap_action(AP_CONFIRM_ENDPOINT, "AP 确认", "AP 配置已确认");
    let cancel_ap = ap_action(AP_CANCEL_ENDPOINT, "AP 取消", "AP 配置已取消并回滚");
    let request_ap_apply = {
        let intent = network_apply_intent.clone();
        let confirmation_open = network_confirmation_open.clone();
        Callback::from(move |_| {
            *intent.borrow_mut() = Some(NetworkApplyIntent::Ap);
            confirmation_open.set(true);
        })
    };
    let confirm_network_apply = {
        let state = state.clone();
        let csrf = csrf.clone();
        let intent = network_apply_intent.clone();
        let confirmation_open = network_confirmation_open.clone();
        let sta_expanded = sta_expanded.clone();
        let ap_expanded = ap_expanded.clone();
        let sta_toggle = sta_toggle.clone();
        let ap_toggle = ap_toggle.clone();
        Callback::from(move |_| {
            confirmation_open.set(false);
            let Some(intent) = intent.borrow_mut().take() else {
                return;
            };
            match intent {
                NetworkApplyIntent::Sta(request) => {
                    sta_expanded.set(false);
                    reveal_and_focus_after_render(sta_toggle.clone(), sta_toggle.clone());
                    dispatch_disruptive_settings_mutation(
                        state.clone(),
                        STA_APPLY_ENDPOINT,
                        csrf.clone(),
                        request,
                        "STA 应用",
                        "STA 配置已应用",
                    );
                }
                NetworkApplyIntent::Ap => {
                    ap_expanded.set(false);
                    reveal_and_focus_after_render(ap_toggle.clone(), ap_toggle.clone());
                    dispatch_disruptive_settings_mutation(
                        state.clone(),
                        AP_APPLY_ENDPOINT,
                        csrf.clone(),
                        EmptyRequest {},
                        "AP 应用",
                        "AP 正在切换，请连接新 AP 后确认",
                    );
                }
            }
        })
    };
    let cancel_network_apply = {
        let intent = network_apply_intent.clone();
        let confirmation_open = network_confirmation_open.clone();
        let sta_apply_button = sta_apply_button.clone();
        let ap_apply_button = ap_apply_button.clone();
        Callback::from(move |_| {
            let focus_target = match intent.borrow_mut().take() {
                Some(NetworkApplyIntent::Sta(_)) => sta_apply_button.clone(),
                Some(NetworkApplyIntent::Ap) => ap_apply_button.clone(),
                None => return,
            };
            confirmation_open.set(false);
            reveal_and_focus_after_render(focus_target.clone(), focus_target);
        })
    };
    let toggle_login = {
        let expanded = login_expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    let toggle_sta = {
        let expanded = sta_expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    let toggle_ap = {
        let expanded = ap_expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    let network_confirmation = if *network_confirmation_open {
        network_apply_intent
            .borrow()
            .as_ref()
            .map(NetworkApplyIntent::confirmation)
    } else {
        None
    };
    let sta_summary = state.network.as_ref().map_or_else(
        || "读取中".to_owned(),
        |network| format!("当前 · {}", network.sta_ssid),
    );
    let ap_summary = state
        .pending_network
        .as_ref()
        .and_then(|pending| {
            pending
                .pending
                .as_ref()
                .map(|candidate| (pending.applied, candidate))
        })
        .map_or_else(
            || {
                state.network.as_ref().map_or_else(
                    || "读取中".to_owned(),
                    |network| format!("当前 · {}", network.ap_ssid),
                )
            },
            |(applied, candidate)| {
                format!(
                    "{} · {}",
                    if applied {
                        "等待确认"
                    } else {
                        "候选待应用"
                    },
                    candidate.config.ap_ssid
                )
            },
        );
    let save_subscription = {
        let state = state.clone();
        let csrf = csrf.clone();
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
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| {
            dispatch_settings_mutation(
                state.clone(),
                SUBSCRIPTION_REFRESH_ENDPOINT,
                csrf.clone(),
                EmptyRequest {},
                "订阅更新",
                "订阅已更新",
            )
        })
    };

    html! {
        <SectionCard title_id="settings-title" busy={Some(busy)}>
            <PageHeader title_id="settings-title" eyebrow="ADMIN" title="管理设置" centered=true>
                if authenticated {
                    <div class={SESSION_ACTIONS}>
                        <span>{"管理员 · admin"}</span>
                        <button class="btn btn-ghost btn-sm text-base-content" type="button" onclick={logout} disabled={busy || csrf.is_empty()}>{"退出登录"}</button>
                    </div>
                } else {
                    <span class={SECTION_META}>{"状态面板无需登录，设置需要管理员身份"}</span>
                }
            </PageHeader>
            if let Some(notice) = &state.settings_notice {
                <FeedbackState message={AttrValue::from(notice.clone())} />
            }
            if let Some((title, message)) = network_confirmation {
                <section ref={network_confirmation_panel} id="network-confirmation-panel" class={CONFIRMATION_PANEL} role="region" aria-live="assertive" aria-atomic="true" aria-labelledby="network-confirmation-title" aria-describedby="network-confirmation-message">
                    <div>
                        <p class={EYEBROW}>{"NETWORK CHANGE"}</p>
                        <h3 id="network-confirmation-title" class={CONFIRMATION_TITLE}>{title}</h3>
                    </div>
                    <p id="network-confirmation-message" class={CONFIRMATION_COPY}>{message}</p>
                    <div class={CONFIRMATION_ACTIONS}>
                        <button ref={confirmation_cancel_button} class={BUTTON} type="button" onclick={cancel_network_apply}>{"返回检查"}</button>
                        <button class={BUTTON_ERROR} type="button" onclick={confirm_network_apply}>{"确认并开始应用"}</button>
                    </div>
                </section>
            }
            if !state.session_checked {
                <div class={SETTINGS_EMPTY} role="status">{"正在检查登录状态…"}</div>
            } else if !authenticated {
                <article class={LOGIN_DISCLOSURE}>
                    <button id="admin-login-toggle" class={DISCLOSURE_TOGGLE} type="button" onclick={toggle_login} aria-expanded={login_expanded.to_string()} aria-controls="admin-login-detail">
                        <span class={DISCLOSURE_COPY}>
                            <strong class={DISCLOSURE_TITLE}>{"管理员登录"}</strong>
                            <small class={DISCLOSURE_SUMMARY}>{"设置保持锁定，状态面板仍可直接查看"}</small>
                        </span>
                        <span class={DISCLOSURE_ACTION} aria-hidden="true">{if *login_expanded { "收起" } else { "展开" }}</span>
                    </button>
                    if *login_expanded {
                        <form id="admin-login-detail" class={AUTH_FORM} role="region" aria-labelledby="admin-login-toggle" onsubmit={login} autocomplete="on">
                            <label class={FIELD}>
                                <span class={FIELD_LABEL}>{"用户名"}</span>
                                <input class={READONLY_INPUT} value="admin" readonly=true autocomplete="username" />
                            </label>
                            <label class={FIELD}>
                                <span class={FIELD_LABEL}>{"密码"}</span>
                                <input class={INPUT} ref={login_password} type="password" required=true autocomplete="current-password" />
                            </label>
                            <button class={BUTTON_BLOCK_MOBILE} type="submit" disabled={busy || csrf.is_empty()}>{if busy { "登录中…" } else { "登录" }}</button>
                            <small class={HELP_TEXT}>{"新设备首次登录密码为 admin；登录后必须立即修改。"}</small>
                        </form>
                    }
                </article>
            } else if must_change {
                <div class={FORCED_PASSWORD}>
                    <div class={classes!(RISK_ALERT, "alert-error", "border-error/20")} role="alert">
                        <div>
                            <strong>{"必须先修改默认密码"}</strong>
                            <p class={RISK_COPY}>{"新密码至少 12 字节，不能继续使用默认密码。完成前其他设置保持锁定。"}</p>
                        </div>
                    </div>
                    <form class={FORM_GRID} onsubmit={change_password} autocomplete="on" aria-describedby="password-policy">
                        <label class={FIELD}><span class={FIELD_LABEL}>{"当前密码"}</span><input class={INPUT} ref={current_password} type="password" required=true autocomplete="current-password" /></label>
                        <label class={FIELD}><span class={FIELD_LABEL}>{"新密码"}</span><input class={INPUT} ref={new_password} type="password" required=true maxlength="1024" autocomplete="new-password" /></label>
                        <label class={FIELD}><span class={FIELD_LABEL}>{"确认新密码"}</span><input class={INPUT} ref={confirm_password} type="password" required=true maxlength="1024" autocomplete="new-password" /></label>
                        <p id="password-policy" class="sr-only">{"新密码长度必须为 12 至 1024 字节，且两次输入必须一致。"}</p>
                        <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="submit" disabled={busy || csrf.is_empty()}>{"修改密码"}</button></div>
                    </form>
                </div>
            } else {
                <>
                    <div class={SETTINGS_GRID}>
                    <article class={DISCLOSURE}>
                        <button id="sta-settings-toggle" ref={sta_toggle} class={DISCLOSURE_TOGGLE} type="button" onclick={toggle_sta} aria-expanded={sta_expanded.to_string()} aria-controls="sta-settings-detail">
                            <span class={DISCLOSURE_COPY}><strong class={DISCLOSURE_TITLE}>{"上游 Wi-Fi (STA)"}</strong><small class={DISCLOSURE_SUMMARY}>{sta_summary}</small></span>
                            <span class={DISCLOSURE_ACTION} aria-hidden="true">{if *sta_expanded { "收起" } else { "展开" }}</span>
                        </button>
                        if *sta_expanded {
                            <div id="sta-settings-detail" class={DISCLOSURE_DETAIL} role="region" aria-labelledby="sta-settings-toggle">
                                <div class={DETAIL_TOOLBAR}><small class={HELP_TEXT}>{"扫描附近网络，或手工填写新的上游 Wi-Fi。"}</small><button class={BUTTON} type="button" onclick={scan} disabled={busy}>{if busy { "处理中…" } else { "扫描" }}</button></div>
                                if !state.scan_entries.is_empty() {
                                    <div class={SCAN_LIST} role="group" aria-label="扫描到的 Wi-Fi">
                                        {for state.scan_entries.iter().map(|entry| {
                                            let input = sta_ssid.clone();
                                            let ssid = entry.ssid.clone();
                                            let choose = Callback::from(move |_| { if let Some(input) = input.cast::<HtmlInputElement>() { input.set_value(&ssid); } });
                                            html! { <button type="button" class={SCAN_ENTRY} onclick={choose} disabled={busy} aria-label={format!("选择网络 {}", entry.ssid)}><strong class={SCAN_NAME}>{&entry.ssid}</strong><span class={SCAN_META}>{format!("{} MHz · {} dBm · {}", entry.frequency_mhz, entry.signal_dbm, if entry.secured { "加密" } else { "开放" })}</span></button> }
                                        })}
                                    </div>
                                }
                                <form class={FORM_GRID_COMPACT} onsubmit={apply_sta} autocomplete="off">
                                    <label class={FIELD}><span class={FIELD_LABEL}>{"SSID"}</span><input class={INPUT} ref={sta_ssid} required=true maxlength="32" autocomplete="off" /></label>
                                    <label class={FIELD}><span class={FIELD_LABEL}>{"密码"}</span><input class={INPUT} ref={sta_password} type="password" required=true minlength="8" maxlength="63" autocomplete="new-password" /></label>
                                    <div class={RISK_NOTE} role="note">{"若 STA 与当前 AP 信道不同，设备可能重启 AP 跟随信道，管理连接会短暂断开。"}</div>
                                    <div class={FORM_ACTIONS}><button ref={sta_apply_button} class={BUTTON_PRIMARY} type="submit" disabled={busy}>{"检查并应用 STA"}</button></div>
                                </form>
                            </div>
                        }
                    </article>
                    <article class={DISCLOSURE}>
                        <button id="ap-settings-toggle" ref={ap_toggle} class={DISCLOSURE_TOGGLE} type="button" onclick={toggle_ap} aria-expanded={ap_expanded.to_string()} aria-controls="ap-settings-detail">
                            <span class={DISCLOSURE_COPY}><strong class={DISCLOSURE_TITLE}>{"下游 Wi-Fi (AP)"}</strong><small class={DISCLOSURE_SUMMARY}>{ap_summary}</small></span>
                            <span class={DISCLOSURE_ACTION} aria-hidden="true">{if *ap_expanded { "收起" } else { "展开" }}</span>
                        </button>
                        if *ap_expanded {
                            <div id="ap-settings-detail" class={DISCLOSURE_DETAIL} role="region" aria-labelledby="ap-settings-toggle">
                                {render_ap_settings(
                                    state,
                                    ApSettingsRefs { ssid: &ap_ssid, password: &ap_password, apply_button: &ap_apply_button },
                                    ApSettingsActions { prepare: prepare_ap, apply: request_ap_apply, confirm: confirm_ap, cancel: cancel_ap },
                                    busy,
                                )}
                            </div>
                        }
                    </article>
                    <DevicePolicies state={state.clone()} csrf={csrf.clone()} />
                    <article class={INNER_CARD} aria-labelledby="subscription-title">
                        <div class={CONTROL_TITLE}><h3 id="subscription-title" class={CONTROL_HEADING}>{"代理订阅"}</h3><span class={CONTROL_META}>{"来源只写"}</span></div>
                        if let Some(subscription) = &state.subscription {
                            <div class={SUMMARY}><span>{if subscription.configured { "已配置" } else { "未配置" }}</span><strong class={subscription_tone(subscription.state)}>{subscription_state_label(subscription.state)}</strong></div>
                        }
                        <form class={FORM_GRID_COMPACT} onsubmit={save_subscription} autocomplete="off">
                            <label class={FIELD}><span class={FIELD_LABEL}>{"订阅 URL"}</span><input class={INPUT} ref={subscription_url} type="url" required=true placeholder="https://…" autocomplete="off" autocapitalize="none" spellcheck="false" aria-describedby="subscription-secret-note" /></label>
                            <small id="subscription-secret-note" class={HELP_TEXT}>{"已保存的 URL 永不回显；输入只用于本次提交。"}</small>
                            <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="submit" disabled={busy}>{"保存并立即更新"}</button><button class={BUTTON} type="button" onclick={refresh_subscription} disabled={busy || !state.subscription.as_ref().is_some_and(|value| value.configured)}>{"手动刷新"}</button></div>
                        </form>
                    </article>
                    </div>
                </>
            }
        </SectionCard>
    }
}

pub(crate) fn dispatch_device_policy_update(
    state: UseReducerHandle<AppState>,
    csrf: String,
    generation: u64,
    entries: Vec<DevicePolicyEntryDto>,
) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        let result = post_json(
            DEVICE_POLICIES_UPDATE_ENDPOINT,
            &csrf,
            &DevicePolicyUpdateDto {
                expected_generation: generation,
                entries,
            },
            "设备代理策略",
        )
        .await;
        if result.is_ok() {
            let settings = fetch_settings_data().await;
            let peers = fetch_tailscale_peers().await;
            state.dispatch(Action::SettingsFinished(settings, peers));
        }
        let message = result
            .map(|_| "设备代理策略已保存".to_owned())
            .map_err(|error| {
                if error.contains("HTTP 409") {
                    "设备策略更新未完成（HTTP 409）；请重新加载并核对当前配置后重试".to_owned()
                } else {
                    error
                }
            });
        state.dispatch(Action::SettingsMutationFinished(message));
    });
}

#[derive(Properties, PartialEq)]
pub(crate) struct DevicePoliciesProps {
    state: UseReducerHandle<AppState>,
    csrf: String,
}

#[function_component(DevicePolicies)]
pub(crate) fn device_policies(props: &DevicePoliciesProps) -> Html {
    let manual_mac = use_node_ref();
    let manual_label = use_node_ref();
    let Some(snapshot) = props.state.device_policies.as_ref() else {
        return html! { <article class={INNER_CARD} aria-label="设备代理"><span class={HELP_TEXT}>{"正在读取设备代理策略…"}</span></article> };
    };
    let generation = snapshot.config.generation;
    let configured = snapshot.config.entries.clone();
    let add = {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let mac = manual_mac.clone();
        let label = manual_label.clone();
        let entries = configured.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let (Some(mac), Some(label)) = (
                mac.cast::<HtmlInputElement>(),
                label.cast::<HtmlInputElement>(),
            ) else {
                return;
            };
            let mut next = entries.clone();
            next.retain(|entry| !entry.mac.eq_ignore_ascii_case(&mac.value()));
            next.push(DevicePolicyEntryDto {
                mac: mac.value(),
                label: label.value(),
                policy: DevicePolicyDto::Proxy,
            });
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, next);
        })
    };
    html! {
        <article class={INNER_CARD} aria-labelledby="device-policies-title">
            <div class={CONTROL_TITLE}>
                <h3 id="device-policies-title" class={CONTROL_HEADING}>{"设备代理"}</h3>
                <span class={CONTROL_META}>{format!("配置代次 {}", generation)}</span>
            </div>
            if !snapshot.effective {
                <div class={RISK_NOTE} role="note">{"策略已保存但当前透明代理未启用；仅在全局 TUN 模式生效。"}</div>
            }
            <p class={HELP_TEXT}>{"未配置设备默认使用代理。可为每个 MAC 保存稳定显示名；手机私有/随机 MAC 改变后仍会被识别为新设备。MAC 是家庭 LAN 标识，不是强认证。"}</p>
            <div class="grid gap-3">
                {for snapshot.clients.iter().cloned().map(|client| html! {
                    <DevicePolicyRow
                        state={props.state.clone()}
                        csrf={props.csrf.clone()}
                        generation={generation}
                        configured={configured.clone()}
                        client={client}
                    />
                })}
            </div>
            <form class={FORM_GRID_COMPACT} onsubmit={add} autocomplete="off">
                <label class={FIELD}><span class={FIELD_LABEL}>{"手工添加 MAC"}</span><input class={INPUT} ref={manual_mac} required=true placeholder="02:00:00:00:00:01" maxlength="17" autocapitalize="none" spellcheck="false" /></label>
                <label class={FIELD}><span class={FIELD_LABEL}>{"显示名"}</span><input class={INPUT} ref={manual_label} maxlength="32" /></label>
                <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="submit" disabled={props.state.settings_busy}>{"添加或更新为代理"}</button></div>
            </form>
        </article>
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct DevicePolicyRowProps {
    state: UseReducerHandle<AppState>,
    csrf: String,
    generation: u64,
    configured: Vec<DevicePolicyEntryDto>,
    client: LanClientDto,
}

#[function_component(DevicePolicyRow)]
pub(crate) fn device_policy_row(props: &DevicePolicyRowProps) -> Html {
    let persisted_label = props
        .configured
        .iter()
        .find(|entry| entry.mac == props.client.mac)
        .map(|entry| entry.label.clone())
        .unwrap_or_default();
    let initial_label = if persisted_label.is_empty() {
        props.client.hostname.clone().unwrap_or_default()
    } else {
        persisted_label.clone()
    };
    let label = use_state(|| initial_label.clone());
    {
        let label = label.clone();
        use_effect_with(initial_label, move |current| {
            label.set(current.clone());
            || ()
        });
    }

    let on_label_input = {
        let label = label.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            label.set(input.value());
        })
    };
    let save_label = {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let mac = props.client.mac.clone();
        let entries = props.configured.clone();
        let label = label.clone();
        let policy = props.client.policy;
        let generation = props.generation;
        Callback::from(move |_| {
            let saved_label = label.trim().to_owned();
            let mut next = entries.clone();
            next.retain(|entry| entry.mac != mac);
            if !saved_label.is_empty() || policy == DevicePolicyDto::Direct {
                next.push(DevicePolicyEntryDto {
                    mac: mac.clone(),
                    label: saved_label,
                    policy,
                });
            }
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, next);
        })
    };
    let change_policy = {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let mac = props.client.mac.clone();
        let entries = props.configured.clone();
        let label = label.clone();
        let generation = props.generation;
        Callback::from(move |event: Event| {
            let select: HtmlSelectElement = event.target_unchecked_into();
            let policy = if select.value() == "direct" {
                DevicePolicyDto::Direct
            } else {
                DevicePolicyDto::Proxy
            };
            let saved_label = label.trim().to_owned();
            let mut next = entries.clone();
            next.retain(|entry| entry.mac != mac);
            if policy == DevicePolicyDto::Direct || !saved_label.is_empty() {
                next.push(DevicePolicyEntryDto {
                    mac: mac.clone(),
                    label: saved_label,
                    policy,
                });
            }
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, next);
        })
    };
    let clear = {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let mac = props.client.mac.clone();
        let entries = props.configured.clone();
        let generation = props.generation;
        Callback::from(move |_| {
            let mut next = entries.clone();
            next.retain(|entry| entry.mac != mac);
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, next);
        })
    };
    let is_configured = props
        .configured
        .iter()
        .any(|entry| entry.mac == props.client.mac);
    let display_name = if label.is_empty() {
        props.client.mac.clone()
    } else {
        (*label).clone()
    };

    html! {
        <article class="grid min-w-0 gap-3 rounded-box border border-base-content/10 p-3">
            <div class="min-w-0">
                <strong class="block truncate">{display_name}</strong>
                <small class="block truncate font-mono text-base-content/65">{format!("{} · {} · {}", props.client.mac, props.client.lease_address.as_deref().unwrap_or("无租约 IP"), if props.client.associated { "在线" } else { "离线" })}</small>
            </div>
            <div class="grid min-w-0 gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto] sm:items-end">
                <label class={FIELD}>
                    <span class={FIELD_LABEL}>{"显示名"}</span>
                    <input class={INPUT} value={(*label).clone()} oninput={on_label_input} maxlength="32" aria-label={format!("{} 显示名", props.client.mac)} />
                </label>
                <button class={BUTTON} type="button" onclick={save_label} disabled={props.state.settings_busy}>{"保存名称"}</button>
                <select class={SELECT} aria-label={format!("{} 代理策略", props.client.mac)} onchange={change_policy} disabled={props.state.settings_busy}>
                    <option value="proxy" selected={props.client.policy == DevicePolicyDto::Proxy}>{"代理"}</option>
                    <option value="direct" selected={props.client.policy == DevicePolicyDto::Direct}>{"直连"}</option>
                </select>
            </div>
            if is_configured {
                <div class={FORM_ACTIONS}>
                    <button class={BUTTON_GHOST} type="button" onclick={clear} disabled={props.state.settings_busy} aria-label={format!("清除 {} 的名称和设备策略", props.client.mac)}>{"清除名称和自定义策略"}</button>
                </div>
            }
        </article>
    }
}

pub(crate) fn render_ap_settings(
    state: &AppState,
    refs: ApSettingsRefs<'_>,
    actions: ApSettingsActions,
    busy: bool,
) -> Html {
    let pending = state
        .pending_network
        .as_ref()
        .and_then(|value| value.pending.as_ref());
    let applied = state
        .pending_network
        .as_ref()
        .is_some_and(|value| value.applied);
    if let Some(pending) = pending {
        let seconds = state
            .pending_network
            .as_ref()
            .and_then(|value| value.remaining_seconds)
            .unwrap_or(0);
        html! {
            <div class={PENDING_AP}>
                <div class={CONFIG_SUMMARY}>
                    <span class={CONFIG_LABEL}>{"候选 AP"}</span>
                    <strong class={CONFIG_VALUE}>{&pending.config.ap_ssid}</strong>
                    <small class={CONFIG_META}>{format!("国家 / 地区 · {}", pending.config.country.as_str())}</small>
                </div>
                if applied {
                    <div class={classes!(RISK_ALERT, "alert-warning", "border-warning/20")} role="alert">
                        <div><strong>{format!("等待确认 · 后端剩余约 {seconds} 秒")}</strong><p class={RISK_COPY}>{"请连接新的 AP 后确认。剩余时间以路由器为准，重新打开页面会刷新；到期会自动回滚。"}</p></div>
                    </div>
                    <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="button" onclick={actions.confirm.clone()} disabled={busy}>{"确认保留"}</button><button class={BUTTON} type="button" onclick={actions.cancel.clone()} disabled={busy}>{"取消并回滚"}</button></div>
                } else {
                    <div class={classes!(RISK_ALERT, "alert-error", "border-error/20")} role="alert">
                        <div><strong>{"应用会立即断开当前 AP 连接"}</strong><p class={RISK_COPY}>{"请先记住新 SSID 和密码。应用后连接新 AP，再回到本页确认；未确认会自动回滚。"}</p></div>
                    </div>
                    <div class={FORM_ACTIONS}><button ref={refs.apply_button.clone()} class={BUTTON_ERROR} type="button" onclick={actions.apply.clone()} disabled={busy}>{"检查风险并应用"}</button><button class={BUTTON} type="button" onclick={actions.cancel.clone()} disabled={busy}>{"取消"}</button></div>
                }
            </div>
        }
    } else {
        html! {
            <>
                if let Some(network) = &state.network {
                    <div class={CONFIG_SUMMARY}><span class={CONFIG_LABEL}>{"当前配置"}</span><strong class={CONFIG_VALUE}>{&network.ap_ssid}</strong><small class={CONFIG_META}>{format!("国家 / 地区 · {}", network.country.as_str())}</small></div>
                }
                <form class={FORM_GRID_COMPACT} onsubmit={actions.prepare} autocomplete="off">
                    <label class={FIELD}><span class={FIELD_LABEL}>{"SSID"}</span><input class={INPUT} ref={refs.ssid.clone()} required=true maxlength="32" autocomplete="off" /></label>
                    <label class={FIELD}><span class={FIELD_LABEL}>{"密码"}</span><input class={INPUT} ref={refs.password.clone()} type="password" required=true minlength="8" maxlength="63" autocomplete="new-password" /></label>
                    <label class={FIELD}><span class={FIELD_LABEL}>{"国家 / 地区"}</span><input class={READONLY_INPUT} value="中国 (CN)" readonly=true aria-readonly="true" /></label>
                    <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="submit" disabled={busy}>{"准备 AP 变更"}</button></div>
                </form>
            </>
        }
    }
}
