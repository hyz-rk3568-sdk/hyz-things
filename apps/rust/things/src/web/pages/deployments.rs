use super::*;

// The registry is a snapshot: one currently deployed version per component, not a history log.
pub(crate) fn render_deployment_records(state: &UseReducerHandle<AppState>) -> Html {
    let body = match (&state.apps, &state.apps_error) {
        (Some(apps), _) if !apps.is_empty() => {
            let mut deployed = apps.iter().collect::<Vec<_>>();
            deployed.sort_by(|a, b| {
                b.deployed_at_unix_ms
                    .cmp(&a.deployed_at_unix_ms)
                    .then_with(|| a.name.cmp(&b.name))
            });
            html! {
                <ul class="grid gap-2">
                    {for deployed.into_iter().map(|app| {
                        let versions = if app.protocol_versions.is_empty() {
                            "未记录".to_owned()
                        } else {
                            app.protocol_versions
                                .iter()
                                .map(|(protocol, version)| format!("{protocol}=v{version}"))
                                .collect::<Vec<_>>()
                                .join(" · ")
                        };
                        let deployed_at = deployment_time_label(app.deployed_at_unix_ms);
                        let sha = app.sha256.as_deref().unwrap_or("未记录");
                        html! {
                            <li class="min-w-0 rounded-box border border-base-content/10 bg-base-100/60 p-4" key={app.name.clone()}>
                                <div class={CONTROL_TITLE}>
                                    <strong class="min-w-0 truncate">{&app.name}</strong>
                                    <span class="badge badge-outline badge-success shrink-0">{"已部署"}</span>
                                </div>
                                <dl class="mt-3 grid min-w-0 gap-2 text-xs text-base-content/70 sm:grid-cols-2">
                                    <div class="min-w-0">
                                        <dt>{"部署时间"}</dt>
                                        <dd class="break-all font-medium text-base-content">{deployed_at}</dd>
                                    </div>
                                    <div class="min-w-0">
                                        <dt>{"协议版本"}</dt>
                                        <dd class="break-all font-medium text-base-content">{versions}</dd>
                                    </div>
                                    <div class="min-w-0 sm:col-span-2">
                                        <dt>{"SHA256"}</dt>
                                        <dd class="break-all font-mono text-base-content">{sha}</dd>
                                    </div>
                                </dl>
                            </li>
                        }
                    })}
                </ul>
            }
        }
        (Some(_), _) => html! {
            <p class={HELP_TEXT} role="status">{"暂无热部署记录（组件可能使用固件内置版本）。"}</p>
        },
        (None, Some(error)) => html! {
            <p class={HELP_TEXT} role="status">{format!("部署记录读取失败：{error}")}</p>
        },
        (None, None) => html! {
            <p class={HELP_TEXT} role="status">{"正在读取热部署记录…"}</p>
        },
    };
    html! {
        <div class="mt-4 grid gap-3" role="region" aria-label="热部署记录">
            <p class={HELP_TEXT}>{"registry.json 仅保存各组件当前的热部署版本，不包含历次部署日志。"}</p>
            if state.apps.is_some() {
                if let Some(error) = &state.apps_error {
                    <p class={HELP_TEXT} role="status">{format!("刷新失败，显示上次记录：{error}")}</p>
                }
            }
            {body}
        </div>
    }
}
