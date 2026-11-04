#![cfg_attr(not(test), warn(clippy::unwrap_used))]
use clap::Parser;
use std::process::ExitCode;
use tmux_sidecar::cli::{self, Cli};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "hook") {
        tmux_sidecar::hook::entry(args.into_iter().skip(2).collect());
        return ExitCode::SUCCESS;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create runtime")
        .block_on(run())
}
async fn run() -> ExitCode {
    let cli = Cli::parse();
    let detached = std::env::var_os("TMUX_SIDECAR_DETACHED").is_some()
        && matches!(
            &cli.command,
            cli::Command::Daemon {
                command: cli::Daemon::Run
            }
        );
    match cli::execute(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, error)) => {
            if !detached {
                eprintln!("tmux-sidecar: {error}");
            }
            ExitCode::from(code)
        }
    }
}
