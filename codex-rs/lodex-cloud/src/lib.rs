//! Bounded cloud controllers with separate subscription and API credentials.
use std::{fmt, time::Duration};

pub mod agents;
#[cfg(feature = "chatgpt")]
mod legacy;
mod transport;
#[cfg(feature = "chatgpt")]
pub use legacy::{CloudClient, Environment, Phase, TaskResult, TaskStatus, validate_backend};
#[cfg(feature = "api-cli")]
pub mod api_cli;
#[cfg(feature = "cli")]
pub mod cli;

pub const MAX_METADATA_BYTES: usize = 256 * 1024;
pub const MAX_RESULT_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_RESULT_TEXT_BYTES: usize = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, PartialEq, Eq)]
pub enum CloudError {
    UntrustedOrigin,
    InvalidIdentifier,
    InvalidResponse,
    InvalidSubmission,
    Authentication,
    ApiAuthentication,
    InvalidApiSubmission,
    Http(u16),
    ResponseTooLarge(usize),
    Timeout,
    Transport,
    SubmissionOutcomeUnknown,
    CancellationUnavailable,
}
impl fmt::Display for CloudError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UntrustedOrigin => f.write_str("saved ChatGPT credentials require a trusted HTTPS ChatGPT backend origin"),
            Self::InvalidIdentifier => f.write_str("invalid bounded cloud identifier or repository"),
            Self::InvalidResponse => f.write_str("cloud response has an unsupported or invalid schema"),
            Self::InvalidSubmission => f.write_str("task must have explicit environment, git_ref and bounded task text"),
            Self::ApiAuthentication => f.write_str("API mode requires a separate OPENAI_API_KEY; saved ChatGPT login is not an API key"),
            Self::InvalidApiSubmission => f.write_str("API submission requires an explicit model, bounded input and supported environment"),
            Self::Authentication => f.write_str("ChatGPT authentication is unavailable or changed"),
            Self::Http(code) => write!(f,"cloud request returned HTTP {code}"),
            Self::ResponseTooLarge(limit) => write!(f,"cloud response exceeds the {limit} byte limit; nothing was truncated"),
            Self::Timeout => f.write_str("cloud request timed out"),
            Self::Transport => f.write_str("cloud transport failed"),
            Self::SubmissionOutcomeUnknown => f.write_str("cloud submission outcome is unknown; inspect the selected environment before retrying"),
            Self::CancellationUnavailable => f.write_str("the pinned CLI cloud-task API exposes no verified cancellation endpoint; the task remains unchanged"),
        }
    }
}
impl std::error::Error for CloudError {}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}
