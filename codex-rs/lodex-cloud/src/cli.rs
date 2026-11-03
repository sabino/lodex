use crate::{CloudClient, CloudError, validate_backend};
use clap::{Args, Subcommand};
use codex_core::config::Config;
use codex_lodex_policy::{CloudTask, MAX_TASK_BYTES, encode_cloud_submission, read_cloud_task};
use codex_login::AuthManager;
use codex_utils_cli::CliConfigOverrides;
use std::io::{self, Read, Write};

#[derive(Debug, Args)]
pub struct CloudArgs {
    #[command(subcommand)]
    command: CloudCommand,
}

#[derive(Subcommand)]
enum CloudCommand {
    /// Describe the pinned adapter; does not load credentials or make a request.
    Capabilities,
    /// API-key-backed hosted agent sessions; credentials are separate from ChatGPT.
    Api(crate::api_cli::ApiArgs),
    /// List available environments; --repo limits discovery to OWNER/REPOSITORY.
    Environments {
        #[arg(long)]
        repo: Option<String>,
    },
    /// Submit one explicit JSON task from bounded stdin; git_ref is required.
    Submit,
    /// Noninteractive Codex-style submission; branch is explicit, one attempt.
    Exec {
        #[arg(long = "env")]
        environment: String,
        #[arg(long)]
        branch: String,
        #[arg(long="attempts",default_value="1",value_parser=clap::value_parser!(u8).range(1..=1))]
        _attempts: u8,
        query: Option<String>,
    },
    /// Read minimal task status, discarding transcript and diff fields.
    Status { task_id: String },
    /// Retrieve bounded assistant messages, without the original prompt or diff.
    Result { task_id: String },
    /// Report unsupported cancellation; never pretend a running task stopped.
    Cancel { task_id: String },
}

impl std::fmt::Debug for CloudCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Capabilities => "Capabilities",
            Self::Api(..) => "Api",
            Self::Environments { .. } => "Environments",
            Self::Submit => "Submit",
            Self::Exec { .. } => "Exec { <selected task redacted> }",
            Self::Status { .. } => "Status",
            Self::Result { .. } => "Result",
            Self::Cancel { .. } => "Cancel",
        })
    }
}

fn output(value: &impl serde::Serialize) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}

impl CloudArgs {
    pub async fn run(
        self,
        overrides: CliConfigOverrides,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let CloudCommand::Api(args) = self.command {
            return args
                .run_with_factory("codex", async move {
                    let config = Config::load_with_cli_overrides(
                        overrides
                            .parse_overrides()
                            .map_err(|_| CloudError::InvalidApiSubmission)?,
                    )
                    .await
                    .map_err(|_| CloudError::ApiAuthentication)?;
                    Ok(config.http_client_factory())
                })
                .await;
        }
        if matches!(self.command, CloudCommand::Capabilities) {
            return output(
                &serde_json::json!({"adapter":"codex_cli_cloud_tasks", "new_task":true,
                "cancellation":false,"thread_migration":false,"automatic_profile_export":false,
                "max_task_bytes":32768,"max_result_text_bytes":crate::MAX_RESULT_TEXT_BYTES}),
            );
        }
        if let CloudCommand::Cancel { task_id } = &self.command {
            if !super::identifier(task_id) {
                return Err(CloudError::InvalidIdentifier.into());
            }
            return Err(CloudError::CancellationUnavailable.into());
        }
        // Validate the entire selected payload before resolving credentials.
        let task = match &self.command {
            CloudCommand::Submit => Some(read_cloud_task(io::stdin().lock())?),
            CloudCommand::Exec {
                environment,
                branch,
                query,
                ..
            } => {
                let text = if let Some(query) = query {
                    query.clone()
                } else {
                    let mut bytes = Vec::new();
                    io::stdin()
                        .lock()
                        .take((MAX_TASK_BYTES + 1) as u64)
                        .read_to_end(&mut bytes)?;
                    if bytes.len() > MAX_TASK_BYTES {
                        return Err(CloudError::InvalidSubmission.into());
                    }
                    String::from_utf8(bytes).map_err(|_| CloudError::InvalidSubmission)?
                };
                Some(CloudTask::new(
                    environment.clone(),
                    Some(branch.clone()),
                    text,
                )?)
            }
            _ => None,
        };
        if let Some(task) = task.as_ref() {
            encode_cloud_submission(task)?;
        }
        let config = Config::load_with_cli_overrides(
            overrides
                .parse_overrides()
                .map_err(|_| CloudError::InvalidSubmission)?,
        )
        .await
        .map_err(|_| CloudError::Authentication)?;
        let base = validate_backend(&config.chatgpt_base_url)?;
        let manager = AuthManager::shared_from_config(&config, false)
            .await
            .map_err(|_| CloudError::Authentication)?;
        let auth = manager.auth().await.ok_or(CloudError::Authentication)?;
        if !auth.uses_codex_backend() {
            return Err(CloudError::Authentication.into());
        }
        let provider = codex_model_provider::auth_provider_from_auth_manager(manager, &auth);
        let client = CloudClient::new(base.as_str(), config.http_client_factory(), provider)?;
        match self.command {
            CloudCommand::Environments { repo } => output(
                &serde_json::json!({"environments":client.environments(repo.as_deref()).await?}),
            ),
            CloudCommand::Submit | CloudCommand::Exec { .. } => {
                let task = task.ok_or(CloudError::InvalidSubmission)?;
                output(&serde_json::json!({"task_id":client.submit(&task).await?}))
            }
            CloudCommand::Status { task_id } => output(&client.status(&task_id).await?),
            CloudCommand::Result { task_id } => output(&client.result(&task_id).await?),
            CloudCommand::Capabilities | CloudCommand::Cancel { .. } | CloudCommand::Api(..) => {
                unreachable!("handled before auth")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[derive(Parser)]
    struct Fixture {
        #[command(flatten)]
        args: CloudArgs,
    }
    #[test]
    fn explicit_exec_schema_and_debug_do_not_expose_query() {
        let arguments = [
            "fixture",
            "exec",
            "--env",
            "env_fixture",
            "--branch",
            "lodex",
            "private-cli-query-canary",
        ];
        let fixture = Fixture::try_parse_from(arguments).expect("explicit schema");
        assert!(!format!("{:?}", fixture.args).contains("private-cli-query-canary"));
        assert!(
            Fixture::try_parse_from(["fixture", "exec", "--env", "env_fixture", "query"]).is_err()
        );
        assert!(
            Fixture::try_parse_from([
                "fixture",
                "exec",
                "--env",
                "env_fixture",
                "--branch",
                "lodex",
                "--attempts",
                "2",
                "query"
            ])
            .is_err()
        );
    }
}
