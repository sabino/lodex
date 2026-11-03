//! Small synchronous local profile store. No network client or serialization API.
//! Async callers must run disk operations on their blocking executor.
//! Private directories and symlink checks prevent accidental traversal; this is
//! not a boundary against another privileged process modifying the same files.

use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const MAX_SOUL_BYTES: usize = 8192;
pub const MAX_MEMORY_BYTES: usize = 32768;
static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub enum StoreError {
    Io(io::Error),
    InvalidName,
    InvalidLayout,
    TooLarge,
    InvalidUtf8,
    Conflict,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "local profile I/O failed: {error}"),
            Self::InvalidName => f.write_str("invalid profile name"),
            Self::InvalidLayout => {
                f.write_str("profile path must be a private directory with regular files")
            }
            Self::TooLarge => f.write_str("local profile file exceeds its size limit"),
            Self::InvalidUtf8 => f.write_str("local profile file is not UTF-8"),
            Self::Conflict => f.write_str("memory changed since it was read"),
        }
    }
}

impl std::error::Error for StoreError {}
impl From<io::Error> for StoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MemoryRevision([u8; 32]);
impl fmt::Debug for MemoryRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<local-memory-revision>")
    }
}

pub struct ProfileSnapshot {
    pub soul: String,
    pub memory: String,
    pub memory_revision: MemoryRevision,
}

impl fmt::Debug for ProfileSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProfileSnapshot")
            .field("soul_bytes", &self.soul.len())
            .field("memory_bytes", &self.memory.len())
            .finish()
    }
}

pub struct LocalProfiles {
    root: PathBuf,
}

fn private_directory(path: &Path, create: bool) -> Result<(), StoreError> {
    if create && !path.exists() {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Err(StoreError::InvalidLayout);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(StoreError::InvalidLayout);
        }
    }
    Ok(())
}

