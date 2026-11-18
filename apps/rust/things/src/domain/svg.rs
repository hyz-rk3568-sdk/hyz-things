const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

fn local_reference(value: &str) -> bool {
    let Some(id) = value.strip_prefix('#') else {
        return false;
    };
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
}

fn safe_css(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let compact: String = lower.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if compact.contains("image(") || compact.contains("src(") {
        return false;
    }
    if lower.contains(['\\', '@'])
        || [
            "/*",
            "*/",
            "image-set",
            "src(",
            "http:",
            "https:",
            "data:",
            "file:",
            "//",
        ]
        .iter()
        .any(|s| lower.contains(s))
    {
        return false;
    }
    let mut rest = lower.as_str();
    while let Some(pos) = rest.find("url") {
        let after = rest[pos + 3..].trim_start();
        if let Some(after) = after.strip_prefix('(') {
            let Some(end) = after.find(')') else {
                return false;
            };
            let reference = after[..end].trim().trim_matches(['\'', '"']);
            if !local_reference(reference) {
                return false;
            }
            rest = &after[end + 1..];
        } else {
            rest = &rest[pos + 3..];
        }
    }
    true
}

pub fn safe_svg(source: &str) -> bool {
    if source.is_empty() || source.len() > 32_768 || source.contains("<!") || source.contains("<?")
    {
        return false;
    }
    let Ok(doc) = roxmltree::Document::parse(source) else {
        return false;
    };
    let root = doc.root_element();
    if root.tag_name().name() != "svg" || root.tag_name().namespace() != Some(SVG_NS) {
        return false;
    }
    let mut count = 0;
    for node in root.descendants().filter(|n| n.is_element()) {
        count += 1;
        let tag = node.tag_name();
        if count > 256
            || node.ancestors().filter(|n| n.is_element()).count() > 16
            || tag.namespace() != Some(SVG_NS)
            || [
                "script",
                "foreignObject",
                "animate",
                "animateMotion",
                "animateTransform",
                "set",
                "discard",
            ]
            .contains(&tag.name())
        {
            return false;
        }
        if tag.name() == "style"
            && !safe_css(
                &node
                    .descendants()
                    .filter_map(|n| n.text())
                    .collect::<String>(),
            )
        {
            return false;
        }
        for attr in node.attributes() {
            let name = attr.name();
            if name.to_ascii_lowercase().starts_with("on")
                || name == "constructor"
                || name == "base"
            {
                return false;
            }
            if name == "href" {
                if !matches!(attr.namespace(), None | Some(XLINK_NS))
                    || !local_reference(attr.value())
                {
                    return false;
                }
            } else if !safe_css(attr.value()) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::safe_svg;

    #[test]
    fn rejects_css_image_function_with_relative_external_resource() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect style="fill: image('private.png')"/></svg>"#;
        assert!(!safe_svg(source));
    }
}
