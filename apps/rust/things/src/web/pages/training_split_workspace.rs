//! Shared fixed-height, two-pane training workspace for exam and review.
//! Each caller owns the content and independent scrolling of its panes.
use super::*;

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
}

#[function_component(TrainingSplitWorkspace)]
pub(super) fn training_split_workspace(props: &TrainingSplitWorkspaceProps) -> Html {
    // Keep data analysis' 60:40 geometry and portrait stacking unchanged.
    // Logic uses the same geometry but activates the split on iPad widths.
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
