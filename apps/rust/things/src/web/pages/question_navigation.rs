//! Shared question navigation for timed drills and post-submit review.
use super::*;
use gloo_timers::callback::Timeout;

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
        <footer class="relative z-40 flex shrink-0 items-center justify-between gap-2 border-t border-base-content/10 py-2" data-testid={test_id}>
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
            <TrainingGeneratedAnswerSheet current={current.clone()} markers={markers.to_vec()} is_review={is_review} group_size={group}/>
            <button type="button" class="btn btn-primary btn-sm" disabled={index+1>=total} onclick={next}>{"下一题"}</button>
        </footer>
    }
}


#[derive(Properties, PartialEq)]
struct TrainingGeneratedAnswerSheetProps {
    current: UseStateHandle<usize>,
    markers: Vec<Option<bool>>,
    is_review: bool,
    group_size: Option<usize>,
}

/// AI exercises use the same source-paper answer sheet widget, but only for
/// navigation and grading; this component never creates persistent wrong items.
#[function_component(TrainingGeneratedAnswerSheet)]
fn training_generated_answer_sheet(props: &TrainingGeneratedAnswerSheetProps) -> Html {
    let total = props.markers.len();
    let size = props.group_size.filter(|size| *size > 0).unwrap_or(total.max(1));
    let groups = (0..total.div_ceil(size))
        .map(|g| NavigationGroup {
            label: if props.group_size.is_some() {
                format!("材料{}", g + 1)
            } else {
                "全部题目".to_owned()
            },
            subtitle: None,
            start: g * size,
            end: ((g + 1) * size).min(total),
        })
        .collect::<Vec<_>>();
    html! {
        <TrainingAnswerSheet current={props.current.clone()}
            numbers={(1..=total).map(|i|i as u32).collect::<Vec<_>>()}
            answered={props.markers.iter().map(Option::is_some).collect::<Vec<_>>()}
            grades={props.is_review.then(|| props.markers.clone())}
            groups={groups} source_paper={false}/>
    }
}

#[cfg(test)]
mod tests {
    use super::{page_turn_direction, question_tab_style, swipe_pointer};

    #[test]
    fn swipe_accepts_touch_and_pen_but_not_mouse() {
        assert!(swipe_pointer("touch"));
        assert!(swipe_pointer("pen"));
        assert!(!swipe_pointer("mouse"));
    }

    #[test]
    fn page_turn_slides_out_towards_the_requested_neighbour() {
        assert_eq!(page_turn_direction(1, 2), "next");
        assert_eq!(page_turn_direction(2, 1), "previous");
    }

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

fn allows_navigation(guard: &Option<Callback<(), bool>>) -> bool {
    match guard {
        Some(guard) => guard.emit(()),
        None => true,
    }
}

/// Shared swipe region for all question-based training screens. Touch and pen
/// swipes with a decisive horizontal displacement change the item. Mouse and
/// interactive controls stay inert, and active inking keeps the pen for ink.
#[derive(Properties, PartialEq)]
pub(super) struct TrainingSwipeSurfaceProps {
    pub(super) current: UseStateHandle<usize>,
    pub(super) total: usize,
    pub(super) children: Children,
    #[prop_or_default]
    pub(super) navigation_allowed: Option<Callback<(), bool>>,
    /// Notified whenever a page turn commits or rolls back, so callers can
    /// drive a session instead of a plain index.
    #[prop_or_default]
    pub(super) on_change: Option<Callback<usize>>,
}

fn navigate_to(
    current: &UseStateHandle<usize>,
    on_change: &Option<Callback<usize>>,
    target: usize,
) {
    current.set(target);
    if let Some(callback) = on_change {
        callback.emit(target);
    }
}

fn swipe_pointer(pointer_type: &str) -> bool {
    matches!(pointer_type, "touch" | "pen")
}

fn page_turn_direction(from: usize, to: usize) -> &'static str {
    if to > from {
        "next"
    } else {
        "previous"
    }
}

