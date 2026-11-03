//! API-key-backed hosted harness. Independent of saved ChatGPT credentials.
use crate::{
    CloudError, MAX_METADATA_BYTES, MAX_RESULT_BODY_BYTES, MAX_RESULT_TEXT_BYTES, identifier,
};
use codex_http_client::HttpClientFactory;
use http::{HeaderValue, Method};
use serde::{Deserialize, Serialize};

const MAX_CREATE_BYTES: usize = 64 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Environment {
    None,
    OpenaiHosted,
    SelfHosted {
        workspace_directory: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        capability_directories: Vec<String>,
    },
}
/// Deliberately explicit, with no implicit profile, attachments, Git or parent thread.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub agent: Agent,
    pub environment: Environment,
    pub input: String,
}
impl Create {
    pub fn encode(&self) -> Result<Vec<u8>, CloudError> {
        if self.agent.model.is_empty()
            || self.agent.model.len() > 256
            || !self
                .agent
                .model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
            || self.input.trim().is_empty()
            || self.input.len() > 32 * 1024
            || self
                .agent
                .instructions
                .as_ref()
                .is_some_and(|s| s.len() > 8 * 1024)
        {
            return Err(CloudError::InvalidApiSubmission);
        }
        if let Environment::SelfHosted {
            workspace_directory,
            capability_directories,
        } = &self.environment
            && (!absolute_directory(workspace_directory)
                || capability_directories.len() > 16
                || capability_directories
                    .iter()
                    .any(|p| !absolute_directory(p)))
        {
            return Err(CloudError::InvalidApiSubmission);
        }
        let bytes = serde_json::to_vec(self).map_err(|_| CloudError::InvalidApiSubmission)?;
        if bytes.len() > MAX_CREATE_BYTES {
            return Err(CloudError::InvalidApiSubmission);
        }
        Ok(bytes)
    }
}

fn absolute_directory(path: &str) -> bool {
    path.starts_with('/') && path.len() <= 4096 && !path.chars().any(char::is_control)
}

/// Constructor accepts only an explicit API key; never consults AuthManager.
pub struct AgentsClient(crate::transport::ControllerHttp);
impl AgentsClient {
    pub fn new(key: &str, factory: HttpClientFactory) -> Result<Self, CloudError> {
        if key.is_empty()
            || key.len() > 8192
            || key
                .bytes()
                .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        {
            return Err(CloudError::ApiAuthentication);
        }
        let mut value = HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| CloudError::ApiAuthentication)?;
        value.set_sensitive(true);
        Ok(Self(crate::transport::ControllerHttp::new(
            url::Url::parse("https://api.openai.com/v1/")
                .map_err(|_| CloudError::UntrustedOrigin)?,
            factory,
            crate::transport::Credentials::Api(value),
        )))
    }
    async fn request(
        &self,
        method: Method,
        parts: &[&str],
        body: Option<Vec<u8>>,
        limit: usize,
    ) -> Result<bytes::Bytes, CloudError> {
        self.0
            .request(method, self.0.url(parts)?, body, limit)
            .await
    }
    pub async fn create(&self, input: &Create) -> Result<Session, CloudError> {
        let bytes = self
            .request(
                Method::POST,
                &["agents", "sessions"],
                Some(input.encode()?),
                MAX_METADATA_BYTES,
            )
            .await
            .map_err(unknown_mutation)?;
        Session::parse(&bytes, None).map_err(|_| CloudError::SubmissionOutcomeUnknown)
    }
    pub async fn status(&self, id: &str) -> Result<Session, CloudError> {
        if !identifier(id) {
            return Err(CloudError::InvalidIdentifier);
        }
        let bytes = self
            .request(
                Method::GET,
                &["agents", "sessions", id],
                None,
                MAX_METADATA_BYTES,
            )
            .await?;
        Session::parse(&bytes, Some(id))
    }
    /// Acknowledges only event submission. Caller must retrieve status/output afterward.
    pub async fn cancel(&self, id: &str) -> Result<(), CloudError> {
        if !identifier(id) {
            return Err(CloudError::InvalidIdentifier);
        }
        self.request(
            Method::POST,
            &["agents", "sessions", id, "events"],
            Some(br#"{"events":[{"type":"agent.session.input.cancel"}]}"#.to_vec()),
            MAX_METADATA_BYTES,
        )
        .await
        .map_err(unknown_mutation)?;
        Ok(())
    }
    pub async fn result(&self, id: &str, after: Option<&str>) -> Result<ResultPage, CloudError> {
        if !identifier(id) || after.is_some_and(|v| !identifier(v)) {
            return Err(CloudError::InvalidIdentifier);
        }
        let mut url = url::Url::parse(&self.0.url(&["agents", "sessions", id, "items"])?)
            .map_err(|_| CloudError::InvalidIdentifier)?;
        url.query_pairs_mut()
            .append_pair("order", "asc")
            .append_pair("limit", "100");
        if let Some(after) = after {
            url.query_pairs_mut().append_pair("after", after);
        }
        let bytes = self
            .0
            .request(Method::GET, url.into(), None, MAX_RESULT_BODY_BYTES)
            .await?;
        ResultPage::parse(&bytes)
    }
}
fn unknown_mutation(error: CloudError) -> CloudError {
    match error {
        CloudError::Transport
        | CloudError::Timeout
        | CloudError::ResponseTooLarge(_)
        | CloudError::Http(300..=399 | 500..=599) => CloudError::SubmissionOutcomeUnknown,
        other => other,
    }
}