fn regular_file_or_missing(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(StoreError::InvalidLayout),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn open_private(path: &Path, create_new: bool) -> Result<File, StoreError> {
    regular_file_or_missing(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn read_bounded(path: &Path, max: usize) -> Result<String, StoreError> {
    regular_file_or_missing(path)?;
    let mut bytes = Vec::new();
    File::open(path)?
        .take((max + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(StoreError::TooLarge);
    }
    String::from_utf8(bytes).map_err(|_| StoreError::InvalidUtf8)
}

fn revision(memory: &str) -> MemoryRevision {
    MemoryRevision(Sha256::digest(memory.as_bytes()).into())
}

struct TempGuard(PathBuf);
impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn atomic_replace_with(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), StoreError> {
    regular_file_or_missing(path)?;
    let parent = path.parent().ok_or(StoreError::InvalidLayout)?;
    let temp = parent.join(format!(
        ".write-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = open_private(&temp, true)?;
    let guard = TempGuard(temp);
    write(&mut file)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&guard.0, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

impl LocalProfiles {
    /// Existing roots must already be private. Never chmod an existing directory.
    /// The parent must exist; profiles belong in a dedicated external data root.
    pub fn open(root: PathBuf) -> Result<Self, StoreError> {
        if !root.is_absolute() {
            return Err(StoreError::InvalidLayout);
        }
        private_directory(&root, true)?;
        Ok(Self { root })
    }

    fn profile_path(&self, name: &str) -> Result<PathBuf, StoreError> {
        if name.is_empty()
            || name.len() > 64
            || !name.as_bytes()[0].is_ascii_alphanumeric()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(StoreError::InvalidName);
        }
        Ok(self.root.join(name))
    }

    pub fn init(&self, name: &str) -> Result<(), StoreError> {
        let path = self.profile_path(name)?;
        if path.exists() {
            return Err(StoreError::InvalidLayout);
        }
        private_directory(&path, true)?;
        let mut soul = open_private(&path.join("SOUL.md"), true)?;
        soul.write_all(b"# Personality\n\nDescribe this agent here.\n")?;
        soul.sync_all()?;
        let mut memory = open_private(&path.join("MEMORY.md"), true)?;
        memory.write_all(b"# Memory\n")?;
        memory.sync_all()?;
        File::open(&path)?.sync_all()?;
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }

    fn locked_profile(&self, name: &str) -> Result<(PathBuf, File), StoreError> {
        private_directory(&self.root, false)?;
        let path = self.profile_path(name)?;
        private_directory(&path, false)?;
        let lock = open_private(&path.join(".memory.lock"), false)?;
        lock.lock()?;
        Ok((path, lock))
    }

    /// Local-only read. The snapshot deliberately has no Serialize implementation.
    pub fn read(&self, name: &str) -> Result<ProfileSnapshot, StoreError> {
        let (path, _lock) = self.locked_profile(name)?;
        let soul = read_bounded(&path.join("SOUL.md"), MAX_SOUL_BYTES)?;
        let memory = read_bounded(&path.join("MEMORY.md"), MAX_MEMORY_BYTES)?;
        Ok(ProfileSnapshot {
            memory_revision: revision(&memory),
            soul,
            memory,
        })
    }

    /// Locked compare-and-replace; retain one previous version before committing.
    /// On an I/O error after rename, read again to determine the committed version.
    pub fn write_memory(
        &self,
        name: &str,
        expected: MemoryRevision,
        memory: &str,
    ) -> Result<MemoryRevision, StoreError> {
        if memory.len() > MAX_MEMORY_BYTES {
            return Err(StoreError::TooLarge);
        }
        let (path, _lock) = self.locked_profile(name)?;
        let old = read_bounded(&path.join("MEMORY.md"), MAX_MEMORY_BYTES)?;
        if revision(&old) != expected {
            return Err(StoreError::Conflict);
        }
        atomic_replace_with(&path.join("MEMORY.previous.md"), |file| {
            file.write_all(old.as_bytes())
        })?;
        atomic_replace_with(&path.join("MEMORY.md"), |file| {
            file.write_all(memory.as_bytes())
        })?;
        Ok(revision(memory))
    }

    pub fn read_previous_memory(&self, name: &str) -> Result<String, StoreError> {
        let (path, _lock) = self.locked_profile(name)?;
        read_bounded(&path.join("MEMORY.previous.md"), MAX_MEMORY_BYTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, LocalProfiles) {
        let temp = tempfile::tempdir().expect("temp root");
        let store = LocalProfiles::open(temp.path().join("profiles")).expect("private root");
        store.init("main").expect("initialize");
        (temp, store)
    }

    #[test]
    fn update_preserves_previous_and_rejects_stale_writers() {
        let (_temp, store) = store();
        let before = store.read("main").expect("read");
        let updated = store
            .write_memory("main", before.memory_revision, "new local memory")
            .expect("update");
        assert_ne!(before.memory_revision, updated);
        assert_eq!(
            store.read_previous_memory("main").expect("backup"),
            before.memory
        );
        assert!(matches!(
            store.write_memory("main", before.memory_revision, "stale"),
            Err(StoreError::Conflict)
        ));
        assert_eq!(store.read("main").expect("read").memory, "new local memory");
    }

    #[test]
    fn traversal_and_existing_profiles_are_rejected() {
        let (_temp, store) = store();
        for name in ["", "..", "../outside", "/absolute", "a/b", ".hidden"] {
            assert!(matches!(store.init(name), Err(StoreError::InvalidName)));
        }
        assert!(store.init("main").is_err());
    }

    #[test]
    fn bounds_and_debug_redaction_hold() {
        let (_temp, store) = store();
        let before = store.read("main").expect("read");
        store
            .write_memory("main", before.memory_revision, "private-canary")
            .expect("write");
        assert!(!format!("{:?}", store.read("main").expect("read")).contains("private-canary"));
        assert!(matches!(
            store.write_memory(
                "main",
                before.memory_revision,
                &"a".repeat(MAX_MEMORY_BYTES + 1)
            ),
            Err(StoreError::TooLarge)
        ));
        fs::write(
            store.root.join("main/SOUL.md"),
            vec![b'a'; MAX_SOUL_BYTES + 1],
        )
        .expect("oversized fixture");
        assert!(matches!(store.read("main"), Err(StoreError::TooLarge)));
    }

    #[test]
    fn interrupted_temp_write_preserves_committed_data() {
        let (_temp, store) = store();
        let target = store.root.join("main/MEMORY.md");
        let before = fs::read(&target).expect("before");
        let result = atomic_replace_with(&target, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected write interruption"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&target).expect("after"), before);
        assert!(
            !fs::read_dir(target.parent().expect("parent"))
                .expect("directory")
                .any(|entry| entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".write-"))
        );
    }

    #[test]
    fn concurrent_writers_do_not_lose_an_update_silently() {
        let (_temp, store) = store();
        let before = store.read("main").expect("read");
        let store = std::sync::Arc::new(store);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let writers = ["writer-one", "writer-two"].map(|text| {
            let store = std::sync::Arc::clone(&store);
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.write_memory("main", before.memory_revision, text)
            })
        });
        barrier.wait();
        let results: Vec<_> = writers
            .into_iter()
            .map(|writer| writer.join().expect("writer"))
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(StoreError::Conflict)))
                .count(),
            1
        );
        assert_eq!(
            store.read_previous_memory("main").expect("backup"),
            before.memory
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_files_and_profile_directories_are_rejected() {
        use std::os::unix::fs::symlink;
        let (temp, store) = store();
        let memory = store.root.join("main/MEMORY.md");
        fs::remove_file(&memory).expect("remove fixture");
        symlink(temp.path().join("outside"), &memory).expect("symlink");
        assert!(matches!(store.read("main"), Err(StoreError::InvalidLayout)));
        symlink(temp.path(), store.root.join("alias")).expect("directory link");
        assert!(matches!(
            store.read("alias"),
            Err(StoreError::InvalidLayout)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn existing_public_directory_is_not_silently_chmodded() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("public");
        fs::create_dir(&root).expect("directory");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).expect("permissions");
        assert!(matches!(
            LocalProfiles::open(root.clone()),
            Err(StoreError::InvalidLayout)
        ));
        assert_eq!(
            fs::metadata(root).expect("metadata").permissions().mode() & 0o777,
            0o755
        );
    }
}
