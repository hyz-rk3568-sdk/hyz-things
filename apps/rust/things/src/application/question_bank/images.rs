//! Pure image-reference validation; no filesystem or HTTP adapters.
use super::BankError;
pub fn asset_type(name: &str) -> Result<&'static str, BankError> {
    let (hash, extension) = name.split_once('.').ok_or(BankError::Invalid)?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(BankError::Invalid);
    }
    match extension {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "webp" => Ok("image/webp"),
        "gif" => Ok("image/gif"),
        "bmp" => Ok("image/bmp"),
        _ => Err(BankError::Invalid),
    }
}
fn resolve_image(file: &str, reference: &str) -> Result<String, BankError> {
    if reference.contains([':', '\\', '\0', '?', '#']) || reference.starts_with('/') {
        return Err(BankError::Invalid);
    }
    let mut parts = file.split('/').collect::<Vec<_>>();
    parts.pop();
    for part in reference.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or(BankError::Invalid)?;
            }
            part => parts.push(part),
        }
    }
    if parts.len() != 2 || parts[0] != "assets" {
        return Err(BankError::Invalid);
    }
    asset_type(parts[1])?;
    Ok(parts[1].into())
}
pub fn rewrite_assets(text: &str, file: &str) -> Result<(String, Vec<String>), BankError> {
    let mut text = text.to_string();
    // Convert only img tags to Markdown; arbitrary source HTML remains escaped by the SPA.
    let mut offset = 0;
    while let Some(start) = text[offset..].find("<img") {
        let start = start + offset;
        let end = text[start..].find('>').ok_or(BankError::Invalid)? + start + 1;
        let fragment = scraper::Html::parse_fragment(&text[start..end]);
        let selector = scraper::Selector::parse("img").map_err(|_| BankError::Invalid)?;
        let src = fragment
            .select(&selector)
            .next()
            .and_then(|img| img.value().attr("src"))
            .ok_or(BankError::Invalid)?;
        let replacement = format!("![]({src})");
        text.replace_range(start..end, &replacement);
        offset = start + replacement.len();
    }
    let mut output = String::new();
    let mut references = Vec::new();
    let mut remaining = text.as_str();
    while let Some(start) = remaining.find("![") {
        output.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        let Some(close) = remaining.find("](") else {
            return Err(BankError::Invalid);
        };
        let Some(end) = remaining[close + 2..].find(')') else {
            return Err(BankError::Invalid);
        };
        let end = close + 2 + end;
        let reference = remaining[close + 2..end]
            .split_whitespace()
            .next()
            .ok_or(BankError::Invalid)?
            .trim_matches(['<', '>']);
        let name = resolve_image(file, reference)?;
        output.push_str(&remaining[..close + 2]);
        output.push_str(&format!("hyz-bank-asset:{name}"));
        output.push(')');
        references.push(name);
        remaining = &remaining[end + 1..];
    }
    output.push_str(remaining);
    Ok((output, references))
}
