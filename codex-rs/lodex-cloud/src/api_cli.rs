use crate::CloudError;
use clap::{Args, Subcommand};
use std::io::{self, Read, Write};

fn output(value: &impl serde::Serialize) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}

#[derive(Subcommand)]
enum ApiCommand {
    /// Describe API adapter without credentials or network access.
    Capabilities,
    /// Create a hosted or self-hosted session from explicit bounded JSON stdin.
    Create,
    /// Validate key/configuration setup without making an Agents API request.
    Check,
    /// Project session status; idle is not proof that a turn succeeded.
    Status { session_id: String },
    /// One page of assistant text, with an explicit continuation cursor.
    Result {
        session_id: String,
        #[arg(long)]
        after: Option<String>,
    },
    /// Submit a real cancellation event; acknowledgement is not a terminal outcome.
    Cancel { session_id: String },
}
#[derive(Args)]
pub struct ApiArgs {
    #[command(subcommand)]
    command: ApiCommand,
}
impl std::fmt::Debug for ApiArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiArgs { <redacted> }")
    }
}
impl ApiArgs {
    pub async fn run_with_factory(
        self,
        configuration: &'static str,
        factory: impl std::future::Future<
            Output = Result<codex_http_client::HttpClientFactory, CloudError>,
        >,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let command = self.command;
        if matches!(command, ApiCommand::Capabilities) {
            return output(
                &serde_json::json!({"adapter":"agents_api_v1","configuration":configuration,"credential":"OPENAI_API_KEY","environments":["none","openai_hosted","self_hosted"],"cancellation_event":true,"streaming":false,"thread_migration":false,"automatic_profile_export":false,"max_task_bytes":32768}),
            );
        }
        let create = if matches!(command, ApiCommand::Create) {
            let mut bytes = Vec::new();
            io::stdin().lock().take(65537).read_to_end(&mut bytes)?;
            if bytes.len() > 65536 {
                return Err(CloudError::InvalidApiSubmission.into());
            }
            let create: crate::agents::Create =
                serde_json::from_slice(&bytes).map_err(|_| CloudError::InvalidApiSubmission)?;
            create.encode()?;
            Some(create)
        } else {
            None
        };
        match &command {
            ApiCommand::Status { session_id }
            | ApiCommand::Cancel { session_id }
            | ApiCommand::Result { session_id, .. }
                if !super::identifier(session_id) =>
            {
                return Err(CloudError::InvalidIdentifier.into());
            }
            ApiCommand::Result {
                after: Some(after), ..
            } if !super::identifier(after) => return Err(CloudError::InvalidIdentifier.into()),
            _ => {}
        }
        let key = std::env::var("OPENAI_API_KEY").map_err(|_| CloudError::ApiAuthentication)?;
        let client = crate::agents::AgentsClient::new(&key, factory.await?)?;
        match command {
            ApiCommand::Check => output(
                &serde_json::json!({"configuration":configuration,"api_key_present":true,"credentials_verified":false,"agents_api_request_sent":false}),
            ),
            ApiCommand::Create => output(
                &client
                    .create(&create.ok_or(CloudError::InvalidApiSubmission)?)
                    .await?,
            ),
            ApiCommand::Status { session_id } => output(&client.status(&session_id).await?),
            ApiCommand::Result { session_id, after } => {
                output(&client.result(&session_id, after.as_deref()).await?)
            }
            ApiCommand::Cancel { session_id } => {
                client.cancel(&session_id).await?;
                output(
                    &serde_json::json!({"session_id":session_id,"cancel_event_accepted":true,"terminal_outcome_verified":false}),
                )
            }
            ApiCommand::Capabilities => unreachable!("handled before credentials"),
        }
    }
}
