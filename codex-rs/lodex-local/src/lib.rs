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
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "cli")]
pub mod cli;

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
    InvalidRevision,
    InvalidWriter,
    InvalidRecord,
    RecoveryConflict,
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
            Self::InvalidRevision => f.write_str("expected a 64-character SHA-256 revision"),
            Self::InvalidWriter => f.write_str("writer must contain 1..64 ASCII letters, digits, dots, colons, underscores or dashes"),
            Self::InvalidRecord => f.write_str("invalid local memory transaction record"),
            Self::RecoveryConflict => f.write_str("memory transaction conflicts with local files; preserve them for inspection"),
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
impl MemoryRevision {
    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
            output.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
        }
        output
    }

    pub fn from_hex(value: &str) -> Result<Self, StoreError> {
        if value.len() != 64 {
            return Err(StoreError::InvalidRevision);
        }
        let mut bytes = [0; 32];
        for (output, pair) in bytes.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
            let digit = |value: u8| match value {
                b'0'..=b'9' => Ok(value - b'0'),
                b'a'..=b'f' => Ok(value - b'a' + 10),
                _ => Err(StoreError::InvalidRevision),
            };
            *output = digit(pair[0])? * 16 + digit(pair[1])?;
        }
        Ok(Self(bytes))
    }
}
impl fmt::Debug for MemoryRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<local-memory-revision>")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct WriterId(String);