fn copy_scroll_positions(source: &Element, clone: &Element) {
    let (Ok(source_nodes), Ok(clone_nodes)) = (
        source.query_selector_all("*"),
        clone.query_selector_all("*"),
    ) else {
        return;
    };
    for index in 0..source_nodes.length().min(clone_nodes.length()) {
        let (Some(source_node), Some(clone_node)) =
            (source_nodes.item(index), clone_nodes.item(index))
        else {
            continue;
        };
        let (Ok(source_element), Ok(clone_element)) = (
            source_node.dyn_into::<Element>(),
            clone_node.dyn_into::<Element>(),
        ) else {
            continue;
        };
        let top = source_element.scroll_top();
        let left = source_element.scroll_left();
        if top != 0 {
            clone_element.set_scroll_top(top);
        }
        if left != 0 {
            clone_element.set_scroll_left(left);
        }
    }
}

const SWIPE_LOCK_PX: i32 = 8;
const SWIPE_COMMIT_PX: i32 = 72;

#[derive(Clone, Copy)]
struct SwipeDrag {
    from: usize,
    direction: &'static str,
    width: f64,
    generation: u32,
}

/// Snapshot the live page into the overlay. The incoming page renders
/// underneath, so the snapshot can follow the finger while both pages stay
/// readable.
fn clone_page_turn(
    content: &HtmlElement,
    overlay: &HtmlElement,
    direction: &str,
) -> Option<HtmlElement> {
    while let Some(child) = overlay.first_child() {
        let _ = overlay.remove_child(&child);
    }
    let clone = content.clone_node_with_deep(true).ok()?;
    let element = clone.dyn_into::<HtmlElement>().ok()?;
    // The snapshot must not duplicate ids or test ids that belong to the live
    // page, otherwise locators become ambiguous while the page turn runs.
    let _ = element.remove_attribute("id");
    let _ = element.remove_attribute("data-testid");
    if let Ok(nodes) = element.query_selector_all("[id], [data-testid]") {
        for index in 0..nodes.length() {
            let Some(node) = nodes.item(index) else {
                continue;
            };
            let Ok(child) = node.dyn_into::<Element>() else {
                continue;
            };
            let _ = child.remove_attribute("id");
            let _ = child.remove_attribute("data-testid");
        }
    }
    let _ = element.set_attribute("aria-hidden", "true");
    let _ = element.set_attribute("inert", "");
    let _ = element.set_attribute("data-page-turn", "out");
    let _ = element.set_attribute("data-direction", direction);
    element.set_class_name(format!("{} training-page-turn", element.class_name()).trim());
    overlay.append_child(&element).ok()?;
    // Scroll offsets only apply once the snapshot has a layout box.
    copy_scroll_positions(content, &element);
    Some(element)
}

fn activate_page_turn(element: &HtmlElement) {
    element.set_class_name(format!("{} training-page-turn-active", element.class_name()).trim());
}

fn page_turn_offset(dx: i32, direction: &str) -> i32 {
    if direction == "next" {
        dx.min(0)
    } else {
        dx.max(0)
    }
}

fn apply_page_turn_offset(element: &HtmlElement, dx: i32, width: f64, direction: &str) {
    let offset = page_turn_offset(dx, direction);
    let ratio = (offset as f64 / width.max(1.0)).clamp(-1.0, 1.0);
    let angle = ratio * 8.0;
    let scale = 1.0 - ratio.abs() * 0.015;
    let style = element.style();
    let _ = style.set_property("transition", "none");
    let _ = style.set_property(
        "transform",
        &format!("perspective(1200px) translateX({offset}px) rotateY({angle}deg) scale({scale})"),
    );
}

fn settle_page_turn(element: &HtmlElement, direction: &str, commit: bool) {
    let target = if !commit {
        "perspective(1200px) translateX(0) rotateY(0) scale(1)"
    } else if direction == "next" {
        "perspective(1200px) translateX(-100%) rotateY(-8deg) scale(.985)"
    } else {
        "perspective(1200px) translateX(100%) rotateY(8deg) scale(.985)"
    };
    let style = element.style();
    let _ = style.set_property("transition", "transform 260ms cubic-bezier(.4,0,.2,1)");
    // Flush the dragging transform so the transition starts from the finger.
    let _ = element.offset_width();
    let _ = style.set_property("transform", target);
}

