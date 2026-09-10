#!/usr/bin/env python3
"""Extract the first low-risk shared Yew presentation components.

This stage intentionally preserves existing DOM semantics, accessible names, and
visual class tokens. It does not change navigation, Overview information priority,
or capability state machines.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
MAIN = WEB / "main.rs"
OVERVIEW = WEB / "pages/overview.rs"
COMPONENTS = WEB / "components"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if new in text:
        return text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def write_components() -> None:
    COMPONENTS.mkdir(exist_ok=True)

    (COMPONENTS / "mod.rs").write_text(
        '''use super::*;

mod metric_card;
mod states;

pub(crate) use metric_card::*;
pub(crate) use states::*;
'''
    )

    (COMPONENTS / "metric_card.rs").write_text(
        '''use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct MetricCardProps {
    pub(crate) label: AttrValue,
    pub(crate) value: AttrValue,
    pub(crate) meta: AttrValue,
}

#[function_component(MetricCard)]
pub(crate) fn metric_card(props: &MetricCardProps) -> Html {
    html! {
        <article class={KPI_CARD}>
            <span class={KPI_LABEL}>{props.label.clone()}</span>
            <strong class={KPI_VALUE} title={props.value.clone()}>{props.value.clone()}</strong>
            <small class={KPI_META}>{props.meta.clone()}</small>
        </article>
    }
}
'''
    )

    (COMPONENTS / "states.rs").write_text(
        '''use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct LoadingStateProps {
    pub(crate) title_id: AttrValue,
    pub(crate) title: AttrValue,
    #[prop_or(3)]
    pub(crate) skeletons: usize,
}

#[function_component(LoadingState)]
pub(crate) fn loading_state(props: &LoadingStateProps) -> Html {
    html! {
        <section class={LOADING_GRID} aria-labelledby={props.title_id.clone()} aria-busy="true">
            <h2 id={props.title_id.clone()} class="sr-only">{props.title.clone()}</h2>
            {for (0..props.skeletons).map(|_| html! { <div class={SKELETON} aria-hidden="true"></div> })}
        </section>
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct EmptyStateProps {
    pub(crate) title_id: AttrValue,
    pub(crate) title: AttrValue,
    pub(crate) message: AttrValue,
}

#[function_component(EmptyState)]
pub(crate) fn empty_state(props: &EmptyStateProps) -> Html {
    html! {
        <section class={EMPTY_STATE} role="alert" aria-labelledby={props.title_id.clone()}>
            <span class={EMPTY_ICON} aria-hidden="true">{"!"}</span>
            <h2 id={props.title_id.clone()} class={EMPTY_TITLE}>{props.title.clone()}</h2>
            <p class={EMPTY_COPY}>{props.message.clone()}</p>
        </section>
    }
}
'''
    )


def update_main() -> None:
    text = MAIN.read_text()
    text = replace_once(
        text,
        "mod app;\nmod ui;\n",
        "mod app;\nmod components;\nmod ui;\n",
        "components module declaration",
    )
    text = replace_once(
        text,
        "use app::*;\nuse hooks::*;\n",
        "use app::*;\nuse components::*;\nuse hooks::*;\n",
        "components module import",
    )
    MAIN.write_text(text)


def update_overview() -> None:
    text = OVERVIEW.read_text()

    old_states = '''            } else if state.loading {
                <section class={LOADING_GRID} aria-labelledby="overview-loading-title" aria-busy="true">
                    <h2 id="overview-loading-title" class="sr-only">{"正在加载状态"}</h2>
                    {for (0..3).map(|_| html! { <div class={SKELETON} aria-hidden="true"></div> })}
                </section>
            } else {
                <section class={EMPTY_STATE} role="alert" aria-labelledby="overview-empty-title">
                    <span class={EMPTY_ICON} aria-hidden="true">{"!"}</span>
                    <h2 id="overview-empty-title" class={EMPTY_TITLE}>{"暂时无法读取状态"}</h2>
                    <p class={EMPTY_COPY}>{"面板会自动重试，无需刷新页面。"}</p>
                </section>
            }
'''
    new_states = '''            } else if state.loading {
                <LoadingState title_id="overview-loading-title" title="正在加载状态" />
            } else {
                <EmptyState
                    title_id="overview-empty-title"
                    title="暂时无法读取状态"
                    message="面板会自动重试，无需刷新页面。"
                />
            }
'''
    text = replace_once(text, old_states, new_states, "Overview loading/empty states")

    old_kpis = '''        <section class={KPI_GRID} aria-label="关键网络指标">
            {kpi("活动上行", active, "Ethernet 优先 · Wi-Fi fallback")}
            {kpi("Ethernet WAN", ethernet_address, "DHCP / 默认路由 metric 100")}
            {kpi("Wi-Fi WAN", wifi_address, "DHCP / 默认路由 metric 600")}
            {kpi("LAN 数据面", clients, "代理 TUN / 普通 NAT · AP 客户端")}
        </section>
    }
}

pub(crate) fn kpi(label: &'static str, value: String, meta: &'static str) -> Html {
    html! {
        <article class={KPI_CARD}>
            <span class={KPI_LABEL}>{label}</span>
            <strong class={KPI_VALUE} title={value.clone()}>{value}</strong>
            <small class={KPI_META}>{meta}</small>
        </article>
    }
}
'''
    new_kpis = '''        <section class={KPI_GRID} aria-label="关键网络指标">
            <MetricCard label="活动上行" value={active} meta="Ethernet 优先 · Wi-Fi fallback" />
            <MetricCard label="Ethernet WAN" value={ethernet_address} meta="DHCP / 默认路由 metric 100" />
            <MetricCard label="Wi-Fi WAN" value={wifi_address} meta="DHCP / 默认路由 metric 600" />
            <MetricCard label="LAN 数据面" value={clients} meta="代理 TUN / 普通 NAT · AP 客户端" />
        </section>
    }
}
'''
    text = replace_once(text, old_kpis, new_kpis, "Overview metric cards")
    OVERVIEW.write_text(text)


def validate() -> None:
    main = MAIN.read_text()
    overview = OVERVIEW.read_text()

    for path in [
        COMPONENTS / "mod.rs",
        COMPONENTS / "metric_card.rs",
        COMPONENTS / "states.rs",
    ]:
        if not path.exists():
            raise SystemExit(f"shared component file missing: {path.relative_to(ROOT)}")

    for marker in ["mod components;", "use components::*;"]:
        if marker not in main:
            raise SystemExit(f"main.rs missing shared component wiring: {marker}")

    required_overview = [
        '<LoadingState title_id="overview-loading-title" title="正在加载状态" />',
        '<EmptyState',
        'title_id="overview-empty-title"',
        'aria-label="关键网络指标"',
        '<MetricCard label="活动上行"',
        '<MetricCard label="Ethernet WAN"',
        '<MetricCard label="Wi-Fi WAN"',
        '<MetricCard label="LAN 数据面"',
    ]
    missing = [marker for marker in required_overview if marker not in overview]
    if missing:
        raise SystemExit(f"Overview shared-component migration incomplete: {missing}")
    if "pub(crate) fn kpi(" in overview:
        raise SystemExit("Overview still owns the old KPI rendering helper")
    if overview.count("<MetricCard ") != 4:
        raise SystemExit("Overview must render exactly four existing KPI cards")


if __name__ == "__main__":
    write_components()
    update_main()
    update_overview()
    validate()
    print("extracted first shared presentation components")
