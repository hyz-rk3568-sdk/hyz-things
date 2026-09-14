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
    let sta_expanded = use_state(|| false);
    let ap_expanded = use_state(|| false);
    let network_confirmation_open = use_state(|| false);
    let network_apply_intent = use_mut_ref(|| None::<NetworkApplyIntent>);

    {
        let panel = network_confirmation_panel.clone();
        let cancel = confirmation_cancel_button.clone();
        use_effect_with(*network_confirmation_open, move |open| {
            if *open {
                reveal_and_focus_after_render(panel.clone(), cancel.clone());
            }
            || ()
        });
    }

    let csrf = state
        .panel
        .as_ref()
        .map(|panel| panel.csrf_token.clone())
        .unwrap_or_default();
    let busy = state.settings_busy;
    let pending_uncertain = state.pending_meta.error.is_some();

    let logout = {
        let state = state.clone();
        let csrf = csrf.clone();
        let camera_stop_generation = props.camera_stop_generation.clone();
        Callback::from(move |_| {
            camera_stop_generation.set((*camera_stop_generation).wrapping_add(1));
            dispatch_auth(
                state.clone(),
                AUTH_LOGOUT_ENDPOINT,
                csrf.clone(),
                EmptyRequest {},
                "已退出登录",
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
            dispatch_network_mutation(
                state.clone(),
                AP_PREPARE_ENDPOINT,
                csrf.clone(),
                request,
                "AP 准备",
                "AP 配置已准备，请确认断线风险后应用",
                false,
            );
        })
    };
    let ap_action = |endpoint: &'static str, label: &'static str, success: &'static str| {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| {
            dispatch_network_mutation(
                state.clone(),
                endpoint,
                csrf.clone(),
                EmptyRequest {},
                label,
                success,
                false,
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
                    dispatch_network_mutation(
                        state.clone(),
                        STA_APPLY_ENDPOINT,
                        csrf.clone(),
                        request,
                        "STA 应用",
                        "STA 配置已应用",
                        true,
                    );
                }
                NetworkApplyIntent::Ap => {
                    ap_expanded.set(false);
                    reveal_and_focus_after_render(ap_toggle.clone(), ap_toggle.clone());
                    dispatch_network_mutation(
                        state.clone(),
                        AP_APPLY_ENDPOINT,
                        csrf.clone(),
                        EmptyRequest {},
                        "AP 应用",
                        "AP 正在切换，请连接新 AP 后确认",
                        true,
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
    let toggle_sta = {
        let expanded = sta_expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    let toggle_ap = {
        let expanded = ap_expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    let retry_network = {
        let state = state.clone();
        Callback::from(move |_| dispatch_network_refresh(state.clone()))
    };
    let retry_pending = {
        let state = state.clone();
        Callback::from(move |_| dispatch_pending_refresh(state.clone()))
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
                    if applied { "等待确认" } else { "候选待应用" },
                    candidate.config.ap_ssid
                )
            },
        );

    html! {
        <SectionCard title_id="network-settings-title" busy={Some(busy)}>
            <PageHeader title_id="network-settings-title" eyebrow="NETWORK" title="网络设置">
                <div class={SESSION_ACTIONS}>
                    <span>{format!("配置 {} · 待确认 {}", state.network_meta.status_text(), state.pending_meta.status_text())}</span>
                    <button class="btn btn-ghost btn-sm text-base-content" type="button" onclick={logout} disabled={busy || csrf.is_empty()}>{"退出登录"}</button>
                </div>
            </PageHeader>
            if let Some(notice) = &state.settings_notice {
                <FeedbackState message={AttrValue::from(notice.clone())} />
            }
            if let Some(error) = &state.network_meta.error {
                <div class="grid gap-2">
                    <ErrorState message={format!("网络配置读取失败；保留最近一次成功数据：{error}")} />
                    <div class={BUTTON_ROW}><button class={BUTTON} type="button" onclick={retry_network.clone()} disabled={state.network_meta.loading}>{"重试网络配置"}</button></div>
                </div>
            }
            if let Some(error) = &state.pending_meta.error {
                <div class="grid gap-2">
                    <ErrorState message={format!("待确认网络配置读取失败；危险 AP 操作已暂停：{error}")} />
                    <div class={BUTTON_ROW}><button class={BUTTON} type="button" onclick={retry_pending.clone()} disabled={state.pending_meta.loading}>{"重试待确认状态"}</button></div>
                </div>
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
                        <button class={BUTTON_ERROR} type="button" onclick={confirm_network_apply} disabled={pending_uncertain}>{"确认并开始应用"}</button>
                    </div>
                </section>
            }
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
                                <div class={FORM_ACTIONS}><button ref={sta_apply_button} class={BUTTON_PRIMARY} type="submit" disabled={busy || pending_uncertain}>{"检查并应用 STA"}</button></div>
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
                                busy || pending_uncertain,
                            )}
                        </div>
                    }
                </article>
            </div>
        </SectionCard>
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
