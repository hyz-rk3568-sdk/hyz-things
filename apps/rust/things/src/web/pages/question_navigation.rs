//! Shared question navigation for timed drills and post-submit review.
use super::*;

// A selected question keeps its grade during review. An active paper must
// never expose correctness: Some(_) only means "already answered" there.
fn question_tab_style(selected: bool, marker: Option<bool>, is_review: bool) -> &'static str {
    if selected {
        if is_review {
            match marker {
                Some(true) => "btn btn-success btn-xs shrink-0",
                Some(false) => "btn btn-error btn-xs shrink-0",
                None => "btn btn-primary btn-xs shrink-0",
            }
        } else {
            "btn btn-primary btn-xs shrink-0"
        }
    } else if !is_review && marker.is_some() {
        "btn btn-primary btn-outline btn-xs shrink-0"
    } else {
        match marker {
            Some(true) => "btn btn-success btn-outline btn-xs shrink-0",
            Some(false) => "btn btn-error btn-outline btn-xs shrink-0",
            None => "btn btn-ghost btn-xs shrink-0",
        }
    }
}

/// Markers: None = unanswered, Some(true) = answered / correct,
/// Some(false) = incorrect (only after submission).
///
/// Data analysis groups its 20 questions into four materials of five.
/// Logic judgment displays all ten questions without material tabs.
pub(super) fn question_navigation(
    current: UseStateHandle<usize>,
    index: usize,
    markers: &[Option<bool>],
    is_review: bool,
    group_size: Option<usize>,
    test_id: &'static str,
    tab_test_id: &'static str,
) -> Html {
    let total = markers.len();
    let group = group_size.filter(|size| *size > 0);
    let group_index = group.map(|size| index / size);
    let start = group.map_or(0, |size| index / size * size);
    let end = group.map_or(total, |size| (start + size).min(total));
    let previous = {
        let current = current.clone();
        Callback::from(move |_| current.set(index.saturating_sub(1)))
    };
    let next = {
        let current = current.clone();
        Callback::from(move |_| current.set((index + 1).min(total.saturating_sub(1))))
    };

    html! {
        <footer class="flex shrink-0 items-center justify-between gap-2 py-2" data-testid={test_id}>
            <button type="button" class="btn btn-outline btn-sm" disabled={index==0} onclick={previous}>{"上一题"}</button>
            <nav class="flex min-w-0 flex-1 items-center justify-center gap-1 overflow-x-auto" aria-label="题号导航">
                {for group.into_iter().flat_map(|size| 0..total.div_ceil(size)).map(|g| {
                    let current = current.clone();
                    let selected = group_index == Some(g);
                    let style = if selected { "btn btn-primary btn-xs shrink-0" } else { "btn btn-ghost btn-xs shrink-0" };
                    let size = group.unwrap_or(total);
                    html! {
                        <button type="button" class={style}
                            onclick={Callback::from(move |_| current.set(g*size))}>{format!("材料{}",g+1)}</button>
                    }
                })}
                {for (start..end).map(|i| {
                    let current = current.clone();
                    let marker = markers[i];
                    let style = question_tab_style(i == index, marker, is_review);
                    let state = if is_review {
                        match marker {
                            Some(true) => " 正确",
                            Some(false) => " 错误",
                            None => " 未答",
                        }
                    } else if marker.is_some() {
                        " 已答"
                    } else {
                        " 未答"
                    };
                    html! {
                        <button type="button" class={style} data-testid={tab_test_id}
                            aria-label={format!("第{}题{}",i+1,state)}
                            aria-current={(i==index).to_string()}
                            onclick={Callback::from(move |_| current.set(i))}>{i+1}</button>
                    }
                })}
            </nav>
            <button type="button" class="btn btn-primary btn-sm" disabled={index+1>=total} onclick={next}>{"下一题"}</button>
        </footer>
    }
}

#[cfg(test)]
mod tests {
    use super::question_tab_style;

    #[test]
    fn review_selected_question_keeps_graded_colour() {
        assert!(question_tab_style(true, Some(true), true).contains("btn-success"));
        assert!(question_tab_style(true, Some(false), true).contains("btn-error"));
        assert!(question_tab_style(true, None, true).contains("btn-primary"));
        assert!(question_tab_style(false, Some(true), true).contains("btn-success"));
        assert!(question_tab_style(false, Some(false), true).contains("btn-error"));
    }

    #[test]
    fn active_paper_never_reveals_grading() {
        assert!(question_tab_style(true, Some(false), false).contains("btn-primary"));
        assert!(question_tab_style(false, Some(true), false).contains("btn-primary"));
        assert!(!question_tab_style(false, Some(true), false).contains("btn-success"));
    }
}


/// Shared swipe region for all question-based training screens. Only a primary
/// finger gesture with a decisive horizontal displacement changes the item.
/// Pen, mouse, interactive controls and active inking are deliberately ignored.
#[derive(Properties, PartialEq)]
pub(super) struct TrainingSwipeSurfaceProps {
    pub(super) current: UseStateHandle<usize>,
    pub(super) total: usize,
    pub(super) children: Children,
}