/// Session status is not a turn outcome: idle does not prove a successful turn.
#[derive(Deserialize, Serialize)]
pub struct Session {
    pub id: String,
    pub status: String,
    pub environment: Option<SessionEnvironment>,
    #[serde(default)]
    pub required_actions: Vec<RequiredAction>,
}
#[derive(Deserialize, Serialize)]
pub struct SessionEnvironment {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: Option<String>,
    pub remote_url: Option<String>,
}
#[derive(Deserialize, Serialize)]
pub struct RequiredAction {
    #[serde(rename = "type")]
    pub kind: String,
}
impl Session {
    fn parse(bytes: &[u8], expected: Option<&str>) -> Result<Self, CloudError> {
        let session: Self =
            serde_json::from_slice(bytes).map_err(|_| CloudError::InvalidResponse)?;
        if !identifier(&session.id)
            || expected.is_some_and(|id| id != session.id)
            || !matches!(
                session.status.as_str(),
                "idle" | "in_progress" | "requires_action" | "failed"
            )
            || session.required_actions.len() > 64
            || session
                .required_actions
                .iter()
                .any(|a| !identifier(&a.kind))
        {
            return Err(CloudError::InvalidResponse);
        }
        if let Some(env) = &session.environment {
            if !matches!(env.kind.as_str(), "none" | "openai_hosted" | "self_hosted")
                || env.id.as_ref().is_some_and(|id| !identifier(id))
            {
                return Err(CloudError::InvalidResponse);
            }
            if let Some(remote) = &env.remote_url {
                let url = url::Url::parse(remote).map_err(|_| CloudError::InvalidResponse)?;
                if remote.len() > 4096
                    || !matches!(
                        (url.scheme(), url.host_str()),
                        ("https", Some("api.openai.com"))
                            | ("wss", Some("codex-cloud-environments.chatgpt.com"))
                    )
                    || url.port_or_known_default() != Some(443)
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.fragment().is_some()
                {
                    return Err(CloudError::InvalidResponse);
                }
            }
        }
        Ok(session)
    }
}
#[derive(Serialize)]
pub struct ResultPage {
    pub messages: Vec<Message>,
    pub has_more: bool,
    pub next_after: Option<String>,
}
#[derive(Serialize)]
pub struct Message {
    pub id: Option<String>,
    pub turn_id: String,
    pub status: String,
    pub phase: Option<String>,
    pub text: String,
}
impl ResultPage {
    fn parse(bytes: &[u8]) -> Result<Self, CloudError> {
        #[derive(Deserialize)]
        struct Page {
            data: Vec<Item>,
            has_more: bool,
            last_id: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(tag = "type")]
        enum Item {
            #[serde(rename = "message")]
            Message {
                id: Option<String>,
                role: String,
                turn_id: String,
                status: String,
                phase: Option<String>,
                content: Vec<Content>,
            },
            #[serde(other)]
            Other,
        }
        #[derive(Deserialize)]
        struct Content {
            #[serde(rename = "type")]
            kind: String,
            text: Option<String>,
        }
        let page: Page = serde_json::from_slice(bytes).map_err(|_| CloudError::InvalidResponse)?;
        if page.data.len() > 100
            || page.last_id.as_ref().is_some_and(|id| !identifier(id))
            || (page.has_more && page.last_id.is_none())
        {
            return Err(CloudError::InvalidResponse);
        }
        let mut result = Self {
            messages: Vec::new(),
            has_more: page.has_more,
            next_after: page.has_more.then_some(page.last_id).flatten(),
        };
        let mut total = 0usize;
        for item in page.data {
            let Item::Message {
                id,
                role,
                turn_id,
                status,
                phase,
                content,
            } = item
            else {
                continue;
            };
            if role != "assistant" {
                continue;
            }
            if content.len() > 100
                || !identifier(&turn_id)
                || !matches!(status.as_str(), "in_progress" | "completed" | "incomplete")
            {
                return Err(CloudError::InvalidResponse);
            }
            if id.as_ref().is_some_and(|id| !identifier(id))
                || phase
                    .as_ref()
                    .is_some_and(|v| !matches!(v.as_str(), "commentary" | "final_answer"))
            {
                return Err(CloudError::InvalidResponse);
            }
            let mut text = String::new();
            for content in content {
                if content.kind == "output_text" {
                    let part = content.text.ok_or(CloudError::InvalidResponse)?;
                    total = total
                        .checked_add(part.len())
                        .ok_or(CloudError::ResponseTooLarge(MAX_RESULT_TEXT_BYTES))?;
                    if total > MAX_RESULT_TEXT_BYTES {
                        return Err(CloudError::ResponseTooLarge(MAX_RESULT_TEXT_BYTES));
                    }
                    text.push_str(&part);
                }
            }
            if !text.is_empty() {
                result.messages.push(Message {
                    id,
                    turn_id,
                    status,
                    phase,
                    text,
                });
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
