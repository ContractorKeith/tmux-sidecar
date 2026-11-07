//! M0: CLI grammar; handlers arrive in the indicated milestones.
mod args;
mod execute;
mod output;
mod watch;
pub use args::*;
pub use execute::execute;

impl Command {
    pub fn pending(&self) -> (&'static str, u8) {
        match self {
            Self::List { .. } => ("list", 1),
            Self::Attention { .. } => ("attention", 4),
            Self::Watch { .. } => ("watch", 4),
            Self::Wait { .. } => ("wait", 4),
            Self::Send { .. } => ("send", 4),
            Self::Ack { .. } => ("ack", 4),
            Self::History { .. } => ("history", 4),
            Self::Name { .. } => ("name", 4),
            Self::Group { command } => (
                match command {
                    Group::Add { .. } => "group add",
                    Group::Rm { .. } => "group rm",
                    Group::Ls { .. } => "group ls",
                },
                4,
            ),
            Self::Hooks { command } => (
                match command {
                    Hooks::Install { .. } => "hooks install",
                    Hooks::Uninstall { .. } => "hooks uninstall",
                },
                2,
            ),
            Self::Hook { .. } => ("hook", 2),
            Self::Daemon { command } => (
                match command {
                    Daemon::Status { .. } => "daemon status",
                    Daemon::Stop => "daemon stop",
                    Daemon::Run => "daemon run",
                },
                1,
            ),
            Self::Doctor { .. } => ("doctor", 7),
            Self::Ui => ("ui", 5),
            Self::Mcp { .. } => ("mcp", 6),
        }
    }
}
