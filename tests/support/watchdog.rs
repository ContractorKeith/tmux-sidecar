use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, mpsc},
    time::Duration,
};
#[derive(Clone)]
struct Fixture {
    root: PathBuf,
    server_pid: Option<u32>,
}
fn fixtures() -> &'static Mutex<BTreeMap<PathBuf, Fixture>> {
    static FIXTURES: OnceLock<Mutex<BTreeMap<PathBuf, Fixture>>> = OnceLock::new();
    FIXTURES.get_or_init(Mutex::default)
}
pub fn register(root: &Path) {
    fixtures().lock().unwrap().insert(
        root.to_owned(),
        Fixture {
            root: root.to_owned(),
            server_pid: None,
        },
    );
}
pub fn server(root: &Path, pid: u32) {
    fixtures().lock().unwrap().get_mut(root).unwrap().server_pid = Some(pid);
}
pub fn unregister(root: &Path) {
    fixtures().lock().unwrap().remove(root);
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
    let (send, receive) = mpsc::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
            let _ = send.send(result);
        })
        .unwrap();
    match receive.recv_timeout(timeout) {
        Ok(Ok(())) => (),
        Ok(Err(error)) => std::panic::resume_unwind(error),
        Err(error) => {
            if diagnose {
                dump_and_cleanup(name);
            }
            panic!("e2e watchdog: {name} did not finish within {timeout:?}: {error}");
        }
    }
}
fn dump_and_cleanup(name: &str) {
    let fixtures: Vec<_> = fixtures().lock().unwrap().values().cloned().collect();
    let mut roots = BTreeSet::from([std::process::id()]);
    super::stage(&format!(
        "watchdog expired in {name}; dumping owned scratch trees"
    ));
    for fixture in &fixtures {
        roots.extend(fixture.server_pid.filter(|pid| *pid > 1));
        if let Ok(pid) = std::fs::read_to_string(fixture.root.join("run/daemon.pid"))
            && let Ok(pid) = pid.parse::<u32>()
            && pid > 1
        {
            roots.insert(pid);
        }
        let log = std::fs::read(fixture.root.join("state/daemon.log")).unwrap_or_default();
        super::stage(&format!(
            "{} daemon.log tail:\n{}",
            fixture.root.display(),
            String::from_utf8_lossy(&log[log.len().saturating_sub(8192)..])
        ));
    }
    let (send, receive) = mpsc::channel();
    let snapshot_roots = roots.clone();
    std::thread::spawn(move || {
        let mut command = std::process::Command::new("/bin/ps");
        command.args(["-A", "-o", "pid=,ppid=,comm="]);
        let result = super::process::try_output(&mut command).map(|output| {
            let snapshot = tmux_sidecar::proc::parse(&String::from_utf8_lossy(&output.stdout));
            let owned = descendants(&snapshot_roots, &snapshot);
            for pid in &owned {
                if let Some(process) = snapshot.get(pid) {
                    super::stage(&format!(
                        "scratch ps: pid={pid} ppid={} comm={}",
                        process.ppid, process.comm
                    ));
                }
            }
            owned
        });
        let _ = send.send(result);
    });
    let owned = match receive.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(owned)) => owned,
        error => {
            super::stage(&format!(
                "watchdog ps snapshot failed or timed out: {error:?}"
            ));
            roots
        }
    };
    for pid in owned
        .into_iter()
        .rev()
        .filter(|pid| *pid != std::process::id() && *pid > 0)
    {
        if let Ok(pid) = i32::try_from(pid) {
            // SAFETY: pids are fixture roots or their descendants in the e2e process tree.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}
fn descendants(
    roots: &BTreeSet<u32>,
    snapshot: &tmux_sidecar::seams::ProcessSnapshot,
) -> BTreeSet<u32> {
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
