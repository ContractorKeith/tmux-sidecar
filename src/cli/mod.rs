//! M0: CLI grammar; handlers arrive in the indicated milestones.
mod args;
mod execute;
mod input;
mod output;
mod watch;
pub use args::*;
pub use execute::execute;

impl Command {
    pub fn pending(&self) -> (&'static str, u8) {
        match self {
            Self::Doctor { .. } => ("doctor", 7),
            Self::Ui => ("ui", 5),
            _ => unreachable!("implemented commands do not call pending"),
        }
    }
}
