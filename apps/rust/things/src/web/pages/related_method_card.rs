use super::*;
use hyz_things::domain::flashcards::CurrentFlashcard;
use std::{cell::Cell, rc::Rc};

#[derive(Properties, PartialEq)]
pub(crate) struct RelatedMethodCardProps {
    pub card_id: Option<String>,
    pub test_id: AttrValue,
}

#[function_component(RelatedMethodCard)]
pub(crate) fn related_method_card(props: &RelatedMethodCardProps) -> Html {
    let card = use_state(|| None::<Result<CurrentFlashcard, String>>);
    {
        let card = card.clone();
        use_effect_with(props.card_id.clone(), move |id| {
            let alive = Rc::new(Cell::new(true));
            card.set(None);
            if let Some(id) = id.clone() {
                let alive = alive.clone();
                spawn_local(async move {
                    let id: String = url::form_urlencoded::byte_serialize(id.as_bytes()).collect();
                    let result = fetch_json::<CurrentFlashcard>(
                        &format!("/api/v1/study/cards/{id}/current"),
                        "关联卡片",
                    )
                    .await;
                    if alive.get() {
                        card.set(Some(result));
                    }
                });
            }
            move || alive.set(false)
        });
    }
    use_effect_with((*card).clone(), move |_| {
        hydrate_math();
        || ()
    });
    let on_toggle = Callback::from(|_: Event| hydrate_math());
    html! {
        <details class="mt-4 rounded-box border border-base-content/15 p-3"
            data-testid={props.test_id.clone()} ontoggle={on_toggle}>
            <summary class="cursor-pointer font-semibold">{"查看关联方法卡"}</summary>
            if let Some(Ok(card)) = (*card).as_ref() {
                <div class="mt-3 space-y-3">
                    <div class="study-markdown">{render_markdown(&card.front_markdown, &card.source_file)}</div>
                    <div class="divider"></div>
                    <div class="study-markdown">{render_markdown(&card.back_markdown, &card.source_file)}</div>
                </div>
            } else if props.card_id.is_none() || matches!((*card).as_ref(), Some(Err(_))) {
                <p class="mt-3 text-sm text-base-content/60">{"关联卡片不可用。"}</p>
            } else {
                <p class="mt-3 text-sm text-base-content/60">{"正在加载关联卡片…"}</p>
            }
        </details>
    }
}
