use super::*;

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
