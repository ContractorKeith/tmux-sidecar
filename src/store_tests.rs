use super::*;
use crate::{
    engine,
    model::AgentState,
    seams::{PaneSnapshot, Process},
};
#[test]
fn migrates_reopens_wal_and_preserves_history_and_pane_names() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("test.db");
    let store = Store::open(&db).unwrap();
    let panes = [PaneSnapshot {
        server_start: 42,
        pane_id: "%0".into(),
        pane_pid: 12,
        cwd: "/work".into(),
        current_command: "claude".into(),
        pane_active: false,
        window_active: false,
        session_attached: false,
    }];
    let processes = [(
        12,
        Process {
            ppid: 1,
            comm: "claude".into(),
        },
    )]
    .into();
    let run = engine::detect(&panes, &processes, &[], 100).start.remove(0);
    let id = store.insert(&run).unwrap();
    store.name_pane(42, "%0", Some("test")).unwrap();
    // Reopen while the writer remains open: committed data must be read from WAL.
    assert!(db.with_extension("db-wal").exists());
    let reopened = Store::open(&db).unwrap();
    assert_eq!(reopened.list().unwrap()[0].id, id);
    assert_eq!(
        reopened.pane_name(42, "%0").unwrap().as_deref(),
        Some("test")
    );
    assert!(reopened.get(RunSelector::Pane(0), 43).unwrap().is_none());
    assert!(reopened.get(RunSelector::Pane(0), 42).unwrap().is_some());
    reopened.end(&id, 200).unwrap();
    assert!(reopened.get(RunSelector::Pane(0), 42).unwrap().is_none());
    let ended = reopened.get(RunSelector::Run(1), 42).unwrap().unwrap();
    assert_eq!(ended.state, AgentState::Done);
    assert_eq!(ended.ended_at, Some(200));
    assert_eq!(reopened.insert(&run).unwrap(), "r2");
    assert_eq!(reopened.active().unwrap().len(), 1);
    reopened.end_all(300).unwrap();
    assert!(reopened.active().unwrap().is_empty());
    assert!(
        reopened
            .list()
            .unwrap()
            .iter()
            .all(|r| r.ended_at.is_some())
    );
}

#[test]
fn wal_crash_writer() {
    let Some(db) = std::env::var_os("TSX_CRASH_DB") else {
        return;
    };
    let db = std::path::PathBuf::from(db);
    let store = Store::open(&db).unwrap();
    store.name_pane(10, "%3", Some("survives crash")).unwrap();
    std::fs::write(db.with_extension("ready"), b"ready").unwrap();
    loop {
        std::thread::park_timeout(std::time::Duration::from_secs(60));
    }
}
#[test]
fn recovers_committed_wal_after_writer_is_killed() {
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("crash.db");
    let mut child = ChildGuard(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "store::tests::wal_crash_writer", "--nocapture"])
            .env("TSX_CRASH_DB", &db)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !db.with_extension("ready").exists() {
        assert!(std::time::Instant::now() < deadline);
        assert!(child.0.try_wait().unwrap().is_none());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(db.with_extension("db-wal").exists());
    let recovered = Store::open(&db).unwrap();
    assert_eq!(
        recovered.pane_name(10, "%3").unwrap().as_deref(),
        Some("survives crash")
    );
}
