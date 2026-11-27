mod args;
mod execute;
mod hooks;
mod input;
mod output;
mod watch;
pub use args::*;
pub use execute::execute;

pub(crate) use execute::stdout_error;