fn advance_generation(state: &Rc<RefCell<u32>>) -> u32 {
    let mut value = state.borrow_mut();
    *value = value.wrapping_add(1);
    *value
}

fn schedule_turn_cleanup(
    overlay: HtmlElement,
    generation_state: Rc<RefCell<u32>>,
    expected: u32,
    delay: u32,
) {
    Timeout::new(delay, move || {
        if *generation_state.borrow() != expected {
            return;
        }
        while let Some(child) = overlay.first_child() {
            let _ = overlay.remove_child(&child);
        }
    })
    .forget();
}

#[function_component(TrainingSwipeSurface)]
pub(super) fn training_swipe_surface(props: &TrainingSwipeSurfaceProps) -> Html {
    let origin = use_mut_ref(|| None::<(i32, i32, i32)>);
    let drag = use_mut_ref(|| None::<SwipeDrag>);
    let ink_active = use_context::<bool>().unwrap_or(false);
    let surface_ref = use_node_ref();
    let content_ref = use_node_ref();
    let overlay_ref = use_node_ref();
    // Older cleanup timers must not remove a newer page snapshot.
    let turn_generation = use_mut_ref(|| 0_u32);
    let down = {
        let origin = origin.clone();
        let drag = drag.clone();
        let surface_ref = surface_ref.clone();
        let navigation_allowed = props.navigation_allowed.clone();
        Callback::from(move |event: PointerEvent| {
            if !event.is_primary() || !swipe_pointer(&event.pointer_type()) {
                return;
            }
            *origin.borrow_mut() = None;
            *drag.borrow_mut() = None;
            if !allows_navigation(&navigation_allowed) || ink_active || !swipe_start_allowed(&event)
            {
                return;
            }
            *origin.borrow_mut() = Some((event.pointer_id(), event.client_x(), event.client_y()));
            if let Some(surface) = surface_ref.cast::<HtmlElement>() {
                let _ = surface.set_pointer_capture(event.pointer_id());
            }
        })
    };
    let on_move = {
        let origin = origin.clone();
        let drag = drag.clone();
        let current = props.current.clone();
        let total = props.total;
        let navigation_allowed = props.navigation_allowed.clone();
        let on_change = props.on_change.clone();
        let content_ref = content_ref.clone();
        let overlay_ref = overlay_ref.clone();
        let turn_generation = turn_generation.clone();
        Callback::from(move |event: PointerEvent| {
            let Some((pointer_id, start_x, start_y)) = *origin.borrow() else {
                return;
            };
            if !swipe_pointer(&event.pointer_type()) || event.pointer_id() != pointer_id {
                return;
            }
            let dx = event.client_x() - start_x;
            let dy = event.client_y() - start_y;
            if let Some(active) = *drag.borrow() {
                if let Some(element) = overlay_ref
                    .cast::<HtmlElement>()
                    .and_then(|overlay| overlay.first_child())
                    .and_then(|node| node.dyn_into::<HtmlElement>().ok())
                {
                    apply_page_turn_offset(&element, dx, active.width, active.direction);
                }
                return;
            }
            // Lock horizontal once the finger clearly leaves the tap zone.
            if !allows_navigation(&navigation_allowed)
                || dx.abs() < SWIPE_LOCK_PX
                || dx.abs() < dy.abs()
            {
                return;
            }
            let index = *current;
            let target = if dx < 0 {
                if index + 1 >= total {
                    return;
                }
                index + 1
            } else {
                if index == 0 {
                    return;
                }
                index - 1
            };
            let direction = page_turn_direction(index, target);
            let (Some(content), Some(overlay)) = (
                content_ref.cast::<HtmlElement>(),
                overlay_ref.cast::<HtmlElement>(),
            ) else {
                return;
            };
            let width = content.client_width().max(1) as f64;
            let Some(element) = clone_page_turn(&content, &overlay, direction) else {
                return;
            };
            let generation = advance_generation(&turn_generation);
            *drag.borrow_mut() = Some(SwipeDrag {
                from: index,
                direction,
                width,
                generation,
            });
            navigate_to(&current, &on_change, target);
            apply_page_turn_offset(&element, dx, width, direction);
        })
    };
    let up = {
        let origin = origin.clone();
        let drag = drag.clone();
        let current = props.current.clone();
        let total = props.total;
        let navigation_allowed = props.navigation_allowed.clone();
        let on_change = props.on_change.clone();
        let content_ref = content_ref.clone();
        let overlay_ref = overlay_ref.clone();
        let turn_generation = turn_generation.clone();
        Callback::from(move |event: PointerEvent| {
            let Some((pointer_id, start_x, start_y)) = *origin.borrow() else {
                return;
            };
            if !swipe_pointer(&event.pointer_type()) || event.pointer_id() != pointer_id {
                return;
            }
            *origin.borrow_mut() = None;
            let active = drag.borrow_mut().take();
            let Some(overlay) = overlay_ref.cast::<HtmlElement>() else {
                return;
            };
            if let Some(active) = active {
                let offset = page_turn_offset(event.client_x() - start_x, active.direction);
                let commit =
                    offset.abs() >= SWIPE_COMMIT_PX && allows_navigation(&navigation_allowed);
                if let Some(element) = overlay
                    .first_child()
                    .and_then(|node| node.dyn_into::<HtmlElement>().ok())
                {
                    settle_page_turn(&element, active.direction, commit);
                }
                if !commit {
                    navigate_to(&current, &on_change, active.from);
                }
                schedule_turn_cleanup(overlay, turn_generation.clone(), active.generation, 340);
                return;
            }
            if !allows_navigation(&navigation_allowed) {
                return;
            }
            // Coalesced or synthetic flicks may have no pointermove at all.
            let dx = event.client_x() - start_x;
            let dy = event.client_y() - start_y;
            if dx.abs() < SWIPE_COMMIT_PX || dx.abs() <= dy.abs() * 3 / 2 {
                return;
            }
            let index = *current;
            let target = if dx < 0 && index + 1 < total {
                index + 1
            } else if dx > 0 && index > 0 {
                index - 1
            } else {
                return;
            };
            let direction = page_turn_direction(index, target);
            if let Some(content) = content_ref.cast::<HtmlElement>() {
                if let Some(element) = clone_page_turn(&content, &overlay, direction) {
                    activate_page_turn(&element);
                    let generation = advance_generation(&turn_generation);
                    schedule_turn_cleanup(overlay, turn_generation.clone(), generation, 1_500);
                }
            }
            navigate_to(&current, &on_change, target);
        })
    };
    let cancel = {
        let origin = origin.clone();
        let drag = drag.clone();
        let current = props.current.clone();
        let on_change = props.on_change.clone();
        let overlay_ref = overlay_ref.clone();
        let turn_generation = turn_generation.clone();
        Callback::from(move |event: PointerEvent| {
            let Some((pointer_id, _, _)) = *origin.borrow() else {
                return;
            };
            if event.pointer_id() != pointer_id {
                return;
            }
            *origin.borrow_mut() = None;
            let Some(active) = drag.borrow_mut().take() else {
                return;
            };
            if let Some(overlay) = overlay_ref.cast::<HtmlElement>() {
                if let Some(element) = overlay
                    .first_child()
                    .and_then(|node| node.dyn_into::<HtmlElement>().ok())
                {
                    settle_page_turn(&element, active.direction, false);
                }
                schedule_turn_cleanup(overlay, turn_generation.clone(), active.generation, 340);
            }
            navigate_to(&current, &on_change, active.from);
        })
    };
    let turn_animation_end = Callback::from(move |event: web_sys::AnimationEvent| {
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<HtmlElement>().ok())
        else {
            return;
        };
        if target.get_attribute("data-page-turn").as_deref() != Some("out") {
            return;
        }
        if let Some(overlay) = target.parent_element() {
            let target = target.clone();
            // Keep the settled snapshot briefly so a very fast next swipe
            // starts from a stable layer instead of racing the teardown.
            Timeout::new(120, move || {
                let _ = overlay.remove_child(&target);
            })
            .forget();
        }
    });
    html! {
        <div ref={surface_ref} class="training-swipe-surface relative h-full min-h-0 min-w-0"
            data-testid="training-swipe-surface"
            onpointerdown={down} onpointermove={on_move}
            onpointerup={up} onpointercancel={cancel.clone()}
            onlostpointercapture={cancel}>
            <div ref={content_ref} class="training-swipe-content h-full min-h-0 min-w-0"
                data-testid="training-swipe-content">
                {for props.children.iter()}
            </div>
            <div ref={overlay_ref} class="pointer-events-none absolute inset-0 z-20"
                data-testid="training-page-turn-overlay" aria-hidden="true"
                onanimationend={turn_animation_end} />
        </div>
    }
}

