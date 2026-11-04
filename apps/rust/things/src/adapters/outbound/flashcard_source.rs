#[path = "flashcard_source_guard.rs"]
mod guard;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs, io,
    path::{Path, PathBuf},
};
use uuid::Uuid;

use crate::{
    application::flashcards::{FlashcardError, FlashcardSourcePort},
    domain::flashcards::{FlashcardId, SourceCard, SourceSnapshot, SyncError},
};

pub const FLASHCARD_SOURCE_ROOT: &str = "/mnt/hyz-cards";
const MAX_MARKDOWN_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;
#[cfg(test)]
use std::{
    fs::{File, OpenOptions},
    io::Write,
    sync::atomic::AtomicU64,
};

#[cfg(test)]
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct FlashcardAsset {
    pub contents: Vec<u8>,
    pub content_type: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashcardAssetError {
    InvalidPath,
    UnsupportedFormat,
    NotFound,
    OutsideRoot,
    TooLarge,
    Unavailable,
}

#[derive(Clone, Debug)]
pub struct FilesystemFlashcardSource {
    root: PathBuf,
    mount_checked: bool,
}

impl Default for FilesystemFlashcardSource {
    fn default() -> Self {
        Self {
            root: FLASHCARD_SOURCE_ROOT.into(),
            mount_checked: true,
        }
    }
}

impl FilesystemFlashcardSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            mount_checked: false,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn read_asset(&self, relative_path: &str) -> Result<FlashcardAsset, FlashcardAssetError> {
        if !safe_relative_asset_path(relative_path) {
            return Err(FlashcardAssetError::InvalidPath);
        }
        let extension = Path::new(relative_path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or(FlashcardAssetError::UnsupportedFormat)?;
        let content_type = match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            _ => return Err(FlashcardAssetError::UnsupportedFormat),
        };
        if self.mount_checked {
            self.mount_identity()
                .map_err(|_| FlashcardAssetError::Unavailable)?;
        }
        let root = fs::canonicalize(&self.root).map_err(|_| FlashcardAssetError::Unavailable)?;
        let candidate = root.join(relative_path);
        let canonical = fs::canonicalize(&candidate).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                FlashcardAssetError::NotFound
            } else {
                FlashcardAssetError::Unavailable
            }
        })?;
        if !canonical.starts_with(&root) {
            return Err(FlashcardAssetError::OutsideRoot);
        }
        let metadata = fs::metadata(&canonical).map_err(|_| FlashcardAssetError::NotFound)?;
        if !metadata.is_file() {
            return Err(FlashcardAssetError::NotFound);
        }
        if metadata.len() > MAX_ASSET_BYTES {
            return Err(FlashcardAssetError::TooLarge);
        }
        let contents = fs::read(canonical).map_err(|_| FlashcardAssetError::Unavailable)?;
        Ok(FlashcardAsset {
            contents,
            content_type,
        })
    }

    fn mount_identity(&self) -> Result<Option<String>, FlashcardError> {
        self.mount_checked
            .then(|| guard::mount_identity(&self.root))
            .transpose()
            .map_err(|error| FlashcardError::SourceUnavailable(error.to_string()))
    }

    #[cfg(test)]
    fn scan_guarded(
        &self,
        guard: &guard::SourceGuard<'_>,
    ) -> Result<SourceSnapshot, FlashcardError> {
        guard
            .verify()
            .map_err(|error| FlashcardError::SourceUnavailable(error.to_string()))?;
        let mut source = self.clone();
        source.mount_checked = false;
        let snapshot = source.scan_blocking()?;
        guard
            .verify()
            .map_err(|error| FlashcardError::SourceUnavailable(error.to_string()))?;
        Ok(snapshot)
    }

    fn scan_blocking(&self) -> Result<SourceSnapshot, FlashcardError> {
        let guard = guard::SourceGuard::open(&self.root, self.mount_checked)
            .map_err(|error| FlashcardError::SourceUnavailable(error.to_string()))?;
        let root = fs::canonicalize(&self.root).map_err(|error| {
            FlashcardError::SourceUnavailable(format!("{}: {error}", self.root.display()))
        })?;
        if !root.is_dir() {
            return Err(FlashcardError::SourceUnavailable(format!(
                "source root is not a directory: {}",
                self.root.display()
            )));
        }

        let mut snapshot = SourceSnapshot {
            complete: true,
            ..SourceSnapshot::default()
        };
        let mut directories = VecDeque::from([root.clone()]);
        while let Some(directory) = directories.pop_front() {
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    snapshot.complete = false;
                    snapshot.errors.push(SyncError {
                        source_file: relative_path(&root, &directory),
                        line: None,
                        code: "source_directory_unreadable".to_owned(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        snapshot.complete = false;
                        snapshot.errors.push(SyncError {
                            source_file: relative_path(&root, &directory),
                            line: None,
                            code: "source_entry_unreadable".to_owned(),
                            message: error.to_string(),
                        });
                        continue;
                    }
                };
                let path = entry.path();
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        snapshot.complete = false;
                        snapshot.errors.push(SyncError {
                            source_file: relative_path(&root, &path),
                            line: None,
                            code: "source_metadata_unreadable".to_owned(),
                            message: error.to_string(),
                        });
                        continue;
                    }
                };
                let canonical = match fs::canonicalize(&path) {
                    Ok(canonical) => canonical,
                    Err(error) => {
                        snapshot.complete = false;
                        snapshot.errors.push(SyncError {
                            source_file: relative_path(&root, &path),
                            line: None,
                            code: "source_path_unreadable".to_owned(),
                            message: error.to_string(),
                        });
                        continue;
                    }
                };
                if !canonical.starts_with(&root) {
                    snapshot.complete = false;
                    snapshot.errors.push(SyncError {
                        source_file: relative_path(&root, &path),
                        line: None,
                        code: "source_symlink_escape".to_owned(),
                        message: "source path resolves outside the card root".to_owned(),
                    });
                    continue;
                }
                if metadata.is_dir() {
                    directories.push_back(canonical);
                    continue;
                }
                if !metadata.is_file()
                    || path.extension().and_then(|extension| extension.to_str()) != Some("md")
                {
                    continue;
                }

                let source_file = relative_path(&root, &path);
                snapshot.scanned_markdown_files.insert(source_file.clone());
                match parse_guarded_file(&guard, &source_file) {
                    Ok((parsed, original)) => {
                        snapshot
                            .successful_markdown_files
                            .insert(source_file.clone());
                        snapshot.cards.extend(parsed.cards);
                        if parsed.generated_ids {
                            if let Err(error) = guard.replace(
                                Path::new(&source_file),
                                &original,
                                parsed.rewritten_text.as_bytes(),
                            ) {
                                let source_file = relative_path(&root, &path);
                                snapshot.successful_markdown_files.remove(&source_file);
                                snapshot.failed_markdown_files.insert(source_file.clone());
                                snapshot
                                    .cards
                                    .retain(|card| card.source_file != source_file);
                                snapshot.errors.push(SyncError {
                                    source_file,
                                    line: None,
                                    code: "card_id_write_failed".to_owned(),
                                    message: error.to_string(),
                                });
                            }
                        }
                    }
                    Err(error) => {
                        snapshot.failed_markdown_files.insert(source_file.clone());
                        snapshot.errors.push(error);
                    }
                }
            }
        }

        remove_duplicate_ids(&mut snapshot);
        guard
            .verify()
            .map_err(|error| FlashcardError::SourceUnavailable(error.to_string()))?;
        Ok(snapshot)
    }
}