#[function_component(TrainingSwipeSurface)]
pub(super) fn training_swipe_surface(props: &TrainingSwipeSurfaceProps) -> Html {
    let origin = use_mut_ref(|| None::<(i32, i32, i32)>);
    let ink_active = use_context::<bool>().unwrap_or(false);
    let down = {
        let origin = origin.clone();
        Callback::from(move |event: PointerEvent| {
            *origin.borrow_mut() = None;
            if ink_active || event.pointer_type() != "touch" || !event.is_primary()
                || !swipe_start_allowed(&event)
            {
                return;
            }
            *origin.borrow_mut() = Some((event.pointer_id(), event.client_x(), event.client_y()));
        })
    };
    let up = {
        let origin = origin.clone();
        let current = props.current.clone();
        let total = props.total;
        Callback::from(move |event: PointerEvent| {
            let start = origin.borrow_mut().take();
            let Some((pointer_id, start_x, start_y)) = start else { return; };
            if event.pointer_type() != "touch" || event.pointer_id() != pointer_id {
                return;
            }
            let dx = event.client_x() - start_x;
            let dy = event.client_y() - start_y;
            if dx.abs() < 72 || dx.abs() <= dy.abs() * 3 / 2 {
                return;
            }
            let index = *current;
            if dx < 0 && index + 1 < total {
                current.set(index + 1);
            } else if dx > 0 && index > 0 {
                current.set(index - 1);
            }
        })
    };
    let cancel = {
        let origin = origin.clone();
        Callback::from(move |_: PointerEvent| *origin.borrow_mut() = None)
    };
    html! {
        <div class="training-swipe-surface relative h-full min-h-0 min-w-0"
            data-testid="training-swipe-surface"
            onpointerdown={down} onpointerup={up} onpointercancel={cancel}>
            {for props.children.iter()}
        </div>
    }
}

/// Compact shared navigation used by large papers. All question numbers live
/// in an overlay and never grow the fixed footer beyond one row.
#[derive(Properties, PartialEq)]
pub(super) struct CompactQuestionNavigationProps {
    pub(super) current: UseStateHandle<usize>,
    pub(super) numbers: Vec<u32>,
    pub(super) answered: Vec<bool>,
}

#[function_component(CompactQuestionNavigation)]
pub(super) fn compact_question_navigation(props: &CompactQuestionNavigationProps) -> Html {
    let open = use_state(|| false);
    let total = props.numbers.len();
    let index = (*props.current).min(total.saturating_sub(1));
    let previous = {
        let current = props.current.clone();
        Callback::from(move |_| current.set(index.saturating_sub(1)))
    };
    let next = {
        let current = props.current.clone();
        Callback::from(move |_| current.set((index + 1).min(total.saturating_sub(1))))
    };
    let toggle = {
        let open = open.clone();
        Callback::from(move |_| open.set(!*open))
    };
    html! {
        <footer class="training-compact-footer relative z-40 flex shrink-0 items-center justify-between gap-2 border-t border-base-content/10 py-2"
            data-testid="question-bank-practice-navigation">
            <button type="button" class="btn btn-outline btn-sm" disabled={index==0} onclick={previous}>{"上一题"}</button>
            <button type="button" class="btn btn-ghost btn-sm" aria-expanded={open.to_string()}
                aria-controls="training-question-sheet" data-testid="training-answer-sheet-toggle" onclick={toggle.clone()}>
                {format!("答题卡 · {}/{}",index+1,total)}
            </button>
            <button type="button" class="btn btn-primary btn-sm" disabled={index+1>=total} onclick={next}>{"下一题"}</button>
            if *open {
                <nav id="training-question-sheet" aria-label="答题卡" data-testid="training-answer-sheet"
                    class="absolute bottom-full left-0 right-0 max-h-[min(55dvh,28rem)] overflow-y-auto overscroll-contain rounded-t-box border border-base-content/15 bg-base-100 p-3 shadow-xl">
                    <div class="mb-3 flex items-center justify-between">
                        <strong>{"答题卡"}</strong>
                        <button class="btn btn-ghost btn-xs" onclick={toggle.clone()}>{"收起"}</button>
                    </div>
                    <div class="grid grid-cols-6 gap-2 sm:grid-cols-10">
                        {for props.numbers.iter().enumerate().map(|(i,number)| {
                            let current = props.current.clone();
                            let open = open.clone();
                            let done = props.answered.get(i).copied().unwrap_or(false);
                            html! {<button type="button"
                                class={classes!("btn","btn-sm",if i==index {"btn-primary"} else if done {"btn-outline border-primary/40"} else {"btn-ghost"})}
                                data-testid="question-bank-practice-number"
                                aria-current={(i==index).to_string()}
                                aria-label={format!("原卷第 {number} 题")}
                                onclick={Callback::from(move |_|{current.set(i);open.set(false);})}>{number}</button>}
                        })}
                    </div>
                </nav>
            }
        </footer>
    }
}
