use yew::prelude::*;

/// Shared shell for the 行测 question-bank AI review and the AI-practice agent.
/// Both modes keep the question/answer context, the status line, the transcript
/// scroll area and a bottom composer in exactly the same places.
#[derive(Properties, PartialEq)]
pub(crate) struct AiReviewPanelProps {
    pub section_test_id: &'static str,
    pub messages_test_id: &'static str,
    #[prop_or_default]
    pub question_test_id: Option<AttrValue>,
    pub status_test_id: &'static str,
    #[prop_or_default]
    pub section_ref: NodeRef,
    #[prop_or_default]
    pub messages_ref: NodeRef,
    #[prop_or_default]
    pub on_messages_scroll: Callback<Event>,
    #[prop_or_default]
    pub question: Option<Html>,
    pub title: AttrValue,
    pub status: AttrValue,
    #[prop_or_default]
    pub composer: Html,
    pub children: Children,
}

#[function_component(AiReviewPanel)]
pub(crate) fn ai_review_panel(props: &AiReviewPanelProps) -> Html {
    html! {
        <section class="flex min-h-0 min-w-0 flex-1 flex-col" data-testid={props.section_test_id}
            ref={props.section_ref.clone()}>
            <div class="min-h-0 flex-1 space-y-3 overflow-y-auto overscroll-contain touch-pan-y p-4"
                ref={props.messages_ref.clone()} onscroll={props.on_messages_scroll.clone()}
                data-testid={props.messages_test_id} data-ink-pane="answer"
                role="log" aria-label="AI 对话记录" aria-live="polite">
                if let Some(question) = props.question.clone() {
                    <div class="space-y-4" data-testid={props.question_test_id.clone()}>{question}</div>
                }
                <div class="flex items-center justify-between gap-2">
                    <strong>{props.title.clone()}</strong>
                    <span class="text-xs text-base-content/60" data-testid={props.status_test_id}>{props.status.clone()}</span>
                </div>
                {for props.children.iter()}
            </div>
            {props.composer.clone()}
        </section>
    }
}
