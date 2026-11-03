//! Explicit local storage operations. No agent prompt or cloud configuration.
use crate::{LocalProfiles, MAX_MEMORY_BYTES, MemoryRevision, StoreError, WriterId};
use clap::{Args, Subcommand, ValueEnum};
use std::io::{self, Read, Write};
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct ProfileArgs {
    /// Absolute private profile directory, outside the source checkout.
    #[arg(long, env = "LODEX_PROFILE_ROOT", value_name = "DIRECTORY")]
    root: PathBuf,
    #[command(subcommand)]
    command: ProfileCommand,
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// Create a private profile with editable SOUL.md and MEMORY.md templates.
    Init { name: String },
    /// Show sizes, a revision and attribution, without printing profile text.
    Inspect { name: String },
    /// Print local file paths for a human editor.
    Paths { name: String },
    /// Explicitly print the selected private file to stdout.
    Show {
        name: String,
        #[arg(value_enum)]
        field: ProfileText,
    },
    /// Replace MEMORY.md from bounded UTF-8 stdin using a compare-and-swap revision.
    WriteMemory {
        name: String,
        #[arg(long, value_name = "SHA256")]
        expect: String,
        #[arg(long, value_name = "ID")]
        writer: String,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ProfileText {
    Soul,
    Memory,
    PreviousMemory,
}

impl ProfileArgs {
    pub fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.run_with(&mut io::stdin().lock(), &mut io::stdout().lock())
    }

    fn run_with(
        self,
        input: &mut impl Read,
        output: &mut impl Write,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let store = LocalProfiles::open(self.root)?;
        match self.command {
            ProfileCommand::Init { name } => {
                store.init(&name)?;
                writeln!(output, "initialized={name}")?;
            }
            ProfileCommand::Inspect { name } => {
                let snapshot = store.read(&name)?;
                writeln!(
                    output,
                    "soul_bytes={}\nmemory_bytes={}\nrevision={}",
                    snapshot.soul.len(),
                    snapshot.memory.len(),
                    snapshot.memory_revision.to_hex()
                )?;
                if let Some(record) = snapshot.memory_write {
                    writeln!(
                        output,
                        "last_writer={}\nlast_write_unix_millis={}\nlast_write_pid={}\nlast_write_current={}",
                        record.writer.as_str(),
                        record.unix_millis,
                        record.process_id,
                        record.revision == snapshot.memory_revision
                    )?;
                } else {
                    writeln!(output, "last_write_current=false")?;
                }
            }
            ProfileCommand::Paths { name } => {
                let path = store.directory(&name)?;
                writeln!(
                    output,
                    "soul={}\nmemory={}",
                    path.join("SOUL.md").display(),
                    path.join("MEMORY.md").display()
                )?;
            }
            ProfileCommand::Show { name, field } => {
                let text = match field {
                    ProfileText::Soul => store.read(&name)?.soul,
                    ProfileText::Memory => store.read(&name)?.memory,
                    ProfileText::PreviousMemory => store.read_previous_memory(&name)?,
                };
                output.write_all(text.as_bytes())?;
            }
            ProfileCommand::WriteMemory {
                name,
                expect,
                writer,
            } => {
                let expected = MemoryRevision::from_hex(&expect)?;
                let writer = WriterId::new(&writer)?;
                let mut bytes = Vec::new();
                input
                    .take((MAX_MEMORY_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > MAX_MEMORY_BYTES {
                    return Err(StoreError::TooLarge.into());
                }
                let memory = String::from_utf8(bytes).map_err(|_| StoreError::InvalidUtf8)?;
                let revision = store.write_memory_as(&name, expected, &memory, &writer)?;
                writeln!(output, "{}", revision.to_hex())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_roundtrip_redacts_inspection_and_rejects_stale_and_oversized_input() {
        let temporary = tempfile::tempdir().expect("fixture");
        let root = temporary.path().join("profiles");
        let args = |command| ProfileArgs {
            root: root.clone(),
            command,
        };
        args(ProfileCommand::Init {
            name: "main".into(),
        })
        .run_with(&mut &b""[..], &mut Vec::new())
        .expect("init");
        let store = LocalProfiles::open(root.clone()).expect("open");
        let before = store.read("main").expect("read");
        let command = || ProfileCommand::WriteMemory {
            name: "main".into(),
            expect: before.memory_revision.to_hex(),
            writer: "operator:fixture".into(),
        };
        args(command())
            .run_with(&mut &b"private-cli-canary\n"[..], &mut Vec::new())
            .expect("write");
        assert!(
            args(command())
                .run_with(&mut &b"stale"[..], &mut Vec::new())
                .is_err()
        );
        assert!(
            args(command())
                .run_with(&mut &vec![b'x'; MAX_MEMORY_BYTES + 1][..], &mut Vec::new())
                .is_err()
        );
        let mut output = Vec::new();
        args(ProfileCommand::Inspect {
            name: "main".into(),
        })
        .run_with(&mut &b""[..], &mut output)
        .expect("inspect");
        assert!(
            !output
                .windows(b"private-cli-canary".len())
                .any(|part| part == b"private-cli-canary")
        );
        assert!(
            String::from_utf8(output)
                .expect("utf8")
                .contains("last_writer=operator:fixture")
        );
        let mut output = Vec::new();
        args(ProfileCommand::Show {
            name: "main".into(),
            field: ProfileText::Memory,
        })
        .run_with(&mut &b""[..], &mut output)
        .expect("show");
        assert_eq!(output, b"private-cli-canary\n");
        assert_eq!(
            store.read("main").expect("unchanged").memory,
            "private-cli-canary\n"
        );
    }
}
