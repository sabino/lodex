//! Headless entry point. Reuse upstream protocol and authentication implementations.
#![recursion_limit = "256"]

use clap::Parser;
use codex_app_server::{AppServerRuntimeOptions, AppServerTransport};
use codex_app_server_daemon::{LifecycleCommand, RemoteControlMode};
use codex_arg0::{Arg0DispatchPaths, arg0_dispatch_or_else};
use codex_config::LoaderOverrides;
use codex_utils_cli::CliConfigOverrides;
use std::path::PathBuf;

#[path = "../../cli/src/cloud_config.rs"]
mod cloud_config;
#[path = "../../cli/src/login.rs"]
mod login;
#[path = "../../cli/src/mcp_cmd.rs"]
mod mcp_cmd;
#[path = "../../cli/src/mcp_login.rs"]
mod mcp_login;
#[path = "../../cli/src/remote_control_cmd.rs"]
mod remote_control_cmd;

// MCP administration needs this auth helper, not the plugin-management CLI.
mod plugin_cmd {
    pub(crate) async fn load_cli_auth_manager(
        config: &codex_core::config::Config,
    ) -> anyhow::Result<std::sync::Arc<codex_login::AuthManager>> {
        Ok(codex_login::AuthManager::shared_from_config(config, true).await?)
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "lodex",
    version,
    about = "Headless Codex runtime",
    arg_required_else_help = true
)]
struct Cli {
    #[command(flatten)]
    config_overrides: CliConfigOverrides,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    AppServer(ServerArgs),
    RemoteControl(remote_control_cmd::RemoteControlCommand),
    Mcp(mcp_cmd::McpCli),
    Exec(codex_exec::Cli),
    ExecServer(codex_exec_server_cli::ExecServerCommand),
    Profile(codex_lodex_local::cli::ProfileArgs),
    Cloud(codex_lodex_cloud::cli::CloudArgs),
    Login(LoginArgs),
    Logout,
}

#[derive(Debug, clap::Args)]
struct ServerArgs {
    #[command(flatten)]
    startup_config: codex_app_server::StartupConfigArgs,
    #[command(flatten)]
    code_mode_host: codex_app_server::AppServerCodeModeHostArgs,
    #[arg(long)]
    cloud_skills_snapshot: Option<PathBuf>,
    #[arg(long, default_value = AppServerTransport::DEFAULT_LISTEN_URL)]
    listen: AppServerTransport,
    #[arg(long, conflicts_with = "listen")]
    stdio: bool,
    #[arg(long)]
    managed_daemon: bool,
    #[arg(long)]
    remote_control: bool,
    #[arg(long)]
    strict_config: bool,
    #[arg(long)]
    analytics_default_enabled: bool,
    #[command(flatten)]
    auth: codex_websocket_auth::WebsocketAuthArgs,
    #[command(subcommand)]
    command: Option<ServerCommand>,
}

#[derive(Debug, clap::Subcommand)]
enum ServerCommand {
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
}

#[derive(Debug, clap::Subcommand)]
enum DaemonCommand {
    Start,
    Stop,
    Restart,
    Version,
    Bootstrap {
        #[arg(long)]
        remote_control: bool,
    },
    EnableRemoteControl,
    DisableRemoteControl,
    #[command(hide = true)]
    PidUpdateLoop {
        #[arg(long)]
        check_package_ownership: bool,
        #[arg(long)]
        restore_release: Option<String>,
    },
}

#[derive(Debug, clap::Args)]
struct LoginArgs {
    #[arg(long, conflicts_with = "with_api_key")]
    device_auth: bool,
    #[arg(long)]
    with_api_key: bool,
    #[command(subcommand)]
    command: Option<LoginCommand>,
}

#[derive(Debug, clap::Subcommand)]
enum LoginCommand {
    Status,
}

