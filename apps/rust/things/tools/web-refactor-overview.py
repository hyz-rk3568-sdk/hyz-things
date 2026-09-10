#!/usr/bin/env python3
# Prioritize core health on the Overview without changing capability behavior.

from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
OVERVIEW = WEB / "pages/overview.rs"
METRIC_CARD = WEB / "components/metric_card.rs"
UI = WEB / "ui.rs"
SHELL_SPEC = ROOT / "apps/rust/things/e2e/shell.spec.ts"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def migrate_metric_card() -> None:
    text = METRIC_CARD.read_text()
    if "pub(crate) status: Option<AttrValue>" not in text:
        text = replace_once(
            text,
            '''    pub(crate) meta: AttrValue,
}''',
            '''    pub(crate) meta: AttrValue,
    #[prop_or_default]
    pub(crate) status: Option<AttrValue>,
    #[prop_or_default]
    pub(crate) status_tone: Classes,
}''',
            "metric card status props",
        )
        text = replace_once(
            text,
            '''            <span class={KPI_LABEL}>{props.label.clone()}</span>
            <strong class={KPI_VALUE} title={props.value.clone()}>{props.value.clone()}</strong>''',
            '''            <div class={KPI_HEAD}>
                <span class={KPI_LABEL}>{props.label.clone()}</span>
                if let Some(status) = &props.status {
                    <StatusBadge label={status.clone()} tone={props.status_tone.clone()} />
                }
            </div>
            <strong class={KPI_VALUE} title={props.value.clone()}>{props.value.clone()}</strong>''',
            "metric card status rendering",
        )
    METRIC_CARD.write_text(text)


def migrate_ui_tokens() -> None:
    text = UI.read_text()
    if "pub const KPI_HEAD:" not in text:
        text = replace_once(
            text,
            'pub const KPI_CARD: &str =\n',
            'pub const KPI_HEAD: &str = "flex min-w-0 items-center justify-between gap-2";\npub const KPI_CARD: &str =\n',
            "KPI header token",
        )
    text = text.replace(
        'pub const KPI_GRID: &str = "mt-3 grid grid-cols-2 gap-3 lg:grid-cols-4";',
        'pub const KPI_GRID: &str = "mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4";',
    )
    UI.write_text(text)


