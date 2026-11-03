use clap::Parser;

#[derive(Parser)]
#[command(
    name = "lodex-profile",
    version,
    about = "Local-only Lodex profile storage"
)]
struct Cli {
    #[command(flatten)]
    args: codex_lodex_local::cli::ProfileArgs,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Cli::parse().args.run()
}
