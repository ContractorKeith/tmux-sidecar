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
    /// List agent runs on this tmux server.
    List {
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long)]
        group: Option<String>,
        #[arg(long, value_enum)]
        state: Option<AgentState>,
        #[arg(long)]
        json: bool,
    },
    /// Show agent runs that need attention.
    Attention {
        #[arg(long)]
        json: bool,
    },
    /// Stream agent state changes.
    Watch {
        #[arg(long)]
        json: bool,
    },
    /// Wait for an agent run to reach a state.
    Wait {
        sel: RunSelector,
        #[arg(long, value_enum)]
        until: WaitState,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    /// Paste a prompt into an agent run.
    #[command(
        after_help = "Use - to read text from stdin. For text starting with a hyphen, use --: send %0 -- -text"
    )]
    Send {
        sel: RunSelector,
        text: String,
        #[arg(long)]
        force: bool,
    },
    /// Mark attention items read.
    Ack {
        #[arg(required = true, num_args = 1..)]
        sel: Vec<RunSelector>,
    },
    /// Show signals and state changes.
    History {
        sel: RunSelector,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        json: bool,
    },
    /// Set or clear a pane name.
    Name {
        sel: RunSelector,
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        name: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    /// Manage manual pane groups.
    Group {
        #[command(subcommand)]
        command: Group,
    },
    /// Install or remove agent hooks.
    Hooks {
        #[command(subcommand)]
        command: Hooks,
    },
    /// Receive an agent hook payload.
    Hook {
        #[arg(help = "Payload format: claude, codex, codex-notify, grok, opencode")]
        agent: String,
        #[arg(allow_hyphen_values = true)]
        payload: Option<String>,
    },
    /// Manage the per-tmux-server daemon.
    Daemon {
        #[command(subcommand)]
        command: Daemon,
    },
    /// Check tmux, hooks and daemon health.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Open the terminal interface.
    Ui,
    /// Serve MCP over standard input and output.
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
#[derive(Debug, Subcommand)]
pub enum Group {
    /// Add panes to a manual group.
    Add {
        group: String,
        #[arg(required = true, num_args = 1..)]
        sel: Vec<RunSelector>,
    },
    /// Remove panes from a manual group.
    Rm {
        group: String,
        #[arg(required = true, num_args = 1..)]
        sel: Vec<RunSelector>,
    },
    /// List manual groups.
    Ls {
        #[arg(long)]
        json: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum Hooks {
    /// Install marked hook entries.
    Install {
        #[arg(long)]
        chain: bool,
        #[arg(long, value_enum, action = clap::ArgAction::Append)]
        agent: Vec<AgentKind>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove marked hook entries.
    Uninstall {
        #[arg(long, value_enum, action = clap::ArgAction::Append)]
        agent: Vec<AgentKind>,
        #[arg(long)]
        dry_run: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum Daemon {
    /// Show daemon status.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Stop the daemon.
    Stop,
    /// Run the daemon in the foreground.
    Run,
}
