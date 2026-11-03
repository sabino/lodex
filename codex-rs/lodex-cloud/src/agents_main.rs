use clap::Parser;
use codex_http_client::{HttpClientFactory, OutboundProxyPolicy};
#[derive(Parser)]
#[command(
    name = "lodex-agents",
    version,
    about = "Stateless API-key-only hosted agent controller; HTTP environment configuration"
)]
struct Cli {
    #[command(flatten)]
    args: codex_lodex_cloud::api_cli::ApiArgs,
}
#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match cli
        .args
        .run_with_factory("http_environment", async {
            Ok(HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault))
        })
        .await
    {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
