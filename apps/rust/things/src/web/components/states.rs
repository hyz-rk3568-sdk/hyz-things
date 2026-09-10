use super::*;

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
