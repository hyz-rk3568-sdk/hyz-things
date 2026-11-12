//! Shared fixed-height, two-pane training workspace for exam and review.
//! Each caller owns the content and independent scrolling of its panes.
use super::*;

#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/web/split_resize.js")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = mountSplitResize)]
    fn mount_split_resize(
        root: &HtmlElement,
        handle: &HtmlElement,
        breakpoint: u32,
    ) -> js_sys::Function;
}

pub(super) const TRAINING_PANE_HEADER: &str =
    "relative z-30 flex min-h-12 shrink-0 items-center justify-between gap-2 px-3 py-2";

#[derive(Properties, PartialEq)]
pub(super) struct TrainingHeaderProps {
    pub(super) class: Classes,
    pub(super) children: Children,
}

#[function_component(TrainingHeader)]
pub(super) fn training_header(props: &TrainingHeaderProps) -> Html {
    html! {
        <header class={props.class.clone()} data-testid="training-workspace-header">
            {for props.children.iter()}
        </header>
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ReviewChoiceState {
    Correct,
    Wrong,
    Neutral,
}

impl ReviewChoiceState {
    pub(super) fn for_single_choice(index: usize, correct: usize, chosen: Option<usize>) -> Self {
        if index == correct {
            Self::Correct
        } else if chosen == Some(index) {
            Self::Wrong
        } else {
            Self::Neutral
        }
    }

    fn appearance(self) -> (&'static str, &'static str) {
        match self {
            Self::Correct => ("correct", "border-success bg-success/10 text-success"),
            Self::Wrong => ("wrong", "border-error bg-error/10 text-error"),
            Self::Neutral => ("neutral", "border-base-content/20"),
        }
    }
}

#[derive(Properties, PartialEq)]
pub(super) struct TrainingReviewChoiceProps {
    pub(super) class: Classes,
    pub(super) test_id: AttrValue,
    pub(super) state: ReviewChoiceState,
    pub(super) children: Children,
}

#[function_component(TrainingReviewChoice)]
pub(super) fn training_review_choice(props: &TrainingReviewChoiceProps) -> Html {
    let (state, appearance) = props.state.appearance();
    html! {
        <div class={classes!(props.class.clone(), appearance)} data-testid={props.test_id.clone()} data-state={state}>
            {for props.children.iter()}
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub(super) struct TrainingSplitWorkspaceProps {
    pub(super) left: Html,
    pub(super) right: Html,
    pub(super) test_id: &'static str,
    #[prop_or_default]
    pub(super) split_at_md: bool,
    #[prop_or_default]
    pub(super) resizable: bool,
}

#[function_component(TrainingSplitWorkspace)]
pub(super) fn training_split_workspace(props: &TrainingSplitWorkspaceProps) -> Html {
    let root = use_node_ref();
    let handle = use_node_ref();
    {
        let root = root.clone();
        let handle = handle.clone();
        use_effect_with(
            (props.resizable, props.split_at_md),
            move |(resizable, split_at_md)| {
                let cleanup = if *resizable {
                    root.cast::<HtmlElement>()
                        .zip(handle.cast::<HtmlElement>())
                        .map(|(root, handle)| {
                            mount_split_resize(
                                &root,
                                &handle,
                                if *split_at_md { 768 } else { 1024 },
                            )
                        })
                } else {
                    None
                };
                move || {
                    if let Some(cleanup) = cleanup {
                        let _ = cleanup.call0(&wasm_bindgen::JsValue::NULL);
                    }
                }
            },
        );
    }
    if props.resizable {
        html! {
            <div ref={root} class="training-split-resizable grid h-full min-h-0 min-w-0"
                data-testid={props.test_id} data-split-resizable="true"
                data-split-breakpoint={if props.split_at_md {"md"} else {"lg"}}>
                {props.left.clone()}
                <div ref={handle} class="training-split-handle" role="separator" tabindex="0"
                    aria-label="调整题目与解析区域比例" title="长按拖动；方向键调整，Home 恢复默认"
                    data-testid="training-split-handle" data-swipe-ignore="true"></div>
                {props.right.clone()}
            </div>
        }
    } else {
        // Data analysis and reasoning retain their existing split geometry.
        let layout = if props.split_at_md {
            "grid h-full min-h-0 min-w-0 grid-cols-1 grid-rows-[minmax(0,3fr)_minmax(0,2fr)] gap-2 md:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] md:grid-rows-1"
        } else {
            "grid h-full min-h-0 min-w-0 grid-cols-1 grid-rows-[minmax(0,3fr)_minmax(0,2fr)] gap-2 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)] lg:grid-rows-1"
        };
        html! {
            <div class={layout} data-testid={props.test_id}>
                {props.left.clone()}
                {props.right.clone()}
            </div>
        }
    }
}
