//! In-memory seam adapters for unit and integration tests.
use super::*;
use std::cell::{Cell, RefCell};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Paste(String, String),
    Enter(String),
    Display(String, String),
}
#[derive(Default)]
pub struct FakeTmux {
    pub panes: RefCell<Vec<PaneSnapshot>>,
    pub actions: RefCell<Vec<Action>>,
}
impl Tmux for FakeTmux {
    fn list_panes(&self) -> io::Result<Vec<PaneSnapshot>> {
        Ok(self.panes.borrow().clone())
    }
    fn paste_text(&self, pane: &str, text: &str) -> io::Result<()> {
        self.actions
            .borrow_mut()
            .push(Action::Paste(pane.into(), text.into()));
        Ok(())
    }
    fn send_enter(&self, pane: &str) -> io::Result<()> {
        self.actions.borrow_mut().push(Action::Enter(pane.into()));
        Ok(())
    }
    fn display_message(&self, pane: &str, text: &str) -> io::Result<()> {
        self.actions
            .borrow_mut()
            .push(Action::Display(pane.into(), text.into()));
        Ok(())
    }
}
#[derive(Default)]
pub struct FakeProcessTree {
    pub processes: RefCell<ProcessSnapshot>,
}
impl ProcessTree for FakeProcessTree {
    fn snapshot(&self) -> io::Result<ProcessSnapshot> {
        Ok(self.processes.borrow().clone())
    }
}
pub struct FakeClock(pub Cell<SystemTime>);
impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tmux_records_actions() {
        let tmux = FakeTmux::default();
        assert!(tmux.list_panes().unwrap().is_empty());
        tmux.paste_text("%7", "hello").unwrap();
        tmux.send_enter("%7").unwrap();
        tmux.display_message("%7", "waiting").unwrap();
        assert_eq!(
            *tmux.actions.borrow(),
            vec![
                Action::Paste("%7".into(), "hello".into()),
                Action::Enter("%7".into()),
                Action::Display("%7".into(), "waiting".into())
            ]
        );
    }
    #[test]
    fn process_ancestors_stop_at_cycles() {
        let tree = FakeProcessTree::default();
        tree.processes.borrow_mut().extend([
            (
                2,
                Process {
                    ppid: 1,
                    comm: "codex".into(),
                },
            ),
            (
                1,
                Process {
                    ppid: 2,
                    comm: "sh".into(),
                },
            ),
        ]);
        assert_eq!(
            tree.ancestors(2).unwrap(),
            vec![(
                1,
                Process {
                    ppid: 2,
                    comm: "sh".into()
                }
            )]
        );
        assert!(tree.ancestors(99).unwrap().is_empty());
    }
    #[test]
    fn clock_can_advance() {
        let clock = FakeClock(Cell::new(SystemTime::UNIX_EPOCH));
        clock
            .0
            .set(clock.now() + std::time::Duration::from_secs(42));
        assert_eq!(
            clock
                .now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            42
        );
    }
}
