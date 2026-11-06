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

fn bank_asset_name(name: &str) -> Option<&str> {
    let (hash, extension) = name.split_once('.')?;
    (hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && ["png", "jpg", "jpeg", "webp", "gif", "bmp"].contains(&extension))
    .then_some(name)
}

fn question_bank_destination(destination: &str) -> Option<&str> {
    if let Some(name) = destination.strip_prefix("hyz-bank-asset:") {
        return bank_asset_name(name);
    }
    // Indexes synced before the asset rewrite still contain relative source
    // paths such as ../../../assets/<hash>.png. Resolve their final asset name
    // so existing question banks render without a full rescan.
    if destination.contains([':', '\\', '\0', '?', '#']) {
        return None;
    }
    let segments = destination.split('/').collect::<Vec<_>>();
    if !segments.iter().any(|segment| *segment == "assets") {
        return None;
    }
    bank_asset_name(segments.last()?)
}

fn question_bank_markdown_html(markdown: &str) -> String {
    let mut html = markdown_with_assets(markdown, |destination| {
        question_bank_destination(destination)
            .map(|name| format!("/api/v1/question-bank/assets/{name}"))
    });
    // Source HTML is already escaped; rewrite only generated table-cell tags.
    for alignment in ["left", "center", "right"] {
        for cell in ["th", "td"] {
            html = html.replace(
                &format!("<{cell} style=\"text-align: {alignment}\">"),
                &format!("<{cell} class=\"study-table-align-{alignment}\">"),
            );
        }
    }
    html
}

fn normalize_latex_delimiters(markdown: &str) -> String {
    let mut output = String::with_capacity(markdown.len());
    let mut fence: Option<(char, usize)> = None;
    for line in markdown.split_inclusive('\n') {
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let trimmed = line.trim_start_matches(' ');
        if let Some((marker, length)) = fence {
            output.push_str(line);
            if closes_fence(trimmed, marker, length) {
                fence = None;
            }
            continue;
        }
        if indent <= 3 {
            if let Some((marker, length)) = opens_fence(trimmed) {
                fence = Some((marker, length));
                output.push_str(line);
                continue;
            }
        }
        if line.starts_with("    ") || line.starts_with('\t') {
            output.push_str(line);
        } else {
            output.push_str(&normalize_inline_latex(line));
        }
    }
    output
}

fn opens_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = line.chars().take_while(|c| *c == marker).count();
    (length >= 3).then_some((marker, length))
}

fn closes_fence(line: &str, marker: char, length: usize) -> bool {
    let run = line.chars().take_while(|c| *c == marker).count();
    run >= length && line.chars().skip(run).all(|c| c.is_whitespace())
}

fn normalize_inline_latex(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(line.len());
    let mut code_ticks = 0usize;
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`' {
            let mut end = index;
            while end < chars.len() && chars[end] == '`' {
                end += 1;
            }
            let run = end - index;
            if code_ticks == 0 {
                code_ticks = run;
            } else if code_ticks == run {
                code_ticks = 0;
            }
            output.extend(chars[index..end].iter());
            index = end;
            continue;
        }
        if chars[index] == '\\' && code_ticks == 0 {
            let mut end = index;
            while end < chars.len() && chars[end] == '\\' {
                end += 1;
            }
            let count = end - index;
            if count % 2 == 1 && end < chars.len() && matches!(chars[end], '(' | ')' | '[' | ']') {
                output.extend(std::iter::repeat_n('\\', count - 1));
                output.push_str(if matches!(chars[end], '(' | ')') {
                    "$"
                } else {
                    "$$"
                });
                index = end + 1;
                continue;
            }
            output.extend(std::iter::repeat_n('\\', count));
            index = end;
            continue;
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn markdown_with_assets(markdown: &str, mut resolve: impl FnMut(&str) -> Option<String>) -> String {
    let markdown = normalize_latex_delimiters(markdown);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_MATH);
    let events = Parser::new_ext(&markdown, options).map(|event| match event {
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
        let legacy = question_bank_markdown_html(&format!("![](../../../assets/{hash}.png)"));
        assert!(legacy.contains(&format!("/api/v1/question-bank/assets/{hash}.png")));
        assert!(!legacy.contains("../../../assets"));
        let nested = question_bank_markdown_html(&format!("![](../../assets/{hash}.jpg)"));
        assert!(nested.contains(&format!("/api/v1/question-bank/assets/{hash}.jpg")));
        assert!(!nested.contains("../../assets"));
        assert!(!question_bank_markdown_html(&format!(
            "![](https://example.com/assets/{hash}.png)"
        ))
        .contains("/api/v1/question-bank/assets/"));
    }

    #[test]
    fn backslash_latex_delimiters_become_math_spans() {
        let html = question_bank_markdown_html(
            "设 \\(x\\) 元。\n\n\\[\n0.7x-25=48.5\n\\]\n\n\\(x=105\\div0.7\\)",
        );
        assert!(html.contains("math-inline"), "{html}");
        assert!(html.contains("math-display"), "{html}");
        assert!(html.contains("0.7x-25=48.5"), "{html}");
        assert!(html.contains("\\div"), "{html}");
        assert!(!html.contains("\\(x\\)"), "{html}");
    }

    #[test]
    fn backslash_latex_inside_code_is_not_math() {
        let html = question_bank_markdown_html("`\\(x\\)`\n\n```\n\\(y\\)\n```");
        assert!(!html.contains("math-inline"), "{html}");
        assert!(!html.contains("math-display"), "{html}");
    }
}