def migrate_overview() -> None:
    text = OVERVIEW.read_text()
    if "{render_health_summary(snapshot)}" not in text:
        text = replace_once(
            text,
            '''                {render_topology(snapshot)}
                {render_kpis(snapshot)}
                {render_issues(snapshot)}''',
            '''                {render_health_summary(snapshot)}
                {render_issues(snapshot)}
                {render_topology(snapshot)}''',
            "Overview information priority",
        )

    old_kpis_start = text.find("pub(crate) fn render_kpis(snapshot: &StatusSnapshot) -> Html {")
    if old_kpis_start >= 0:
        text = text[:old_kpis_start] + r'''pub(crate) fn internet_health_tone(
    router: Option<&hyz_things::domain::status::RouterStatus>,
) -> Tone {
    let Some(router) = router else {
        return Tone::Neutral;
    };
    let ethernet = router.ethernet.as_ref();
    let wifi = router.wifi.as_ref();
    if let Some(active) = router.active_uplink {
        return match active {
            UplinkId::Ethernet => uplink_tone(ethernet),
            UplinkId::Wifi => uplink_tone(wifi),
        };
    }
    if [ethernet, wifi]
        .into_iter()
        .flatten()
        .any(|status| status.session_up == Some(true))
    {
        Tone::Warn
    } else if [ethernet, wifi]
        .into_iter()
        .flatten()
        .all(|status| status.link_up == Some(false))
    {
        Tone::Bad
    } else {
        Tone::Neutral
    }
}

pub(crate) fn health_status_label(tone: Tone) -> &'static str {
    match tone {
        Tone::Good => "正常",
        Tone::Warn => "需检查",
        Tone::Bad => "不可用",
        Tone::Neutral => "未知",
    }
}

pub(crate) fn render_health_summary(snapshot: &StatusSnapshot) -> Html {
    let router = snapshot.router.data.as_ref();
    let proxy = snapshot.proxy.data.as_ref();
    let tailscale = snapshot.tailscale.data.as_ref();

    let internet_tone = internet_health_tone(router);
    let internet_value = active_uplink_detail(router);
    let internet_meta = router
        .and_then(|router| router.active_resolver.as_ref())
        .map(|resolver| format!("DNS {}", uplink_label(resolver.uplink)))
        .unwrap_or_else(|| "默认路由 / DNS 尚未确认".to_owned());

    let lan_tone = component_tone(&snapshot.router);
    let lan_value = router.map_or_else(
        || "LAN 状态不可用".to_owned(),
        |router| {
            format!(
                "{} · {} 台客户端",
                router.lan_address.as_deref().unwrap_or(MISSING),
                router.ap_client_count.unwrap_or(0)
            )
        },
    );
    let lan_meta = router
        .and_then(|router| router.sta_ssid.as_deref())
        .map_or_else(
            || "AP / LAN 与上游 Wi-Fi".to_owned(),
            |ssid| format!("上游 Wi-Fi · {ssid}"),
        );

    let proxy_tone = component_tone(&snapshot.proxy);
    let proxy_value = proxy
        .map(mihomo_core_status_label)
        .unwrap_or_else(|| "代理状态不可用".to_owned());
    let proxy_meta = proxy
        .map(|proxy| format!("LAN TUN · {}", lan_tun_status_label(proxy)))
        .unwrap_or_else(|| "LAN TUN 状态不可用".to_owned());

    let tailscale_tone = component_tone(&snapshot.tailscale);
    let tailscale_value = tailscale
        .map(tailscale_status_detail)
        .unwrap_or_else(|| "Tailscale 状态不可用".to_owned());

    html! {
        <section class={SECTION} aria-labelledby="overview-health-title">
            <PageHeader
                title_id="overview-health-title"
                eyebrow="HEALTH"
                title="核心健康状态"
            >
                <span class={SECTION_META}>{"先确认 WAN、LAN、Proxy 与 Tailscale，再查看拓扑和运行详情"}</span>
            </PageHeader>
            <div class={KPI_GRID} aria-label="核心网络健康">
                <MetricCard
                    label="Internet / WAN"
                    value={internet_value}
                    meta={internet_meta}
                    status={Some(AttrValue::from(health_status_label(internet_tone)))}
                    status_tone={classes!(internet_tone.class())}
                />
                <MetricCard
                    label="LAN / Wi-Fi"
                    value={lan_value}
                    meta={lan_meta}
                    status={Some(AttrValue::from(health_status_label(lan_tone)))}
                    status_tone={classes!(lan_tone.class())}
                />
                <MetricCard
                    label="Proxy"
                    value={proxy_value}
                    meta={proxy_meta}
                    status={Some(AttrValue::from(health_status_label(proxy_tone)))}
                    status_tone={classes!(proxy_tone.class())}
                />
                <MetricCard
                    label="Tailscale"
                    value={tailscale_value}
                    meta="独立覆盖网络状态"
                    status={Some(AttrValue::from(health_status_label(tailscale_tone)))}
                    status_tone={classes!(tailscale_tone.class())}
                />
            </div>
        </section>
    }
}
'''
    old = r'''    let internet_tone = active_status
        .map(|status| uplink_tone(Some(status)))
        .unwrap_or_else(|| {
            if [ethernet, wifi]
                .into_iter()
                .flatten()
                .any(|status| status.session_up == Some(true))
            {
                Tone::Warn
            } else if [ethernet, wifi]
                .into_iter()
                .flatten()
                .all(|status| status.link_up == Some(false))
            {
                Tone::Bad
            } else {
                Tone::Neutral
            }
        });'''
    if old in text:
        text = replace_once(
            text,
            old,
            "    let internet_tone = internet_health_tone(router);",
            "shared WAN health semantics",
        )
    OVERVIEW.write_text(text)


