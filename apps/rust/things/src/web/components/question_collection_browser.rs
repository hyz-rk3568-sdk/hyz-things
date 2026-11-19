use super::*;
use yew::prelude::*;

/// One deck group (module or 卡组) with its second-level categories.
#[derive(Clone, PartialEq)]
pub(crate) struct CollectionBrowserGroup {
    pub name: String,
    pub summary: String,
    pub value: usize,
    pub max: usize,
    pub categories: Vec<(String, usize)>,
}

/// One openable row. The caller keeps the underlying items so `on_open`
/// only has to report the position inside the filtered list.
#[derive(Clone, PartialEq)]
pub(crate) struct CollectionBrowserRow {
    pub title: String,
    pub subtitle: String,
    pub badge: String,
    pub disabled: bool,
}

/// Shared 收藏/错题/卡片/AI 练习收藏 browser: left groups and categories,
/// right filtered rows. 真题 collections and AI-practice favorites render
/// through this one component so their interaction and layout cannot drift.
#[derive(Properties, PartialEq)]
pub(crate) struct QuestionCollectionBrowserProps {
    pub groups: Vec<CollectionBrowserGroup>,
    pub rows: Vec<CollectionBrowserRow>,
    #[prop_or_default]
    pub selected_group: String,
    #[prop_or_default]
    pub selected_category: String,
    pub groups_label: AttrValue,
    pub all_label: AttrValue,
    pub all_selected: bool,
    pub result_label: AttrValue,
    pub result_count: AttrValue,
    pub empty_label: AttrValue,
    /// Suppresses the results body while the caller shows its loading state.
    #[prop_or_default]
    pub pending: bool,
    pub group_test_id: &'static str,
    pub category_test_id: &'static str,
    pub item_test_id: &'static str,
    #[prop_or_default]
    pub list_test_id: Option<AttrValue>,
    #[prop_or_default]
    pub count_test_id: Option<AttrValue>,
    pub on_select_all: Callback<MouseEvent>,
    pub on_select_group: Callback<String>,
    pub on_select_category: Callback<(String, String)>,
    pub on_open: Callback<usize>,
}

#[function_component(QuestionCollectionBrowser)]
pub(crate) fn question_collection_browser(props: &QuestionCollectionBrowserProps) -> Html {
    html! {
        <div class="question-bank-collections-layout">
            <div class="space-y-3">
                <div class="flex items-center justify-between gap-2">
                    <h3 class="font-semibold">{props.groups_label.clone()}</h3>
                    <button type="button" class="btn btn-ghost btn-xs"
                        aria-pressed={props.all_selected.to_string()}
                        onclick={props.on_select_all.clone()}>
                        {props.all_label.clone()}
                    </button>
                </div>
                <div class="question-bank-collection-grid">
                    {for props.groups.iter().map(|group| {
                        let selected = props.selected_group == group.name;
                        let group_name = group.name.clone();
                        let select_group = Callback::from({
                            let on_select_group = props.on_select_group.clone();
                            move |_| on_select_group.emit(group_name.clone())
                        });
                        html! {
                            <QuestionBankModuleCard name={group.name.clone()} summary={group.summary.clone()}
                                value={group.value} max={group.max} selected={selected}
                                test_id={props.group_test_id} on_select={select_group.clone()}>
                                <QuestionBankCategoryButton label="全部" count={group.value.to_string()}
                                    selected={selected && props.selected_category.is_empty()}
                                    test_id={props.category_test_id} on_select={select_group.clone()}/>
                                {for group.categories.iter().map(|(category, count)| {
                                    let group_name = group.name.clone();
                                    let category_name = category.clone();
                                    let on_select_category = props.on_select_category.clone();
                                    html! {
                                        <QuestionBankCategoryButton label={category.clone()} count={count.to_string()}
                                            selected={selected && props.selected_category == *category}
                                            test_id={props.category_test_id}
                                            on_select={Callback::from(move |_| on_select_category.emit((
                                                group_name.clone(), category_name.clone()
                                            )))}/>
                                    }
                                })}
                            </QuestionBankModuleCard>
                        }
                    })}
                </div>
            </div>
            <div class="space-y-3 min-w-0">
                <div class="flex min-h-6 items-center justify-between gap-2">
                    <h3 class="font-semibold">{props.result_label.clone()}</h3>
                    if let Some(test_id) = props.count_test_id.clone() {
                        <span class="text-xs text-base-content/50" data-testid={test_id}>{props.result_count.clone()}</span>
                    } else {
                        <span class="text-xs text-base-content/50">{props.result_count.clone()}</span>
                    }
                </div>
                <div class="question-bank-results">
                    if !props.pending {
                        if props.rows.is_empty() {
                            <p class="p-4 text-sm text-base-content/60">{props.empty_label.clone()}</p>
                        } else {
                            <div class="question-bank-results-list" data-testid={props.list_test_id.clone()}>
                                {for props.rows.iter().enumerate().map(|(index, row)| {
                                    let on_open = props.on_open.clone();
                                    html! {
                                        <QuestionCollectionRow key={index.to_string()} title={row.title.clone()} subtitle={row.subtitle.clone()}
                                            badge={row.badge.clone()} test_id={props.item_test_id}
                                            disabled={row.disabled}
                                            on_open={Callback::from(move |_| on_open.emit(index))}/>
                                    }
                                })}
                            </div>
                        }
                    }
                </div>
            </div>
        </div>
    }
}
