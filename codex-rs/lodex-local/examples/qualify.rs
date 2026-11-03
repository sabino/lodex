//! Synthetic native filesystem qualification; never opens an existing profile root.
use codex_lodex_local::{LocalProfiles, MAX_MEMORY_BYTES, StoreError};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("supply a NEW absolute test root")?,
    );
    if root.try_exists()? {
        return Err("test root must not already exist".into());
    }
    let store = LocalProfiles::open(root.clone())?;
    store.init("qualification")?;
    let before = store.read("qualification")?;
    let revision = store.write_memory(
        "qualification",
        before.memory_revision,
        "native local canary\n",
    )?;
    assert_eq!(store.read_previous_memory("qualification")?, before.memory);
    assert!(matches!(
        store.write_memory("qualification", before.memory_revision, "stale"),
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store.write_memory("qualification", revision, &"x".repeat(MAX_MEMORY_BYTES + 1)),
        Err(StoreError::TooLarge)
    ));
    drop(store);
    let reopened = LocalProfiles::open(root)?;
    assert_eq!(
        reopened.read("qualification")?.memory,
        "native local canary\n"
    );
    println!("PASS native local profile write, sync, lock, revision, bounds and reopen");
    Ok(())
}
