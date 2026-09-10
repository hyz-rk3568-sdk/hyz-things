use super::*;

pub(crate) fn render_tailscale_peers(state: &UseReducerHandle<AppState>) -> Html {
    let refresh = {
        let state = state.clone();
        Callback::from(move |_| dispatch_settings_refresh(state.clone()))
    };
    match (&state.tailscale_peers, &state.tailscale_peers_error) {
        (Some(snapshot), error) => {
            let summary = format!(
                "Tailnet 设备 · {} / {} 在线",
                snapshot.device_online(),
                snapshot.device_total()
            );
            html! {
                <div class="grid gap-3 rounded-box border border-base-content/10 bg-base-200/40 p-4" role="region" aria-label="Tailnet 设备">
                    <div class={CONTROL_TITLE}>
                        <h3 class={CONTROL_HEADING}>{"Tailnet 设备"}</h3>
                        <span class={CONTROL_META}>{summary}</span>
                    </div>
                    <div class={BUTTON_ROW}>
                        <button class={BUTTON} type="button" onclick={refresh.clone()} disabled={state.settings_busy}>{"重新读取设备"}</button>
                    </div>
                    if let Some(error) = error {
                        <ErrorState message={format!("设备列表读取失败，当前显示上次成功数据，数据可能已过期：{error}")} />
                    }
                    if snapshot.device_total() == 0 {
                        <div class={SETTINGS_EMPTY} role="status">{"暂无 Tailnet 设备"}</div>
                    } else {
                        <details class="group rounded-box border border-base-content/10 bg-base-100/70 p-3">
                            <summary class="cursor-pointer font-medium">{"查看设备列表"}</summary>
                            <ul class="mt-3 grid gap-2">
                                if let Some(local) = snapshot.self_node.as_ref() {
                                    {render_tailscale_peer(local, true)}
                                }
                                {for snapshot.peers.iter().map(|peer| render_tailscale_peer(peer, false))}
                            </ul>
                        </details>
                    }
                    <small class={HELP_TEXT}>{"“在线”仅表示该设备当前连接到 Tailnet，不表示它正在访问本路由器的 LAN。"}</small>
                </div>
            }
        }
        (None, Some(error)) => html! {
            <div class="grid gap-3">
                <ErrorState message={format!("Tailnet 设备列表暂不可用：{error}")} />
                <div class={BUTTON_ROW}><button class={BUTTON} type="button" onclick={refresh.clone()} disabled={state.settings_busy}>{"重新读取设备"}</button></div>
            </div>
        },
        (None, None) => html! {
            <div class={SETTINGS_EMPTY} role="status">{"正在读取 Tailnet 设备列表…"}</div>
        },
    }
}

pub(crate) fn render_tailscale_peer(peer: &TailscalePeer, local: bool) -> Html {
    let status = if peer.online { "在线" } else { "离线" };
    let tone = if peer.online {
        Tone::Good
    } else {
        Tone::Neutral
    };
    let platform = peer
        .os
        .as_deref()
        .map_or_else(String::new, |os| format!(" · {os}"));
    let detail = tailscale_peer_detail(peer);
    html! {
        <li class="grid min-w-0 gap-2 rounded-box border border-base-content/10 p-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center" key={format!("{}-{}", if local { "self" } else { "peer" }, peer.ipv4)}>
            <div class="min-w-0">
                <div class="flex min-w-0 items-center gap-2">
                    <strong class="block truncate" title={peer.name.clone()}>{&peer.name}</strong>
                    if local {
                        <span class="badge badge-primary badge-outline shrink-0 text-[0.6rem] font-bold">{"本机"}</span>
                    }
                </div>
                <span class={HELP_TEXT}>{format!("{}{platform}", peer.ipv4)}</span>
                <small class="block text-xs text-base-content/65">{detail}</small>
            </div>
            <StatusBadge label={status} tone={classes!(tone.class())} />
        </li>
    }
}

pub(crate) fn tailscale_peer_detail(peer: &TailscalePeer) -> String {
    let mut details = Vec::new();
    if peer.online {
        if peer.active == Some(true) {
            details.push("active".to_owned());
            if let Some(connection) = &peer.connection {
                details.push(tailscale_connection_label(connection));
            }
            if let (Some(tx), Some(rx)) = (peer.tx_bytes, peer.rx_bytes) {
                details.push(format!("tx {} · rx {}", format_bytes(tx), format_bytes(rx)));
            }
        } else {
            details.push("-".to_owned());
        }
    } else {
        details.push("offline".to_owned());
        if let Some(last_seen) = peer.last_seen_unix_ms {
            details.push(format!("最近看到 {}", tailscale_last_seen_label(last_seen)));
        }
    }
    details.join(" · ")
}

