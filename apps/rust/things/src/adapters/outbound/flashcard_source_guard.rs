use std::{
    ffi::{CString, OsStr, OsString},
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
};

pub(super) fn mount_identity(root: &Path) -> io::Result<String> {
    let unavailable = || io::Error::other("expected writable hyz-cards CIFS mount");
    let mounts = fs::read_to_string("/proc/self/mountinfo").map_err(|_| unavailable())?;
    let mut identity = None;
    for line in mounts.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let fields = left.split_whitespace().collect::<Vec<_>>();
        let filesystem = right.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 6 || filesystem.len() < 3 {
            continue;
        }
        let mount_path = fields[4]
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\");
        if Path::new(&mount_path) != root {
            continue;
        }
        if identity.is_some()
            || filesystem[0] != "cifs"
            || !filesystem[1].ends_with("/hyz-cards")
            || !fields[5].split(',').any(|value| value == "rw")
            || !filesystem[2].split(',').any(|value| value == "rw")
        {
            return Err(unavailable());
        }
        identity = Some(format!(
            "{}:{}:{}:{}",
            fields[0], fields[2], fields[3], filesystem[1]
        ));
    }
    identity.ok_or_else(unavailable)
}

type MountObserver = dyn Fn(&Path, &File) -> io::Result<String>;

pub(super) struct SourceGuard<'a> {
    root_path: PathBuf,
    root: File,
    mount_id: String,
    expected_mount: Option<String>,
    observe: &'a MountObserver,
}

impl SourceGuard<'static> {
    pub(super) fn open(root: &Path, checked: bool) -> io::Result<Self> {
        Self::with_observer(root, checked, &observe_mount)
    }
}

impl<'a> SourceGuard<'a> {
    pub(super) fn with_observer(
        root: &Path,
        checked: bool,
        observe: &'a MountObserver,
    ) -> io::Result<Self> {
        let root_path = if root.is_absolute() {
            root.to_owned()
        } else {
            std::env::current_dir()?.join(root)
        };
        let root = open_root(&root_path)?;
        let expected_mount = checked.then(|| observe(&root_path, &root)).transpose()?;
        let guard = Self {
            mount_id: descriptor_mount_id(&root)?,
            root_path,
            root,
            expected_mount,
            observe,
        };
        guard.verify()?;
        Ok(guard)
    }

    pub(super) fn verify(&self) -> io::Result<()> {
        if let Some(expected) = &self.expected_mount {
            if (self.observe)(&self.root_path, &self.root)? != *expected {
                return Err(conflict("source mount identity changed"));
            }
        }
        let current = open_root(&self.root_path)?;
        if identity(&current)? != identity(&self.root)?
            || descriptor_mount_id(&current)? != self.mount_id
        {
            return Err(conflict("source root changed"));
        }
        Ok(())
    }

    pub(super) fn directory(&self, relative: &Path) -> io::Result<File> {
        let mut directory = self.root.try_clone()?;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(conflict("unsafe relative directory"));
            };
            directory = open_child(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY)?;
            if descriptor_mount_id(&directory)? != self.mount_id {
                return Err(conflict("nested source mount is not allowed"));
            }
        }
        Ok(directory)
    }

    pub(super) fn read_source(&self, relative: &Path, limit: u64) -> io::Result<SourceFile> {
        self.verify()?;
        let (parent, name) = self.parent(relative)?;
        read_at(&parent, name, limit, &self.mount_id)
    }

    fn parent<'p>(&self, relative: &'p Path) -> io::Result<(File, &'p OsStr)> {
        let parent = relative
            .parent()
            .ok_or_else(|| conflict("missing source parent"))?;
        let name = relative
            .file_name()
            .ok_or_else(|| conflict("missing source filename"))?;
        Ok((self.directory(parent)?, name))
    }

    pub(super) fn replace(
        &self,
        relative: &Path,
        original: &SourceFile,
        contents: &[u8],
    ) -> io::Result<()> {
        self.verify()?;
        let (parent, name) = self.parent(relative)?;
        let limit = original.bytes.len() as u64;
        if read_at(&parent, name, limit, &self.mount_id)? != *original {
            return Err(conflict("source changed after parsing"));
        }
        let mut temporary = TemporaryFile::write(&parent, "tmp", contents)?;
        let mut backup = TemporaryFile::write(&parent, "bak", &original.bytes)?;
        parent.sync_all()?;
        self.verify()?;
        let (current_parent, _) = self.parent(relative)?;
        if identity(&current_parent)? != identity(&parent)?
            || read_at(&current_parent, name, limit, &self.mount_id)? != *original
        {
            return Err(conflict("source changed before UUID commit"));
        }
        let from = cstring(&temporary.name)?;
        let to = cstring(name)?;
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                from.as_ptr(),
                parent.as_raw_fd(),
                to.as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        temporary.keep = true;
        backup.keep = true;
        parent.sync_all()?;
        self.verify()
    }
}

