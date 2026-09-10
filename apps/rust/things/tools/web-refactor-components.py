#!/usr/bin/env python3
"""Extract shared Yew presentation components as one behavior-preserving batch.

This stage preserves existing DOM semantics, accessible names, visual class tokens,
and capability state machines. It intentionally does not change navigation or the
Overview information hierarchy.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
MAIN = WEB / "main.rs"
OVERVIEW = WEB / "pages/overview.rs"
NETWORK = WEB / "pages/network.rs"
PROXY = WEB / "pages/proxy.rs"
TAILSCALE = WEB / "pages/tailscale.rs"
SYSTEM = WEB / "pages/system.rs"
COMPONENTS = WEB / "components"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if new in text:
        return text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def function_bounds(text: str, marker: str) -> tuple[int, int]:
    start = text.index(marker)
    search_from = start + len(marker)
    candidates = [
        index
        for token in ("\npub(crate) fn ", "\n#[function_component")
        if (index := text.find(token, search_from)) >= 0
    ]
    return start, min(candidates) if candidates else len(text)


def migrate_root_section(
    path: Path,
    function_marker: str,
    old_open: str,
    new_open: str,
    label: str,
) -> None:
    """Replace one function's outer SECTION wrapper without touching nested sections."""
    text = path.read_text()
    start, end = function_bounds(text, function_marker)
    segment = text[start:end]

    if new_open not in segment:
        segment = replace_once(segment, old_open, new_open, f"{label} opening")

    if "</SectionCard>" not in segment:
        close = segment.rfind("</section>")
        if close < 0:
            raise SystemExit(f"{label}: outer closing section not found")
        segment = segment[:close] + "</SectionCard>" + segment[close + len("</section>") :]

    path.write_text(text[:start] + segment + text[end:])