#[async_trait]
impl FlashcardSourcePort for FilesystemFlashcardSource {
    async fn scan(&self) -> Result<SourceSnapshot, FlashcardError> {
        let source = self.clone();
        tokio::task::spawn_blocking(move || source.scan_blocking())
            .await
            .map_err(|error| FlashcardError::SourceUnavailable(error.to_string()))?
    }
}

pub(crate) struct ParsedFile {
    pub(crate) cards: Vec<SourceCard>,
    pub(crate) rewritten_text: String,
    pub(crate) generated_ids: bool,
}

fn parse_guarded_file(
    guard: &guard::SourceGuard<'_>,
    source_file: &str,
) -> Result<(ParsedFile, guard::SourceFile), SyncError> {
    let original = guard
        .read_source(Path::new(source_file), MAX_MARKDOWN_BYTES)
        .map_err(|error| SyncError {
            source_file: source_file.to_owned(),
            line: None,
            code: "source_file_unreadable".to_owned(),
            message: error.to_string(),
        })?;
    let text = String::from_utf8(original.bytes.clone()).map_err(|error| SyncError {
        source_file: source_file.to_owned(),
        line: None,
        code: "source_file_invalid_utf8".to_owned(),
        message: error.to_string(),
    })?;
    parse_markdown_text(&text, source_file).map(|parsed| (parsed, original))
}

