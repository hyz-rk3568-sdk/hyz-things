use super::*;

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
