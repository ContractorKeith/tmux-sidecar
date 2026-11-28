use crate::model::{AgentKind, AgentState, RunSelector};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("TMUX_SIDECAR_BUILD_ID"), ")"), about)]
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
    #[command(about = "List agent runs on this tmux server")]
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
    #[command(about = "Show agent runs that need attention")]
    Attention {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Stream agent state changes")]
    Watch {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Wait for an agent run to reach a state")]
    Wait {
        sel: RunSelector,
        #[arg(long, value_enum)]
        until: WaitState,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Paste a prompt into an agent run")]
    #[command(
        after_help = "Use - to read text from stdin. For text starting with a hyphen, use --: send %0 -- -text"
    )]
    Send {
        sel: RunSelector,
        text: String,
        #[arg(long)]
        force: bool,
    },
    #[command(about = "Mark attention items read")]
    Ack {
        #[arg(required = true, num_args = 1..)]
        sel: Vec<RunSelector>,
    },
    #[command(about = "Show signals and state changes")]
    History {
        sel: RunSelector,
        #[arg(long)]
        limit: Option<u32>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Set or clear a pane name")]
    Name {
        sel: RunSelector,
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        name: Option<String>,
        #[arg(long)]
        clear: bool,
    },
    #[command(about = "Manage manual pane groups")]
    Group {
        #[command(subcommand)]
        command: Group,
    },
    #[command(about = "Install or remove agent hooks")]
    Hooks {
        #[command(subcommand)]
        command: Hooks,
    },
    #[command(about = "Receive an agent hook payload")]
    #[command(disable_help_flag = true)]
    Hook {
        #[arg(
            default_value = "unknown",
            allow_hyphen_values = true,
            help = "Payload format: claude, codex, codex-notify, grok, opencode"
        )]
        agent: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        payload: Vec<String>,
    },
    #[command(about = "Manage the per-tmux-server daemon")]
    Daemon {
        #[command(subcommand)]
        command: Daemon,
    },
    #[command(about = "Check tmux, hooks and daemon health")]
    Doctor {
        #[arg(long)]
        json: bool,
        #[arg(long, value_enum, action = clap::ArgAction::Append)]
        agent: Vec<AgentKind>,
    },
    #[command(about = "Open the terminal interface")]
    Ui,
    #[command(about = "Serve MCP over standard input and output")]
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
    #[command(about = "Add panes to a manual group")]
    Add {
        group: String,
        #[arg(required = true, num_args = 1..)]
        sel: Vec<RunSelector>,
    },
    #[command(about = "Remove panes from a manual group")]
    Rm {
        group: String,
        #[arg(required = true, num_args = 1..)]
        sel: Vec<RunSelector>,
    },
    #[command(about = "List manual groups")]
    Ls {
        #[arg(long)]
        json: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum Hooks {
    #[command(about = "Install marked hook entries")]
    Install {
        #[arg(long)]
        chain: bool,
        #[arg(long, value_enum, action = clap::ArgAction::Append)]
        agent: Vec<AgentKind>,
        #[arg(long)]
        dry_run: bool,
    },
    #[command(about = "Remove marked hook entries")]
    Uninstall {
        #[arg(long, value_enum, action = clap::ArgAction::Append)]
        agent: Vec<AgentKind>,
        #[arg(long)]
        dry_run: bool,
    },
}
#[derive(Debug, Subcommand)]
pub enum Daemon {
    #[command(about = "Show daemon status")]
    Status {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Stop the daemon")]
    Stop,
    #[command(about = "Run the daemon in the foreground")]
    Run,
}
