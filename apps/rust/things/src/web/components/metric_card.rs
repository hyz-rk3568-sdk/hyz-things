use super::*;

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
