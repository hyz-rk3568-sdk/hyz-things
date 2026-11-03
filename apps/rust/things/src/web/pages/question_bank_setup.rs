//! Shared module cards and category choices for original questions and saved collections.
use super::*;

#[derive(Properties, PartialEq)]
pub(super) struct QuestionBankModuleCardProps {
    pub name: AttrValue,
    pub summary: AttrValue,
    pub value: usize,
    pub max: usize,
    pub selected: bool,
    pub on_select: Callback<MouseEvent>,
    pub test_id: &'static str,
    pub children: Children,
}

#[function_component(QuestionBankModuleCard)]
pub(super) fn question_bank_module_card(props: &QuestionBankModuleCardProps) -> Html {
    html! {
        <article class="question-bank-module-card" data-selected={props.selected.to_string()}>
            <div class="question-bank-module-heading">
                <button type="button" data-testid={props.test_id} aria-pressed={props.selected.to_string()}
                    onclick={props.on_select.clone()}>
                    <strong>{props.name.clone()}</strong><span>{props.summary.clone()}</span>
                </button>
                <progress class="progress progress-primary" max={props.max.max(1).to_string()}
                    value={props.value.to_string()} aria-label={format!("{}进度",props.name)} />
            </div>
            <div class="question-bank-categories" role="group" aria-label={format!("{}二级题型",props.name)}>
                {for props.children.iter()}
            </div>
        </article>
    }
}

#[derive(Properties, PartialEq)]
pub(super) struct QuestionBankCategoryButtonProps {
    pub label: AttrValue,
    pub count: AttrValue,
    pub selected: bool,
    pub on_select: Callback<MouseEvent>,
    pub test_id: &'static str,
}

#[function_component(QuestionBankCategoryButton)]
pub(super) fn question_bank_category_button(props: &QuestionBankCategoryButtonProps) -> Html {
    html! {
        <button type="button" class="question-bank-category" data-testid={props.test_id}
            aria-pressed={props.selected.to_string()} onclick={props.on_select.clone()}>
            <span>{props.label.clone()}</span><small>{props.count.clone()}</small>
        </button>
    }
}
