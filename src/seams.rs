use std::{
    collections::{BTreeMap, HashSet},
    io,
    path::PathBuf,
    time::SystemTime,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSnapshot {
    pub server_start: u64,
    pub pane_id: String,
    pub pane_pid: u32,
    pub cwd: PathBuf,
    pub current_command: String,
    pub pane_active: bool,
    pub window_active: bool,
    pub session_attached: bool,
}

pub trait Tmux {
    fn list_panes(&self) -> io::Result<Vec<PaneSnapshot>>;
    fn scan(&self) -> io::Result<crate::tmux::PaneScan> {
        Ok(crate::tmux::PaneScan {
            panes: self.list_panes()?,
            skipped: Vec::new(),
        })
    }
    fn paste_text(&self, pane: &str, text: &str) -> io::Result<()>;
    fn send_enter(&self, pane: &str) -> io::Result<()>;
    fn display_message(&self, pane: &str, text: &str) -> io::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub ppid: u32,
    pub comm: String,
}
pub type ProcessSnapshot = BTreeMap<u32, Process>;

pub trait ProcessTree {
    fn cwd(&self, _pid: u32) -> io::Result<PathBuf> {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "process cwd unavailable",
        ))
    }
    fn snapshot(&self) -> io::Result<ProcessSnapshot>;
    /// Nearest parent first; one snapshot, stopping at missing parents or cycles.
    fn ancestors(&self, pid: u32) -> io::Result<Vec<(u32, Process)>> {
        let snapshot = self.snapshot()?;
        let mut seen = HashSet::from([pid]);
        let mut current = pid;
        let mut result = Vec::new();
        while let Some(process) = snapshot.get(&current) {
            let parent = process.ppid;
            if !seen.insert(parent) {
                break;
            }
            let Some(process) = snapshot.get(&parent) else {
                break;
            };
            result.push((parent, process.clone()));
            current = parent;
        }
        Ok(result)
    }
}

pub trait Clock {
    fn now(&self) -> SystemTime;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[cfg(feature = "test-util")]
pub mod fakes;

/// Prompt text cannot escape bracketed paste or synthesize terminal keys.
pub fn validate_prompt(text: &str) -> Result<(), &'static str> {
    if text.is_empty() {
        return Err("empty prompt");
    }
    if text.bytes().any(|b| b < 0x20 && b != b'\n' && b != b'\t') {
        return Err(
            "prompt contains a terminal control character; only newline and tab are allowed",
        );
    }
    Ok(())
}
