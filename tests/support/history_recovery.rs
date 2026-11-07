use super::{enabled, lifecycle, support::TmuxGuard};
use crate::support::BoundedCommand;

e2e_test!(history_deletion_after_clean_stop_is_reported_once, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let notice = "history database was deleted; a new one was created";
    let first = g
        .sidecar()
        .args(["list", "--json"])
        .bounded_output()
        .unwrap();
    assert!(first.status.success());
    assert!(!String::from_utf8_lossy(&first.stderr).contains(notice));
    lifecycle::stop(&g);
    std::fs::remove_file(g.scratch.path().join("state/tmux-sidecar.db")).unwrap();
    let recreated = g
        .sidecar()
        .args(["list", "--json"])
        .bounded_output()
        .unwrap();
    assert!(recreated.status.success());
    assert!(
        String::from_utf8_lossy(&recreated.stderr).contains(notice),
        "{recreated:?}"
    );
    let again = g
        .sidecar()
        .args(["list", "--json"])
        .bounded_output()
        .unwrap();
    assert!(again.status.success());
    assert!(
        !String::from_utf8_lossy(&again.stderr).contains(notice),
        "{again:?}"
    );
    let log = std::fs::read_to_string(g.scratch.path().join("state/daemon.log")).unwrap();
    assert_eq!(log.matches(notice).count(), 1, "{log}");
});
