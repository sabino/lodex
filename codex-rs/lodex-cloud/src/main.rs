use clap::Parser;

#[derive(Parser)]
#[command(
    name = "lodex-cloud",
    version,
    about = "Bounded Codex cloud tasks and API-backed agent sessions"
)]
struct Cli {
    #[command(flatten)]
    config: codex_utils_cli::CliConfigOverrides,
    #[command(flatten)]
    args: codex_lodex_cloud::cli::CloudArgs,
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli.args.run(cli.config).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
