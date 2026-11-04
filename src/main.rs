#![cfg_attr(not(test), warn(clippy::unwrap_used))]
use clap::Parser;
use std::process::ExitCode;
use tmux_sidecar::cli::Cli;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (verb, milestone) = cli.command.pending();
    eprintln!("tmux-sidecar: {verb} is not implemented yet (M{milestone})");
    ExitCode::from(2)
}