pub(crate) fn tailscale_connection_label(connection: &TailscalePeerConnection) -> String {
    match connection {
        TailscalePeerConnection::Direct { address } => format!("direct {address}"),
        TailscalePeerConnection::Relay { region } => format!("relay \"{region}\""),
    }
}

pub(crate) fn tailscale_last_seen_label(unix_ms: u64) -> String {
    const MAX_DATE_MILLIS: u64 = 8_640_000_000_000_000;
    let now = js_sys::Date::now();
    if !now.is_finite() || unix_ms > MAX_DATE_MILLIS {
        return "未知".to_owned();
    }
    let seen = unix_ms as f64;
    if seen >= now {
        return "刚刚".to_owned();
    }
    let minutes = ((now - seen) / 60_000.0).floor() as u64;
    if minutes < 1 {
        "刚刚".to_owned()
    } else if minutes < 60 {
        format!("{minutes} 分钟前")
    } else {
        let hours = minutes / 60;
        if hours < 24 {
            format!("{hours} 小时前")
        } else {
            format!("{} 天前", hours / 24)
        }
    }
}

pub(crate) fn render_tailscale_control(state: &UseReducerHandle<AppState>, csrf: &str) -> Html {
    let Some(tailscale) = state.tailscale.as_ref() else {
        return html! {
            <section class={SECTION} aria-labelledby="tailscale-title">
                <PageHeader title_id="tailscale-title" eyebrow="REMOTE LAN" title="Tailscale 远程 LAN" />
                <div class={SETTINGS_EMPTY} role="status">{"正在读取 Tailscale 状态…"}</div>
            </section>
        };
    };
    let busy = state.settings_busy;
    let enable = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| {
            dispatch_tailscale_mutation(
                state.clone(),
                TAILSCALE_MODE_ENDPOINT,
                csrf.clone(),
                TailscaleModeRequestDto {
                    mode: TailscaleMode::LanSubnetAccess,
                },
                "已请求启用远程 LAN 访问",
            )
        })
    };
    let disable = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| {
            dispatch_tailscale_mutation(
                state.clone(),
                TAILSCALE_MODE_ENDPOINT,
                csrf.clone(),
                TailscaleModeRequestDto {
                    mode: TailscaleMode::Disabled,
                },
                "Tailscale 已停用，设备认证已保留",
            )
        })
    };
    let request_login = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| {
            dispatch_tailscale_mutation(
                state.clone(),
                TAILSCALE_LOGIN_ENDPOINT,
                csrf.clone(),
                EmptyRequest {},
                "已取得一次性登录链接",
            )
        })
    };
    let logout = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| {
            dispatch_tailscale_mutation(
                state.clone(),
                TAILSCALE_LOGOUT_ENDPOINT,
                csrf.clone(),
                EmptyRequest {},
                "Tailscale 已注销并停用",
            )
        })
    };
    let desired = tailscale
        .desired_mode
        .map(tailscale_mode_label)
        .unwrap_or("未知");
    let effective = tailscale
        .effective_mode
        .map(tailscale_mode_label)
        .unwrap_or("尚未就绪");
    let needs_login = tailscale.backend_state == TailscaleBackendState::NeedsLogin
        || tailscale.authenticated == Some(false)
            && tailscale.desired_mode != Some(TailscaleMode::Disabled);
    let lan_access_ready = tailscale.effective_mode == Some(TailscaleMode::LanSubnetAccess)
        && tailscale.backend_state == TailscaleBackendState::Running
        && tailscale.authenticated == Some(true)
        && tailscale.route_advertised == Some(true)
        && tailscale.local_firewall_ready == Some(true);
    let disabled_ready = tailscale.desired_mode == Some(TailscaleMode::Disabled)
        && tailscale.effective_mode == Some(TailscaleMode::Disabled)
        && tailscale.backend_state == TailscaleBackendState::Stopped;
    let enable_label = if lan_access_ready {
        "远程 LAN 访问已启用"
    } else {
        "启用远程 LAN 访问"
    };
    let disable_label = if disabled_ready {
        "Tailscale 已停用"
    } else {
        "停用（保留认证）"
    };

    html! {
        <section class={SECTION} aria-labelledby="tailscale-title" aria-busy={busy.to_string()}>
            <PageHeader title_id="tailscale-title" eyebrow="REMOTE LAN" title="Tailscale 远程 LAN">
                <span class={SECTION_META}>{"固定 192.168.8.0/24 · 不提供 Exit Node"}</span>
            </PageHeader>
            <article class={INNER_CARD} role="region" aria-label="Tailscale 远程 LAN 状态">
                <div class={CONTROL_TITLE}><h3 class={CONTROL_HEADING}>{"LAN Access"}</h3><span class={CONTROL_META}>{format!("期望 {desired} · 当前 {effective}")}</span></div>
                <dl class={METRIC_LIST}>
                    <div class={METRIC}><dt class={METRIC_LABEL}>{"认证"}</dt><dd class={METRIC_VALUE}>{tailscale.authenticated.map(|value| if value { "已认证" } else { "需要登录" }).unwrap_or("未知")}</dd></div>
                    <div class={METRIC}><dt class={METRIC_LABEL}>{"Tailscale IPv4"}</dt><dd class={METRIC_VALUE}>{tailscale.ipv4.map(|value| value.to_string()).unwrap_or_else(missing)}</dd></div>
                    <div class={METRIC}><dt class={METRIC_LABEL}>{"本地路由 / 防火墙"}</dt><dd class={METRIC_VALUE}>{format!("路由 {} · 防火墙 {}", tailscale.route_advertised.map(format_bool).unwrap_or_else(missing), tailscale.local_firewall_ready.map(format_bool).unwrap_or_else(missing))}</dd></div>
                </dl>
                {render_tailscale_peers(state)}
                if needs_login {
                    <div class={classes!(RISK_ALERT, "alert-warning", "border-warning/20")} role="alert">
                        <div>
                            <strong>{"需要完成 Tailscale 登录"}</strong>
                            <p class={RISK_COPY}>{"登录链接只在本次管理员写操作响应中返回，不会保存或出现在状态 GET 中。"}</p>
                        </div>
                    </div>
                    if let Some(login_url) = &state.tailscale_login_url {
                        <div class={BUTTON_ROW}>
                            <a class={BUTTON_PRIMARY} href={login_url.clone()} target="_blank" rel="noopener noreferrer">{"打开一次性 Tailscale 登录链接"}</a>
                            <button class={BUTTON} type="button" onclick={enable.clone()} disabled={busy}>{"已完成登录，继续启用"}</button>
                        </div>
                    } else {
                        <button class={BUTTON_PRIMARY} type="button" onclick={request_login.clone()} disabled={busy}>{"取得一次性登录链接"}</button>
                    }
                }
                if lan_access_ready {
                    <div class={classes!(RISK_ALERT, "alert-success", "border-success/20")} role="status">
                        <div>
                            <strong>{"本机远程 LAN 访问已启用"}</strong>
                            <p class={RISK_COPY}>{"Tailscale 已认证，固定子网路由和本地防火墙均已就绪。"}</p>
                        </div>
                    </div>
                }
                if let Some(category) = tailscale.error_category {
                    <div class={RISK_NOTE} role="note">{format!("错误类别：{}", tailscale_error_label(category))}</div>
                }
                <div class={BUTTON_ROW} role="group" aria-label="Tailscale 操作">
                    <button
                        class={if lan_access_ready { BUTTON } else { BUTTON_PRIMARY }}
                        type="button"
                        onclick={enable}
                        disabled={busy || lan_access_ready}
                        aria-pressed={lan_access_ready.to_string()}
                    >{enable_label}</button>
                    <button
                        class={BUTTON}
                        type="button"
                        onclick={disable}
                        disabled={busy || disabled_ready}
                        aria-pressed={disabled_ready.to_string()}
                    >{disable_label}</button>
                    <button class={BUTTON_ERROR} type="button" onclick={logout} disabled={busy}>{"注销并移除认证"}</button>
                </div>
                <small class={HELP_TEXT}>{"仅支持固定 RouterOnly / LAN Access 安全模式；浏览器不能输入 URL、auth key、子网、端口或控制参数。"}</small>
            </article>
        </section>
    }
}
