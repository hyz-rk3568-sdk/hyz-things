use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct MetricCardProps {
    pub(crate) label: AttrValue,
    pub(crate) value: AttrValue,
    pub(crate) meta: AttrValue,
    #[prop_or_default]
    pub(crate) status: Option<AttrValue>,
    #[prop_or_default]
    pub(crate) status_tone: Classes,
}

#[function_component(MetricCard)]
pub(crate) fn metric_card(props: &MetricCardProps) -> Html {
    html! {
        <article class={KPI_CARD}>
            <div class={KPI_HEAD}>
                <span class={KPI_LABEL}>{props.label.clone()}</span>
                if let Some(status) = &props.status {
                    <StatusBadge label={status.clone()} tone={props.status_tone.clone()} />
                }
            </div>
            <strong class={KPI_VALUE} title={props.value.clone()}>{props.value.clone()}</strong>
            <small class={KPI_META}>{props.meta.clone()}</small>
        </article>
    }
}
