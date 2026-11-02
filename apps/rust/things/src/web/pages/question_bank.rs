use super::*;
use hyz_things::domain::question_bank::{BankSyncSummary, PaperSummary, PaperView};

#[derive(Properties, PartialEq)]
pub(crate) struct QuestionBankPanelProps {
    pub state: UseReducerHandle<AppState>,
    pub is_admin: bool,
}

#[function_component(QuestionBankPanel)]
pub(crate) fn question_bank_panel(props: &QuestionBankPanelProps) -> Html {
    let papers = use_state(Vec::<PaperSummary>::new);
    let paper = use_state(|| None::<PaperView>);
    let notice = use_state(|| None::<String>);
    let busy = use_state(|| false);
    let reload = use_state(|| 0u64);
    {
        let papers = papers.clone();
        let paper = paper.clone();
        let notice = notice.clone();
        use_effect_with((props.is_admin, *reload), move |(is_admin, _)| {
            if *is_admin {
                spawn_local(async move {
                    match fetch_json::<Vec<PaperSummary>>(
                        "/api/v1/question-bank/papers",
                        "题库列表",
                    )
                    .await
                    {
                        Ok(list) => papers.set(list),
                        Err(error) => notice.set(Some(error)),
                    }
                });
            } else {
                papers.set(Vec::new());
                paper.set(None);
            }
            || ()
        });
    }
    {
        let current = (*paper).clone();
        use_effect_with(current, move |_| {
            hydrate_math();
            || ()
        });
    }
    let csrf = props
        .state
        .panel
        .as_ref()
        .map(|panel| panel.csrf_token.clone())
        .unwrap_or_default();
    let on_sync = {
        let busy = busy.clone();
        let notice = notice.clone();
        let reload = reload.clone();
        let paper = paper.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            busy.set(true);
            notice.set(None);
            let busy = busy.clone();
            let notice = notice.clone();
            let reload = reload.clone();
            let paper = paper.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let result: Result<BankSyncSummary, String> = post_json_response(
                    "/api/v1/control/question-bank/sync",
                    &csrf,
                    &EmptyRequest {},
                    "同步题库",
                )
                .await;
                match result {
                    Ok(summary) => {
                        notice.set(Some(format!(
                            "已同步 {} 份文件、{} 个题位、{} 份材料",
                            summary.papers, summary.questions, summary.materials
                        )));
                        paper.set(None);
                        reload.set(*reload + 1);
                    }
                    Err(error) => notice.set(Some(error)),
                }
                busy.set(false);
            });
        })
    };
    let on_open = {
        let busy = busy.clone();
        let notice = notice.clone();
        let paper = paper.clone();
        Callback::from(move |id: String| {
            if *busy {
                return;
            }
            busy.set(true);
            notice.set(None);
            let busy = busy.clone();
            let notice = notice.clone();
            let paper = paper.clone();
            spawn_local(async move {
                match fetch_json::<PaperView>(
                    &format!("/api/v1/question-bank/papers/{id}"),
                    "整卷加载",
                )
                .await
                {
                    Ok(value) => paper.set(Some(value)),
                    Err(error) => notice.set(Some(error)),
                }
                busy.set(false);
            });
        })
    };
    html! {
        <SectionCard title_id="question-bank-title" busy={Some(*busy)}>
            <div class={SECTION_HEAD}>
                <div><h3 id="question-bank-title" class={SECTION_TITLE}>{"行测题库"}</h3><p class={SECTION_META}>{"历年真题 · 整卷阅读"}</p></div>
                if props.is_admin {
                    <button class={BUTTON_PRIMARY} type="button" data-testid="question-bank-sync" disabled={*busy || csrf.is_empty()} onclick={on_sync}>{if *busy {"处理中…"} else {"同步题库"}}</button>
                }
            </div>
            if !props.is_admin {
                <crate::app::AdminAuthGate state={props.state.clone()} />
            } else {
                if let Some(message)=&*notice {<div class={FEEDBACK} role="status" data-testid="question-bank-notice">{message.clone()}</div>}
                if let Some(current)=&*paper {
                    <button class={BUTTON} type="button" onclick={Callback::from({let paper=paper.clone();move |_|paper.set(None)})}>{"返回试卷列表"}</button>
                    {render_paper(current)}
                } else {
                    <div class="max-h-96 overflow-y-auto space-y-2" data-testid="question-bank-papers">
                        if papers.is_empty() {<p class={HELP_TEXT}>{"题库为空。首次使用请先同步题库。"}</p>}
                        {for papers.iter().map(|entry| {
                            let open=on_open.clone();let id=entry.id.clone();
                            html! {<button class="btn btn-outline h-auto min-h-12 w-full justify-between py-3 text-left" type="button" data-testid="question-bank-open" disabled={*busy} onclick={Callback::from(move |_|open.emit(id.clone()))}>
                                <span>{entry.title.clone()}</span><span class="text-xs opacity-60">{format!("{} · {} · {} 个题位",entry.year,entry.region,entry.questions)}</span>
                            </button>}
                        })}
                    </div>
                }
            }
        </SectionCard>
    }
}

fn render_paper(paper: &PaperView) -> Html {
    let mut shown = std::collections::BTreeSet::new();
    html! {<div class="space-y-6" data-testid="question-bank-paper">
        <h4 class={SECTION_TITLE}>{paper.title.clone()}</h4>
        <p class={HELP_TEXT}>{"保留来源题号；缺损或待核项随题标明。本页只加载题面和材料。"}</p>
        {for paper.questions.iter().map(|question| {
            let materials=question.materials.iter().filter(|id|shown.insert((*id).clone())).filter_map(|id|paper.materials.iter().find(|m|&m.id==id)).collect::<Vec<_>>();
            html! {<article class="space-y-3 border-t border-base-content/10 pt-5" data-testid="question-bank-question">
                {for materials.iter().map(|material|html!{<section class="rounded-box bg-base-200 p-4" data-testid="question-bank-material"><h5 class="font-bold">{format!("材料 {}",material.name)}</h5><div class="study-markdown mt-3">{render_question_bank_markdown(&material.markdown)}</div></section>})}
                <h5 class="font-bold">{format!("原卷第 {} 题",question.number)}</h5>
                if !question.issues.is_empty() {<p class="text-xs text-warning">{format!("待核标记：{}",question.issues.join("、"))}</p>}
                if !question.materials.is_empty() {<p class={HELP_TEXT}>{"引用材料见本题组前文。"}</p>}
                <div class="study-markdown">{render_question_bank_markdown(&question.stem)}</div>
                <div class="study-markdown">{render_question_bank_markdown(&question.options_markdown)}</div>
            </article>}
        })}
    </div>}
}
