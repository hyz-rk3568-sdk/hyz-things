use super::*;
use pulldown_cmark::{html::push_html, CowStr, Event as MarkdownEvent, Options, Parser, Tag};

pub(crate) fn hydrate_math() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(renderer) = Reflect::get(&window, &JsValue::from_str("hyzRenderMath")) else {
        return;
    };
    let Ok(renderer) = renderer.dyn_into::<Function>() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let _ = renderer.call1(&window, &document.into());
}

pub(crate) fn render_markdown(markdown: &str, source_file: &str) -> Html {
    Html::from_html_unchecked(AttrValue::from(markdown_html(markdown, source_file)))
}

pub(crate) fn markdown_html(markdown: &str, source_file: &str) -> String {
    markdown_with_assets(markdown, |destination| {
        resolve_asset_url(source_file, destination)
    })
}

pub(crate) fn render_question_bank_markdown(markdown: &str) -> Html {
    Html::from_html_unchecked(AttrValue::from(question_bank_markdown_html(markdown)))
}

fn question_bank_markdown_html(markdown: &str) -> String {
    markdown_with_assets(markdown, |destination| {
        let name = destination.strip_prefix("hyz-bank-asset:")?;
        let (hash, extension) = name.split_once('.')?;
        (hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && ["png", "jpg", "jpeg", "webp", "gif", "bmp"].contains(&extension))
        .then(|| format!("/api/v1/question-bank/assets/{name}"))
    })
}

fn markdown_with_assets(markdown: &str, mut resolve: impl FnMut(&str) -> Option<String>) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_MATH);
    let events = Parser::new_ext(markdown, options).map(|event| match event {
        MarkdownEvent::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => MarkdownEvent::Start(Tag::Image {
            link_type,
            dest_url: CowStr::Boxed(
                resolve(dest_url.as_ref())
                    .unwrap_or_default()
                    .into_boxed_str(),
            ),
            title,
            id,
        }),
        MarkdownEvent::Html(raw) | MarkdownEvent::InlineHtml(raw) => MarkdownEvent::Text(raw),
        event => event,
    });
    let mut html = String::new();
    push_html(&mut html, events);
    html
}

pub(crate) fn resolve_asset_url(source_file: &str, destination: &str) -> Option<String> {
    if destination.is_empty()
        || destination.starts_with('/')
        || destination.contains('\\')
        || destination.contains('\0')
        || destination.contains("://")
        || destination.starts_with("data:")
    {
        return None;
    }
    let source_directory = source_file
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let mut segments = source_directory
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    for segment in destination.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }
    if segments.is_empty() {
        return None;
    }
    let encoded = segments
        .into_iter()
        .map(|segment| url::form_urlencoded::byte_serialize(segment.as_bytes()).collect::<String>())
        .collect::<Vec<_>>()
        .join("/");
    Some(format!("/api/v1/study/assets/{encoded}"))
}

#[cfg(test)]
mod question_bank_markdown_tests {
    use super::*;
    #[test]
    fn bank_images_are_local_and_source_html_is_escaped() {
        let hash = "a".repeat(64);
        let html=question_bank_markdown_html(&format!("![题图](hyz-bank-asset:{hash}.png)\n\n<script>alert(1)</script>\n\n![外链](https://example.com/x.png)"));
        assert!(html.contains(&format!("/api/v1/question-bank/assets/{hash}.png")));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("src=\"https://"));
        assert!(
            question_bank_markdown_html(&format!("![](hyz-bank-asset:{hash}.bmp)"))
                .contains(&format!("/api/v1/question-bank/assets/{hash}.bmp"))
        );
    }
}
