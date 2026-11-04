//! Mount-checked, descriptor-anchored favorite card source writer.
use crate::adapters::outbound::flashcard_source::parse_markdown_text;
use crate::application::{
    favorite_flashcard_ports::{
        FavoriteCardAsset, FavoriteFlashcardWriterPort, FavoriteWriteResult, NewFavoriteCard,
    },
    flashcards::FlashcardError,
};
use crate::domain::favorite_flashcard::{
    resolve_target, resolve_web_target, FlashcardCreationMode,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{CString, OsStr},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
    sync::Arc,
};

const ROOT: &str = "/mnt/hyz-cards";
const MAX_MARKDOWN: u64 = 8 * 1024 * 1024;
const MAX_ASSET: usize = 16 * 1024 * 1024;
const MAX_SOURCE_FILES: usize = 100_000;

pub use crate::application::favorite_flashcard_ports::{
    FavoriteMountIdentity, FavoriteMountObservationPort,
};

struct ProcMountObserver;
impl FavoriteMountObservationPort for ProcMountObserver {
    fn observe(&self, root: &Path) -> Result<FavoriteMountIdentity, FlashcardError> {
        let mounts = fs::read_to_string("/proc/self/mountinfo").map_err(source_io)?;
        parse_mount_identity(&mounts, root)
    }
}

fn parse_mount_identity(
    mounts: &str,
    root: &Path,
) -> Result<FavoriteMountIdentity, FlashcardError> {
    let mut found = None;
    for line in mounts.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let fields: Vec<_> = left.split_whitespace().collect();
        let filesystem: Vec<_> = right.split_whitespace().collect();
        if fields.len() < 6 || filesystem.len() < 3 {
            continue;
        }
        let mount_path = unescape_mount(fields[4]);
        if Path::new(&mount_path) != root {
            continue;
        }
        if found.is_some()
            || filesystem[0] != "cifs"
            || !filesystem[1].ends_with("/hyz-cards")
            || !fields[5].split(',').any(|v| v == "rw")
            || !filesystem[2].split(',').any(|v| v == "rw")
        {
            return Err(unavailable("expected writable hyz-cards CIFS mount"));
        }
        found = Some(FavoriteMountIdentity {
            mount_id: format!("{}:{}:{}", fields[0], fields[2], fields[3]),
            source: unescape_mount(filesystem[1]),
        });
    }
    found.ok_or_else(|| unavailable("hyz-cards CIFS mount is absent"))
}

