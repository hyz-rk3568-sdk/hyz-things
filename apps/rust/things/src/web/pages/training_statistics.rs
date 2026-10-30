//! Real stored learning durations, not quiz elapsed-time estimates.
use super::*;
use std::collections::BTreeMap;

fn module_name(id:&str)->&'static str {
    match id {
        "calculation"=>"速算训练",
        "flashcards"=>"错题复练",
        "comprehensive"=>"资料分析",
        "logic"=>"判断推理",
        "quantitative"=>"数量关系",
        _=>"其他",
    }
}
fn hours(ms:i64)->String { format!("{:.1} 小时",ms as f64/3_600_000.0) }
fn minutes(ms:i64)->String {format!("{} 分钟",ms/60_000)}

#[function_component(TrainingStatistics)]
pub(super) fn training_statistics()->Html {
    let stats=use_state(||None::<TrainingTimeStats>);
    let error=use_state(||None::<String>);
    let selected=use_state(||None::<String>);
    {
        let stats=stats.clone();let error=error.clone();
        use_effect_with((),move |_|{
            spawn_local(async move {
                match fetch_json_typed::<TrainingTimeStats>(&format!("{TRAINING_TIME_ROOT}/stats"),"学习统计").await {
                    Ok(v)=>stats.set(Some(v)),
                    Err(e)=>error.set(Some(e.to_string())),
                }
            });||()
        });
    }
    let Some(data)=stats.as_ref() else {
        return html!{<TrainingSetupLayout test_id="training-statistics">
            <h2 class="text-xl font-bold">{"学习统计"}</h2>
            if let Some(e)=error.as_ref(){<p role="alert" class="text-error">{e.clone()}</p>}
            else{<p class="opacity-60">{"正在读取真实学习记录…"}</p>}
        </TrainingSetupLayout>};
    };
    let today=data.days.last().map(|d|d.total_ms).unwrap_or_default();
    let week=data.days.iter().rev().take(7).map(|d|d.total_ms).sum::<i64>();
    let total=data.days.iter().map(|d|d.total_ms).sum::<i64>();
    let streak=data.days.iter().rev().skip_while(|d|d.total_ms==0).take_while(|d|d.total_ms>0).count();
    let chosen=data.days.iter().find(|d|Some(&d.date)==selected.as_ref())
        .or_else(||data.days.last());
    let mut by_module=BTreeMap::<String,i64>::new();
    if let Some(day)=chosen {
        for (k,v) in &day.modules {*by_module.entry(k.clone()).or_default()+=v;}
    }
    html!{
        <TrainingSetupLayout test_id="training-statistics">
            <div class="space-y-5" data-testid="training-statistics-content">
                <div><h2 class="text-2xl font-bold">{"学习统计"}</h2><p class="mt-1 text-sm opacity-60">{"按北京时间统计有效学习时间 · 真实数据"}</p></div>
                <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
                    {for [("今日",hours(today)),("最近7天",hours(week)),("最近35天",hours(total)),("连续学习",format!("{streak} 天"))]
                        .into_iter().map(|(label,value)|html!{<div class="rounded-xl bg-base-100 p-3"><p class="text-xs opacity-60">{label}</p><strong class="text-lg">{value}</strong></div>})}
                </div>
                <div class="rounded-xl bg-base-100 p-4 space-y-3">
                    <h3 class="font-semibold">{"每日学习热力图"}</h3>
                    <div class="grid grid-cols-7 gap-2" data-testid="training-heatmap">
                        {for ["一","二","三","四","五","六","日"].into_iter().map(|d|html!{<span class="text-center text-xs opacity-60">{d}</span>})}
                        {for data.days.iter().map(|day|{
                            let date=day.date.clone();
                            let active=Some(&date)==selected.as_ref();
                            let level=if day.total_ms==0 {0}else if day.total_ms<3_600_000{1}else if day.total_ms<10_800_000{2}else if day.total_ms<21_600_000{3}else{4};
                            let color=["bg-base-300","bg-emerald-950","bg-emerald-800","bg-emerald-600","bg-emerald-400"][level];
                            let onselect={let selected=selected.clone();let date=date.clone();Callback::from(move |_|selected.set(Some(date.clone())))};
                            html!{<button type="button" class={classes!("aspect-square","min-h-7","rounded-md",color,active.then_some("ring-2 ring-primary"))}
                                data-testid="study-heat-day" aria-label={format!("{}：{}",day.date,minutes(day.total_ms))}
                                title={format!("{} · {}",day.date,minutes(day.total_ms))} onclick={onselect}/>}
                        })}
                    </div>
                    <p class="text-xs opacity-60">{"颜色越深，学习时间越长。点击日期查看明细。"}</p>
                </div>
                <div class="rounded-xl bg-base-100 p-4 space-y-3">
                    <h3 class="font-semibold">{format!("{} · 模块耗时",chosen.map(|d|d.date.as_str()).unwrap_or("今日"))}</h3>
                    if by_module.is_empty() {
                        <p class="text-sm opacity-60">{"当天暂无有效学习记录，开始训练后自动生成。"}</p>
                    }else {
                        {for by_module.iter().map(|(id,ms)|{
                            let max=chosen.map(|d|d.total_ms).unwrap_or(0).max(1);
                            let pct=(*ms as f64/max as f64*100.0).clamp(0.0,100.0);
                            html!{<div class="space-y-1"><div class="flex justify-between text-sm"><span>{module_name(id)}</span><span>{minutes(*ms)}</span></div>
                                <div class="h-2 rounded-full bg-base-300"><div class="h-full rounded-full bg-primary" style={format!("width:{pct:.1}%")}/></div></div>}
                        })}
                    }
                </div>
            </div>
        </TrainingSetupLayout>
    }
}
