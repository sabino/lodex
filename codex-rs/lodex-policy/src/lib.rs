//! Explicit worker-task boundary: no parent-thread, profile or filesystem inputs.
//! This controls adapter payload construction, not arbitrary text semantics or
//! the network/filesystem permissions of a root process.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{self, Read, Write};

pub const MAX_TASK_BYTES: usize = 32 * 1024;
pub const MAX_IDENTIFIER_BYTES: usize = 256;
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionError {
    TooLarge,
    MissingGitRef,
}

impl fmt::Display for SubmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "encoded cloud submission exceeds the 64 KiB limit",
            Self::MissingGitRef => "cloud submission requires an explicit git_ref",
        })
    }
}

impl std::error::Error for SubmissionError {}

/// Build the pinned Codex new-task wire payload without consulting the process
/// environment, working tree, parent thread or local profile store. In particular,
/// do not inherit CODEX_STARTING_DIFF as the upstream cloud UI adapter does.
/// An explicit branch is required; there is no ambient Git discovery or default.
pub fn encode_cloud_submission(task: &CloudTask) -> Result<Vec<u8>, SubmissionError> {
    #[derive(Serialize)]
    struct NewTask<'a> {
        environment_id: &'a str,
        branch: &'a str,
        run_environment_in_qa_mode: bool,
    }
    #[derive(Serialize)]
    struct Text<'a> {
        content_type: &'static str,
        text: &'a str,
    }
    #[derive(Serialize)]
    struct Message<'a> {
        #[serde(rename = "type")]
        kind: &'static str,
        role: &'static str,
        content: [Text<'a>; 1],
    }
    #[derive(Serialize)]
    struct Submission<'a> {
        new_task: NewTask<'a>,
        input_items: [Message<'a>; 1],
    }
    struct BoundedBytes(Vec<u8>);
    impl Write for BoundedBytes {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > MAX_REQUEST_BYTES.saturating_sub(self.0.len()) {
                return Err(io::Error::other(SubmissionError::TooLarge));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let submission = Submission {
        new_task: NewTask {
            environment_id: task.environment_id(),
            branch: task.git_ref().ok_or(SubmissionError::MissingGitRef)?,
            run_environment_in_qa_mode: false,
        },
        input_items: [Message {
            kind: "message",
            role: "user",
            content: [Text {
                content_type: "text",
                text: task.task(),
            }],
        }],
    };
    let mut bytes = BoundedBytes(Vec::with_capacity(MAX_REQUEST_BYTES));
    serde_json::to_writer(&mut bytes, &submission).map_err(|_| SubmissionError::TooLarge)?;
    Ok(bytes.0)
}

#[derive(Debug)]
pub enum ReadTaskError {
    Io(std::io::Error),
    RequestTooLarge,
    InvalidRequest,
}

impl fmt::Display for ReadTaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "could not read worker task: {error}"),
            Self::RequestTooLarge => f.write_str("worker request exceeds the 64 KiB input limit"),
            Self::InvalidRequest => {
                f.write_str("worker request has invalid JSON, fields or values")
            }
        }
    }
}

impl std::error::Error for ReadTaskError {}

/// The adapter's stream entry point. Bound the encoded input before JSON parsing;
/// field validation alone cannot prevent allocations for oversized JSON strings.
pub fn read_cloud_task(reader: impl Read) -> Result<CloudTask, ReadTaskError> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(ReadTaskError::Io)?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(ReadTaskError::RequestTooLarge);
    }
    serde_json::from_slice(&bytes).map_err(|_| ReadTaskError::InvalidRequest)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCloudTask {
    environment_id: String,
    #[serde(default)]
    git_ref: Option<String>,
    task: String,
}

/// A new, explicitly selected cloud task. It does not represent migrating an
/// existing local thread, and contains no implicit conversation or profile data.
#[derive(Serialize, Deserialize)]
#[serde(try_from = "RawCloudTask")]
pub struct CloudTask {
    environment_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    git_ref: Option<String>,
    task: String,
}

