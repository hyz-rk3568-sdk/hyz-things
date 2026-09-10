use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct SectionCardProps {
    pub(crate) title_id: AttrValue,
    #[prop_or_default]
    pub(crate) extra_class: Classes,
    #[prop_or_default]
    pub(crate) busy: Option<bool>,
    #[prop_or_default]
    pub(crate) children: Children,
}

#[function_component(SectionCard)]
pub(crate) fn section_card(props: &SectionCardProps) -> Html {
    let class = classes!(SECTION, props.extra_class.clone());
    if let Some(busy) = props.busy {
        html! {
            <section class={class} aria-labelledby={props.title_id.clone()} aria-busy={busy.to_string()}>
                {for props.children.iter()}
            </section>
        }
    } else {
        html! {
            <section class={class} aria-labelledby={props.title_id.clone()}>
                {for props.children.iter()}
            </section>
        }
    }
}
