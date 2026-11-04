#![cfg_attr(not(test), warn(clippy::unwrap_used))]
use clap::Parser;
use std::process::ExitCode;
use tmux_sidecar::cli::{self, Cli};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli::execute(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, error)) => {
            eprintln!("tmux-sidecar: {error}");
            ExitCode::from(code)
        }
    }
}