/// One contiguous section in the compact answer sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NavigationGroup {
    pub(super) label: String,
    pub(super) subtitle: Option<String>,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// Compact shared navigation used by large papers. All question numbers live
/// in an overlay and never grow the fixed footer beyond one row.
#[derive(Properties, PartialEq)]
pub(super) struct CompactQuestionNavigationProps {
    pub(super) current: UseStateHandle<usize>,
    pub(super) numbers: Vec<u32>,
    pub(super) answered: Vec<bool>,
    pub(super) grades: Option<Vec<Option<bool>>>,
    pub(super) groups: Vec<NavigationGroup>,
    #[prop_or_default]
    pub(super) navigation_allowed: Option<Callback<(), bool>>,
}

#[derive(Properties, PartialEq)]
struct TrainingAnswerSheetProps {
    current: UseStateHandle<usize>,
    numbers: Vec<u32>,
    answered: Vec<bool>,
    grades: Option<Vec<Option<bool>>>,
    groups: Vec<NavigationGroup>,
    #[prop_or_default]
    navigation_allowed: Option<Callback<(), bool>>,
    #[prop_or(true)]
    source_paper: bool,
}

/// One answer-sheet component shared by source papers and generated drills.
/// Open/scroll state lives here; the caller owns answers, grades and navigation.
#[function_component(TrainingAnswerSheet)]
fn training_answer_sheet(props: &TrainingAnswerSheetProps) -> Html {
    let open = use_state(|| false);
    let sheet_ref = use_node_ref();
    let sheet_scroll_top = use_mut_ref(|| 0i32);
    {
        let sheet_ref = sheet_ref.clone();
        let sheet_scroll_top = sheet_scroll_top.clone();
        use_effect_with(*open, move |is_open| {
            if *is_open {
                if let Some(sheet) = sheet_ref.cast::<HtmlElement>() {
                    sheet.set_scroll_top(*sheet_scroll_top.borrow());
                }
            }
            || ()
        });
    }
    let on_sheet_scroll = {
        let sheet_scroll_top = sheet_scroll_top.clone();
        Callback::from(move |event: Event| {
            let sheet = event.target_unchecked_into::<HtmlElement>();
            *sheet_scroll_top.borrow_mut() = sheet.scroll_top();
        })
    };
    let total = props.numbers.len();
    let index = (*props.current).min(total.saturating_sub(1));
    let toggle = {
        let open = open.clone();
        let allowed = props.navigation_allowed.clone();
        Callback::from(move |_| {
            if allows_navigation(&allowed) {
                open.set(!*open);
            }
        })
    };
    html! {
        <div class="contents">
            <button type="button" class="btn btn-ghost btn-sm shrink-0" aria-expanded={open.to_string()}
                aria-controls="training-question-sheet" data-testid="training-answer-sheet-toggle" onclick={toggle.clone()}>
                {format!("答题卡 · {}/{}", index+1, total)}
            </button>
            if *open {
                <nav id="training-question-sheet" aria-label="答题卡" data-testid="training-answer-sheet"
                    ref={sheet_ref} onscroll={on_sheet_scroll}
                    class="absolute bottom-full left-0 right-0 z-50 max-h-[min(55dvh,28rem)] overflow-y-auto overscroll-contain rounded-t-box border border-base-content/15 bg-base-100 p-3 shadow-xl">
                    <div class="mb-3 flex items-center justify-between">
                        <strong>{"答题卡"}</strong>
                        <button type="button" class="btn btn-ghost btn-xs" onclick={toggle.clone()}>{"收起"}</button>
                    </div>
                    {for props.groups.iter().map(|group| {
                        let start = group.start.min(total);
                        let end = group.end.min(total);
                        html! {
                            <section class="mb-3 last:mb-0" data-testid="question-bank-practice-group"
                                data-group-label={group.label.clone()}>
                                <div class="mb-1.5 flex items-baseline justify-between gap-2">
                                    <div class="min-w-0">
                                        if let Some(subtitle) = &group.subtitle {
                                            <span class="block text-[10px] text-base-content/45"
                                                data-testid="question-bank-practice-group-module">{subtitle.clone()}</span>
                                        }
                                        <strong class="text-xs text-base-content/70">{group.label.clone()}</strong>
                                    </div>
                                    <span class="text-[11px] text-base-content/45">
                                        {format!("第 {}—{} 题 · {} 题",start+1,end,end-start)}
                                    </span>
                                </div>
                                <div class="grid grid-cols-6 gap-2 sm:grid-cols-10" data-testid="question-bank-practice-group-grid">
                                    {for (start..end).map(|i| {
                                        let current = props.current.clone();
                                        let open = open.clone();
                                        let allowed = props.navigation_allowed.clone();
                                        let number = props.numbers.get(i).copied().unwrap_or_default();
                                        let done = props.answered.get(i).copied().unwrap_or(false);
                                        let is_review = props.grades.is_some();
                                        let grade = props.grades.as_ref().and_then(|grades| grades.get(i)).copied().flatten();
                                        let style = if is_review {
                                            match grade {
                                                Some(true) => "btn-success",
                                                Some(false) => "btn-error",
                                                None => "btn-outline",
                                            }
                                        } else if i == index { "btn-primary" }
                                        else if done { "btn-outline border-primary/40" }
                                        else { "btn-ghost" };
                                        let prefix = if props.source_paper { "原卷第" } else { "第" };
                                        let label = if is_review {
                                            match grade {
                                                Some(true) => format!("{prefix} {number} 题 正确"),
                                                Some(false) => format!("{prefix} {number} 题 错误"),
                                                None => format!("{prefix} {number} 题"),
                                            }
                                        } else { format!("{prefix} {number} 题") };
                                        html! {
                                            <button type="button"
                                                class={classes!("btn","btn-sm",style,if i==index && is_review {"ring-2 ring-primary"} else {""})}
                                                data-testid={if props.source_paper {"question-bank-practice-number"} else {"training-answer-sheet-number"}}
                                                aria-current={(i==index).to_string()}
                                                aria-label={label}
                                                onclick={Callback::from(move |_| {
                                                    if allows_navigation(&allowed) {
                                                        current.set(i);
                                                        open.set(false);
                                                    }
                                                })}>{number}</button>
                                        }
                                    })}
                                </div>
                            </section>
                        }
                    })}
                </nav>
            }
        </div>
    }
}

