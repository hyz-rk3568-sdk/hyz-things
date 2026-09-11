use super::*;

#[function_component(ActivityDashboard)]
fn activity_dashboard() -> Html {
    let snapshot = use_state(|| None::<DevicePolicySnapshotDto>);
    let error = use_state(|| None::<String>);
    let selected_mac = use_state(|| None::<String>);

    {
        let snapshot = snapshot.clone();
        let error = error.clone();
        use_effect_with((), move |_| {
            let cancelled = Rc::new(Cell::new(false));
            let task_cancelled = cancelled.clone();
            spawn_local(async move {
                loop {
                    match fetch_json::<DevicePolicySnapshotDto>(
                        DEVICE_POLICIES_ENDPOINT,
                        "设备活动",
                    )
                    .await
                    {
                        Ok(next) => {
                            snapshot.set(Some(next));
                            error.set(None);
                        }
                        Err(message) => error.set(Some(message)),
                    }
                    if task_cancelled.get() {
                        break;
                    }
                    TimeoutFuture::new(POLL_DELAY_MS).await;
                    if task_cancelled.get() {
                        break;
                    }
                }
            });
            move || cancelled.set(true)
        });
    }

    let activity = snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.activity.as_ref());
    let mut devices = activity
        .map(|activity| {
            activity
                .records
                .iter()
                .map(|record| (record.mac.clone(), record.device_name.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    devices.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    devices.dedup_by(|left, right| left.0 == right.0);

    let show_all = {
        let selected_mac = selected_mac.clone();
        Callback::from(move |_| selected_mac.set(None))
    };

    html! {
        <SectionCard title_id="activity-title" extra_class={classes!("gap-5")}>
            <PageHeader title_id="activity-title" eyebrow="ACTIVITY" title="活动">
                <span class={SECTION_META}>{"Mihomo 实时连接与近期内存历史"}</span>
            </PageHeader>
            <div class="flex flex-wrap items-center gap-2" aria-label="设备筛选">
                <button
                    type="button"
                    class={classes!("btn", "btn-sm", selected_mac.is_none().then_some("btn-primary"))}
                    aria-pressed={selected_mac.is_none().to_string()}
                    onclick={show_all}
                >
                    {"全部设备"}
                </button>
                {for devices.iter().map(|(mac, name)| {
                    let active = selected_mac.as_deref() == Some(mac.as_str());
                    let selected_mac = selected_mac.clone();
                    let filter_mac = mac.clone();
                    let filter_name = name.clone();
                    html! {
                        <button
                            type="button"
                            class={classes!("btn", "btn-sm", active.then_some("btn-primary"))}
                            aria-label={format!("筛选设备 {filter_name}")}
                            aria-pressed={active.to_string()}
                            onclick={Callback::from(move |_| selected_mac.set(Some(filter_mac.clone())))}
                        >
                            <span>{name}</span>
                            <span class="font-mono text-[0.68rem] opacity-60">{mac}</span>
                        </button>
                    }
                })}
            </div>

            if let Some(message) = error.as_ref() {
                <FeedbackState message={AttrValue::from(format!("活动刷新失败：{message}；保留最近一次数据"))} />
            }

            if let Some(activity) = activity {
                if activity.records.is_empty() {
                    <p class={HELP_TEXT}>{"暂无 Mihomo 连接记录。仅统计经过 Mihomo 的 LAN 连接。"}</p>
                } else {
                    <div class="grid gap-3" aria-label="连接活动">
                        {for activity.records.iter().filter(|record| {
                            selected_mac
                                .as_deref()
                                .is_none_or(|mac| record.mac == mac)
                        }).map(|record| render_activity_record(record, activity.observed_at_unix_ms))}
                    </div>
                }
            } else if snapshot.is_some() {
                <p class={HELP_TEXT}>{"Mihomo 活动暂不可用；设备策略与代理控制仍可独立使用。"}</p>
            } else {
                <p class={HELP_TEXT}>{"正在读取连接活动…"}</p>
            }

            <p class={HELP_TEXT}>
                {"设备按 MAC 归属；设置过设备名时显示自定义名称，否则显示 MAC。仅保留有界内存历史，不记录请求正文、HTTPS 路径或凭据。"}
            </p>
        </SectionCard>
    }
}

fn render_activity_record(record: &ActivityRecordDto, observed_at_unix_ms: u64) -> Html {
    let route = if record.chains.is_empty() {
        "DIRECT".to_owned()
    } else {
        record.chains.join(" → ")
    };
    let state = if record.active { "实时" } else { "历史" };
    let state_class = if record.active {
        "badge badge-success badge-sm"
    } else {
        "badge badge-ghost badge-sm"
    };
    let age = relative_age(observed_at_unix_ms, record.last_seen_unix_ms);
    let duration = record
        .last_seen_unix_ms
        .saturating_sub(record.first_seen_unix_ms)
        / 1_000;
    html! {
        <article
            class={INNER_CARD}
            key={record.connection_id.clone()}
            data-activity-mac={record.mac.clone()}
        >
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div class="grid min-w-0 gap-1">
                    <strong class={CONTROL_HEADING}>{&record.device_name}</strong>
                    <span class="font-mono text-xs text-base-content/55">{format!("{} · {}", record.mac, record.source_address)}</span>
                </div>
                <div class="flex items-center gap-2">
                    <span class="badge badge-outline badge-sm">{record.network.label()}</span>
                    <span class={state_class}>{state}</span>
                </div>
            </div>
            <div class="mt-3 grid gap-1">
                <strong class="break-all text-sm">{format!("{}:{}", record.target, record.destination_port)}</strong>
                <span class={CONTROL_META}>{format!("规则 · {}", record.rule)}</span>
                <span class={CONTROL_META}>{format!("出口 · {route}")}</span>
            </div>
            <div class="mt-3 flex flex-wrap items-center justify-between gap-2 text-xs text-base-content/60">
                <span>{format!("↑ {}   ↓ {}", format_bytes(record.upload_bytes), format_bytes(record.download_bytes))}</span>
                <span>{format!("{age} · 持续 {duration}s")}</span>
            </div>
        </article>
    }
}

fn relative_age(observed_at_unix_ms: u64, last_seen_unix_ms: u64) -> String {
    let seconds = observed_at_unix_ms.saturating_sub(last_seen_unix_ms) / 1_000;
    match seconds {
        0..=2 => "刚刚".to_owned(),
        3..=59 => format!("{seconds}s 前"),
        60..=3_599 => format!("{}m 前", seconds / 60),
        _ => format!("{}h 前", seconds / 3_600),
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

pub(crate) fn render_activity() -> Html {
    html! { <ActivityDashboard /> }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_and_age_labels_are_stable() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2 * 1024), "2.0 KiB");
        assert_eq!(relative_age(10_000, 9_000), "刚刚");
        assert_eq!(relative_age(70_000, 10_000), "1m 前");
    }
}