fn main() -> anyhow::Result<()> {
    #[cfg(all(target_os = "linux", target_arch = "mips"))]
    if std::env::var_os("LODEX_MMAP_READAHEAD").as_deref() != Some(std::ffi::OsStr::new("normal")) {
        let report = codex_lodex_platform::advise_executable_random_access();
        if std::env::var_os("LODEX_MEMORY_DIAGNOSTICS").is_some() {
            eprintln!(
                "lodex memory advice: {} mappings, {} failures",
                report.regions, report.failures
            );
        }
    }
    let remote_disabled = codex_app_server::take_remote_control_disabled_env();
    arg0_dispatch_or_else(move |paths| async move {
        let cli = Cli::parse();
        match cli.command {
            Command::AppServer(args) => {
                run_server(args, paths, cli.config_overrides, remote_disabled).await
            }
            Command::RemoteControl(args) => {
                remote_control_cmd::run(args, paths, cli.config_overrides).await
            }
            Command::Mcp(mut args) => {
                args.config_overrides
                    .raw_overrides
                    .splice(0..0, cli.config_overrides.raw_overrides);
                args.run(LoaderOverrides::default()).await
            }
            Command::Exec(mut args) => {
                args.config_overrides
                    .raw_overrides
                    .splice(0..0, cli.config_overrides.raw_overrides);
                codex_exec::run_main(args, paths).await
            }
            Command::ExecServer(args) => args.run(&paths, &cli.config_overrides).await,
            Command::Profile(args) => tokio::task::spawn_blocking(move || args.run())
                .await?
                .map_err(anyhow::Error::from_boxed),
            Command::Cloud(args) => args
                .run(cli.config_overrides)
                .await
                .map_err(anyhow::Error::from_boxed),
            Command::Login(args) => match args.command {
                Some(LoginCommand::Status) => login::run_login_status(cli.config_overrides).await,
                None if args.with_api_key => {
                    let key = login::read_api_key_from_stdin();
                    login::run_login_with_api_key(cli.config_overrides, key).await
                }
                None => login::run_login_with_device_code(cli.config_overrides, None, None).await,
            },
            Command::Logout => login::run_logout(cli.config_overrides).await,
        }
    })
}

async fn run_server(
    args: ServerArgs,
    paths: Arg0DispatchPaths,
    overrides: CliConfigOverrides,
    remote_disabled: bool,
) -> anyhow::Result<()> {
    if let Some(ServerCommand::Daemon { command }) = args.command {
        anyhow::ensure!(
            !args.startup_config.print_startup_config && !args.strict_config,
            "startup config flags cannot be combined with daemon commands"
        );
        return run_daemon(command).await;
    }
    if args.startup_config.print_startup_config {
        return codex_app_server::print_startup_config(
            paths,
            overrides,
            LoaderOverrides::default(),
            args.strict_config,
            args.startup_config,
        )
        .await
        .map_err(Into::into);
    }
    let transport = if args.stdio {
        AppServerTransport::Stdio
    } else {
        args.listen
    };
    if args.managed_daemon && matches!(transport, AppServerTransport::UnixSocket { .. }) {
        let _ = codex_app_server_daemon::maintain_stderr_log();
    }
    let options = AppServerRuntimeOptions {
        cloud_skills_snapshot: args.cloud_skills_snapshot,
        code_mode_host_transport: args.code_mode_host.into(),
        managed_daemon: args.managed_daemon,
        remote_control_startup_mode: match (args.remote_control, remote_disabled) {
            (true, _) => codex_app_server::RemoteControlStartupMode::EnabledEphemeral,
            (false, true) => codex_app_server::RemoteControlStartupMode::DisabledEphemeral,
            _ => codex_app_server::RemoteControlStartupMode::ResolvePersisted,
        },
        ..Default::default()
    };
    let exit = codex_app_server::run_main_with_transport_options(
        paths,
        overrides,
        LoaderOverrides::default(),
        args.strict_config,
        args.analytics_default_enabled,
        transport,
        codex_protocol::protocol::SessionSource::VSCode,
        args.auth.try_into_settings()?,
        options,
    )
    .await?;
    if exit == codex_app_server::AppServerExit::Forced {
        std::process::exit(0);
    }
    Ok(())
}