pub(crate) fn parse_markdown_text(text: &str, source_file: &str) -> Result<ParsedFile, SyncError> {
    let lines = text
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let tags = parse_file_tags(&lines);
    let card_starts = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.trim() == "## Card").then_some(index))
        .collect::<Vec<_>>();
    if card_starts.is_empty() {
        return Err(SyncError {
            source_file: source_file.to_owned(),
            line: Some(1),
            code: "card_heading_missing".to_owned(),
            message: "Markdown file does not contain a `## Card` section".to_owned(),
        });
    }

    let deck_path = Path::new(source_file)
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let mut cards = Vec::with_capacity(card_starts.len());
    let mut insertions = BTreeMap::new();
    for (source_order, start) in card_starts.iter().copied().enumerate() {
        let end = card_starts
            .get(source_order + 1)
            .copied()
            .unwrap_or(lines.len());
        let section = &lines[start..end];
        let front_heading = section
            .iter()
            .position(|line| line.trim() == "### Front")
            .ok_or_else(|| {
                parse_error(
                    source_file,
                    start + 1,
                    "front_heading_missing",
                    "Card is missing `### Front`",
                )
            })?;
        let back_heading = section
            .iter()
            .position(|line| line.trim() == "### Back")
            .ok_or_else(|| {
                parse_error(
                    source_file,
                    start + 1,
                    "back_heading_missing",
                    "Card is missing `### Back`",
                )
            })?;
        if back_heading <= front_heading {
            return Err(parse_error(
                source_file,
                start + back_heading + 1,
                "card_sections_invalid",
                "`### Back` must follow `### Front`",
            ));
        }

        let (id, generated_id) = card_id_from_section(section, source_file, start + 1)?;
        if generated_id {
            insertions.insert(start + 1, id.to_string());
        }
        let card_tags = parse_card_tags(section);
        let mut all_tags = tags.clone();
        all_tags.extend(card_tags);
        all_tags.sort();
        all_tags.dedup();

        let front = trim_markdown(&section[front_heading + 1..back_heading]);
        let back_end = trim_separator(&section[back_heading + 1..]);
        let back = trim_markdown(back_end);
        if front.is_empty() || back.is_empty() {
            return Err(parse_error(
                source_file,
                start + 1,
                "card_content_missing",
                "Card front and back must both contain Markdown",
            ));
        }
        let (source_question_id, correct_answer) =
            card_metadata(section, front_heading, source_file, start + 1)?;
        let source_hash = source_hash(
            &deck_path,
            &front,
            &back,
            &all_tags,
            source_question_id.as_deref(),
            &correct_answer,
        );
        cards.push(SourceCard {
            source_question_id,
            correct_answer,
            id,
            source_file: source_file.to_owned(),
            source_order: source_order as u32,
            deck_path: deck_path.clone(),
            front_markdown: front,
            back_markdown: back,
            tags: all_tags,
            source_hash,
            generated_id,
        });
    }

    let rewritten_text = if insertions.is_empty() {
        text.to_owned()
    } else {
        let mut rewritten = String::with_capacity(text.len() + insertions.len() * 60);
        for (index, line) in lines.iter().enumerate() {
            rewritten.push_str(line);
            if let Some(id) = insertions.get(&(index + 1)) {
                let line_ending = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
                rewritten.push_str(&format!("<!-- hyz-card-id: {id} -->{line_ending}"));
            }
        }
        rewritten
    };
    Ok(ParsedFile {
        cards,
        rewritten_text,
        generated_ids: !insertions.is_empty(),
    })
}

