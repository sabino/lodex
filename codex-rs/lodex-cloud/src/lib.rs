//! Bounded, noninteractive adapter to the pinned Codex CLI cloud-task API.
//! No parent-thread/profile input, ambient Git discovery, UI, or body diagnostics.
use codex_api::SharedAuthProvider;
use codex_http_client::{
    ClientRouteClass, HttpClientFactory, HttpTransport, Request, ReqwestTransport,
    RouteAwareClientPool, TransportError,
};
use codex_lodex_policy::{CloudTask, encode_cloud_submission};
use http::{HeaderValue, Method, StatusCode};
use serde::{Deserialize, Serialize};
use std::{fmt, time::Duration};
use url::Url;

pub mod agents;

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

pub fn validate_backend(input: &str) -> Result<Url, CloudError> {
    let mut url = Url::parse(input).map_err(|_| CloudError::UntrustedOrigin)?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("chatgpt.com" | "chat.openai.com"))
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path().trim_end_matches('/'), "" | "/backend-api")
    {
        return Err(CloudError::UntrustedOrigin);
    }
    url.set_path("/backend-api/");
    Ok(url)
}

pub struct CloudClient {
    base: Url,
    transport: ReqwestTransport,
    auth: SharedAuthProvider,
}
impl fmt::Debug for CloudClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudClient").finish_non_exhaustive()
    }
}

#[derive(Deserialize, Serialize)]
pub struct Environment {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Serialize)]
pub struct TaskStatus {
    pub task_id: String,
    pub turn_id: Option<String>,
    pub phase: Phase,
}
#[derive(Serialize)]
pub struct TaskResult {
    pub status: TaskStatus,
    pub messages: Vec<String>,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}
// Turn IDs are opaque output tokens, sometimes composite. They are never URL
// path inputs; keep task/environment URL identifiers under the stricter rule.
fn turn_identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn phase(value: Option<&str>) -> Phase {
    match value {
        Some("pending") => Phase::Pending,
        Some("in_progress") => Phase::InProgress,
        Some("completed") => Phase::Completed,
        Some("failed") => Phase::Failed,
        Some("cancelled") => Phase::Cancelled,
        _ => Phase::Unknown,
    }
}

