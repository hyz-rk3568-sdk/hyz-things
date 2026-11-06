//! Fixed, mount-checked question-bank source. No process/network execution.
use crate::application::question_bank::images::{asset_type, rewrite_assets};
use crate::application::question_bank::source_hash;
use crate::application::question_bank::{
    document::{self, Document},
    prepare_sync, BankAsset, BankError, BankSnapshot, IndexedPaper, QuestionBankSourcePort,
    SourceFile,
};
use async_trait::async_trait;
use std::os::{
    fd::{AsRawFd, FromRawFd},
    unix::{ffi::OsStrExt, fs::OpenOptionsExt},
};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};

const ROOT: &str = "/mnt/hyz-question-bank";
const MAX_MD: u64 = 8 * 1024 * 1024;
const MAX_ASSET: u64 = 16 * 1024 * 1024;
#[derive(Clone)]
pub struct FilesystemQuestionBankSource {
    root: PathBuf,
}
impl Default for FilesystemQuestionBankSource {
    fn default() -> Self {
        Self { root: ROOT.into() }
    }
}
impl FilesystemQuestionBankSource {
    fn mounted(&self) -> Result<(), BankError> {
        let mounts = fs::read_to_string("/proc/mounts").map_err(|_| BankError::Unavailable)?;
        let root = self.root.to_str().ok_or(BankError::Unavailable)?;
        if !mounts.lines().any(|line| {
            let v: Vec<_> = line.split_whitespace().collect();
            v.len() >= 4
                && v[1] == root
                && v[2] == "cifs"
                && v[0].ends_with("/hyz-question-bank")
                && v[3].split(',').any(|option| option == "rw")
        }) {
            return Err(BankError::Unavailable);
        }
        Ok(())
    }
    fn directory(&self, relative: &Path) -> Result<fs::File, BankError> {
        let mut directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&self.root)
            .map_err(|_| BankError::Unavailable)?;
        for component in relative.components() {
            let Component::Normal(part) = component else {
                return Err(BankError::Invalid);
            };
            directory = open_child(&directory, part, libc::O_RDONLY | libc::O_DIRECTORY)?;
        }
        Ok(directory)
    }
    fn file(&self, relative: &str, limit: u64) -> Result<Vec<u8>, BankError> {
        let path = Path::new(relative);
        if path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(BankError::Invalid);
        }
        let parent = self.directory(path.parent().ok_or(BankError::Invalid)?)?;
        let file = open_child(
            &parent,
            path.file_name().ok_or(BankError::Invalid)?,
            libc::O_RDONLY | libc::O_NONBLOCK,
        )?;
        let metadata = file.metadata().map_err(|_| BankError::Unavailable)?;
        if !metadata.is_file() || metadata.len() > limit {
            return Err(BankError::Invalid);
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| BankError::Unavailable)?;
        if bytes.len() as u64 > limit {
            return Err(BankError::Invalid);
        }
        Ok(bytes)
    }
    fn asset_blocking(&self, name: &str) -> Result<BankAsset, BankError> {
        let content_type = asset_type(name)?;
        let contents = self.file(&format!("assets/{name}"), MAX_ASSET)?;
        if source_hash(&contents) != name.split('.').next().unwrap_or("") {
            return Err(BankError::Invalid);
        }
        Ok(BankAsset {
            contents,
            content_type,
        })
    }
    fn backup(&self, text: &str) -> Result<(), BankError> {
        let root = self.directory(Path::new(""))?;
        let name = std::ffi::CString::new(".hyz-sync-backups").map_err(|_| BankError::Invalid)?;
        if unsafe { libc::mkdirat(root.as_raw_fd(), name.as_ptr(), 0o700) } != 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(BankError::Unavailable);
        }
        let parent = self.directory(Path::new(".hyz-sync-backups"))?;
        let leaf = format!("{}.md", source_hash(text.as_bytes()));
        if let Ok(bytes) = self.file(&format!(".hyz-sync-backups/{leaf}"), MAX_MD) {
            return if bytes == text.as_bytes() {
                Ok(())
            } else {
                Err(BankError::Conflict)
            };
        }
        let mut file = open_child(
            &parent,
            std::ffi::OsStr::new(&leaf),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        file.write_all(text.as_bytes())
            .map_err(|_| BankError::Unavailable)?;
        file.sync_all().map_err(|_| BankError::Unavailable)
    }
    fn scan_blocking(&self, known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        self.mounted()?;
        let mut queue = VecDeque::from([self.root.join("papers")]);
        let collections = self.root.join("collections");
        if collections.is_dir() {
            queue.push_back(collections);
        }
        let mut files = Vec::new();
        let mut total_bytes = 0usize;
        let mut directories = 0usize;
        let mut assets = std::collections::BTreeSet::new();
        while let Some(directory) = queue.pop_front() {
            directories += 1;
            if directories > 20000 {
                return Err(BankError::Invalid);
            }
            if fs::symlink_metadata(&directory)
                .map_err(|_| BankError::Unavailable)?
                .file_type()
                .is_symlink()
            {
                return Err(BankError::Invalid);
            }
            for entry in fs::read_dir(directory).map_err(|_| BankError::Unavailable)? {
                let entry = entry.map_err(|_| BankError::Unavailable)?;
                let kind = entry.file_type().map_err(|_| BankError::Unavailable)?;
                if kind.is_symlink() {
                    return Err(BankError::Invalid);
                }
                if kind.is_dir() {
                    queue.push_back(entry.path());
                    continue;
                }
                if !kind.is_file()
                    || entry.path().extension().and_then(|e| e.to_str()) != Some("md")
                {
                    continue;
                }
                if files.len() >= 10000 {
                    return Err(BankError::Invalid);
                }
                let path = entry
                    .path()
                    .strip_prefix(&self.root)
                    .map_err(|_| BankError::Invalid)?
                    .to_str()
                    .ok_or(BankError::Invalid)?
                    .to_string();
                let text =
                    String::from_utf8(self.file(&path, MAX_MD)?).map_err(|_| BankError::Invalid)?;
                total_bytes += text.len();
                if total_bytes > 256 * 1024 * 1024 {
                    return Err(BankError::Invalid);
                }
                let (_, references) = rewrite_assets(&text, &path)?;
                assets.extend(references);
                files.push(SourceFile { path, text });
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        // Complete validation and image closure before any UUID write-back.
        for name in assets {
            self.asset_blocking(&name)?;
        }
        let plan = prepare_sync(files, known, || uuid::Uuid::now_v7().to_string())?;
        if plan
            .writes
            .iter()
            .any(|write| write.after.len() as u64 > MAX_MD)
        {
            return Err(BankError::Invalid);
        }
        for write in &plan.writes {
            self.mounted()?;
            if self.file(&write.path, MAX_MD)? != write.before.as_bytes() {
                return Err(BankError::Conflict);
            }
            self.backup(&write.before)?;
            let path = Path::new(&write.path);
            let parent = self.directory(path.parent().ok_or(BankError::Invalid)?)?;
            let name =
                std::ffi::CString::new(path.file_name().ok_or(BankError::Invalid)?.as_bytes())
                    .map_err(|_| BankError::Invalid)?;
            let temporary = format!(".hyz-bank-{}.tmp", uuid::Uuid::now_v7());
            let temp_name =
                std::ffi::CString::new(temporary.as_str()).map_err(|_| BankError::Invalid)?;
            let result = (|| {
                let mut file = open_child(
                    &parent,
                    std::ffi::OsStr::new(&temporary),
                    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                )?;
                file.write_all(write.after.as_bytes())
                    .map_err(|_| BankError::Unavailable)?;
                file.sync_all().map_err(|_| BankError::Unavailable)?;
                if self.file(&write.path, MAX_MD)? != write.before.as_bytes() {
                    return Err(BankError::Conflict);
                }
                // Both names are relative to one held, symlink-free directory descriptor.
                if unsafe {
                    libc::renameat(
                        parent.as_raw_fd(),
                        temp_name.as_ptr(),
                        parent.as_raw_fd(),
                        name.as_ptr(),
                    )
                } != 0
                {
                    return Err(BankError::Unavailable);
                }
                Ok(())
            })();
            if result.is_err() {
                unsafe {
                    libc::unlinkat(parent.as_raw_fd(), temp_name.as_ptr(), 0);
                }
            }
            result?;
        }
        // A partial CIFS batch is never committed to the index; retry reuses written IDs.
        for paper in &plan.snapshot.papers {
            let raw = self.file(&paper.path, MAX_MD)?;
            if source_hash(&raw) != paper.hash {
                return Err(BankError::Conflict);
            }
        }
        self.mounted()?;
        Ok(plan.snapshot)
    }
}

fn open_child(
    parent: &fs::File,
    name: &std::ffi::OsStr,
    flags: i32,
) -> Result<fs::File, BankError> {
    let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| BankError::Invalid)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(BankError::Unavailable);
    }
    Ok(unsafe { fs::File::from_raw_fd(fd) })
}

#[async_trait]
impl QuestionBankSourcePort for FilesystemQuestionBankSource {
    async fn scan(&self, known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        let source = self.clone();
        tokio::task::spawn_blocking(move || source.scan_blocking(known))
            .await
            .map_err(|_| BankError::Unavailable)?
    }
    async fn load(&self, paper: IndexedPaper) -> Result<Document, BankError> {
        let source = self.clone();
        tokio::task::spawn_blocking(move || {
            source.mounted()?;
            let raw = source.file(&paper.path, MAX_MD)?;
            if source_hash(&raw) != paper.hash {
                return Err(BankError::Stale);
            }
            let text = String::from_utf8(raw).map_err(|_| BankError::Invalid)?;
            let (text, _) = rewrite_assets(&text, &paper.path)?;
            document::parse(&text).map_err(|_| BankError::Invalid)
        })
        .await
        .map_err(|_| BankError::Unavailable)?
    }
    async fn asset(&self, name: String) -> Result<BankAsset, BankError> {
        let source = self.clone();
        tokio::task::spawn_blocking(move || {
            source.mounted()?;
            source.asset_blocking(&name)
        })
        .await
        .map_err(|_| BankError::Unavailable)?
    }
}