impl WriterId {
    pub fn new(value: &str) -> Result<Self, StoreError> {
        if value.is_empty()
            || value.len() > 64
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-')
            })
        {
            return Err(StoreError::InvalidWriter);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Caller-supplied attribution, not an authenticated identity or an audit chain.
pub struct MemoryWrite {
    pub previous_revision: MemoryRevision,
    pub revision: MemoryRevision,
    pub writer: WriterId,
    pub unix_millis: u64,
    pub process_id: u32,
}

impl MemoryWrite {
    fn encode(&self) -> String {
        format!(
            "lodex-memory-v1\n{}\n{}\n{}\n{}\n{}\n",
            self.previous_revision.to_hex(),
            self.revision.to_hex(),
            self.writer.as_str(),
            self.unix_millis,
            self.process_id
        )
    }

    fn decode(value: &str) -> Result<Self, StoreError> {
        let lines: Vec<_> = value.lines().collect();
        if lines.len() != 6 || lines[0] != "lodex-memory-v1" {
            return Err(StoreError::InvalidRecord);
        }
        let result = Self {
            previous_revision: MemoryRevision::from_hex(lines[1])?,
            revision: MemoryRevision::from_hex(lines[2])?,
            writer: WriterId::new(lines[3])?,
            unix_millis: lines[4].parse().map_err(|_| StoreError::InvalidRecord)?,
            process_id: lines[5].parse().map_err(|_| StoreError::InvalidRecord)?,
        };
        if result.process_id == 0 || result.previous_revision == result.revision {
            return Err(StoreError::InvalidRecord);
        }
        Ok(result)
    }
}

pub struct ProfileSnapshot {
    pub soul: String,
    pub memory: String,
    pub memory_revision: MemoryRevision,
    pub memory_write: Option<MemoryWrite>,
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

#[cfg(any(test, feature = "fault-injection"))]
fn crash_checkpoint(path: &Path, stage: &str) -> io::Result<()> {
    if std::env::var("LODEX_PROFILE_FAULT_STAGE").as_deref() == Ok(stage) {
        fs::write(path.join(".fault-ready"), stage)?;
        loop {
            std::thread::park();
        }
    }
    Ok(())
}

fn optional_text(path: &Path, max: usize) -> Result<Option<String>, StoreError> {
    match read_bounded(path, max) {
        Ok(text) => Ok(Some(text)),
        Err(StoreError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn remove_if_present(path: &Path) -> Result<(), StoreError> {
    regular_file_or_missing(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn recover_memory(path: &Path) -> Result<(), StoreError> {
    let pending = path.join(".memory.pending");
    let previous = path.join(".memory.previous.pending");
    if let Some(text) = optional_text(&pending, 512)? {
        let record = MemoryWrite::decode(&text)?;
        let current = revision(&read_bounded(&path.join("MEMORY.md"), MAX_MEMORY_BYTES)?);
        if current == record.revision {
            if let Some(old) = optional_text(&previous, MAX_MEMORY_BYTES)? {
                if revision(&old) != record.previous_revision {
                    return Err(StoreError::RecoveryConflict);
                }
                regular_file_or_missing(&path.join("MEMORY.previous.md"))?;
                fs::rename(&previous, path.join("MEMORY.previous.md"))?;
                File::open(path)?.sync_all()?;
                #[cfg(any(test, feature = "fault-injection"))]
                crash_checkpoint(path, "after-backup")?;
            } else if optional_text(&path.join("MEMORY.previous.md"), MAX_MEMORY_BYTES)?
                .as_deref()
                .map(revision)
                != Some(record.previous_revision)
            {
                return Err(StoreError::RecoveryConflict);
            }
            atomic_replace_with(&path.join(".memory.last-write"), |file| {
                file.write_all(text.as_bytes())
            })?;
            #[cfg(any(test, feature = "fault-injection"))]
            crash_checkpoint(path, "after-attribution")?;
        } else if current != record.previous_revision {
            return Err(StoreError::RecoveryConflict);
        }
        // Old revision: preparation was interrupted; leave the last committed
        // backup and attribution intact. New revision: finish its commit above.
        remove_if_present(&previous)?;
        remove_if_present(&pending)?;
        File::open(path)?.sync_all()?;
    } else {
        remove_if_present(&previous)?;
    }
    // The lock excludes active writers. Only our exact reserved temp grammar
    // is disposable after process death; unrelated files are never removed.
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str().and_then(|name| name.strip_prefix(".write-")) else {
            continue;
        };
        let Some((pid, serial)) = name.split_once('-') else {
            continue;
        };
        if !pid.is_empty()
            && !serial.is_empty()
            && pid.bytes().all(|byte| byte.is_ascii_digit())
            && serial.bytes().all(|byte| byte.is_ascii_digit())
        {
            remove_if_present(&entry.path())?;
        }
    }
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
        recover_memory(&path)?;
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
            memory_write: optional_text(&path.join(".memory.last-write"), 512)?
                .map(|text| MemoryWrite::decode(&text))
                .transpose()?,
        })
    }

    pub fn directory(&self, name: &str) -> Result<PathBuf, StoreError> {
        let (path, _lock) = self.locked_profile(name)?;
        Ok(path)
    }

    /// Locked compare-and-replace; retain one previous version before committing.
    /// On an I/O error after rename, read again to determine the committed version.
    pub fn write_memory(
        &self,
        name: &str,
        expected: MemoryRevision,
        memory: &str,
    ) -> Result<MemoryRevision, StoreError> {
        self.write_memory_as(
            name,
            expected,
            memory,
            &WriterId::new(&format!("process:{}", std::process::id()))?,
        )
    }

    /// Explicit caller attribution; no-op writes preserve the previous writer.
    pub fn write_memory_as(
        &self,
        name: &str,
        expected: MemoryRevision,
        memory: &str,
        writer: &WriterId,
    ) -> Result<MemoryRevision, StoreError> {
        if memory.len() > MAX_MEMORY_BYTES {
            return Err(StoreError::TooLarge);
        }
        let (path, _lock) = self.locked_profile(name)?;
        let old = read_bounded(&path.join("MEMORY.md"), MAX_MEMORY_BYTES)?;
        if revision(&old) != expected {
            return Err(StoreError::Conflict);
        }
        if old == memory {
            return Ok(expected);
        }
        let record = MemoryWrite {
            previous_revision: expected,
            revision: revision(memory),
            writer: writer.clone(),
            unix_millis: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| StoreError::InvalidRecord)?
                .as_millis()
                .try_into()
                .map_err(|_| StoreError::InvalidRecord)?,
            process_id: std::process::id(),
        };
        atomic_replace_with(&path.join(".memory.previous.pending"), |file| {
            file.write_all(old.as_bytes())
        })?;
        #[cfg(any(test, feature = "fault-injection"))]
        crash_checkpoint(&path, "after-previous-stage")?;
        atomic_replace_with(&path.join(".memory.pending"), |file| {
            file.write_all(record.encode().as_bytes())
        })?;
        #[cfg(any(test, feature = "fault-injection"))]
        crash_checkpoint(&path, "after-intent")?;
        atomic_replace_with(&path.join("MEMORY.md"), |file| {
            let bytes = memory.as_bytes();
            let middle = bytes.len() / 2;
            file.write_all(&bytes[..middle])?;
            #[cfg(any(test, feature = "fault-injection"))]
            crash_checkpoint(&path, "during-memory-write")?;
            file.write_all(&bytes[middle..])
        })?;
        #[cfg(any(test, feature = "fault-injection"))]
        crash_checkpoint(&path, "after-memory")?;
        recover_memory(&path)?;
        Ok(record.revision)
    }

    /// Last managed commit; a manual editor can subsequently change MEMORY.md.
    pub fn last_memory_write(&self, name: &str) -> Result<Option<MemoryWrite>, StoreError> {
        let (path, _lock) = self.locked_profile(name)?;
        optional_text(&path.join(".memory.last-write"), 512)?
            .map(|text| MemoryWrite::decode(&text))
            .transpose()
    }

    pub fn read_previous_memory(&self, name: &str) -> Result<String, StoreError> {
        let (path, _lock) = self.locked_profile(name)?;
        read_bounded(&path.join("MEMORY.previous.md"), MAX_MEMORY_BYTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribution_and_noop_preserve_last_committed_writer() {
        let (_temp, store) = store();
        let before = store.read("main").expect("read");
        let writer = WriterId::new("operator:fixture").expect("writer");
        let current = store
            .write_memory_as("main", before.memory_revision, "new", &writer)
            .expect("write");
        let record = store
            .last_memory_write("main")
            .expect("record")
            .expect("commit");
        assert_eq!(record.writer.as_str(), "operator:fixture");
        assert_eq!(record.previous_revision, before.memory_revision);
        assert_eq!(record.revision, current);
        assert_eq!(record.process_id, std::process::id());
        let other = WriterId::new("other").expect("writer");
        store
            .write_memory_as("main", current, "new", &other)
            .expect("no-op");
        assert_eq!(
            store
                .last_memory_write("main")
                .expect("read")
                .expect("record")
                .writer
                .as_str(),
            writer.as_str()
        );
        assert_eq!(
            MemoryRevision::from_hex(&current.to_hex()).expect("decode"),
            current
        );
        assert!(WriterId::new("inject\nrecord").is_err());
        assert!(MemoryRevision::from_hex(&"x".repeat(64)).is_err());
    }

    #[test]
    fn conflicting_pending_record_preserves_evidence() {
        let (_temp, store) = store();
        let path = store.root.join("main");
        let record = MemoryWrite {
            previous_revision: revision("old"),
            revision: revision("new"),
            writer: WriterId::new("fixture").expect("writer"),
            unix_millis: 1,
            process_id: 1,
        };
        fs::write(path.join(".memory.pending"), record.encode()).expect("intent");
        fs::write(path.join(".memory.previous.pending"), "old").expect("previous");
        assert!(matches!(
            store.read("main"),
            Err(StoreError::RecoveryConflict)
        ));
        assert!(path.join(".memory.pending").is_file());
        assert!(path.join(".memory.previous.pending").is_file());
        assert_eq!(
            fs::read_to_string(path.join("MEMORY.md")).expect("read"),
            "# Memory\n"
        );
    }

    #[test]
    #[ignore = "spawned by process_kill_recovers_each_commit_boundary"]
    fn crash_child() {
        let root =
            PathBuf::from(std::env::var_os("LODEX_PROFILE_TEST_ROOT").expect("fixture root"));
        let store = LocalProfiles::open(root).expect("open");
        let before = store.read("main").expect("before");
        store
            .write_memory_as(
                "main",
                before.memory_revision,
                "new child memory",
                &WriterId::new("crashed-child").expect("writer"),
            )
            .expect("write");
        panic!("fault checkpoint was not reached");
    }

    #[test]
    fn process_kill_recovers_each_commit_boundary() {
        for (stage, committed) in [
            ("after-previous-stage", false),
            ("after-intent", false),
            ("during-memory-write", false),
            ("after-memory", true),
            ("after-backup", true),
            ("after-attribution", true),
        ] {
            let (_temp, store) = store();
            let before = store.read("main").expect("before");
            store
                .write_memory_as(
                    "main",
                    before.memory_revision,
                    "old parent memory",
                    &WriterId::new("parent").expect("writer"),
                )
                .expect("baseline");
            let mut child =
                std::process::Command::new(std::env::current_exe().expect("test executable"))
                    .args(["--exact", "tests::crash_child", "--ignored", "--nocapture"])
                    .env("LODEX_PROFILE_TEST_ROOT", &store.root)
                    .env("LODEX_PROFILE_FAULT_STAGE", stage)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .expect("spawn fixture");
            let marker = store.root.join("main/.fault-ready");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !marker.is_file() {
                if child.try_wait().expect("poll").is_some()
                    || std::time::Instant::now() >= deadline
                {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("child did not reach {stage}");
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            child.kill().expect("kill at checkpoint");
            assert!(!child.wait().expect("reap").success());
            let after = store.read("main").expect("recover");
            let writer = store
                .last_memory_write("main")
                .expect("record")
                .expect("writer");
            if committed {
                assert_eq!(after.memory, "new child memory", "{stage}");
                assert_eq!(
                    store.read_previous_memory("main").expect("backup"),
                    "old parent memory"
                );
                assert_eq!(writer.writer.as_str(), "crashed-child");
            } else {
                assert_eq!(after.memory, "old parent memory", "{stage}");
                assert_eq!(
                    store.read_previous_memory("main").expect("backup"),
                    before.memory
                );
                assert_eq!(writer.writer.as_str(), "parent");
            }
            assert_eq!(writer.revision, after.memory_revision);
            assert!(!store.root.join("main/.memory.pending").exists());
            assert!(
                !fs::read_dir(store.root.join("main"))
                    .expect("entries")
                    .any(|entry| entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".write-"))
            );
        }
    }

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
