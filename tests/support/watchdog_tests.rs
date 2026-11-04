use super::*;
e2e_test!(watchdog_timeout_fixture, {
    if std::env::var_os("TSX_WATCHDOG_FIXTURE").is_none() {
        return;
    }
    let g = crate::support::TmuxGuard::start();
    crate::lifecycle::list(&g);
    run_with_timeout(
        "intentional watchdog fixture",
        Duration::from_millis(20),
        true,
        || {
            std::thread::sleep(Duration::from_millis(200));
        },
    );
});
e2e_test!(watchdog_dumps_owned_tree_and_log_before_failing, {
    use crate::support::BoundedCommand;
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "support::watchdog::tests::watchdog_timeout_fixture",
            "--nocapture",
        ])
        .env("TSX_WATCHDOG_FIXTURE", "1")
        .env("TMUX_SIDECAR_E2E", "1");
    let output = command.bounded_output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    for expected in [
        "intentional watchdog fixture",
        "scratch ps:",
        "daemon.log tail:",
        "did not finish",
    ] {
        assert!(error.contains(expected), "missing {expected}: {error}");
    }
});

e2e_test!(watchdog_fails_a_stalled_body_with_its_name, {
    let (send, receive) = mpsc::channel();
    let result = std::panic::catch_unwind(|| {
        run_with_timeout(
            "stalled fixture",
            Duration::from_millis(20),
            false,
            move || {
                let _ = receive.recv_timeout(Duration::from_secs(1));
            },
        )
    });
    let _ = send.send(());
    let error = result.unwrap_err();
    let text = error.downcast_ref::<String>().unwrap();
    assert!(text.contains("stalled fixture") && text.contains("did not finish"));
});

e2e_test!(scratch_tree_excludes_unrelated_agents, {
    use tmux_sidecar::seams::Process;
    let snapshot = [
        (
            10,
            Process {
                ppid: 1,
                comm: "e2e".into(),
            },
        ),
        (
            11,
            Process {
                ppid: 10,
                comm: "claude".into(),
            },
        ),
        (
            20,
            Process {
                ppid: 1,
                comm: "tmux".into(),
            },
        ),
        (
            21,
            Process {
                ppid: 20,
                comm: "codex".into(),
            },
        ),
        (
            99,
            Process {
                ppid: 1,
                comm: "claude".into(),
            },
        ),
    ]
    .into();
    assert_eq!(
        descendants(&BTreeSet::from([10, 20]), &snapshot),
        BTreeSet::from([10, 11, 20, 21])
    );
});
