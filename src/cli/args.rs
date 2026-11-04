use crate::model::{AgentKind, AgentState, RunSelector};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[arg(long, global = true)]
    pub tmux_socket: Option<PathBuf>,
    #[arg(short = 'q', global = true)]
    pub quiet: bool,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    List {
        #[arg(long)]
        group: Option<String>,
        #[arg(long, value_enum)]
        state: Option<AgentState>,
        #[arg(long)]
        json: bool,
    },
    Attention {
        #[arg(long)]
        json: bool,
    },
    Watch {
        #[arg(long)]
        json: bool,
    },
    Wait {
        sel: RunSelector,
        #[arg(long, value_enum)]
        until: WaitState,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    Send {
        sel: RunSelector,
        #[arg(allow_hyphen_values = true)]
        text: String,
        #[arg(long)]
        force: bool,
    },
    Ack {
        #[arg(required = true, num_args = 1..)]
        sel: Vec<RunSelector>,
    },
    History {
        sel: RunSelector,
        #[arg(long)]
        limit: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    Name {
        sel: RunSelector,
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        name: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    Group {
        #[command(subcommand)]
        command: Group,
    },
    Hooks {
        #[command(subcommand)]
        command: Hooks,
    },
    Hook {
        #[arg(value_enum)]
        agent: HookAgent,
        payload: Option<String>,
    },
    Daemon {
        #[command(subcommand)]
        command: Daemon,
    },
    Doctor {
        #[arg(long)]
        json: bool,
    },
    Ui,
    Mcp {
        #[arg(long)]
        allow_send: bool,
    },
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum WaitState {
    Waiting,
    Blocked,
    Done,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum HookAgent {
    Claude,
    Codex,
    CodexNotify,
    Grok,
    Opencode,
}
#[derive(Debug, Subcommand)]
pub enum Group {
    Add {
        group: String,
        #[arg(required = true, num_args = 1..)]
        sel: Vec<RunSelector>,
    },
    Rm {
        group: String,
        #[arg(required = true, num_args = 1..)]
        sel: Vec<RunSelector>,
    },
    Ls {
        #[arg(long)]
        json: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum Hooks {
    Install {
        #[arg(long)]
        chain: bool,
        #[arg(long, value_enum, action = clap::ArgAction::Append)]
        agent: Vec<AgentKind>,
        #[arg(long)]
        dry_run: bool,
    },
    Uninstall {
        #[arg(long, value_enum, action = clap::ArgAction::Append)]
        agent: Vec<AgentKind>,
        #[arg(long)]
        dry_run: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum Daemon {
    Status {
        #[arg(long)]
        json: bool,
    },
    Stop,
    Run,
}