#[function_component(CompactQuestionNavigation)]
pub(super) fn compact_question_navigation(props: &CompactQuestionNavigationProps) -> Html {
    let total = props.numbers.len();
    let index = (*props.current).min(total.saturating_sub(1));
    let previous = {
        let current = props.current.clone();
        let allowed = props.navigation_allowed.clone();
        Callback::from(move |_| {
            if allows_navigation(&allowed) {
                current.set(index.saturating_sub(1));
            }
        })
    };
    let next = {
        let current = props.current.clone();
        let allowed = props.navigation_allowed.clone();
        Callback::from(move |_| {
            if allows_navigation(&allowed) {
                current.set((index + 1).min(total.saturating_sub(1)));
            }
        })
    };
    html! {
        <footer class="training-compact-footer relative z-40 flex shrink-0 items-center justify-between gap-2 border-t border-base-content/10 py-2"
            data-testid="question-bank-practice-navigation">
            <button type="button" class="btn btn-outline btn-sm" disabled={index==0} onclick={previous}>{"上一题"}</button>
            <TrainingAnswerSheet current={props.current.clone()} numbers={props.numbers.clone()}
                answered={props.answered.clone()} grades={props.grades.clone()}
                groups={props.groups.clone()} navigation_allowed={props.navigation_allowed.clone()}/>
            <button type="button" class="btn btn-primary btn-sm" disabled={index+1>=total} onclick={next}>{"下一题"}</button>
        </footer>
    }
}
