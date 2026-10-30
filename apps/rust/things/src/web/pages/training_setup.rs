//! Viewport-bounded settings, history, and results inside the training shell.
use super::*;

pub(super) const TRAINING_PRIMARY_BUTTON: &str = "btn btn-primary h-auto min-h-12 w-full p-3";

#[derive(Properties, PartialEq)]
pub(super) struct TrainingSetupLayoutProps {
    pub(super) test_id: AttrValue,
    #[prop_or_default]
    pub(super) summary: AttrValue,
    #[prop_or_default]
    pub(super) detail: AttrValue,
    #[prop_or_default]
    pub(super) action: Option<Html>,
    #[prop_or_default]
    pub(super) heading: Option<Html>,
    pub(super) children: Children,
}

#[function_component(TrainingSetupLayout)]
pub(super) fn training_setup_layout(props: &TrainingSetupLayoutProps) -> Html {
    let scroll = use_node_ref();
    {
        let scroll = scroll.clone();
        use_effect_with(props.test_id.clone(), move |_| {
            if let Some(element) = scroll.cast::<web_sys::HtmlElement>() {
                element.set_scroll_top(0);
            }
            || ()
        });
    }
    html! {
        <section class={classes!("training-setup-layout",props.action.is_none().then_some("training-setup-without-action"))} data-testid={props.test_id.clone()}>
            if let Some(heading) = props.heading.clone() {
                <div class="training-setup-heading">{heading}</div>
            }
            <div ref={scroll} class="training-setup-scroll" data-testid="training-setup-scroll">
                <div class="training-setup-content">{for props.children.iter()}</div>
            </div>
            if let Some(action) = props.action.clone() {
                <footer class="training-setup-action" data-testid="training-setup-action">
                    <div class="training-action-inner">
                        <div class="min-w-0"><strong>{props.summary.clone()}</strong><p>{props.detail.clone()}</p></div>
                        <div class="training-primary-action">{action}</div>
                    </div>
                </footer>
            }
        </section>
    }
}

#[derive(Properties, PartialEq)]
pub(super) struct TrainingQuestionCountProps {
    pub(super) value: usize,
    pub(super) on_change: Callback<usize>,
    pub(super) test_id: &'static str,
    #[prop_or("题")]
    pub(super) unit: &'static str,
}

#[function_component(TrainingQuestionCount)]
pub(super) fn training_question_count(props: &TrainingQuestionCountProps) -> Html {
    html! {
        <div class="training-count-options" role="group" aria-label="本轮题数" data-testid={props.test_id}>
            {for [10, 20, 50].into_iter().map(|value| {
                let on_change = props.on_change.clone();
                html! {<button type="button" data-testid={format!("{}-{value}",props.test_id)} aria-pressed={(props.value == value).to_string()}
                    onclick={Callback::from(move |_| on_change.emit(value))}>{format!("{value} {}",props.unit)}</button>}
            })}
        </div>
    }
}