impl CloudClient {
    pub fn new(
        base: &str,
        factory: HttpClientFactory,
        auth: SharedAuthProvider,
    ) -> Result<Self, CloudError> {
        let base = validate_backend(base)?;
        Ok(Self::with_base(base, factory, auth))
    }
    fn with_base(base: Url, factory: HttpClientFactory, auth: SharedAuthProvider) -> Self {
        Self {
            base,
            transport: ReqwestTransport::from_route_aware_client_pool(
                RouteAwareClientPool::new_without_redirects_or_request_logging(
                    factory,
                    ClientRouteClass::Api,
                ),
            ),
            auth,
        }
    }
    fn url(&self, segments: &[&str]) -> Result<String, CloudError> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| CloudError::UntrustedOrigin)?
            .pop_if_empty()
            .extend(segments);
        Ok(url.into())
    }
    async fn request(
        &self,
        method: Method,
        url: String,
        body: Option<Vec<u8>>,
        limit: usize,
    ) -> Result<bytes::Bytes, CloudError> {
        let body = body.map(bytes::Bytes::from);
        let operation = async {
            for attempt in 0..2 {
                let mut request = Request::new(method.clone(), url.clone());
                request.timeout = Some(REQUEST_TIMEOUT);
                request.response_body_limit_bytes = Some(limit);
                request.headers.insert(
                    http::header::ACCEPT,
                    HeaderValue::from_static("application/json"),
                );
                request.headers.insert(
                    http::header::USER_AGENT,
                    HeaderValue::from_static("lodex-cloud/0.166.0"),
                );
                if let Some(body) = body.as_ref() {
                    request = request.with_raw_body(body.clone());
                    request.headers.insert(
                        http::header::CONTENT_TYPE,
                        HeaderValue::from_static("application/json"),
                    );
                }
                let request = self
                    .auth
                    .apply_auth(request)
                    .await
                    .map_err(|_| CloudError::Authentication)?;
                let rejected = request.headers.clone();
                match self.transport.execute(request).await {
                    Ok(response) => return Ok(response.body),
                    Err(TransportError::Http {
                        status: StatusCode::UNAUTHORIZED,
                        ..
                    }) if attempt == 0 => {
                        if !self.auth.refresh_after_unauthorized(rejected).await {
                            return Err(CloudError::Http(401));
                        }
                    }
                    Err(TransportError::Http { status, .. }) => {
                        return Err(CloudError::Http(status.as_u16()));
                    }
                    Err(TransportError::ResponseTooLarge { max_bytes }) => {
                        return Err(CloudError::ResponseTooLarge(max_bytes));
                    }
                    Err(_) => return Err(CloudError::Transport),
                }
            }
            Err(CloudError::Http(401))
        };
        tokio::time::timeout(REQUEST_TIMEOUT, operation)
            .await
            .map_err(|_| CloudError::Timeout)?
    }

    pub async fn environments(
        &self,
        repository: Option<&str>,
    ) -> Result<Vec<Environment>, CloudError> {
        let url = if let Some(repository) = repository {
            let (owner, repo) = repository
                .split_once('/')
                .ok_or(CloudError::InvalidIdentifier)?;
            if !identifier(owner) || !identifier(repo) {
                return Err(CloudError::InvalidIdentifier);
            }
            self.url(&["wham", "environments", "by-repo", "github", owner, repo])?
        } else {
            self.url(&["wham", "environments"])?
        };
        let bytes = self
            .request(Method::GET, url, None, MAX_METADATA_BYTES)
            .await?;
        let environments: Vec<Environment> =
            serde_json::from_slice(&bytes).map_err(|_| CloudError::InvalidResponse)?;
        if environments.len() > 256
            || environments
                .iter()
                .any(|e| !identifier(&e.id) || e.label.as_ref().is_some_and(|v| v.len() > 1024))
        {
            return Err(CloudError::InvalidResponse);
        }
        Ok(environments)
    }

    pub async fn submit(&self, task: &CloudTask) -> Result<String, CloudError> {
        let body = encode_cloud_submission(task).map_err(|_| CloudError::InvalidSubmission)?;
        let bytes = self
            .request(
                Method::POST,
                self.url(&["wham", "tasks"])?,
                Some(body),
                MAX_METADATA_BYTES,
            )
            .await
            .map_err(|error| match error {
                CloudError::Transport
                | CloudError::Timeout
                | CloudError::ResponseTooLarge(_)
                | CloudError::Http(300..=399 | 500..=599) => CloudError::SubmissionOutcomeUnknown,
                other => other,
            })?;
        #[derive(Deserialize)]
        struct Id {
            id: String,
        }
        #[derive(Deserialize)]
        struct Created {
            task: Option<Id>,
            id: Option<String>,
        }
        let created: Created =
            serde_json::from_slice(&bytes).map_err(|_| CloudError::SubmissionOutcomeUnknown)?;
        let id = created
            .task
            .map(|t| t.id)
            .or(created.id)
            .ok_or(CloudError::SubmissionOutcomeUnknown)?;
        if !identifier(&id) {
            return Err(CloudError::SubmissionOutcomeUnknown);
        }
        Ok(id)
    }

    pub async fn status(&self, id: &str) -> Result<TaskStatus, CloudError> {
        if !identifier(id) {
            return Err(CloudError::InvalidIdentifier);
        }
        let bytes = self
            .request(
                Method::GET,
                self.url(&["wham", "tasks", id])?,
                None,
                MAX_METADATA_BYTES,
            )
            .await?;
        let details: StatusDetails =
            serde_json::from_slice(&bytes).map_err(|_| CloudError::InvalidResponse)?;
        details.status(id)
    }
    pub async fn result(&self, id: &str) -> Result<TaskResult, CloudError> {
        if !identifier(id) {
            return Err(CloudError::InvalidIdentifier);
        }
        let bytes = self
            .request(
                Method::GET,
                self.url(&["wham", "tasks", id])?,
                None,
                MAX_RESULT_BODY_BYTES,
            )
            .await?;
        let details: ResultDetails =
            serde_json::from_slice(&bytes).map_err(|_| CloudError::InvalidResponse)?;
        let status = details.status(id)?;
        let mut messages = Vec::new();
        for turn in [
            details.current_diff_task_turn,
            details.current_assistant_turn,
        ]
        .into_iter()
        .flatten()
        {
            for item in turn.output_items {
                if item.kind == "message"
                    && item
                        .role
                        .as_deref()
                        .is_none_or(|r| r.eq_ignore_ascii_case("assistant"))
                {
                    for content in item.content {
                        if let Some(text) = content.into_text() {
                            messages.push(text);
                        }
                    }
                }
            }
            if let Some(log) = turn.worklog {
                for item in log.messages {
                    if item
                        .author
                        .as_ref()
                        .and_then(|a| a.role.as_deref())
                        .is_some_and(|r| r.eq_ignore_ascii_case("assistant"))
                        && let Some(content) = item.content
                    {
                        for part in content.parts {
                            if let Some(text) = part.into_text() {
                                messages.push(text);
                            }
                        }
                    }
                }
            }
        }
        if messages.len() > 64
            || messages.iter().map(String::len).sum::<usize>() > MAX_RESULT_TEXT_BYTES
        {
            return Err(CloudError::ResponseTooLarge(MAX_RESULT_TEXT_BYTES));
        }
        Ok(TaskResult { status, messages })
    }
    pub fn cancel(&self, _id: &str) -> Result<(), CloudError> {
        Err(CloudError::CancellationUnavailable)
    }
}

