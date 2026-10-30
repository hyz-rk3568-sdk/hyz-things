use super::*;
use hyz_things::domain::data_analysis::RelatedCardSnapshot;

#[derive(Properties, PartialEq)]
pub(crate) struct RelatedMethodCardProps {
    pub card: Option<RelatedCardSnapshot>,
    pub source_file: String,
    pub test_id: AttrValue,
}

#[function_component(RelatedMethodCard)]
pub(crate) fn related_method_card(props: &RelatedMethodCardProps) -> Html {
    use_effect_with(props.card.clone(), move |_| {
        hydrate_math();
        || ()
    });
    let on_toggle = Callback::from(|_: Event| hydrate_math());
    html! {
        <details class="mt-4 rounded-box border border-base-content/15 p-3"
            data-testid={props.test_id.clone()} ontoggle={on_toggle}>
            <summary class="cursor-pointer font-semibold">{"查看关联方法卡"}</summary>
            if let Some(card) = props.card.as_ref() {
                <div class="mt-3 space-y-3">
                    <div class="study-markdown">{render_markdown(&card.front, &props.source_file)}</div>
                    <div class="divider"></div>
                    <div class="study-markdown">{render_markdown(&card.back, &props.source_file)}</div>
                </div>
            } else {
                <p class="mt-3 text-sm text-base-content/60">{"此答卷未保存关联卡片内容，且无法确认原卡片版本。"}</p>
            }
        </details>
    }
}
