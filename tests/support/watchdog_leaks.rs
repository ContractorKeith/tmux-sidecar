use super::*;
use crate::support::TmuxGuard;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

e2e_test!(panic_removes_scratch_socket_and_daemon, {
    if !crate::enabled() {
        return;
    }
    let saved = Arc::new(Mutex::new(None));
    let record = saved.clone();
    let result = std::panic::catch_unwind(move || {
        let guard = TmuxGuard::start();
        crate::lifecycle::list(&guard);
        let root = guard.scratch.path().to_owned();
        let command = guard.sidecar();
        for key in [
            "HOME",
            "TMUX_TMPDIR",
            "TMUX_SIDECAR_RUNTIME_DIR",
            "TMUX_SIDECAR_STATE_DIR",
        ] {
            let value = command
                .get_envs()
                .find(|(name, _)| *name == key)
                .unwrap()
                .1
                .unwrap();
            assert!(Path::new(value).starts_with(&root), "{key} escaped scratch");
        }
        assert!(guard.socket().starts_with(root.canonicalize().unwrap()));
        *record.lock().unwrap() = Some((
            root,
            server_pid(guard.scratch.path()).unwrap(),
            crate::lifecycle::pid(&guard),
        ));
        panic!("intentional fixture panic");
    });
    assert!(result.is_err());
    let (root, server, daemon) = saved.lock().unwrap().clone().unwrap();
    assert!(!root.exists());
    crate::support::cleanup::fixture(&root, Some(server), false);
    assert!(
        !root.exists(),
        "repeated cleanup resurrected the scratch tree"
    );
    assert!(!tmux_sidecar::proc::running(server as i32).unwrap());
    assert!(!tmux_sidecar::proc::running(daemon).unwrap());
});

e2e_test!(
    watchdog_removes_its_tree_without_stopping_another_fixture,
    {
        if !crate::enabled() {
            return;
        }
        std::thread::scope(|scope| {
            let (ready, started) = mpsc::channel();
            let (release, finish) = mpsc::channel();
            let other = scope.spawn(move || {
                let guard = TmuxGuard::start();
                ready
                    .send((
                        guard.scratch.path().to_owned(),
                        server_pid(guard.scratch.path()).unwrap(),
                    ))
                    .unwrap();
                let _ = finish.recv_timeout(Duration::from_secs(8));
            });
            let (other_root, other_pid) = started.recv_timeout(Duration::from_secs(5)).unwrap();
            let guard = TmuxGuard::start();
            crate::lifecycle::list(&guard);
            let root = guard.scratch.path().to_owned();
            let server = server_pid(&root).unwrap();
            let (resume, stalled) = mpsc::channel();
            let result = std::panic::catch_unwind(|| {
                run_with_timeout(
                    "owned fixture expiry",
                    Duration::from_millis(20),
                    true,
                    move || {
                        let _guard = guard;
                        let _ = stalled.recv_timeout(Duration::from_secs(5));
                    },
                )
            });
            assert!(result.is_err());
            assert!(!root.exists(), "watchdog left its scratch tree");
            assert!(!tmux_sidecar::proc::running(server as i32).unwrap());
            assert!(other_root.exists(), "watchdog removed another fixture");
            assert!(tmux_sidecar::proc::running(other_pid as i32).unwrap());
            let _ = resume.send(());
            release.send(()).unwrap();
            other.join().unwrap();
            assert!(!other_root.exists());
        });
    }
);