async fn run_daemon(command: DaemonCommand) -> anyhow::Result<()> {
    match command {
        DaemonCommand::Bootstrap { remote_control } => {
            let output =
                codex_app_server_daemon::bootstrap(codex_app_server_daemon::BootstrapOptions {
                    remote_control_enabled: remote_control,
                })
                .await?;
            println!("{}", serde_json::to_string(&output)?);
        }
        DaemonCommand::EnableRemoteControl | DaemonCommand::DisableRemoteControl => {
            let mode = if matches!(command, DaemonCommand::EnableRemoteControl) {
                RemoteControlMode::Enabled
            } else {
                RemoteControlMode::Disabled
            };
            println!(
                "{}",
                serde_json::to_string(&codex_app_server_daemon::set_remote_control(mode).await?)?
            );
        }
        DaemonCommand::PidUpdateLoop {
            check_package_ownership,
            restore_release,
        } => {
            if !check_package_ownership {
                let config = cloud_config::load_config(
                    &CliConfigOverrides::default(),
                    LoaderOverrides::default(),
                )
                .await?;
                codex_app_server_daemon::run_pid_update_loop(
                    config.http_client_factory(),
                    restore_release,
                )
                .await?;
            }
        }
        other => {
            let operation = match other {
                DaemonCommand::Start => LifecycleCommand::Start,
                DaemonCommand::Stop => LifecycleCommand::Stop,
                DaemonCommand::Restart => LifecycleCommand::Restart {
                    feature_overrides: Default::default(),
                },
                DaemonCommand::Version => LifecycleCommand::Version,
                _ => unreachable!(),
            };
            println!(
                "{}",
                serde_json::to_string(&codex_app_server_daemon::run(operation).await?)?
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn headless_command_schema_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn remote_executor_cli_preserves_transport_auth_and_concurrency_options() {
        let cli = Cli::try_parse_from([
            "lodex",
            "exec-server",
            "--listen",
            "ws://127.0.0.1:0",
            "--concurrent-requests",
            "2",
            "--ws-auth",
            "capability-token",
            "--ws-token-file",
            "/tmp/lodex-executor-fixture.token",
        ])
        .expect("upstream executor CLI");
        assert!(matches!(cli.command, Command::ExecServer(_)));
    }

    #[test]
    fn managed_daemon_reexec_arguments_are_accepted() {
        let cli = Cli::try_parse_from([
            "codex",
            "app-server",
            "--managed-daemon",
            "--listen",
            "unix:///tmp/lodex-control.sock",
            "--remote-control",
            "--analytics-default-enabled",
            "-c",
            "features.apps=false",
        ])
        .expect("upstream daemon launch arguments");
        let Command::AppServer(args) = cli.command else {
            panic!("app server")
        };
        assert!(args.managed_daemon && args.remote_control);
        assert!(args.analytics_default_enabled);
        assert!(
            cli.config_overrides
                .raw_overrides
                .contains(&"features.apps=false".to_string())
        );
        assert!(matches!(args.listen, AppServerTransport::UnixSocket { .. }));
    }

    #[test]
    fn mcp_stdio_and_http_configuration_remain_available() {
        for args in [
            vec!["lodex", "mcp", "add", "local", "--", "/usr/bin/example-mcp"],
            vec![
                "lodex",
                "mcp",
                "add",
                "remote",
                "--url",
                "https://example.com/mcp",
            ],
        ] {
            let cli = Cli::try_parse_from(args).expect("supported upstream MCP transport");
            assert!(matches!(cli.command, Command::Mcp(_)));
        }
    }

    #[test]
    fn incompatible_stdio_transport_flags_fail() {
        assert!(
            Cli::try_parse_from(["lodex", "app-server", "--stdio", "--listen", "off",]).is_err()
        );
    }

    #[test]
    fn no_terminal_ui_is_started_implicitly() {
        let err = Cli::try_parse_from(["lodex"]).expect_err("explicit headless command required");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }
}