def write_components() -> None:
    COMPONENTS.mkdir(exist_ok=True)

    (COMPONENTS / "mod.rs").write_text(
        '''use super::*;

mod feedback;
mod metric_card;
mod page_header;
mod section_card;
mod states;
mod status_badge;

pub(crate) use feedback::*;
pub(crate) use metric_card::*;
pub(crate) use page_header::*;
pub(crate) use section_card::*;
pub(crate) use states::*;
pub(crate) use status_badge::*;
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

    (COMPONENTS / "page_header.rs").write_text(
        '''use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct PageHeaderProps {
    pub(crate) title_id: AttrValue,
    pub(crate) eyebrow: AttrValue,
    pub(crate) title: AttrValue,
    #[prop_or(false)]
    pub(crate) centered: bool,
    #[prop_or_default]
    pub(crate) children: Children,
}

#[function_component(PageHeader)]
pub(crate) fn page_header(props: &PageHeaderProps) -> Html {
    let class = if props.centered {
        SECTION_HEAD_CENTERED
    } else {
        SECTION_HEAD
    };
    html! {
        <div class={class}>
            <div>
                <p class={EYEBROW}>{props.eyebrow.clone()}</p>
                <h2 id={props.title_id.clone()} class={SECTION_TITLE}>{props.title.clone()}</h2>
            </div>
            {for props.children.iter()}
        </div>
    }
}
'''
    )

    (COMPONENTS / "section_card.rs").write_text(
        '''use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct SectionCardProps {
    pub(crate) title_id: AttrValue,
    #[prop_or_default]
    pub(crate) extra_class: Classes,
    #[prop_or_default]
    pub(crate) busy: Option<bool>,
    #[prop_or_default]
    pub(crate) children: Children,
}

#[function_component(SectionCard)]
pub(crate) fn section_card(props: &SectionCardProps) -> Html {
    let class = classes!(SECTION, props.extra_class.clone());
    if let Some(busy) = props.busy {
        html! {
            <section class={class} aria-labelledby={props.title_id.clone()} aria-busy={busy.to_string()}>
                {for props.children.iter()}
            </section>
        }
    } else {
        html! {
            <section class={class} aria-labelledby={props.title_id.clone()}>
                {for props.children.iter()}
            </section>
        }
    }
}
'''
    )

    (COMPONENTS / "feedback.rs").write_text(
        '''use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct FeedbackStateProps {
    pub(crate) message: AttrValue,
    #[prop_or(false)]
    pub(crate) hidden: bool,
}

#[function_component(FeedbackState)]
pub(crate) fn feedback_state(props: &FeedbackStateProps) -> Html {
    html! {
        <div
            class={classes!(FEEDBACK, props.hidden.then_some("invisible"))}
            role="status"
            aria-live="polite"
            aria-atomic="true"
        >
            {props.message.clone()}
        </div>
    }
}
'''
    )

    (COMPONENTS / "status_badge.rs").write_text(
        '''use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct StatusBadgeProps {
    pub(crate) label: AttrValue,
    pub(crate) tone: Classes,
}

#[function_component(StatusBadge)]
pub(crate) fn status_badge(props: &StatusBadgeProps) -> Html {
    html! {
        <span class={classes!(STATUS_BADGE, props.tone.clone())}>
            <span class={STATUS_DOT_SMALL} aria-hidden="true"></span>
            {props.label.clone()}
        </span>
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

#[derive(Properties, PartialEq)]
pub(crate) struct ErrorStateProps {
    pub(crate) message: AttrValue,
}

#[function_component(ErrorState)]
pub(crate) fn error_state(props: &ErrorStateProps) -> Html {
    html! { <div class={RISK_NOTE} role="status">{props.message.clone()}</div> }
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

    old_header = '''            <div class={SECTION_HEAD_CENTERED}>
                <div><p class={EYEBROW}>{"PATH"}</p><h2 id="topology-title" class={SECTION_TITLE}>{"网络拓扑"}</h2></div>
                <span class={SECTION_META}>{"Ethernet 优先，Wi-Fi 保持备用；下游同时经过 Router/NAT 与代理运行时"}</span>
            </div>
'''
    new_header = '''            <PageHeader title_id="topology-title" eyebrow="PATH" title="网络拓扑" centered=true>
                <span class={SECTION_META}>{"Ethernet 优先，Wi-Fi 保持备用；下游同时经过 Router/NAT 与代理运行时"}</span>
            </PageHeader>
'''
    text = replace_once(text, old_header, new_header, "Overview topology header")
    OVERVIEW.write_text(text)


def update_network() -> None:
    migrate_root_section(
        NETWORK,
        "pub(crate) fn settings(props: &SettingsProps) -> Html {",
        '<section class={SECTION} aria-labelledby="settings-title" aria-busy={busy.to_string()}>',
        '<SectionCard title_id="settings-title" busy={Some(busy)}>',
        "Network settings section",
    )
    text = NETWORK.read_text()
    old_header = '''            <div class={SECTION_HEAD_CENTERED}>
                <div>
                    <p class={EYEBROW}>{"ADMIN"}</p>
                    <h2 id="settings-title" class={SECTION_TITLE}>{"管理设置"}</h2>
                </div>
                if authenticated {
                    <div class={SESSION_ACTIONS}>
                        <span>{"管理员 · admin"}</span>
                        <button class="btn btn-ghost btn-sm text-base-content" type="button" onclick={logout} disabled={busy || csrf.is_empty()}>{"退出登录"}</button>
                    </div>
                } else {
                    <span class={SECTION_META}>{"状态面板无需登录，设置需要管理员身份"}</span>
                }
            </div>
'''
    new_header = '''            <PageHeader title_id="settings-title" eyebrow="ADMIN" title="管理设置" centered=true>
                if authenticated {
                    <div class={SESSION_ACTIONS}>
                        <span>{"管理员 · admin"}</span>
                        <button class="btn btn-ghost btn-sm text-base-content" type="button" onclick={logout} disabled={busy || csrf.is_empty()}>{"退出登录"}</button>
                    </div>
                } else {
                    <span class={SECTION_META}>{"状态面板无需登录，设置需要管理员身份"}</span>
                }
            </PageHeader>
'''
    text = replace_once(text, old_header, new_header, "Network page header")
    old_feedback = '''            if let Some(notice) = &state.settings_notice {
                <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
            }
'''
    new_feedback = '''            if let Some(notice) = &state.settings_notice {
                <FeedbackState message={AttrValue::from(notice.clone())} />
            }
'''
    text = replace_once(text, old_feedback, new_feedback, "Network action feedback")
    NETWORK.write_text(text)


def update_proxy() -> None:
    migrate_root_section(
        PROXY,
        "pub(crate) fn render_proxy_control(state: &UseReducerHandle<AppState>) -> Html {",
        '<section class={classes!(SECTION, "gap-6")} aria-labelledby="proxy-controls-title">',
        '<SectionCard title_id="proxy-controls-title" extra_class={classes!("gap-6")}>',
        "Proxy control section",
    )
    migrate_root_section(
        PROXY,
        "pub(crate) fn render_proxy_groups_read_only(component: &Component<Vec<ProxyGroup>>) -> Html {",
        '<section class={SECTION} aria-labelledby="proxy-readonly-title">',
        '<SectionCard title_id="proxy-readonly-title">',
        "Proxy read-only section",
    )
    text = PROXY.read_text()
    replacements = [
        (
            '''            <div class={SECTION_HEAD}>
                <div><p class={EYEBROW}>{"PROXY"}</p><h2 id="proxy-controls-title" class={SECTION_TITLE}>{"代理设置"}</h2></div>
                <span class={SECTION_META}>{"两个数据面独立切换，共享 Mihomo core"}</span>
            </div>
''',
            '''            <PageHeader title_id="proxy-controls-title" eyebrow="PROXY" title="代理设置">
                <span class={SECTION_META}>{"两个数据面独立切换，共享 Mihomo core"}</span>
            </PageHeader>
''',
            "Proxy control header",
        ),
        (
            '''            <div class={SECTION_HEAD}>
                <div><p class={EYEBROW}>{"PROXY STATUS"}</p><h2 id="proxy-readonly-title" class={SECTION_TITLE}>{"当前代理与延迟"}</h2></div>
                <span class={SECTION_META}>{"只读 · 修改需管理员登录"}</span>
            </div>
''',
            '''            <PageHeader title_id="proxy-readonly-title" eyebrow="PROXY STATUS" title="当前代理与延迟">
                <span class={SECTION_META}>{"只读 · 修改需管理员登录"}</span>
            </PageHeader>
''',
            "Proxy read-only header",
        ),
        (
            '''                    if let Some(notice) = &state.lan_tun_notice {
                        <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
                    }
''',
            '''                    if let Some(notice) = &state.lan_tun_notice {
                        <FeedbackState message={AttrValue::from(notice.clone())} />
                    }
''',
            "LAN TUN feedback",
        ),
        (
            '''                    if let Some(notice) = &state.local_system_proxy_notice {
                        <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
                    }
''',
            '''                    if let Some(notice) = &state.local_system_proxy_notice {
                        <FeedbackState message={AttrValue::from(notice.clone())} />
                    }
''',
            "Local system proxy feedback",
        ),
        (
            '''            if let Some(notice) = &state.node_notice {
                <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
            }
''',
            '''            if let Some(notice) = &state.node_notice {
                <FeedbackState message={AttrValue::from(notice.clone())} />
            }
''',
            "Proxy node feedback",
        ),
    ]
    for old, new, label in replacements:
        text = replace_once(text, old, new, label)
    PROXY.write_text(text)


def update_tailscale() -> None:
    text = TAILSCALE.read_text()
    replacements = [
        (
            '<div class={SECTION_HEAD}><div><p class={EYEBROW}>{"REMOTE LAN"}</p><h2 id="tailscale-title" class={SECTION_TITLE}>{"Tailscale 远程 LAN"}</h2></div></div>',
            '<PageHeader title_id="tailscale-title" eyebrow="REMOTE LAN" title="Tailscale 远程 LAN" />',
            "Tailscale loading header",
        ),
        (
            '''            <div class={SECTION_HEAD}>
                <div><p class={EYEBROW}>{"REMOTE LAN"}</p><h2 id="tailscale-title" class={SECTION_TITLE}>{"Tailscale 远程 LAN"}</h2></div>
                <span class={SECTION_META}>{"固定 192.168.8.0/24 · 不提供 Exit Node"}</span>
            </div>
''',
            '''            <PageHeader title_id="tailscale-title" eyebrow="REMOTE LAN" title="Tailscale 远程 LAN">
                <span class={SECTION_META}>{"固定 192.168.8.0/24 · 不提供 Exit Node"}</span>
            </PageHeader>
''',
            "Tailscale page header",
        ),
        (
            '<span class={classes!(STATUS_BADGE, tone.class())}><span class={STATUS_DOT_SMALL} aria-hidden="true"></span>{status}</span>',
            '<StatusBadge label={status} tone={classes!(tone.class())} />',
            "Tailscale peer status badge",
        ),
        (
            '<div class={RISK_NOTE} role="status">{format!("设备列表读取失败，当前显示上次成功数据，数据可能已过期：{error}")}</div>',
            '<ErrorState message={format!("设备列表读取失败，当前显示上次成功数据，数据可能已过期：{error}")} />',
            "Tailscale stale peer error",
        ),
        (
            '<div class={RISK_NOTE} role="status">{format!("Tailnet 设备列表暂不可用：{error}")}</div>',
            '<ErrorState message={format!("Tailnet 设备列表暂不可用：{error}")} />',
            "Tailscale peer error",
        ),
    ]
    for old, new, label in replacements:
        text = replace_once(text, old, new, label)
    TAILSCALE.write_text(text)


def update_system() -> None:
    migrate_root_section(
        SYSTEM,
        "pub(crate) fn render_display_control(\n",
        '<section class={SECTION} aria-labelledby="display-controls-title" aria-busy={busy.to_string()}>',
        '<SectionCard title_id="display-controls-title" busy={Some(busy)}>',
        "Display control section",
    )
    text = SYSTEM.read_text()
    replacements = [
        (
            '''                <div class={SECTION_HEAD}>
                    <div><p class={EYEBROW}>{"QUICK CONTROL"}</p><h2 id="display-controls-title" class={SECTION_TITLE}>{"设备快捷控制"}</h2></div>
                    <span class={SECTION_META}>{"仅限管理 LAN · 同源令牌保护"}</span>
                </div>
''',
            '''                <PageHeader title_id="display-controls-title" eyebrow="QUICK CONTROL" title="设备快捷控制">
                    <span class={SECTION_META}>{"仅限管理 LAN · 同源令牌保护"}</span>
                </PageHeader>
''',
            "System display header",
        ),
        (
            '''                <div class={classes!(FEEDBACK, state.display_notice.is_none().then_some("invisible"))} role="status" aria-live="polite" aria-atomic="true">
                    {state.display_notice.as_deref().unwrap_or("等待操作")}
                </div>
''',
            '''                <FeedbackState
                    message={AttrValue::from(state.display_notice.clone().unwrap_or_else(|| "等待操作".to_owned()))}
                    hidden={state.display_notice.is_none()}
                />
''',
            "System display feedback",
        ),
        (
            '<span class={classes!(STATUS_BADGE, status.1.class())}><span class={STATUS_DOT_SMALL} aria-hidden="true"></span>{status.0}</span>',
            '<StatusBadge label={status.0} tone={classes!(status.1.class())} />',
            "System status card badge",
        ),
    ]
    for old, new, label in replacements:
        text = replace_once(text, old, new, label)
    SYSTEM.write_text(text)


def validate() -> None:
    main = MAIN.read_text()
    page_text = {
        "Overview": OVERVIEW.read_text(),
        "Network": NETWORK.read_text(),
        "Proxy": PROXY.read_text(),
        "Tailscale": TAILSCALE.read_text(),
        "System": SYSTEM.read_text(),
    }

    component_paths = [
        COMPONENTS / "mod.rs",
        COMPONENTS / "feedback.rs",
        COMPONENTS / "metric_card.rs",
        COMPONENTS / "page_header.rs",
        COMPONENTS / "section_card.rs",
        COMPONENTS / "states.rs",
        COMPONENTS / "status_badge.rs",
    ]
    for path in component_paths:
        if not path.exists():
            raise SystemExit(f"shared component file missing: {path.relative_to(ROOT)}")

    forbidden_component_dependencies = [
        "AppState",
        "UseReducerHandle",
        "StatusSnapshot",
        "NetworkConfigDto",
        "ProxyStatus",
        "TailscaleStatus",
        "CameraStatus",
        "Request::",
    ]
    for path in component_paths[1:]:
        content = path.read_text()
        leaked = [name for name in forbidden_component_dependencies if name in content]
        if leaked:
            raise SystemExit(
                f"{path.relative_to(ROOT)} leaks capability state/API dependencies: {', '.join(leaked)}"
            )

    for marker in ["mod components;", "use components::*;"]:
        if marker not in main:
            raise SystemExit(f"main.rs missing shared component wiring: {marker}")

    required = {
        "Overview": [
            '<LoadingState title_id="overview-loading-title" title="正在加载状态" />',
            '<EmptyState',
            '<MetricCard label="活动上行"',
            '<PageHeader title_id="topology-title"',
        ],
        "Network": [
            '<SectionCard title_id="settings-title" busy={Some(busy)}>',
            '<PageHeader title_id="settings-title"',
            '<FeedbackState message={AttrValue::from(notice.clone())} />',
        ],
        "Proxy": [
            '<SectionCard title_id="proxy-controls-title"',
            '<SectionCard title_id="proxy-readonly-title">',
            '<PageHeader title_id="proxy-controls-title"',
            '<PageHeader title_id="proxy-readonly-title"',
            '<FeedbackState message={AttrValue::from(notice.clone())} />',
        ],
        "Tailscale": [
            '<PageHeader title_id="tailscale-title"',
            '<StatusBadge label={status} tone={classes!(tone.class())} />',
            '<ErrorState message={format!',
        ],
        "System": [
            '<SectionCard title_id="display-controls-title" busy={Some(busy)}>',
            '<PageHeader title_id="display-controls-title"',
            '<FeedbackState',
            '<StatusBadge label={status.0} tone={classes!(status.1.class())} />',
        ],
    }
    for page, markers in required.items():
        missing = [marker for marker in markers if marker not in page_text[page]]
        if missing:
            raise SystemExit(f"{page} shared-component migration incomplete: {missing}")

    if "pub(crate) fn kpi(" in page_text["Overview"]:
        raise SystemExit("Overview still owns the old KPI rendering helper")
    if page_text["Overview"].count("<MetricCard ") != 4:
        raise SystemExit("Overview must render exactly four existing KPI cards")
    if page_text["Proxy"].count("<FeedbackState ") != 3:
        raise SystemExit("Proxy must preserve all three action feedback regions")
    if page_text["Tailscale"].count("<ErrorState ") != 2:
        raise SystemExit("Tailscale must preserve both peer-list error states")


if __name__ == "__main__":
    write_components()
    update_main()
    update_overview()
    update_network()
    update_proxy()
    update_tailscale()
    update_system()
    validate()
    print("extracted shared headers, sections, status badges, feedback, and error states")
