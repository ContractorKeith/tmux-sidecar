//! Restore the terminal before printing panic diagnostics or returning on a signal.
use crossterm::{execute, terminal};
use std::{io, panic::PanicHookInfo, sync::Arc};
type Hook = Arc<dyn Fn(&PanicHookInfo<'_>) + Send + Sync>;
pub(super) fn restore() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        terminal::LeaveAlternateScreen,
        crossterm::cursor::Show
    );
}
pub(super) struct Restore(Hook);
impl Restore {
    pub fn enter() -> io::Result<Self> {
        let previous: Hook = std::panic::take_hook().into();
        let chain = previous.clone();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            chain(info);
        }));
        let guard = Self(previous);
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), terminal::EnterAlternateScreen)?;
        Ok(guard)
    }
}
impl Drop for Restore {
    fn drop(&mut self) {
        restore();
        // set_hook is forbidden while unwinding; the installed hook already chained.
        if !std::thread::panicking() {
            let previous = self.0.clone();
            std::panic::set_hook(Box::new(move |info| previous(info)));
        }
    }
}
#[cfg(test)]
#[path = "terminal_tests.rs"]
mod tests;