#[derive(Deserialize)]
struct TaskIdentity {
    id: String,
    task_status_display: Option<Display>,
}
#[derive(Deserialize)]
struct TurnStatus {
    #[serde(alias = "turn_id")]
    id: Option<String>,
    turn_status: Option<String>,
}
#[derive(Deserialize)]
struct Display {
    latest_turn_status_display: Option<TurnStatus>,
}
#[derive(Deserialize)]
struct StatusDetails {
    task: TaskIdentity,
    current_assistant_turn: Option<TurnStatus>,
    task_status_display: Option<Display>,
}
impl StatusDetails {
    fn status(self, id: &str) -> Result<TaskStatus, CloudError> {
        if self.task.id != id {
            return Err(CloudError::InvalidResponse);
        }
        let turn = self.current_assistant_turn.or_else(|| {
            self.task_status_display
                .or(self.task.task_status_display)
                .and_then(|d| d.latest_turn_status_display)
        });
        if let Some(turn) = turn {
            if turn.id.as_ref().is_some_and(|id| !turn_identifier(id)) {
                return Err(CloudError::InvalidResponse);
            }
            Ok(TaskStatus {
                task_id: id.into(),
                turn_id: turn.id,
                phase: phase(turn.turn_status.as_deref()),
            })
        } else {
            Ok(TaskStatus {
                task_id: id.into(),
                turn_id: None,
                phase: Phase::Unknown,
            })
        }
    }
}
#[derive(Deserialize)]
struct ResultDetails {
    task: TaskIdentity,
    current_assistant_turn: Option<ResultTurn>,
    current_diff_task_turn: Option<ResultTurn>,
    task_status_display: Option<Display>,
}
impl ResultDetails {
    fn status(&self, id: &str) -> Result<TaskStatus, CloudError> {
        if self.task.id != id {
            return Err(CloudError::InvalidResponse);
        }
        let turn = self
            .current_assistant_turn
            .as_ref()
            .map(|t| (&t.id, &t.turn_status))
            .or_else(|| {
                self.task_status_display
                    .as_ref()
                    .or(self.task.task_status_display.as_ref())?
                    .latest_turn_status_display
                    .as_ref()
                    .map(|t| (&t.id, &t.turn_status))
            });
        let (turn_id, phase) = if let Some((turn_id, state)) = turn {
            (turn_id.clone(), phase(state.as_deref()))
        } else {
            (None, Phase::Unknown)
        };
        if turn_id.as_ref().is_some_and(|id| !turn_identifier(id)) {
            return Err(CloudError::InvalidResponse);
        }
        Ok(TaskStatus {
            task_id: id.into(),
            turn_id,
            phase,
        })
    }
}
#[derive(Deserialize)]
struct ResultTurn {
    id: Option<String>,
    turn_status: Option<String>,
    #[serde(default, deserialize_with = "nullable_vec")]
    output_items: Vec<Item>,
    worklog: Option<Worklog>,
}
#[derive(Deserialize)]
struct Item {
    #[serde(rename = "type")]
    kind: String,
    role: Option<String>,
    #[serde(default, deserialize_with = "nullable_vec")]
    content: Vec<Content>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Structured {
        content_type: Option<String>,
        text: Option<String>,
    },
    Text(String),
}
impl Content {
    fn into_text(self) -> Option<String> {
        match self {
            Self::Text(text) => (!text.trim().is_empty()).then_some(text),
            Self::Structured {
                content_type: Some(kind),
                text,
            } if kind.eq_ignore_ascii_case("text") => text.filter(|text| !text.is_empty()),
            _ => None,
        }
    }
}
#[derive(Deserialize)]
struct Worklog {
    #[serde(default, deserialize_with = "nullable_vec")]
    messages: Vec<LogMessage>,
}
#[derive(Deserialize)]
struct Author {
    role: Option<String>,
}
#[derive(Deserialize)]
struct LogContent {
    #[serde(default, deserialize_with = "nullable_vec")]
    parts: Vec<Content>,
}
#[derive(Deserialize)]
struct LogMessage {
    author: Option<Author>,
    content: Option<LogContent>,
}

fn nullable_vec<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests;
