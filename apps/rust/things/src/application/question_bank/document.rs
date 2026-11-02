//! Pure parsing/identity rules, shared by the source adapter and host tests.
use crate::domain::question_bank::{MaterialView, PaperSummary, PaperView, QuestionView};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
struct Header {
    schema: String,
    title: String,
    year: i32,
    region: String,
    variant: String,
    #[serde(default = "primary")]
    role: String,
}
// This parser never treats source numbering as a stable identity.
fn primary() -> String {
    "primary".into()
}

#[derive(Default, Deserialize)]
struct Metadata {
    module: Option<String>,
    category: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    materials: Vec<String>,
    #[serde(default)]
    issues: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Document {
    view: PaperView,
    answers: BTreeMap<String, AnswerKey>,
}
#[derive(Clone, Debug)]
pub struct AnswerKey {
    pub choices: Vec<String>,
    pub explanation: String,
    pub valid: bool,
}
fn choices(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.trim().split(|c: char| !c.is_ascii_alphabetic()) {
        if token.is_empty() {
            continue;
        }
        for choice in token.chars() {
            let value = choice.to_ascii_uppercase().to_string();
            if !out.contains(&value) {
                out.push(value);
            }
        }
    }
    out.sort();
    out
}
pub(super) fn option_labels(markdown: &str) -> Vec<String> {
    markdown.lines().filter_map(|line| {
        let line = line.trim_start().strip_prefix("- ")?;
        let (label, rest) = line.split_once(['.', '．', '、', ' '])?;
        (label.len() == 1 && label.bytes().all(|b| b.is_ascii_uppercase()) && !rest.trim().is_empty())
            .then(|| label.to_string())
    }).collect()
}
impl Document {
    pub fn view(&self) -> PaperView {
        self.view.clone()
    }
    pub fn key(&self, question_id: &str) -> Option<&AnswerKey> {
        self.answers.get(question_id)
    }

    pub fn summary(&self) -> PaperSummary {
        let v = &self.view;
        PaperSummary {
            id: v.id.clone(),
            title: v.title.clone(),
            year: v.year,
            region: v.region.clone(),
            variant: v.variant.clone(),
            role: v.role.clone(),
            questions: v.questions.len(),
        }
    }
    pub fn question_ids(&self) -> Vec<String> {
        self.view.questions.iter().map(|q| q.id.clone()).collect()
    }
    pub fn material_ids(&self) -> Vec<String> {
        self.view.materials.iter().map(|m| m.id.clone()).collect()
    }
    pub fn ids(&self) -> Vec<String> {
        std::iter::once(self.view.id.clone())
            .chain(self.view.materials.iter().map(|m| m.id.clone()))
            .chain(self.view.questions.iter().map(|q| q.id.clone()))
            .filter(|s| !s.is_empty())
            .collect()
    }
}

pub fn valid_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .is_ok_and(|id| id.get_version_num() == 7 && id.to_string() == value)
}
fn marker(text: &str, kind: &str, required: bool) -> Result<String> {
    let prefix = format!("<!-- hyz-{kind}-id: ");
    let markers: Vec<_> = text
        .lines()
        .filter(|line| line.trim().starts_with("<!-- hyz-"))
        .collect();
    if markers.is_empty() && !required {
        return Ok(String::new());
    }
    if markers.len() != 1 {
        return Err(format!("missing or duplicate {kind} UUID"));
    }
    let id = markers[0]
        .trim()
        .strip_prefix(&prefix)
        .and_then(|s| s.strip_suffix(" -->"))
        .ok_or("malformed UUID marker")?;
    if !valid_id(id) {
        return Err("invalid UUIDv7".into());
    }
    Ok(id.into())
}
fn without_marker(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim().starts_with("<!-- hyz-"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

pub fn parse(text: &str) -> Result<Document> {
    parse_inner(text, true)
}
fn parse_inner(text: &str, required: bool) -> Result<Document> {
    let text = text.strip_prefix("---\n").ok_or("missing frontmatter")?;
    let (header, body) = text
        .split_once("\n---\n")
        .ok_or("unterminated frontmatter")?;
    let header: Header = serde_yaml::from_str(header).map_err(|_| "invalid paper YAML")?;
    if header.schema != "hyz-paper/v1" || !["primary", "evidence"].contains(&header.role.as_str()) {
        return Err("unsupported paper schema/role".into());
    }
    let mut blocks: Vec<(String, String)> = vec![(String::new(), String::new())];
    let mut fenced = false;
    for line in body.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
        }
        if !fenced && (line.starts_with("## Question ") || line.starts_with("## Material ")) {
            blocks.push((line.into(), String::new()));
        } else {
            blocks.last_mut().unwrap().1.push_str(line);
            blocks.last_mut().unwrap().1.push('\n');
        }
    }
    if fenced {
        return Err("unterminated code fence".into());
    }
    let id = marker(&blocks[0].1, "paper", required)?;
    let mut materials = Vec::new();
    let mut questions = Vec::new();
    let mut answers = BTreeMap::new();
    let mut numbers = BTreeSet::new();
    for (heading, content) in blocks.iter().skip(1) {
        if let Some(name) = heading.strip_prefix("## Material ") {
            if !name
                .strip_prefix('M')
                .is_some_and(|n| n.parse::<u32>().is_ok_and(|n| n > 0))
            {
                return Err("invalid material name".into());
            }
            let id = marker(content, "material", required)?;
            materials.push(MaterialView {
                id,
                name: name.into(),
                markdown: without_marker(content),
            });
        } else {
            let number = heading
                .strip_prefix("## Question ")
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|n| *n > 0)
                .ok_or("invalid question number")?;
            if !numbers.insert(number) {
                return Err("duplicate question number".into());
            }
            let (prefix, rest) = content
                .split_once("```yaml\n")
                .ok_or("missing question metadata")?;
            let qid = marker(prefix, "question", required)?;
            let (yaml, sections) = rest
                .split_once("\n```")
                .ok_or("unterminated question metadata")?;
            let meta: Metadata = serde_yaml::from_str(yaml).map_err(|_| "invalid question YAML")?;
            let fields = question_fields(sections)?;
            let labels = option_labels(fields["Options"].trim());
            let answer = choices(fields["Answer"].trim());
            let valid = !fields["Stem"].trim().is_empty()
                && labels.len() >= 2
                && labels.len() <= 12
                && labels.iter().collect::<BTreeSet<_>>().len() == labels.len()
                && !answer.is_empty()
                && answer.iter().all(|choice| labels.contains(choice))
                && meta.issues.is_empty();
            answers.insert(qid.clone(), AnswerKey {
                choices: answer.clone(),
                explanation: fields.get("Explanation").cloned().unwrap_or_default().trim().into(),
                valid,
            });
            // Key and explanation stay server-side, never in the public DTO.
            questions.push(QuestionView {
                id: qid,
                number,
                stem: fields["Stem"].trim().into(),
                options_markdown: fields["Options"].trim().into(),
                materials: meta.materials,
                module: meta.module,
                category: meta.category,
                topics: meta.topics,
                issues: meta.issues,
            });
        }
    }
    if questions.is_empty() || questions.len() > 1000 {
        return Err("invalid question count".into());
    }
    let names: BTreeMap<_, _> = materials
        .iter()
        .map(|m| (m.name.clone(), m.id.clone()))
        .collect();
    if names.len() != materials.len() {
        return Err("duplicate material name".into());
    }
    for q in &mut questions {
        q.materials = q
            .materials
            .iter()
            .map(|name| {
                names
                    .get(name)
                    .cloned()
                    .ok_or_else(|| "dangling material reference".to_string())
            })
            .collect::<Result<_>>()?;
    }
    let doc = Document {
        answers,
        view: PaperView {
            id,
            title: header.title,
            year: header.year,
            region: header.region,
            variant: header.variant,
            role: header.role,
            questions,
            materials,
        },
    };
    let ids = doc.ids();
    if body.matches("<!-- hyz-").count() != ids.len() {
        return Err("misplaced UUID marker".into());
    }
    if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err("duplicate UUID".into());
    }
    Ok(doc)
}

