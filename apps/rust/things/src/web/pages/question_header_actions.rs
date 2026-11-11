use yew::prelude::*;

pub(super) const QUESTION_HEADER_ICON_BUTTON: &str =
    "btn btn-ghost btn-sm btn-square h-8 w-8 min-h-0 shrink-0 p-0";

#[derive(Clone, PartialEq, Properties)]
pub(super) struct QuestionHeaderActionsProps {
    pub favorite: Option<bool>,
    pub favorite_busy: bool,
    pub favorite_label: AttrValue,
    pub favorite_test_id: AttrValue,
    pub on_favorite: Callback<MouseEvent>,
    pub flashcard_count: Option<usize>,
    pub flashcard_can_create: bool,
    pub flashcard_create_test_id: AttrValue,
    pub flashcard_manage_test_id: AttrValue,
    pub on_flashcard: Callback<MouseEvent>,
}

fn flashcard_badge(count: usize) -> String {
    match count {
        0 => "+".into(),
        1..=9 => count.to_string(),
        _ => "9+".into(),
    }
}

#[function_component(QuestionHeaderActions)]
pub(super) fn question_header_actions(props: &QuestionHeaderActionsProps) -> Html {
    let flashcard = props
        .flashcard_count
        .filter(|count| *count > 0 || props.flashcard_can_create);
    html! {
        <div class="flex shrink-0 items-center gap-0">
            <button type="button" class={classes!(QUESTION_HEADER_ICON_BUTTON, "relative")}
                data-testid={props.favorite_test_id.clone()} aria-label={props.favorite_label.clone()}
                title={props.favorite_label.clone()} aria-busy={props.favorite_busy.to_string()}
                disabled={props.favorite_busy || props.favorite.is_none()} onclick={props.on_favorite.clone()}>
                if props.favorite_busy {
                    <span aria-hidden="true">{"…"}</span>
                } else {
                    <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24"
                        fill={if props.favorite == Some(true) { "currentColor" } else { "none" }}
                        stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="m12 3 2.78 5.63 6.22.9-4.5 4.39 1.06 6.2L12 17.19l-5.56 2.93 1.06-6.2L3 9.53l6.22-.9L12 3z" />
                    </svg>
                }
            </button>
            if let Some(count) = flashcard {
                <button type="button" class={classes!(QUESTION_HEADER_ICON_BUTTON, "relative")}
                    data-testid={if count == 0 { props.flashcard_create_test_id.clone() } else { props.flashcard_manage_test_id.clone() }}
                    aria-label={if count == 0 { "为当前题目创建 Flashcard".to_owned() } else { format!("管理当前题目的 {count} 张 Flashcard") }}
                    title={if count == 0 { "为当前题目创建 Flashcard".to_owned() } else { format!("管理当前题目的 {count} 张 Flashcard") }}
                    onclick={props.on_flashcard.clone()}>
                    <svg aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none"
                        stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <rect x="5" y="3" width="15" height="15" rx="2" />
                        <path d="M3 7v12a2 2 0 0 0 2 2h12" />
                    </svg>
                    <span aria-hidden="true" class="absolute right-0 top-0 rounded-full bg-primary px-1 text-[10px] leading-4 text-primary-content">
                        {flashcard_badge(count)}
                    </span>
                </button>
            }
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::flashcard_badge;

    #[test]
    fn flashcard_badge_shows_create_count_and_cap() {
        assert_eq!(flashcard_badge(0), "+");
        assert_eq!(flashcard_badge(1), "1");
        assert_eq!(flashcard_badge(9), "9");
        assert_eq!(flashcard_badge(10), "9+");
    }
}
