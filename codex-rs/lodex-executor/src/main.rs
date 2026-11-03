//! Executor-only image for an off-device Codex harness. Startup, authentication,
//! framing and hidden helper dispatch stay in the upstream implementation.
fn main() -> anyhow::Result<()> {
    codex_exec_server_cli::run_standalone()
}