fn card_metadata(
    section: &[String],
    front_heading: usize,
    source_file: &str,
    first_line: usize,
) -> Result<(Option<String>, Vec<String>), SyncError> {
    let mut source_question_id = None;
    let mut correct_answer = None;
    for (index, line) in section.iter().enumerate().skip(1) {
        let line = line.trim();
        let (name, duplicate) = if line.starts_with("<!-- hyz-source-question-id") {
            ("hyz-source-question-id", source_question_id.is_some())
        } else if line.starts_with("<!-- hyz-correct-answer") {
            ("hyz-correct-answer", correct_answer.is_some())
        } else {
            continue;
        };
        let invalid = || {
            parse_error(
                source_file,
                first_line + index,
                "invalid_card_metadata",
                "Card metadata must be a valid header comment before `### Front`",
            )
        };
        if index >= front_heading {
            return Err(invalid());
        }
        if duplicate {
            return Err(parse_error(
                source_file,
                first_line + index,
                "duplicate_card_metadata",
                "Card metadata marker must occur at most once",
            ));
        }
        let prefix = format!("<!-- {name}:");
        let value = line
            .strip_prefix(&prefix)
            .and_then(|value| value.strip_suffix("-->"))
            .map(str::trim)
            .ok_or_else(invalid)?;
        if name == "hyz-source-question-id" {
            if value.len() != 36 || Uuid::parse_str(value).is_err() {
                return Err(invalid());
            }
            source_question_id = Some(value.to_owned());
        } else {
            correct_answer =
                Some(serde_json::from_str::<Vec<String>>(value).map_err(|_| invalid())?);
        }
    }
    Ok((source_question_id, correct_answer.unwrap_or_default()))
}

fn parse_file_tags(lines: &[String]) -> Vec<String> {
    let Some(first) = lines.first().map(|line| line.trim()) else {
        return Vec::new();
    };
    if first != "---" {
        return Vec::new();
    }
    let Some(end) = lines.iter().skip(1).position(|line| line.trim() == "---") else {
        return Vec::new();
    };
    let mut tags = Vec::new();
    let mut in_tags = false;
    for line in &lines[1..=end] {
        let trimmed = line.trim();
        if trimmed == "tags:" {
            in_tags = true;
            continue;
        }
        if in_tags {
            if let Some(value) = trimmed.strip_prefix('-') {
                push_tag(&mut tags, value);
            } else if let Some(value) = trimmed.strip_prefix("tags:") {
                for tag in value.trim().trim_matches(['[', ']']).split(',') {
                    push_tag(&mut tags, tag);
                }
            } else if !trimmed.is_empty() {
                in_tags = false;
            }
        }
    }
    tags
}

fn parse_card_tags(section: &[String]) -> Vec<String> {
    let mut tags = Vec::new();
    for line in section {
        let trimmed = line.trim();
        let Some(value) = trimmed.strip_prefix("<!-- tags:") else {
            continue;
        };
        let value = value.strip_suffix("-->").unwrap_or(value);
        for tag in value.split(',') {
            push_tag(&mut tags, tag);
        }
    }
    tags
}

fn push_tag(tags: &mut Vec<String>, value: &str) {
    let value = value.trim().trim_matches(['"', '\'']);
    if !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control) {
        tags.push(value.to_owned());
    }
}

fn card_id_from_section(
    section: &[String],
    source_file: &str,
    line: usize,
) -> Result<(FlashcardId, bool), SyncError> {
    let mut ids = section.iter().filter_map(|line| {
        let value = line.trim().strip_prefix("<!-- hyz-card-id:")?;
        Some(value.strip_suffix("-->").unwrap_or(value).trim().to_owned())
    });
    let Some(value) = ids.next() else {
        let id = Uuid::now_v7().to_string();
        return Ok((
            FlashcardId::new(id).expect("Uuid::now_v7 must be UUIDv7"),
            true,
        ));
    };
    if ids.next().is_some() {
        return Err(parse_error(
            source_file,
            line,
            "duplicate_card_id_marker",
            "Card contains more than one card ID marker",
        ));
    }
    let id = FlashcardId::new(value).map_err(|_| {
        parse_error(
            source_file,
            line,
            "invalid_card_id",
            "card ID must be a UUIDv7",
        )
    })?;
    Ok((id, false))
}