impl fmt::Debug for CloudTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudTask")
            .field("environment_id", &self.environment_id)
            .field("git_ref", &self.git_ref)
            .field("task_bytes", &self.task.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidTask {
    InvalidEnvironment,
    InvalidGitRef,
    EmptyTask,
    TaskTooLarge,
}

impl fmt::Display for InvalidTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidEnvironment => "an explicit bounded environment ID is required",
            Self::InvalidGitRef => {
                "git reference is empty, contains control characters or is too large"
            }
            Self::EmptyTask => "worker task is empty",
            Self::TaskTooLarge => "worker task exceeds the 32 KiB input limit",
        })
    }
}

impl std::error::Error for InvalidTask {}

fn valid_identifier(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && !value.chars().any(char::is_control)
}

impl CloudTask {
    pub fn new(
        environment_id: String,
        git_ref: Option<String>,
        task: String,
    ) -> Result<Self, InvalidTask> {
        if !valid_identifier(&environment_id) {
            return Err(InvalidTask::InvalidEnvironment);
        }
        if git_ref
            .as_deref()
            .is_some_and(|value| !valid_identifier(value))
        {
            return Err(InvalidTask::InvalidGitRef);
        }
        if task.trim().is_empty() {
            return Err(InvalidTask::EmptyTask);
        }
        if task.len() > MAX_TASK_BYTES {
            return Err(InvalidTask::TaskTooLarge);
        }
        Ok(Self {
            environment_id,
            git_ref,
            task,
        })
    }

    pub fn environment_id(&self) -> &str {
        &self.environment_id
    }

    pub fn git_ref(&self) -> Option<&str> {
        self.git_ref.as_deref()
    }

    pub fn task(&self) -> &str {
        &self.task
    }

    /// Transfer only explicitly supplied fields to the backend adapter.
    pub fn into_parts(self) -> (String, Option<String>, String) {
        (self.environment_id, self.git_ref, self.task)
    }
}

impl TryFrom<RawCloudTask> for CloudTask {
    type Error = InvalidTask;

    fn try_from(value: RawCloudTask) -> Result<Self, Self::Error> {
        Self::new(value.environment_id, value.git_ref, value.task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cloud_wire_payload_contains_only_one_selected_task() {
        let task = CloudTask::new(
            "selected-env".into(),
            Some("selected-ref".into()),
            "Check: oi, ação!\n\"quoted\"".into(),
        )
        .expect("valid");
        let wire = encode_cloud_submission(&task).expect("bounded wire");
        let value: serde_json::Value = serde_json::from_slice(&wire).expect("JSON");
        assert_eq!(
            value,
            json!({
                "new_task": {"environment_id":"selected-env", "branch":"selected-ref", "run_environment_in_qa_mode":false},
                "input_items": [{"type":"message", "role":"user", "content":[{"content_type":"text", "text":task.task()}]}]
            })
        );
    }

    #[test]
    fn escaped_wire_size_is_bounded_before_any_submission() {
        let task = CloudTask::new(
            "env".into(),
            Some("selected".into()),
            "\0".repeat(MAX_TASK_BYTES),
        )
        .expect("byte bound");
        assert_eq!(
            encode_cloud_submission(&task).err(),
            Some(SubmissionError::TooLarge)
        );
    }

    #[test]
    fn omitted_ref_is_rejected_before_any_submission() {
        let task = CloudTask::new("env".into(), None, "selected task".into()).expect("valid");
        assert_eq!(
            encode_cloud_submission(&task).err(),
            Some(SubmissionError::MissingGitRef)
        );
    }

    #[test]
    fn ambient_starting_diff_is_not_forwarded() {
        const CHILD: &str = "LODEX_WIRE_CANARY_CHILD";
        const CANARY: &str = "private-starting-diff-must-not-be-exported";
        if std::env::var_os(CHILD).is_some() {
            let task = CloudTask::new(
                "env".into(),
                Some("selected".into()),
                "selected task".into(),
            )
            .expect("valid");
            let wire = encode_cloud_submission(&task).expect("wire");
            assert!(
                !String::from_utf8(wire)
                    .expect("UTF-8 JSON")
                    .contains(CANARY)
            );
            return;
        }
        let child = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "tests::ambient_starting_diff_is_not_forwarded"])
            .env(CHILD, "1")
            .env("CODEX_STARTING_DIFF", CANARY)
            .output()
            .expect("isolated canary child");
        assert!(child.status.success(), "canary child failed");
    }

