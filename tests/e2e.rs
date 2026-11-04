mod support;
use support::TmuxGuard;
use tmux_sidecar::seams::Tmux;
fn enabled() -> bool {
    if std::env::var("TMUX_SIDECAR_E2E").as_deref() == Ok("1") {
        true
    } else {
        eprintln!("skip: set TMUX_SIDECAR_E2E=1 for real tmux tests");
        false
    }
}
#[test]
fn real_adapter_sees_isolated_pane() {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let panes = guard.adapter.list_panes().unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].pane_id, "%0");
    assert!(panes[0].pane_pid > 0);
    assert!(panes[0].server_start > 0);
}
#[test]
fn dropping_guard_kills_server() {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let mut check = guard.command();
    assert!(
        guard
            .command()
            .arg("list-sessions")
            .output()
            .unwrap()
            .status
            .success()
    );
    drop(guard);
    assert!(
        !check
            .arg("list-sessions")
            .output()
            .unwrap()
            .status
            .success()
    );
}
