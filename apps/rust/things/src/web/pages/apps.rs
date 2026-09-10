use super::*;

pub(crate) fn render_deployed_apps(state: &UseReducerHandle<AppState>) -> Html {
    let body = match (&state.apps, &state.apps_error) {
        (Some(apps), _) if !apps.is_empty() => html! {
            <ul class="grid gap-2">
                {for apps.iter().map(|app| {
                    let sha = app.sha256.as_deref().map(|sha| {
                        let prefix = &sha[..sha.len().min(12)];
                        format!("已部署 · sha256:{prefix}")
                    }).unwrap_or_else(|| "固件内置".to_owned());
                    let versions = if app.protocol_versions.is_empty() {
                        "无协议记录".to_owned()
                    } else {
                        app.protocol_versions
                            .iter()
                            .map(|(protocol, version)| format!("{protocol}=v{version}"))
                            .collect::<Vec<_>>()
                            .join(" · ")
                    };
                    let deployed_at = deployment_time_label(app.deployed_at_unix_ms);
                    html! {
                        <li class="grid min-w-0 gap-2 rounded-box border border-base-content/10 p-3 sm:grid-cols-[minmax(0,1fr)_auto]" key={app.name.clone()}>
                            <div class="min-w-0">
                                <strong class="block truncate">{&app.name}</strong>
                                <span class={HELP_TEXT}>{format!("部署时间：{deployed_at} · {} · {}", sha, versions)}</span>
                            </div>
                        </li>
                    }
                })}
            </ul>
        },
        (Some(_), _) => html! {
            <div class={SETTINGS_EMPTY} role="status">{"固件内置，暂无热推送部署记录"}</div>
        },
        (None, Some(error)) => html! {
            <p class={HELP_TEXT} role="status">{format!("部署记录读取失败：{error}")}</p>
        },
        (None, None) => html! {
            <p class={HELP_TEXT} role="status">{"正在读取部署记录…"}</p>
        },
    };
    html! {
        <div class="mt-4 grid gap-3" role="region" aria-label="已部署应用">
            <div class={CONTROL_TITLE}>
                <h3 class={CONTROL_HEADING}>{"已部署应用"}</h3>
                <span class={CONTROL_META}>{"热推送记录（registry.json）"}</span>
            </div>
            {body}
        </div>
    }
}
