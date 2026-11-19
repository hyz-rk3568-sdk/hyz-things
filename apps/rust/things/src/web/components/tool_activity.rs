//! Collapsible tool-call group shared by saved AI analysis and AI practice.
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub(crate) struct ToolActivityProps {
    pub test_id: AttrValue,
    #[prop_or_default]
    pub summary_test_id: Option<AttrValue>,
    pub label: String,
    pub summary: String,
    pub opened: bool,
    pub on_toggle: Callback<MouseEvent>,
    #[prop_or_default]
    pub children: Children,
}

#[function_component(ToolActivity)]
pub(crate) fn tool_activity(props: &ToolActivityProps) -> Html {
    html! {
        <div data-testid={props.test_id.clone()} class="min-w-0 max-w-full py-1">
            <button type="button" class="flex w-full min-w-0 items-start gap-2 text-left text-xs text-base-content/60 hover:text-base-content"
                aria-expanded={props.opened.to_string()} onclick={props.on_toggle.clone()}>
                <span class="shrink-0 whitespace-nowrap">{format!("{} {}", if props.opened {"⌄"} else {"›"}, props.label)}</span>
                <span data-testid={props.summary_test_id.clone()} class="question-bank-ai-progress-summary flex-1">{props.summary.clone()}</span>
            </button>
            if props.opened {
                <div class="mt-2 min-w-0 max-w-full space-y-3 border-l border-base-content/15 pl-3">
                    {for props.children.iter()}
                </div>
            }
        </div>
    }
}
