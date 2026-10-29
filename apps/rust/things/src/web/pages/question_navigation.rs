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
