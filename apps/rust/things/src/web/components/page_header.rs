use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct PageHeaderProps {
    pub(crate) title_id: AttrValue,
    pub(crate) eyebrow: AttrValue,
    pub(crate) title: AttrValue,
    #[prop_or(false)]
    pub(crate) centered: bool,
    #[prop_or_default]
    pub(crate) children: Children,
}

#[function_component(PageHeader)]
pub(crate) fn page_header(props: &PageHeaderProps) -> Html {
    let class = if props.centered {
        SECTION_HEAD_CENTERED
    } else {
        SECTION_HEAD
    };
    html! {
        <div class={class}>
            <div>
                <p class={EYEBROW}>{props.eyebrow.clone()}</p>
                <h2 id={props.title_id.clone()} class={SECTION_TITLE}>{props.title.clone()}</h2>
            </div>
            {for props.children.iter()}
        </div>
    }
}