fn unescape_mount(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[derive(Clone)]
pub struct FilesystemFavoriteFlashcardWriter {
    root: PathBuf,
    observer: Arc<dyn FavoriteMountObservationPort>,
}
impl Default for FilesystemFavoriteFlashcardWriter {
    fn default() -> Self {
        Self {
            root: ROOT.into(),
            observer: Arc::new(ProcMountObserver),
        }
    }
}
impl FilesystemFavoriteFlashcardWriter {
    #[cfg(any(test, feature = "e2e"))]
    pub fn with_observer(root: PathBuf, observer: Arc<dyn FavoriteMountObservationPort>) -> Self {
        Self { root, observer }
    }

    fn verify_root(
        &self,
        root: &File,
        identity: &FavoriteMountIdentity,
    ) -> Result<(), FlashcardError> {
        if self.observer.observe(&self.root)? != *identity {
            return Err(unavailable("source mount identity changed"));
        }
        let current = open_root(&self.root)?;
        if file_identity(root)? != file_identity(&current)? {
            return Err(unavailable("source root changed"));
        }
        Ok(())
    }

    fn append_blocking(
        &self,
        card: NewFavoriteCard,
    ) -> Result<FavoriteWriteResult, FlashcardError> {
        validate_card(&card)?;
        let target = resolve_web_target(&card.target_deck)
            .ok_or_else(|| invalid("unsupported favorite target"))?;
        let identity = self.observer.observe(&self.root)?;
        let root = open_root(&self.root)?;
        self.verify_root(&root, &identity)?;
        let scan = scan_source(&root, &card.question_id, target.mode)?;
        self.verify_root(&root, &identity)?;
        if let Some(existing) = scan.existing {
            return Ok(existing);
        }

        let target_path = Path::new(&target.relative_path);
        let parent_path = target_path
            .parent()
            .ok_or_else(|| invalid("invalid registry path"))?;
        let parent = walk_directory(&root, parent_path, true)?;
        let name = target_path
            .file_name()
            .ok_or_else(|| invalid("invalid registry path"))?;
        let original = read_source_optional(&parent, name, MAX_MARKDOWN)?;
        if original.as_ref().map(|file| &file.version) != scan.files.get(&target.relative_path) {
            return Err(unavailable("target changed after scan"));
        }
        let mut text = match &original {
            Some(file) => String::from_utf8(file.bytes.clone())
                .map_err(|_| unavailable("target is not UTF-8"))?,
            None => {
                let tags =
                    serde_yaml::to_string(&target.tags).map_err(|e| invalid(e.to_string()))?;
                format!(
                    "---\ntags:\n{}---\n",
                    tags.lines()
                        .map(|line| format!("  {line}\n"))
                        .collect::<String>()
                )
            }
        };
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        if !text.ends_with('\n') {
            text.push_str(newline);
        }
        if original.is_some()
            && text
                .lines()
                .rev()
                .map(str::trim)
                .find(|line| !line.is_empty())
                != Some("---")
        {
            text.push_str(&format!("{newline}---{newline}"));
        }
        text.push_str(&format!(
            "{newline}## Card{newline}\
             <!-- hyz-source-question-id: {} -->{newline}\
             <!-- hyz-correct-answer: {} -->{newline}\
             ### Front{newline}{newline}{}{newline}{newline}\
             ### Back{newline}{newline}{}{newline}{newline}\
             ---{newline}",
            card.question_id,
            serde_json::to_string(&card.correct_answer).map_err(|e| invalid(e.to_string()))?,
            normalize_newlines(&card.front, newline),
            normalize_newlines(&card.back, newline)
        ));
        parse_markdown_text(&text, &target.relative_path)
            .map_err(|e| unavailable(format!("invalid appended source: {}", e.message)))?;
        if text.len() as u64 > MAX_MARKDOWN {
            return Err(invalid("target exceeds Markdown size limit"));
        }
        for asset in &card.assets {
            self.write_asset(&root, &identity, asset)?;
        }
        let temp = TemporaryFile::write(&parent, "favorite-tmp", text.as_bytes())?;
        self.verify_root(&root, &identity)?;
        if scan_source(&root, &card.question_id, target.mode)?.files != scan.files {
            return Err(unavailable("source changed before commit"));
        }
        if read_source_optional(&parent, name, MAX_MARKDOWN)? != original {
            return Err(unavailable("target changed before commit"));
        }
        if let Some(file) = &original {
            let backup = TemporaryFile::write(&parent, "favorite-backup", &file.bytes)?;
            backup.keep();
            parent.sync_all().map_err(source_io)?;
        }
        self.verify_root(&root, &identity)?;
        if read_source_optional(&parent, name, MAX_MARKDOWN)? != original {
            return Err(unavailable("target changed during backup"));
        }
        // Re-open the path through the anchored root to detect directory replacement.
        let current_parent = walk_directory(&root, parent_path, false)?;
        if file_identity(&parent)? != file_identity(&current_parent)? {
            return Err(unavailable("target directory changed"));
        }
        rename(&parent, &temp.name, name, original.is_none())?;
        temp.keep();
        parent.sync_all().map_err(source_io)?;
        self.verify_root(&root, &identity)?;
        Ok(FavoriteWriteResult {
            relative_path: target.relative_path.to_owned(),
            card_id: None,
            appended: true,
        })
    }

    fn write_asset(
        &self,
        root: &File,
        identity: &FavoriteMountIdentity,
        asset: &FavoriteCardAsset,
    ) -> Result<(), FlashcardError> {
        self.verify_root(root, identity)?;
        let directory = walk_directory(root, Path::new("images"), true)?;
        let name = OsStr::new(&asset.file_name);
        if let Some(existing) = read_optional(&directory, name, MAX_ASSET as u64)? {
            if existing != asset.contents {
                return Err(unavailable("asset content conflicts with hash filename"));
            }
            return Ok(());
        }
        let temporary = TemporaryFile::write(&directory, "favorite-asset", &asset.contents)?;
        self.verify_root(root, identity)?;
        rename(&directory, &temporary.name, name, true)?;
        temporary.keep();
        directory.sync_all().map_err(source_io)
    }
}

#[async_trait]
impl FavoriteFlashcardWriterPort for FilesystemFavoriteFlashcardWriter {
    async fn append(&self, card: NewFavoriteCard) -> Result<FavoriteWriteResult, FlashcardError> {
        let writer = self.clone();
        tokio::task::spawn_blocking(move || writer.append_blocking(card))
            .await
            .map_err(|_| unavailable("favorite writer task failed"))?
    }
}

fn validate_card(card: &NewFavoriteCard) -> Result<(), FlashcardError> {
    let id = uuid::Uuid::parse_str(&card.question_id)
        .map_err(|_| invalid("invalid source question UUID"))?;
    if id.to_string() != card.question_id {
        return Err(invalid("source question UUID must be canonical"));
    }
    if card.correct_answer.is_empty()
        || card.correct_answer.len() > 26
        || card
            .correct_answer
            .iter()
            .any(|v| v.len() != 1 || !v.as_bytes()[0].is_ascii_uppercase())
    {
        return Err(invalid("invalid correct answer"));
    }
    for content in [&card.front, &card.back] {
        if content.trim().is_empty()
            || content.len() as u64 > MAX_MARKDOWN
            || content.contains('\0')
            || content.contains("<!-- hyz-")
            || content.contains("<!-- tags:")
            || content.lines().any(|line| {
                matches!(
                    line.trim(),
                    "## Card" | "### Front" | "### Back" | "---" | "+++"
                )
            })
        {
            return Err(invalid("favorite content contains reserved structure"));
        }
    }
    let mut names = BTreeSet::new();
    for asset in &card.assets {
        crate::application::question_bank::images::asset_type(&asset.file_name)
            .map_err(|_| invalid("invalid asset filename"))?;
        let digest = asset.file_name.split('.').next().unwrap_or_default();
        if asset.contents.len() > MAX_ASSET
            || hash(&asset.contents) != digest
            || !names.insert(&asset.file_name)
        {
            return Err(invalid("invalid content-addressed asset"));
        }
    }
    Ok(())
}

fn normalize_newlines(value: &str, newline: &str) -> String {
    value.replace("\r\n", "\n").replace('\n', newline)
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn invalid(message: impl Into<String>) -> FlashcardError {
    FlashcardError::InvalidRequest(message.into())
}
fn unavailable(message: impl Into<String>) -> FlashcardError {
    FlashcardError::SourceUnavailable(message.into())
}
fn source_io(error: io::Error) -> FlashcardError {
    unavailable(error.to_string())
}
fn cstring(value: &OsStr) -> Result<CString, FlashcardError> {
    CString::new(value.as_bytes()).map_err(|_| invalid("NUL in source path"))
}
fn file_identity(file: &File) -> Result<(u64, u64), FlashcardError> {
    let m = file.metadata().map_err(source_io)?;
    Ok((m.dev(), m.ino()))
}

fn open_root(path: &Path) -> Result<File, FlashcardError> {
    if !path.is_absolute() {
        return Err(invalid("root must be absolute"));
    }
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")
        .map_err(source_io)?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = open_child(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY)
                    .map_err(source_io)?
            }
            _ => return Err(invalid("unsafe root path")),
        }
    }
    Ok(directory)
}
fn open_child(parent: &File, name: &OsStr, flags: i32) -> io::Result<File> {
    let name =
        CString::new(name.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
fn walk_directory(root: &File, path: &Path, create: bool) -> Result<File, FlashcardError> {
    let mut directory = root.try_clone().map_err(source_io)?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("unsafe relative directory"));
        };
        match open_child(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY) {
            Ok(child) => directory = child,
            Err(e) if create && e.kind() == io::ErrorKind::NotFound => {
                let name_c = cstring(name)?;
                if unsafe { libc::mkdirat(directory.as_raw_fd(), name_c.as_ptr(), 0o700) } != 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(source_io(error));
                    }
                }
                directory.sync_all().map_err(source_io)?;
                directory = open_child(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY)
                    .map_err(source_io)?;
            }
            Err(error) => return Err(source_io(error)),
        }
    }
    Ok(directory)
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceMetadata {
    device: u64,
    inode: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    length: u64,
}
impl From<&fs::Metadata> for SourceMetadata {
    fn from(value: &fs::Metadata) -> Self {
        Self {
            device: value.dev(),
            inode: value.ino(),
            modified: (value.mtime(), value.mtime_nsec()),
            changed: (value.ctime(), value.ctime_nsec()),
            length: value.len(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceVersion {
    digest: String,
    metadata: SourceMetadata,
}
#[derive(Debug, PartialEq, Eq)]
struct SourceFile {
    bytes: Vec<u8>,
    version: SourceVersion,
}
fn read_optional(
    parent: &File,
    name: &OsStr,
    limit: u64,
) -> Result<Option<Vec<u8>>, FlashcardError> {
    Ok(read_source_optional(parent, name, limit)?.map(|file| file.bytes))
}
fn read_source_optional(
    parent: &File,
    name: &OsStr,
    limit: u64,
) -> Result<Option<SourceFile>, FlashcardError> {
    let file = match open_child(parent, name, libc::O_RDONLY) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(source_io(error)),
    };
    read_source_file(file, limit).map(Some)
}
fn read_source_file(file: File, limit: u64) -> Result<SourceFile, FlashcardError> {
    let metadata = file.metadata().map_err(source_io)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(unavailable("source is not a bounded regular file"));
    }
    let original = SourceMetadata::from(&metadata);
    let mut bytes = Vec::new();
    (&file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(source_io)?;
    if bytes.len() as u64 > limit {
        return Err(unavailable("source file exceeds size limit"));
    }
    if SourceMetadata::from(&file.metadata().map_err(source_io)?) != original {
        return Err(unavailable("source changed while being read"));
    }
    Ok(SourceFile {
        version: SourceVersion {
            digest: hash(&bytes),
            metadata: original,
        },
        bytes,
    })
}

struct SourceScan {
    files: BTreeMap<String, SourceVersion>,
    existing: Option<FavoriteWriteResult>,
}
fn scan_source(
    root: &File,
    question_id: &str,
    requested_mode: FlashcardCreationMode,
) -> Result<SourceScan, FlashcardError> {
    let mut result = SourceScan {
        files: BTreeMap::new(),
        existing: None,
    };
    // A single original question may have one card in each parallel creation mode.
    // Within a mode, the UUID remains unique, even if someone moved its Markdown.
    let mut questions: BTreeMap<String, BTreeSet<Option<bool>>> = BTreeMap::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut queue = vec![(root.try_clone().map_err(source_io)?, PathBuf::new())];
    let mut entries_seen = 0;
    while let Some((directory, relative)) = queue.pop() {
        // procfs exposes the already-open directory, never a newly resolved source path.
        let entries =
            fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd())).map_err(source_io)?;
        for entry in entries {
            let entry = entry.map_err(source_io)?;
            entries_seen += 1;
            if entries_seen > MAX_SOURCE_FILES {
                return Err(unavailable("source scan limit exceeded"));
            }
            let name = entry.file_name();
            let child = open_child(&directory, &name, libc::O_RDONLY).map_err(source_io)?;
            let metadata = child.metadata().map_err(source_io)?;
            let path = relative.join(&name);
            if metadata.is_dir() {
                queue.push((child, path));
                continue;
            }
            if !metadata.is_file() {
                return Err(unavailable("non-regular source entry"));
            }
            if path.extension().and_then(OsStr::to_str) != Some("md") {
                continue;
            }
            let path = path
                .to_str()
                .ok_or_else(|| unavailable("non UTF-8 Markdown path"))?
                .to_owned();
            let file = read_source_file(child, MAX_MARKDOWN)?;
            let text = std::str::from_utf8(&file.bytes)
                .map_err(|_| unavailable("source Markdown is not UTF-8"))?;
            let parsed = parse_markdown_text(text, &path)
                .map_err(|error| unavailable(format!("{}: {}", path, error.message)))?;
            result.files.insert(path.clone(), file.version);
            let source_mode = path
                .strip_suffix("/cards.md")
                .and_then(resolve_target)
                .map(|target| target.mode == FlashcardCreationMode::UserOrganized);
            for card in parsed.cards {
                if !card.generated_id && !ids.insert(card.id.to_string()) {
                    return Err(unavailable("conflicting duplicate card UUID"));
                }
                if let Some(source_id) = &card.source_question_id {
                    let source_id = source_id.to_ascii_lowercase();
                    let seen = questions.entry(source_id.clone()).or_default();
                    // Unknown/moved decks are conservatively treated as a conflict with
                    // any second copy, so moving an existing card cannot silently clone it.
                    if (!seen.is_empty() && (source_mode.is_none() || seen.contains(&None)))
                        || !seen.insert(source_mode)
                    {
                        return Err(unavailable("conflicting duplicate source question ID"));
                    }
                    if source_id == question_id
                        && (source_mode.is_none()
                            || source_mode
                                == Some(requested_mode == FlashcardCreationMode::UserOrganized))
                    {
                        result.existing = Some(FavoriteWriteResult {
                            relative_path: path.clone(),
                            card_id: (!card.generated_id).then_some(card.id),
                            appended: false,
                        });
                    }
                }
            }
        }
    }
    Ok(result)
}

struct TemporaryFile<'a> {
    directory: &'a File,
    name: std::ffi::OsString,
    remove: std::cell::Cell<bool>,
}
impl<'a> TemporaryFile<'a> {
    fn write(directory: &'a File, prefix: &str, contents: &[u8]) -> Result<Self, FlashcardError> {
        let name = format!(".{prefix}-{}", uuid::Uuid::now_v7());
        let name: std::ffi::OsString = name.into();
        let mut file = open_child(
            directory,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )
        .map_err(source_io)?;
        let result = Self {
            directory,
            name,
            remove: std::cell::Cell::new(true),
        };
        file.write_all(contents).map_err(source_io)?;
        file.sync_all().map_err(source_io)?;
        Ok(result)
    }
    fn keep(&self) {
        self.remove.set(false);
    }
}
impl Drop for TemporaryFile<'_> {
    fn drop(&mut self) {
        if self.remove.get() {
            if let Ok(name) = cstring(&self.name) {
                unsafe {
                    libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0);
                }
            }
        }
    }
}
fn rename(
    directory: &File,
    from: &OsStr,
    to: &OsStr,
    exclusive: bool,
) -> Result<(), FlashcardError> {
    let from = cstring(from)?;
    let to = cstring(to)?;
    let flags = if exclusive { libc::RENAME_NOREPLACE } else { 0 };
    if unsafe {
        libc::renameat2(
            directory.as_raw_fd(),
            from.as_ptr(),
            directory.as_raw_fd(),
            to.as_ptr(),
            flags,
        )
    } != 0
    {
        return Err(source_io(io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::favorite_flashcard_ports::{
        FavoriteFlashcardWriterPort, NewFavoriteCard,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FakeMount(AtomicBool);
    impl FavoriteMountObservationPort for FakeMount {
        fn observe(&self, _: &Path) -> Result<FavoriteMountIdentity, FlashcardError> {
            if !self.0.load(Ordering::SeqCst) {
                return Err(unavailable("mount gone"));
            }
            Ok(FavoriteMountIdentity {
                mount_id: "test".into(),
                source: "//test/hyz-cards".into(),
            })
        }
    }
    fn card() -> NewFavoriteCard {
        NewFavoriteCard {
            question_id: "0194f4e0-1234-7000-8000-000000000001".into(),
            target_deck: "真题练习/数量关系/工程效率/错题".into(),
            front: "Question".into(),
            back: "-".into(),
            correct_answer: vec!["B".into()],
            assets: vec![],
        }
    }
    struct Fixture {
        root: PathBuf,
        mount: Arc<FakeMount>,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("hyz-favorite-writer-{}", uuid::Uuid::now_v7()));
            fs::create_dir(&root).unwrap();
            Self {
                root,
                mount: Arc::new(FakeMount(AtomicBool::new(true))),
            }
        }
        fn writer(&self) -> FilesystemFavoriteFlashcardWriter {
            FilesystemFavoriteFlashcardWriter::with_observer(self.root.clone(), self.mount.clone())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn put(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn existing_card(question: &str, id: Option<&str>) -> String {
        let id = id
            .map(|id| format!("<!-- hyz-card-id: {id} -->\n"))
            .unwrap_or_default();
        format!("## Card\n{id}<!-- hyz-source-question-id: {question} -->\n<!-- hyz-correct-answer: [\"A\"] -->\n### Front\nOriginal question\n### Back\nOriginal answer\n")
    }

    #[tokio::test]
    async fn moved_card_is_deduplicated_without_changing_back_or_target() {
        let fixture = Fixture::new();
        let id = "0194f4e0-1234-7000-8000-000000000002";
        let original = existing_card(&card().question_id, Some(id));
        put(&fixture.root, "moved/deck/cards.md", &original);
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        let result = writer.append(card()).await.unwrap();
        assert!(!result.appended);
        assert_eq!(result.relative_path, "moved/deck/cards.md");
        assert_eq!(result.card_id.unwrap().as_str(), id);
        assert_eq!(
            fs::read_to_string(fixture.root.join("moved/deck/cards.md")).unwrap(),
            original
        );
        assert!(!fixture.root.join("真题练习").exists());
    }

    #[tokio::test]
    async fn card_without_uuid_is_still_deduplicated() {
        let fixture = Fixture::new();
        put(
            &fixture.root,
            "moved/cards.md",
            &existing_card(&card().question_id, None),
        );
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        let result = writer.append(card()).await.unwrap();
        assert!(!result.appended);
        assert!(result.card_id.is_none());
        assert!(!fs::read_to_string(fixture.root.join("moved/cards.md"))
            .unwrap()
            .contains("hyz-card-id:"));
    }

    #[tokio::test]
    async fn incomplete_scan_rejects_even_when_a_duplicate_was_found() {
        let fixture = Fixture::new();
        put(
            &fixture.root,
            "found/cards.md",
            &existing_card(&card().question_id, None),
        );
        put(
            &fixture.root,
            "broken/cards.md",
            "## Card\n### Front\nMissing back",
        );
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        assert!(matches!(
            writer.append(card()).await,
            Err(FlashcardError::SourceUnavailable(_))
        ));
        assert!(!fixture.root.join("数量").exists());
    }

    #[tokio::test]
    async fn conflicting_question_duplicates_reject_instead_of_selecting_one() {
        let fixture = Fixture::new();
        let original = existing_card(&card().question_id, None);
        put(&fixture.root, "a/cards.md", &original);
        put(&fixture.root, "b/cards.md", &original);
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        assert!(matches!(
            writer.append(card()).await,
            Err(FlashcardError::SourceUnavailable(_))
        ));
    }

    #[tokio::test]
    async fn ordinary_review_deck_is_created_then_appended_without_duplication() {
        let fixture = Fixture::new();
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        let mut first = card();
        first.target_deck = "真题练习/言语理解与表达/中心理解/错题".into();
        let result = writer.append(first.clone()).await.unwrap();
        assert!(result.appended);
        assert_eq!(
            result.relative_path,
            "真题练习/言语理解与表达/中心理解/错题/cards.md"
        );
        assert!(!writer.append(first).await.unwrap().appended);

        let mut second = card();
        second.question_id = "0194f4e0-1234-7000-8000-000000000042".into();
        second.target_deck = "真题练习/言语理解与表达/中心理解/错题".into();
        assert!(writer.append(second).await.unwrap().appended);
        let path = fixture
            .root
            .join("真题练习/言语理解与表达/中心理解/错题/cards.md");
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("中心理解"));
        assert_eq!(
            parse_markdown_text(&text, "真题练习/言语理解与表达/中心理解/错题/cards.md")
                .unwrap()
                .cards
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn same_question_can_be_saved_once_in_each_parallel_mode() {
        let fixture = Fixture::new();
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        let ai = card();
        let mut normal = ai.clone();
        normal.target_deck = "用户整理/数量关系/工程效率/错题".into();
        let ai_result = writer.append(ai.clone()).await.unwrap();
        let normal_result = writer.append(normal.clone()).await.unwrap();
        assert!(ai_result.appended);
        assert!(normal_result.appended);
        assert_eq!(
            ai_result.relative_path,
            "真题练习/数量关系/工程效率/错题/cards.md"
        );
        assert_eq!(
            normal_result.relative_path,
            "用户整理/数量关系/工程效率/错题/cards.md"
        );
        assert!(!writer.append(ai).await.unwrap().appended);
        assert!(!writer.append(normal).await.unwrap().appended);
    }

    #[tokio::test]
    async fn missing_mount_never_creates_a_local_fallback() {
        let fixture = Fixture::new();
        fixture.mount.0.store(false, Ordering::SeqCst);
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        assert!(matches!(
            writer.append(card()).await,
            Err(FlashcardError::SourceUnavailable(_))
        ));
        assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn symlinked_source_entries_are_rejected() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        std::os::unix::fs::symlink(&outside.root, fixture.root.join("真题练习")).unwrap();
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        assert!(matches!(
            writer.append(card()).await,
            Err(FlashcardError::SourceUnavailable(_))
        ));
        assert_eq!(fs::read_dir(&outside.root).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn reserved_structure_is_rejected_before_writing() {
        let fixture = Fixture::new();
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        for content in [
            "## Card",
            "### Front",
            "### Back",
            "---",
            "<!-- hyz-card-id: injected -->",
            "<!-- tags: injected -->",
        ] {
            let mut request = card();
            request.front = format!("Question\n{content}\nInjected");
            assert!(
                matches!(
                    writer.append(request).await,
                    Err(FlashcardError::InvalidRequest(_))
                ),
                "{content}"
            );
        }
        assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn unsupported_target_cannot_become_a_filesystem_path() {
        let fixture = Fixture::new();
        let mut request = card();
        request.target_deck = "../escape".into();
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        assert!(matches!(
            writer.append(request).await,
            Err(FlashcardError::InvalidRequest(_))
        ));
        assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn existing_bytes_yaml_and_crlf_are_preserved_with_backup() {
        let fixture = Fixture::new();
        let target = resolve_target(&card().target_deck).unwrap();
        let original = "---\r\ntags: [custom]\r\nother: keep-me\r\n---\r\n## Card\r\n### Front\r\nOld front\r\n### Back\r\nOld back";
        put(&fixture.root, &target.relative_path, original);
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        let result = writer.append(card()).await.unwrap();
        let path = fixture.root.join(result.relative_path);
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(original.as_bytes()));
        assert!(!String::from_utf8_lossy(&bytes)
            .replace("\r\n", "")
            .contains('\n'));
        let backups: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".favorite-backup-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(backups[0].path()).unwrap(), original.as_bytes());
        let parsed =
            parse_markdown_text(std::str::from_utf8(&bytes).unwrap(), &target.relative_path)
                .unwrap();
        assert_eq!(parsed.cards.len(), 2);
        assert_eq!(parsed.cards[0].back_markdown, "Old back");
    }

    #[tokio::test]
    async fn assets_are_content_addressed_under_root_images() {
        let fixture = Fixture::new();
        let contents = b"image bytes".to_vec();
        let file_name = format!("{}.png", hash(&contents));
        let mut request = card();
        request.front = format!("Question ![](../../../images/{file_name})");
        request.assets = vec![FavoriteCardAsset {
            file_name: file_name.clone(),
            contents: contents.clone(),
        }];
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        let result = writer.append(request).await.unwrap();
        assert_eq!(
            fs::read(fixture.root.join("images").join(file_name)).unwrap(),
            contents
        );
        assert!(fs::read_to_string(fixture.root.join(result.relative_path))
            .unwrap()
            .contains("../../../images/"));
    }

    #[tokio::test]
    async fn invalid_asset_hash_is_rejected_before_any_write() {
        let fixture = Fixture::new();
        let mut request = card();
        request.assets = vec![FavoriteCardAsset {
            file_name: format!("{}.png", "0".repeat(64)),
            contents: b"not that hash".to_vec(),
        }];
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        assert!(matches!(
            writer.append(request).await,
            Err(FlashcardError::InvalidRequest(_))
        ));
        assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 0);
    }

    #[test]
    fn mount_observation_requires_exact_root_share_type_and_write_access() {
        let valid = "42 1 0:55 / /mnt/hyz-cards rw,relatime - cifs //server/hyz-cards rw,vers=3.0";
        assert!(parse_mount_identity(valid, Path::new(ROOT)).is_ok());
        for invalid in [
            valid.replace(" rw,relatime", " ro,relatime"),
            valid.replace(" rw,vers", " ro,vers"),
            valid.replace("- cifs", "- ext4"),
            valid.replace("//server/hyz-cards", "//server/other"),
            valid.replace("/mnt/hyz-cards", "/mnt/hyz-cards-other"),
            String::new(),
        ] {
            assert!(
                parse_mount_identity(&invalid, Path::new(ROOT)).is_err(),
                "{invalid}"
            );
        }
    }

    struct HookMount {
        calls: std::sync::atomic::AtomicUsize,
        trigger: usize,
        action: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }
    impl FavoriteMountObservationPort for HookMount {
        fn observe(&self, _: &Path) -> Result<FavoriteMountIdentity, FlashcardError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call == self.trigger {
                if let Some(action) = self.action.lock().unwrap().take() {
                    action();
                }
            }
            Ok(FavoriteMountIdentity {
                mount_id: "test".into(),
                source: "//test/hyz-cards".into(),
            })
        }
    }
    fn hook_writer(
        fixture: &Fixture,
        trigger: usize,
        action: impl FnOnce() + Send + 'static,
    ) -> Arc<dyn FavoriteFlashcardWriterPort> {
        Arc::new(FilesystemFavoriteFlashcardWriter::with_observer(
            fixture.root.clone(),
            Arc::new(HookMount {
                calls: std::sync::atomic::AtomicUsize::new(0),
                trigger,
                action: std::sync::Mutex::new(Some(Box::new(action))),
            }),
        ))
    }

    #[tokio::test]
    async fn same_bytes_external_replacement_is_not_overwritten() {
        let fixture = Fixture::new();
        let target = resolve_target(&card().target_deck).unwrap();
        let original = existing_card("0194f4e0-1234-7000-8000-000000000099", None);
        put(&fixture.root, &target.relative_path, &original);
        let path = fixture.root.join(target.relative_path);
        let edit_path = path.clone();
        let edit_bytes = original.clone();
        let writer = hook_writer(&fixture, 5, move || {
            let replacement = edit_path.with_extension("replacement");
            fs::write(&replacement, edit_bytes).unwrap();
            fs::rename(replacement, edit_path).unwrap();
        });
        assert!(matches!(
            writer.append(card()).await,
            Err(FlashcardError::SourceUnavailable(_))
        ));
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[tokio::test]
    async fn existing_trailing_separator_does_not_change_previous_back() {
        let fixture = Fixture::new();
        let target = resolve_target(&card().target_deck).unwrap();
        let original = "## Card\n### Front\nOld front\n### Back\nOld back\n\n---\n\n";
        put(&fixture.root, &target.relative_path, original);
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        let result = writer.append(card()).await.unwrap();
        let text = fs::read_to_string(fixture.root.join(result.relative_path)).unwrap();
        assert!(text.starts_with(original));
        assert!(text.ends_with("---\n"));
        assert_eq!(text.matches("\n---\n").count(), 2);
        let parsed = parse_markdown_text(&text, &target.relative_path).unwrap();
        assert_eq!(parsed.cards[0].back_markdown, "Old back");
    }

    #[tokio::test]
    async fn source_question_uuid_is_deduplicated_independently_of_letter_case() {
        let fixture = Fixture::new();
        put(
            &fixture.root,
            "moved/cards.md",
            &existing_card(&card().question_id.to_uppercase(), None),
        );
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        let result = writer.append(card()).await.unwrap();
        assert!(!result.appended);
        assert_eq!(result.relative_path, "moved/cards.md");
        assert!(!fixture.root.join("数量").exists());
    }

    #[tokio::test]
    async fn assets_accept_the_question_bank_image_allowlist() {
        let fixture = Fixture::new();
        let contents = b"bitmap bytes".to_vec();
        let file_name = format!("{}.bmp", hash(&contents));
        let mut request = card();
        request.assets = vec![FavoriteCardAsset {
            file_name: file_name.clone(),
            contents: contents.clone(),
        }];
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        assert!(writer.append(request).await.unwrap().appended);
        assert_eq!(
            fs::read(fixture.root.join("images").join(file_name)).unwrap(),
            contents
        );
    }

    #[tokio::test]
    async fn replaced_root_after_staging_never_receives_a_local_fallback_write() {
        let fixture = Fixture::new();
        let holding = Fixture::new();
        let root = fixture.root.clone();
        let detached = holding.root.join("detached");
        let old_root = detached.clone();
        let writer = hook_writer(&fixture, 4, move || {
            fs::rename(&root, detached).unwrap();
            fs::create_dir(&root).unwrap();
        });
        assert!(matches!(
            writer.append(card()).await,
            Err(FlashcardError::SourceUnavailable(_))
        ));
        assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 0);
        let target = resolve_target(&card().target_deck).unwrap();
        let old_parent = old_root.join(Path::new(&target.relative_path).parent().unwrap());
        assert_eq!(fs::read_dir(old_parent).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn externally_edited_target_is_preserved_and_staging_is_removed() {
        let fixture = Fixture::new();
        let target = resolve_target(&card().target_deck).unwrap();
        let original = existing_card("0194f4e0-1234-7000-8000-000000000099", None);
        put(&fixture.root, &target.relative_path, &original);
        let path = fixture.root.join(target.relative_path);
        let changed_path = path.clone();
        let changed = original.replace("Original answer", "External answer");
        let changed_bytes = changed.clone();
        let writer = hook_writer(&fixture, 4, move || {
            fs::write(changed_path, changed_bytes).unwrap();
        });
        assert!(matches!(
            writer.append(card()).await,
            Err(FlashcardError::SourceUnavailable(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), changed);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn source_duplicate_appearing_before_commit_prevents_append() {
        let fixture = Fixture::new();
        let root = fixture.root.clone();
        let existing = existing_card(&card().question_id, None);
        let writer = hook_writer(&fixture, 4, move || {
            put(&root, "moved/cards.md", &existing);
        });
        assert!(matches!(
            writer.append(card()).await,
            Err(FlashcardError::SourceUnavailable(_))
        ));
        let target = resolve_target(&card().target_deck).unwrap();
        assert!(!fixture.root.join(target.relative_path).exists());
        assert!(fixture.root.join("moved/cards.md").exists());
    }

    #[tokio::test]
    async fn conflicting_asset_is_never_overwritten() {
        let fixture = Fixture::new();
        let contents = b"expected image".to_vec();
        let file_name = format!("{}.png", hash(&contents));
        let image = fixture.root.join("images").join(&file_name);
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        fs::write(&image, b"foreign image").unwrap();
        let mut request = card();
        request.assets = vec![FavoriteCardAsset {
            file_name,
            contents,
        }];
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        assert!(matches!(
            writer.append(request).await,
            Err(FlashcardError::SourceUnavailable(_))
        ));
        assert_eq!(fs::read(image).unwrap(), b"foreign image");
        let target = resolve_target(&card().target_deck).unwrap();
        assert!(!fixture.root.join(target.relative_path).exists());
    }

    #[tokio::test]
    async fn append_initializes_tags_and_identity_without_card_uuid() {
        let fixture = Fixture::new();
        let writer: Arc<dyn FavoriteFlashcardWriterPort> = Arc::new(fixture.writer());
        let result = writer.append(card()).await.unwrap();
        assert!(result.appended);
        assert!(result.card_id.is_none());
        let text = fs::read_to_string(fixture.root.join(result.relative_path)).unwrap();
        assert!(text.starts_with("---\ntags:"));
        assert!(
            text.contains("<!-- hyz-source-question-id: 0194f4e0-1234-7000-8000-000000000001 -->")
        );
        assert!(text.contains("<!-- hyz-correct-answer: [\"B\"] -->\n### Front"));
        assert!(text.contains("  - 错题\n"));
        assert!(text.contains("### Front\n\nQuestion\n\n### Back\n\n-\n\n---\n"));
        assert!(text.ends_with("---\n"));
        assert!(!text.contains("hyz-card-id:"));
    }
}
