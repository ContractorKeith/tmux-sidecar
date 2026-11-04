use crate::support::BoundedCommand;
macro_rules! e2e_test {
    ($name:ident, $body:block) => {
        #[test]
        fn $name() {
            crate::support::watchdog::run(
                concat!(module_path!(), "::", stringify!($name)),
                move || $body,
            );
        }
    };
}
macro_rules! until {
    ($condition:expr) => {
        crate::support::wait_for(stringify!($condition), $condition)
    };
}
#[path = "support/hooks.rs"]
mod hooks;
#[path = "support/hooks_identity.rs"]
mod hooks_identity;
mod support;
use support::TmuxGuard;
use tmux_sidecar::seams::Tmux;
fn enabled() -> bool {
    if std::env::var("TMUX_SIDECAR_E2E").as_deref() == Ok("1") {
        true
    } else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI requires TMUX_SIDECAR_E2E=1"
        );
        eprintln!("skip: set TMUX_SIDECAR_E2E=1 for real tmux tests");
        false
    }
}
e2e_test!(real_adapter_sees_isolated_pane, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let panes = guard.adapter.list_panes().unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].pane_id, "%0");
    assert!(panes[0].pane_pid > 0);
    assert!(panes[0].server_start > 0);
});
e2e_test!(dropping_guard_kills_server, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let mut check = guard.command();
    assert!(
        guard
            .command()
            .arg("list-sessions")
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    drop(guard);
    assert!(
        !check
            .arg("list-sessions")
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
});

#[path = "support/lifecycle.rs"]
mod lifecycle;

#[path = "support/protocol.rs"]
mod protocol;

#[path = "support/resilience.rs"]
mod resilience;
#[path = "support/round1.rs"]
mod round1;

#[path = "support/shutdown.rs"]
mod shutdown;