/// Validate before generating IDs; only insert missing comment lines, never rewrite content.
pub fn complete_ids(text: &str, mut next: impl FnMut() -> String) -> Result<(String, usize)> {
    let doc = parse_inner(text, false)?;
    let mut generated = 0;
    let mut output = String::new();
    let mut material = 0;
    let mut question = 0;
    let needs_paper = doc.view.id.is_empty();
    let mut inserted_paper = false;
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        if needs_paper && !inserted_paper && line.starts_with("# ") {
            output.push_str(&format!("<!-- hyz-paper-id: {} -->\n\n", next()));
            generated += 1;
            inserted_paper = true;
        }
        output.push_str(line);
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let kind = if line.starts_with("## Material ") {
            let missing = doc.view.materials[material].id.is_empty();
            material += 1;
            missing.then_some("material")
        } else if line.starts_with("## Question ") {
            let missing = doc.view.questions[question].id.is_empty();
            question += 1;
            missing.then_some("question")
        } else {
            None
        };
        if let Some(kind) = kind {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&format!("\n<!-- hyz-{kind}-id: {} -->\n", next()));
            generated += 1;
        }
    }
    parse(&output)?;
    Ok((output, generated))
}

fn question_fields(sections: &str) -> Result<BTreeMap<String, String>> {
    let mut fields = BTreeMap::new();
    let mut current = String::new();
    for line in sections.lines() {
        if let Some(key) = line
            .strip_prefix("### ")
            .filter(|key| ["Stem", "Options", "Answer", "Explanation", "Notes"].contains(key))
        {
            current = key.to_string();
            if fields.insert(current.clone(), String::new()).is_some() {
                return Err("duplicate question section".into());
            }
        } else if let Some(field) = fields.get_mut(&current) {
            field.push_str(line);
            field.push('\n');
        }
    }
    for key in ["Stem", "Options", "Answer"] {
        if !fields.contains_key(key) {
            return Err(format!("missing {key}"));
        }
    }
    Ok(fields)
}