fn remove_duplicate_ids(snapshot: &mut SourceSnapshot) {
    let mut locations = BTreeMap::<String, Vec<(String, usize)>>::new();
    for (index, card) in snapshot.cards.iter().enumerate() {
        locations
            .entry(card.id.to_string())
            .or_default()
            .push((card.source_file.clone(), index));
    }
    let duplicates = locations
        .into_iter()
        .filter(|(_, locations)| locations.len() > 1)
        .collect::<Vec<_>>();
    if duplicates.is_empty() {
        return;
    }
    let mut duplicate_indexes = BTreeSet::new();
    for (id, locations) in duplicates {
        for (source_file, index) in locations {
            duplicate_indexes.insert(index);
            snapshot.successful_markdown_files.remove(&source_file);
            snapshot.failed_markdown_files.insert(source_file.clone());
            snapshot.errors.push(SyncError {
                source_file,
                line: None,
                code: "duplicate_card_id".to_owned(),
                message: format!("card ID {id} appears more than once in the source tree"),
            });
        }
    }
    snapshot.cards = snapshot
        .cards
        .drain(..)
        .enumerate()
        .filter_map(|(index, card)| (!duplicate_indexes.contains(&index)).then_some(card))
        .collect();
}

fn safe_relative_asset_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn source_hash(
    deck: &str,
    front: &str,
    back: &str,
    tags: &[String],
    source_question_id: Option<&str>,
    correct_answer: &[String],
) -> String {
    let mut digest = Sha256::new();
    digest.update(deck.as_bytes());
    digest.update([0]);
    digest.update(front.as_bytes());
    digest.update([0]);
    digest.update(back.as_bytes());
    digest.update([0]);
    for tag in tags {
        digest.update(tag.as_bytes());
        digest.update([0]);
    }
    // Metadata-free cards retain their original source hash.
    if source_question_id.is_some() || !correct_answer.is_empty() {
        digest.update(b"hyz-favorite-metadata\0");
        digest.update(source_question_id.unwrap_or_default().as_bytes());
        digest.update([0]);
        for answer in correct_answer {
            digest.update((answer.len() as u64).to_be_bytes());
            digest.update(answer.as_bytes());
        }
    }
    let digest = digest.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn trim_separator(lines: &[String]) -> &[String] {
    let mut end = lines.len();
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    if end > 0 && lines[end - 1].trim() == "---" {
        &lines[..end - 1]
    } else {
        &lines[..end]
    }
}

fn trim_markdown(lines: &[String]) -> String {
    let mut value = lines.concat();
    while value.ends_with('\n') || value.ends_with('\r') {
        value.pop();
    }
    value.trim().to_owned()
}

fn parse_error(source_file: &str, line: usize, code: &str, message: &str) -> SyncError {
    SyncError {
        source_file: source_file.to_owned(),
        line: Some(line as u32),
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
fn atomic_replace(path: &Path, contents: String) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "source file has no parent directory",
        )
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("card.md");
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = parent.join(format!(".{file_name}.hyz-card-{sequence}.tmp"));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(contents.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD_ID: &str = "019a31b7-8c42-7c16-a6d1-0c1f0f34c821";

    fn metadata_markdown(metadata: &str) -> String {
        format!("## Card\n{metadata}\n<!-- hyz-card-id: {CARD_ID} -->\n### Front\nQuestion\n### Back\n-\n")
    }

    #[tokio::test]
    async fn mounted_source_rejects_local_directory_without_cifs_before_sync() {
        let root = std::env::temp_dir().join(format!("hyz-flashcard-no-mount-{}", Uuid::now_v7()));
        fs::create_dir(&root).unwrap();
        let source = FilesystemFlashcardSource {
            root: root.clone(),
            mount_checked: true,
        };
        let result = FlashcardSourcePort::scan(&source).await;
        fs::remove_dir(&root).unwrap();
        assert!(matches!(result, Err(FlashcardError::SourceUnavailable(_))));
    }

    #[test]
    fn source_guard_uuid_sync_preserves_non_markdown_original_backup() {
        let root = std::env::temp_dir().join(format!("hyz-source-backup-{}", Uuid::now_v7()));
        fs::create_dir(&root).unwrap();
        let original = "## Card\r\n### Front\r\nQ\r\n### Back\r\nA\r\n";
        fs::write(root.join("cards.md"), original).unwrap();
        let source = FilesystemFlashcardSource::new(&root);
        let first = source.scan_blocking().unwrap();
        assert_eq!(first.cards.len(), 1);
        let backups = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "bak"))
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(&backups[0]).unwrap(), original.as_bytes());
        let second = source.scan_blocking().unwrap();
        assert_eq!(first.cards[0].id, second.cards[0].id);
        assert!(!second.cards[0].generated_id);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_guard_mount_loss_at_end_never_returns_a_complete_snapshot() {
        let root = std::env::temp_dir().join(format!("hyz-source-disconnect-{}", Uuid::now_v7()));
        fs::create_dir(&root).unwrap();
        let observations = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = observations.clone();
        let observe = move |_: &Path, _: &File| {
            counter.set(counter.get() + 1);
            if counter.get() >= 3 {
                Err(io::Error::other("disconnected"))
            } else {
                Ok("mounted".to_owned())
            }
        };
        let guard = guard::SourceGuard::with_observer(&root, true, &observe).unwrap();
        let source = FilesystemFlashcardSource::new(&root);
        assert!(matches!(
            source.scan_guarded(&guard),
            Err(FlashcardError::SourceUnavailable(_))
        ));
        assert_eq!(observations.get(), 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_guard_mounted_assets_reject_local_fallback() {
        let root = std::env::temp_dir().join(format!("hyz-source-asset-mount-{}", Uuid::now_v7()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("image.png"), b"image").unwrap();
        let source = FilesystemFlashcardSource {
            root: root.clone(),
            mount_checked: true,
        };
        assert_eq!(
            source.read_asset("image.png").unwrap_err(),
            FlashcardAssetError::Unavailable
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_favorite_metadata_without_changing_card_id_or_content() {
        let id = CARD_ID;
        let question_id = "019a31b7-8c42-7c16-a6d1-0c1f0f34c822";
        let text = metadata_markdown(&format!(
            "<!-- hyz-source-question-id: {question_id} -->\n<!-- hyz-correct-answer: [\"B\"] -->"
        ));
        let parsed = parse_markdown_text(&text, "数量关系/工程问题/错题/cards.md").unwrap();
        let card = &parsed.cards[0];
        assert_eq!(card.id.as_str(), id);
        assert_eq!(card.source_question_id.as_deref(), Some(question_id));
        assert_eq!(card.correct_answer, vec!["B"]);
        assert_eq!(card.front_markdown, "Question");
        assert_eq!(card.back_markdown, "-");
        assert!(!parsed.generated_ids);
        assert_eq!(parsed.rewritten_text, text);
    }

    #[test]
    fn rejects_malformed_duplicate_and_misplaced_favorite_metadata() {
        let question = "<!-- hyz-source-question-id: 550e8400-e29b-41d4-a716-446655440000 -->";
        for metadata in [
            "<!-- hyz-source-question-id: not-a-uuid -->".to_owned(),
            "<!-- hyz-source-question-id: 550e8400-e29b-41d4-a716-446655440000".to_owned(),
            format!("{question}\n{question}"),
            "<!-- hyz-correct-answer: not-json -->".to_owned(),
            "<!-- hyz-correct-answer: [1] -->".to_owned(),
            "<!-- hyz-correct-answer: [\"B\"]".to_owned(),
            "<!-- hyz-correct-answer: [\"B\"] -->\n<!-- hyz-correct-answer: [\"C\"] -->".to_owned(),
        ] {
            let text = metadata_markdown(&metadata);
            assert!(
                parse_markdown_text(&text, "cards.md").is_err(),
                "{metadata}"
            );
        }
        for body in [
            format!("Question\n{question}\n### Back\n-"),
            format!("Question\n### Back\n-\n{question}"),
        ] {
            assert!(
                parse_markdown_text(&format!("## Card\n### Front\n{body}\n"), "cards.md").is_err()
            );
        }
    }

    #[test]
    fn metadata_changes_source_hash_but_preserves_legacy_identity_and_hash() {
        let legacy = parse_markdown_text(&metadata_markdown(""), "deck/cards.md").unwrap();
        let original = &legacy.cards[0];
        assert_eq!(original.source_question_id, None);
        assert!(original.correct_answer.is_empty());
        assert_eq!(
            original.source_hash,
            "0f994d4f37b5eca9a28cd38a755562dcec2cb1e5bd9c432f63f934d8ab7f425c"
        );
        for marker in [
            "<!-- hyz-source-question-id: 550e8400-e29b-41d4-a716-446655440000 -->",
            "<!-- hyz-correct-answer: [\"B\"] -->",
            "<!-- hyz-correct-answer: [\"A\",\"C\"] -->",
        ] {
            let parsed = parse_markdown_text(&metadata_markdown(marker), "deck/cards.md").unwrap();
            assert_eq!(parsed.cards[0].id, original.id);
            assert_ne!(
                parsed.cards[0].source_hash, original.source_hash,
                "{marker}"
            );
        }
        let first = parse_markdown_text(
            "## Card\n### Front\nQuestion\n### Back\n-\n",
            "deck/cards.md",
        )
        .unwrap();
        let again = parse_markdown_text(&first.rewritten_text, "deck/cards.md").unwrap();
        assert_eq!(first.cards[0].id, again.cards[0].id);
        assert_eq!(first.cards[0].source_hash, again.cards[0].source_hash);
        assert!(!again.generated_ids);
    }

    #[test]
    fn parses_multiple_cards_file_tags_card_tags_and_preserves_latex() {
        let parsed = parse_markdown_text(
            "---\ntags:\n  - file-tag\n---\n\n## Card\n\n### Front\n\nWhy?\n\n### Back\n\n$$x^2$$\n\n---\n\n## Card\n<!-- tags: card-tag, 高频 -->\n\n### Front\n\nSecond\n\n### Back\n\nAnswer\n",
            "判断推理/逻辑判断/cards.md",
        )
        .unwrap();
        assert_eq!(parsed.cards.len(), 2);
        assert_eq!(parsed.cards[0].deck_path, "判断推理/逻辑判断");
        assert_eq!(parsed.cards[0].tags, vec!["file-tag"]);
        assert_eq!(parsed.cards[0].back_markdown, "$$x^2$$");
        assert_eq!(parsed.cards[1].tags, vec!["card-tag", "file-tag", "高频"]);
        assert!(parsed.generated_ids);
        assert_eq!(parsed.rewritten_text.matches("hyz-card-id:").count(), 2);
    }

    #[test]
    fn duplicate_ids_are_reported_and_cards_are_excluded() {
        let id = CARD_ID;
        let first = parse_markdown_text(
            &format!("## Card\n<!-- hyz-card-id: {id} -->\n### Front\nA\n### Back\nB\n"),
            "a.md",
        )
        .unwrap();
        let second = parse_markdown_text(
            &format!("## Card\n<!-- hyz-card-id: {id} -->\n### Front\nC\n### Back\nD\n"),
            "b.md",
        )
        .unwrap();
        let mut snapshot = SourceSnapshot {
            cards: [first.cards, second.cards].concat(),
            scanned_markdown_files: ["a.md", "b.md"].into_iter().map(str::to_owned).collect(),
            successful_markdown_files: ["a.md", "b.md"].into_iter().map(str::to_owned).collect(),
            ..SourceSnapshot::default()
        };
        remove_duplicate_ids(&mut snapshot);
        assert!(snapshot.cards.is_empty());
        assert!(snapshot
            .errors
            .iter()
            .all(|error| error.code == "duplicate_card_id"));
        assert!(snapshot.failed_markdown_files.contains("a.md"));
        assert!(snapshot.failed_markdown_files.contains("b.md"));
    }

    #[test]
    fn atomic_replace_writes_same_directory_and_replaces_original() {
        let root =
            std::env::temp_dir().join(format!("hyz-flashcard-source-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("cards.md");
        fs::write(&path, "old").unwrap();
        atomic_replace(&path, "new".to_owned()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        let leftovers = fs::read_dir(&root).unwrap().count();
        assert_eq!(leftovers, 1);
        let _ = fs::remove_dir_all(&root);
    }
}
