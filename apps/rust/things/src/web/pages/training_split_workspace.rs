//! Shared fixed-height, two-pane training workspace for exam and review.
//! Each caller owns the content and independent scrolling of its panes.
use super::*;

pub(super) const TRAINING_PANE_HEADER: &str =
    "relative z-30 flex min-h-12 shrink-0 items-center justify-between gap-2 px-3 py-2";

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