def migrate_e2e() -> None:
    text = SHELL_SPEC.read_text()
    if 'name: "核心健康状态"' not in text:
        test_start = text.index(
            'test("renders the overview, apps, and anonymous system control"'
        )
        next_test = text.index('\ntest("', test_start + 1)
        segment = text[test_start:next_test]
        anchor = r'''  await expect(
    page.getByRole("button", { name: "总览", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
'''
        assertions = anchor + r'''  const coreHealth = page.getByRole("region", { name: "核心健康状态" });
  await expect(coreHealth).toBeVisible();
  for (const label of ["Internet / WAN", "LAN / Wi-Fi", "Proxy", "Tailscale"]) {
    const card = coreHealth.locator("article").filter({ hasText: label });
    await expect(card).toBeVisible();
    await expect(card.getByText(/正常|需检查|不可用|未知/, { exact: true })).toBeVisible();
  }
'''
        segment = replace_once(segment, anchor, assertions, "Overview core-health E2E")
        text = text[:test_start] + segment + text[next_test:]

    degraded_title = 'test("keeps the last dashboard while a component becomes degraded"'
    if 'const proxyHealth = page.getByRole("region", { name: "核心健康状态" })' not in text:
        test_start = text.index(degraded_title)
        next_test = text.find('\ntest("', test_start + 1)
        next_test = len(text) if next_test < 0 else next_test
        segment = text[test_start:next_test]
        degraded_anchor = r'''  await expect(page.getByText("代理探测暂时不可用")).toBeVisible({
    timeout: 7_500,
  });
  await expect(page.getByRole("heading", { name: "代理状态" })).toBeVisible();
'''
        degraded_new = r'''  await expect(page.getByText("代理探测暂时不可用")).toBeVisible({
    timeout: 7_500,
  });
  const proxyHealth = page
    .getByRole("region", { name: "核心健康状态" })
    .locator("article")
    .filter({ hasText: "Proxy" });
  await expect(proxyHealth.getByText("需检查", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "代理状态" })).toBeVisible();
'''
        segment = replace_once(
            segment,
            degraded_anchor,
            degraded_new,
            "degraded Proxy health E2E",
        )
        text = text[:test_start] + segment + text[next_test:]
    SHELL_SPEC.write_text(text)


def validate() -> None:
    overview = OVERVIEW.read_text()
    required = (
        "{render_health_summary(snapshot)}",
        "{render_issues(snapshot)}",
        "{render_topology(snapshot)}",
        'title="核心健康状态"',
        'label="Internet / WAN"',
        'label="LAN / Wi-Fi"',
        'label="Proxy"',
        'label="Tailscale"',
        "internet_health_tone(router)",
    )
    missing = [token for token in required if token not in overview]
    if missing:
        raise SystemExit(f"Overview health migration incomplete: {missing}")
    if "render_kpis(snapshot)" in overview:
        raise SystemExit("legacy Overview KPI rendering still present")

    metric = METRIC_CARD.read_text()
    forbidden = ("AppState", "StatusSnapshot", "ProxyStatus", "TailscaleStatus")
    found = [token for token in forbidden if token in metric]
    if found:
        raise SystemExit(f"MetricCard gained behavior dependencies: {found}")

    spec = SHELL_SPEC.read_text()
    if 'getByRole("region", { name: "核心健康状态" })' not in spec:
        raise SystemExit("Overview core-health E2E assertion missing")
    if 'getByText("需检查", { exact: true })' not in spec:
        raise SystemExit("degraded Proxy health assertion missing")


migrate_metric_card()
migrate_ui_tokens()
migrate_overview()
migrate_e2e()
validate()
print("web refactor Overview health priority: ready")
