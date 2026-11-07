use std::{
    cell::Cell,
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, mpsc},
    thread::ThreadId,
    time::Duration,
};
thread_local! { static OWNER: Cell<Option<ThreadId>> = const { Cell::new(None) }; }
fn owner() -> ThreadId {
    OWNER
        .with(|o| o.get())
        .unwrap_or_else(|| std::thread::current().id())
}
#[derive(Clone)]
pub(super) struct Fixture {
    pub root: PathBuf,
    pub server_pid: Option<u32>,
    owner: ThreadId,
}
#[derive(Default)]
struct State {
    fixtures: BTreeMap<PathBuf, Fixture>,
    children: BTreeMap<u32, ThreadId>,
    expired: HashSet<ThreadId>,
}
fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(Mutex::default)
}
pub fn register(root: &Path) {
    let mut state = state().lock().unwrap();
    if state.expired.contains(&owner()) {
        let _ = std::fs::remove_dir_all(root);
        drop(state);
        panic!("fixture registered after watchdog expiry");
    }
    state.fixtures.insert(
        root.to_owned(),
        Fixture {
            root: root.to_owned(),
            server_pid: None,
            owner: owner(),
        },
    );
}
pub fn server(root: &Path, pid: u32) {
    if let Some(fixture) = state().lock().unwrap().fixtures.get_mut(root) {
        fixture.server_pid = Some(pid);
    }
    std::fs::write(root.join("tmux.pid"), pid.to_string()).unwrap();
}
pub fn server_pid(root: &Path) -> Option<u32> {
    state()
        .lock()
        .unwrap()
        .fixtures
        .get(root)
        .and_then(|f| f.server_pid)
}
pub fn unregister(root: &Path) {
    state().lock().unwrap().fixtures.remove(root);
}
pub fn track(pid: u32) {
    state().lock().unwrap().children.insert(pid, owner());
}
pub fn untrack(pid: u32) {
    state().lock().unwrap().children.remove(&pid);
}
pub struct Registration(PathBuf);
impl Registration {
    pub fn new(root: &Path) -> Self {
        register(root);
        Self(root.to_owned())
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        unregister(&self.0);
    }
}
pub fn run(name: &'static str, body: impl FnOnce() + Send + 'static) {
    run_with_timeout(name, Duration::from_secs(60), true, body);
}
fn run_with_timeout(
    name: &'static str,
    timeout: Duration,
    diagnose: bool,
    body: impl FnOnce() + Send + 'static,
) {
    let scope = owner();
    let (send, receive) = mpsc::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            OWNER.with(|o| o.set(Some(scope)));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
            let _ = send.send(result);
        })
        .unwrap();
    match receive.recv_timeout(timeout) {
        Ok(Ok(())) => (),
        Ok(Err(error)) => std::panic::resume_unwind(error),
        Err(error) => {
            if diagnose {
                dump_and_cleanup(name, scope);
            }
            panic!("e2e watchdog: {name} did not finish within {timeout:?}: {error}");
        }
    }
}
fn dump_and_cleanup(name: &str, scope: ThreadId) {
    let (fixtures, children) = {
        let mut state = state().lock().unwrap();
        state.expired.insert(scope);
        let fixtures: Vec<_> = state
            .fixtures
            .values()
            .filter(|f| f.owner == scope)
            .cloned()
            .collect();
        let children: Vec<_> = state
            .children
            .iter()
            .filter_map(|(pid, owner)| (*owner == scope).then_some(*pid))
            .collect();
        (fixtures, children)
    };
    super::stage(&format!(
        "watchdog expired in {name}; dumping owned scratch trees"
    ));
    for fixture in &fixtures {
        let log = std::fs::read(fixture.root.join("state/daemon.log")).unwrap_or_default();
        super::stage(&format!(
            "{} daemon.log tail:\n{}",
            fixture.root.display(),
            String::from_utf8_lossy(&log[log.len().saturating_sub(8192)..])
        ));
        super::cleanup::fixture(&fixture.root, fixture.server_pid, true);
    }
    // Terminal/daemon roots have stopped gracefully before any remaining children escalate.
    if let Some(fixture) = fixtures.first() {
        super::cleanup::children(&fixture.root, &children);
    } else {
        super::cleanup::terminate(&children);
    }
    for fixture in fixtures {
        if let Err(e) = super::cleanup::remove_tree(&fixture.root)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            super::stage(&format!(
                "watchdog scratch removal {}: {e}",
                fixture.root.display()
            ));
        }
        unregister(&fixture.root);
    }
}
pub(super) fn descendants(
    roots: &std::collections::BTreeSet<u32>,
    snapshot: &tmux_sidecar::seams::ProcessSnapshot,
) -> std::collections::BTreeSet<u32> {
    let mut owned = roots.clone();
    loop {
        let before = owned.len();
        for (&pid, process) in snapshot {
            if owned.contains(&process.ppid) {
                owned.insert(pid);
            }
        }
        if before == owned.len() {
            return owned;
        }
    }
}
#[path = "watchdog_tests.rs"]
mod tests;

#[path = "watchdog_leaks.rs"]
mod leak_tests;