    #[test]
    fn rejects_parent_context_and_private_asset_fields() {
        for field in [
            "memory",
            "soul",
            "parent_thread",
            "transcript",
            "attachments",
            "source_path",
        ] {
            let mut request =
                json!({"environment_id": "test-env", "task": "Run one selected check"});
            request[field] = json!("private-canary-must-not-cross");
            assert!(
                serde_json::from_value::<CloudTask>(request).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn roundtrip_preserves_only_selected_fields_and_unicode() {
        let input = json!({"environment_id": "test-env", "git_ref": "main", "task": "Check: oi, ação!\n\"quoted\""});
        let task: CloudTask = serde_json::from_value(input.clone()).expect("valid selected task");
        assert_eq!(serde_json::to_value(&task).expect("serialize"), input);
        assert_eq!(task.environment_id(), "test-env");
    }

    #[test]
    fn deserialize_enforces_limits_without_a_separate_validation_call() {
        for text in [" ".to_string(), "a".repeat(MAX_TASK_BYTES + 1)] {
            let request = json!({"environment_id": "test-env", "task": text});
            assert!(serde_json::from_value::<CloudTask>(request).is_err());
        }
    }

    #[test]
    fn identifiers_cannot_select_a_default_or_contain_controls() {
        for environment in ["", " ", "test\nforged"] {
            assert!(CloudTask::new(environment.into(), None, "check".into()).is_err());
        }
        assert!(CloudTask::new("env".into(), Some("".into()), "check".into()).is_err());
    }

    #[test]
    fn limits_count_utf8_bytes_not_characters() {
        let valid = "é".repeat(MAX_TASK_BYTES / 2);
        assert!(CloudTask::new("env".into(), None, valid.clone()).is_ok());
        assert_eq!(
            CloudTask::new("env".into(), None, valid + "é").err(),
            Some(InvalidTask::TaskTooLarge)
        );
    }

    #[test]
    fn debug_and_validation_errors_do_not_echo_task_content() {
        let task = CloudTask::new("env".into(), None, "private-task-canary".into()).expect("valid");
        assert!(!format!("{task:?}").contains("private-task-canary"));
        assert!(
            !InvalidTask::TaskTooLarge
                .to_string()
                .contains("private-task-canary")
        );
    }

    #[test]
    fn input_stream_is_bounded_before_json_decoding() {
        let input = std::io::repeat(b' ');
        assert!(matches!(
            read_cloud_task(input),
            Err(ReadTaskError::RequestTooLarge)
        ));
    }

    #[test]
    fn adapter_reader_rejects_unknown_context_without_echoing_it() {
        let input = br#"{"environment_id":"env","task":"check","memory":"private-canary"}"#;
        let error = read_cloud_task(input.as_slice()).expect_err("no private context field");
        assert!(!error.to_string().contains("private-canary"));
        assert!(matches!(error, ReadTaskError::InvalidRequest));
    }

    #[test]
    fn adapter_reader_accepts_a_selected_task() {
        let input = br#"{"environment_id":"env","task":"check"}"#;
        let task = read_cloud_task(input.as_slice()).expect("valid adapter request");
        assert_eq!(task.into_parts(), ("env".into(), None, "check".into()));
    }
}
