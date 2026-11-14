//! Shared module cards and category choices for original questions and saved collections.
use super::*;
use hyz_things::domain::question_bank::PaperSummary;

pub(super) fn paper_catalog_label(papers: &[PaperSummary]) -> String {
    let mut years = papers
        .iter()
        .filter(|paper| paper.role == "primary")
        .map(|paper| paper.year);
    let Some(first) = years.next() else {
        return "0 份原卷".into();
    };
    let (min, max, count) = years.fold((first, first, 1), |(min, max, count), year| {
        (min.min(year), max.max(year), count + 1)
    });
    format!("{min}—{max} · {count} 份原卷")
}

#[derive(Properties, PartialEq)]
pub(super) struct QuestionBankPaperPickerProps {
    pub papers: Vec<PaperSummary>,
    pub selected_id: String,
    pub on_select: Callback<String>,
}

#[function_component(QuestionBankPaperPicker)]
pub(super) fn question_bank_paper_picker(props: &QuestionBankPaperPickerProps) -> Html {
    let expanded_regions = web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|width| width.as_f64())
        .filter(|width| *width >= 1024.0)
        .map(|_| 2)
        .unwrap_or(1);
    let mut grouped = std::collections::BTreeMap::<
        String,
        std::collections::BTreeMap<i32, Vec<&PaperSummary>>,
    >::new();
    for paper in props.papers.iter().filter(|paper| paper.role == "primary") {
        grouped
            .entry(paper.region.clone())
            .or_default()
            .entry(paper.year)
            .or_default()
            .push(paper);
    }
    html! {
        <div class="question-bank-paper-grid" role="group" aria-label="选择原卷">
            {for grouped.into_iter().enumerate().map(|(group_index,(region,years))| {
                let total=years.values().map(Vec::len).sum::<usize>();
                html! {
                    <details class="rounded-box border border-base-content/10 bg-base-100"
                        open={group_index<expanded_regions} data-testid="question-bank-region-group">
                        <summary class="flex cursor-pointer select-none items-center justify-between px-4 py-3"
                            data-testid="question-bank-region">
                            <span class="font-semibold">{region}</span>
                            <span class="text-xs text-base-content/50">{format!("{total}份")}</span>
                        </summary>
                        <div class="space-y-2 border-t border-base-content/10 p-3">
                            {for years.into_iter().rev().enumerate().map(|(year_index,(year,items))|html! {
                                <details class="question-bank-year-group rounded-box bg-base-200/50"
                                    open={year_index==0} data-testid="question-bank-year-group">
                                    <summary class="flex items-center justify-between px-3 py-2 text-sm"
                                        data-testid="question-bank-year">
                                        <span>{format!("{year} 年")}</span>
                                        <span class="text-xs text-base-content/45">{format!("{} 份",items.len())}</span>
                                    </summary>
                                    <div class="grid gap-2 px-2 pb-2">
                                        {for items.into_iter().map(|paper| {
                                            let id=paper.id.clone();
                                            let active=props.selected_id==id;
                                            let on_select=props.on_select.clone();
                                            html! {
                                                <button type="button" class="question-bank-paper-option"
                                                    data-testid="question-bank-paper-option" aria-pressed={active.to_string()}
                                                    onclick={Callback::from(move |_|on_select.emit(id.clone()))}>
                                                    <span class="min-w-0">
                                                        <strong class="block text-sm">{paper.title.clone()}</strong>
                                                        if !paper.variant.is_empty() {
                                                            <span class="mt-0.5 block text-xs text-base-content/50">{paper.variant.clone()}</span>
                                                        }
                                                    </span>
                                                    <span class="shrink-0 text-xs text-base-content/50">{format!("{}题",paper.questions)}</span>
                                                </button>
                                            }
                                        })}
                                    </div>
                                </details>
                            })}
                        </div>
                    </details>
                }
            })}
        </div>
    }
}

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
