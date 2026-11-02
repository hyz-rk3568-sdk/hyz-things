//! One user-facing entry for historical practices across existing independent stores.
//! This is an aggregation only; sessions remain owned by their original modules.
use super::*;
use hyz_things::domain::question_bank::{PracticeMode, PracticeSession};

#[derive(Clone, PartialEq)]
struct TrainingHistoryRow {
    id: String,
    source: &'static str,
    title: String,
    subtitle: String,
    timestamp: i64,
    active: bool,
}

#[derive(Properties, PartialEq)]
pub(super) struct TrainingHistoryProps {
    pub(super) on_open_source: Callback<String>,
}

#[function_component(TrainingHistory)]
pub(super) fn training_history(props: &TrainingHistoryProps) -> Html {
    let records = use_state(Vec::<TrainingHistoryRow>::new);
    let loading = use_state(|| true);
    let problem = use_state(|| None::<String>);
    {
        let records = records.clone();
        let loading = loading.clone();
        let problem = problem.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let mut rows = Vec::new();
                let mut errors = Vec::new();

                match fetch_json::<Vec<PracticeSession>>("/api/v1/question-bank/practice/history", "真题练习记录").await {
                    Ok(items) => {
                        for run in items {
                            let label = match run.mode {
                                PracticeMode::Paper => "整卷训练",
                                PracticeMode::PaperModule => "原卷模块",
                                PracticeMode::Module => "模块刷题",
                                PracticeMode::Wrong => "真题错题",
                                PracticeMode::Favorite => "真题收藏",
                            };
                            rows.push(TrainingHistoryRow {
                                id: run.id,
                                source: "question_bank",
                                title: format!("真题 · {label}"),
                                subtitle: format!("{} · {}题 · 已用{}分钟",run.module.unwrap_or_default(),run.items.len(),run.elapsed_seconds/60),
                                timestamp: run.updated_at,
                                active: run.status == "active",
                            });
                        }
                    },
                    Err(_) => errors.push("真题"),
                }
                match fetch_json_typed::<Vec<ComprehensiveHistoryEntry>>(&format!("{ANALYSIS_ROOT}/history"),"资料分析历史").await {
                    Ok(items) => for run in items {
                        rows.push(TrainingHistoryRow {
                            id: run.id, source: "analysis", title: "资料分析 · 综合训练".into(),
                            subtitle: format!("正确 {}/{} · 用时{}分钟",run.correct,run.total,run.elapsed_ms/60000),
                            timestamp: run.started_at_ms, active: false,
                        });
                    },
                    Err(_) => errors.push("资料分析"),
                }
                if let Ok(items) = fetch_json_typed::<Vec<UnfinishedRun>>(&format!("{ANALYSIS_ROOT}/unfinished"),"资料分析未完成").await {
                    for run in items {
                        if rows.iter().any(|row|row.source=="analysis" && row.id==run.id) { continue; }
                        rows.push(TrainingHistoryRow {
                            id:run.id, source:"analysis", title:"资料分析 · 未完成".into(),
                            subtitle:format!("已答{}题 · 可继续练习",run.answered),
                            timestamp:0,active:true,
                        });
                    }
                }
                for (source,root,title) in [("logic",LOGIC_ROOT,"判断推理"),("quant",QUANT_ROOT,"数量关系")] {
                    match fetch_json::<Vec<LogicHistory>>(&format!("{root}/history"),"专项训练历史").await {
                        Ok(items) => for run in items {
                            rows.push(TrainingHistoryRow{
                                id:run.id,source,title:format!("{title} · 综合训练"),
                                subtitle:format!("正确 {}/{} · 已答{}题",run.correct,run.total,run.answered),
                                timestamp:run.ended_at_ms,active:false,
                            });
                        },
                        Err(_) => errors.push(title),
                    }
                    if let Ok(items) = fetch_json::<Vec<LogicRun>>(&format!("{root}/unfinished"),"专项未完成练习").await {
                        for run in items {
                            if rows.iter().any(|row|row.source==source && row.id==run.id) { continue; }
                            rows.push(TrainingHistoryRow {
                                id:run.id,source,title:format!("{title} · 未完成"),
                                subtitle:format!("已答{}题 · 可继续练习",run.answers.iter().filter(|answer|answer.is_some()).count()),
                                timestamp:0,active:true,
                            });
                        }
                    }
                }
                rows.sort_by(|a,b| b.active.cmp(&a.active).then_with(||b.timestamp.cmp(&a.timestamp)).then_with(||a.id.cmp(&b.id)));
                records.set(rows);
                if !errors.is_empty() {
                    problem.set(Some(format!("部分模块记录暂不可用：{}",errors.join("、"))));
                }
                loading.set(false);
            });
            || ()
        });
    }
    let on_open_source = props.on_open_source.clone();
    html! {
        <TrainingSetupLayout test_id="training-unified-history">
            <div class="space-y-1">
                <h2 class="text-xl font-bold">{"练习记录"}</h2>
                <p class="text-sm text-base-content/65">{"统一查看各模块的未完成与已交卷记录，原始作答数据仍保留在各自模块。"}</p>
            </div>
            if *loading {
                <p class="text-sm">{"正在加载练习记录… "}</p>
            }
            if let Some(message) = problem.as_ref() {
                <p role="status" class="rounded-box bg-warning/10 px-3 py-2 text-sm">{message.clone()}</p>
            }
            if !*loading && records.is_empty() {
                <div class="rounded-box bg-base-100 p-5 text-sm text-base-content/65">{"还没有练习记录，开始一轮训练后会显示在这里。"}</div>
            }
            <div class="space-y-2" data-testid="training-history-list">
                {for records.iter().map(|record|{
                    let source=record.source.to_string();
                    let click=on_open_source.clone();
                    html!{
                        <button type="button" class="flex min-h-16 w-full items-center justify-between gap-3 rounded-box border border-base-content/10 bg-base-100 px-4 py-3 text-left"
                            data-testid="training-history-item" onclick={Callback::from(move |_|click.emit(source.clone()))}>
                            <span class="min-w-0">
                                <strong class="block truncate">{record.title.clone()}</strong>
                                <small class="mt-1 block text-base-content/60">{record.subtitle.clone()}</small>
                            </span>
                            <span class="shrink-0 text-sm text-primary">{if record.active {"继续 →"} else {"查看 →"}}</span>
                        </button>
                    }
                })}
            </div>
        </TrainingSetupLayout>
    }
}
