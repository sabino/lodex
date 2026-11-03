//! Lab-only process-kill qualification; fault hooks are absent from normal builds.
use codex_lodex_local::{LocalProfiles, WriterId};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let first = args
        .next()
        .ok_or("supply a NEW absolute synthetic test root")?;
    if first == "--child" {
        let root = PathBuf::from(args.next().ok_or("child root")?);
        let store = LocalProfiles::open(root)?;
        let before = store.read("main")?;
        store.write_memory_as(
            "main",
            before.memory_revision,
            "new synthetic memory",
            &WriterId::new("native-child")?,
        )?;
        return Err("expected fault checkpoint was not reached".into());
    }
    let root = PathBuf::from(first);
    if !root.is_absolute() || root.try_exists()? {
        return Err("test root must be new and absolute".into());
    }
    std::fs::create_dir(&root)?;
    for (stage, committed) in [
        ("after-previous-stage", false),
        ("after-intent", false),
        ("during-memory-write", false),
        ("after-memory", true),
        ("after-backup", true),
        ("after-attribution", true),
    ] {
        let store = LocalProfiles::open(root.join(stage))?;
        store.init("main")?;
        let before = store.read("main")?;
        store.write_memory_as(
            "main",
            before.memory_revision,
            "old synthetic memory",
            &WriterId::new("native-parent")?,
        )?;
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--child")
            .arg(root.join(stage))
            .env("LODEX_PROFILE_FAULT_STAGE", stage)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let marker = root.join(stage).join("main/.fault-ready");
        let deadline = Instant::now() + Duration::from_secs(30);
        while !marker.is_file() {
            if child.try_wait()?.is_some() || Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("child failed to reach {stage}").into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child.kill()?;
        assert!(!child.wait()?.success());
        let after = store.read("main")?;
        let record = after.memory_write.as_ref().ok_or("missing attribution")?;
        if committed {
            assert_eq!(after.memory, "new synthetic memory");
            assert_eq!(record.writer.as_str(), "native-child");
            assert_eq!(store.read_previous_memory("main")?, "old synthetic memory");
        } else {
            assert_eq!(after.memory, "old synthetic memory");
            assert_eq!(record.writer.as_str(), "native-parent");
            assert_eq!(store.read_previous_memory("main")?, before.memory);
        }
        assert_eq!(record.revision, after.memory_revision);
        assert!(!root.join(stage).join("main/.memory.pending").exists());
        for entry in std::fs::read_dir(root.join(stage).join("main"))? {
            assert!(!entry?.file_name().to_string_lossy().starts_with(".write-"));
        }
        println!("PASS process-kill recovery {stage}");
    }
    println!("PASS all six process-kill commit boundaries");
    Ok(())
}