fn observe_mount(root: &Path, descriptor: &File) -> io::Result<String> {
    let identity = mount_identity(root)?;
    if identity.split(':').next() != Some(descriptor_mount_id(descriptor)?.as_str()) {
        return Err(conflict("source descriptor is not the observed CIFS mount"));
    }
    Ok(identity)
}

fn descriptor_mount_id(file: &File) -> io::Result<String> {
    let info = fs::read_to_string(format!("/proc/self/fdinfo/{}", file.as_raw_fd()))?;
    info.lines()
        .find_map(|line| line.strip_prefix("mnt_id:").map(|id| id.trim().to_owned()))
        .ok_or_else(|| conflict("source descriptor mount identity is unavailable"))
}

fn open_root(path: &Path) -> io::Result<File> {
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = open_child(&directory, name, libc::O_RDONLY | libc::O_DIRECTORY)?
            }
            _ => return Err(conflict("unsafe source root")),
        }
    }
    Ok(directory)
}

fn cstring(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| conflict("NUL in source path"))
}

fn open_child(parent: &File, name: &OsStr, flags: i32) -> io::Result<File> {
    let name = cstring(name)?;
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

fn identity(file: &File) -> io::Result<(u64, u64)> {
    let metadata = file.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceVersion {
    device: u64,
    inode: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    length: u64,
}
impl From<&fs::Metadata> for SourceVersion {
    fn from(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            length: metadata.len(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SourceFile {
    pub(super) bytes: Vec<u8>,
    version: SourceVersion,
    parent_identity: (u64, u64),
}

fn read_at(parent: &File, name: &OsStr, limit: u64, mount: &str) -> io::Result<SourceFile> {
    let file = open_child(parent, name, libc::O_RDONLY)?;
    if descriptor_mount_id(&file)? != mount {
        return Err(conflict("foreign source file mount"));
    }
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(conflict("source is not a regular file"));
    }
    if metadata.len() > limit {
        return Err(io::Error::from_raw_os_error(libc::EFBIG));
    }
    let version = SourceVersion::from(&metadata);
    let mut bytes = Vec::new();
    (&file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::from_raw_os_error(libc::EFBIG));
    }
    if SourceVersion::from(&file.metadata()?) != version {
        return Err(conflict("source changed while being read"));
    }
    Ok(SourceFile {
        bytes,
        version,
        parent_identity: identity(parent)?,
    })
}

struct TemporaryFile<'a> {
    directory: &'a File,
    name: OsString,
    keep: bool,
}
impl<'a> TemporaryFile<'a> {
    fn write(directory: &'a File, extension: &str, contents: &[u8]) -> io::Result<Self> {
        let name: OsString = format!(".hyz-card-{}.{}", uuid::Uuid::now_v7(), extension).into();
        let mut file = open_child(
            directory,
            &name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )?;
        let temporary = Self {
            directory,
            name,
            keep: false,
        };
        file.write_all(contents)?;
        file.sync_all()?;
        Ok(temporary)
    }
}
impl Drop for TemporaryFile<'_> {
    fn drop(&mut self) {
        if !self.keep {
            if let Ok(name) = cstring(&self.name) {
                unsafe {
                    libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0);
                }
            }
        }
    }
}

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_uuid_replace_rejects_visible_external_changes() {
        for replacement in [false, true] {
            let root =
                std::env::temp_dir().join(format!("hyz-source-conflict-{}", uuid::Uuid::now_v7()));
            fs::create_dir(&root).unwrap();
            let path = root.join("cards.md");
            fs::write(&path, "original").unwrap();
            let guard = SourceGuard::open(&root, false).unwrap();
            let original = guard.read_source(Path::new("cards.md"), 1024).unwrap();
            if replacement {
                fs::write(root.join("replacement"), "original").unwrap();
                fs::rename(root.join("replacement"), &path).unwrap();
            } else {
                fs::write(&path, "external edit").unwrap();
            }
            assert!(guard
                .replace(Path::new("cards.md"), &original, b"generated UUID")
                .is_err());
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                if replacement {
                    "original"
                } else {
                    "external edit"
                }
            );
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
            fs::remove_dir_all(root).unwrap();
        }
    }
}
