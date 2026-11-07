use super::lifecycle::{list, stop};
use super::{TmuxGuard, enabled};
use serde_json::json;
use std::{fs, path::Path};
fn count(guard: &TmuxGuard) -> usize {
    super::state::rpc(guard, "history", json!({"sel":"r2","limit":1000}))
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["source"] == "hook:claude")
        .count()
}
e2e_test!(
    two_hundred_parallel_hooks_survive_down_daemon_and_replay,
    true,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start();
        list(&guard);
        stop(&guard);
        let spawn_lock = tmux_sidecar::client::FileLock::try_acquire(
            &guard.scratch.path().join("run/spawn.lock"),
        )
        .unwrap();
        let mut burst = std::process::Command::new(guard.scratch.path().join("bin/claude"));
        guard.isolate(&mut burst);
        burst
            .env("TMUX", format!("{},0,0", guard.socket().display()))
            .env("TMUX_PANE", "%0")
            .args(["--hook-burst", env!("CARGO_BIN_EXE_tmux-sidecar")])
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks/claude/Stop.json"),
            );
        let result = crate::support::output(&mut burst);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let spool = guard.scratch.path().join("state/spool.ndjson");
        let text = fs::read_to_string(&spool).unwrap();
        assert_eq!(text.lines().filter(|s| !s.trim().is_empty()).count(), 200);
        // Simulate a crash after claim, before any signal commits. Recovery must replay it.
        fs::rename(
            &spool,
            guard.scratch.path().join("state/spool.replaying.99999"),
        )
        .unwrap();
        drop(spawn_lock);
        list(&guard);
        until!(|| count(&guard) == 200);
        // Replaying the same invocations again must not duplicate their receipts/history.
        fs::write(&spool, text).unwrap();
        until!(|| tmux_sidecar::hook::spool::drained(spool.parent().unwrap()).unwrap());
        assert_eq!(count(&guard), 200);
        stop(&guard);
        let log =
            fs::read_to_string(guard.scratch.path().join("state/hook.log")).unwrap_or_default();
        assert!(!log.contains("signal lost"), "{log}");
        assert!(
            !fs::read_dir(guard.scratch.path().join("state"))
                .unwrap()
                .any(|e| e
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("spool.replaying."))
        );
    }
);
