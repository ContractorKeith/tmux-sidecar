use super::{TmuxGuard, enabled};
use serde_json::{Value, json};
use std::{fs, path::Path};
fn frame(run: &Value, nonce: u32) -> Value {
    json!({"jsonrpc":"2.0","method":"signal","params":{"server_start":run["server_start"],"pane_id":run["pane_id"],"agent":run["agent"],"agent_pid":run["agent_pid"],"at":1000,"nonce":format!("{nonce:032x}"),"source":"hook:claude","event":"Stop","detail":"end_turn","agent_session_id":null,"cwd":"/fixture","transcript_path":null,"payload":{"hook_event_name":"Stop"}}})
}
fn history(guard: &TmuxGuard) -> Vec<Value> {
    tmux_sidecar::store::Store::open(&guard.scratch.path().join("state/tmux-sidecar.db"))
        .unwrap()
        .signal_history()
        .unwrap()
}
e2e_test!(
    corrupt_spool_is_quarantined_and_genuine_duplicate_payloads_survive,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start();
        let runs = super::lifecycle::list(&guard);
        super::lifecycle::stop(&guard);
        let first = frame(&runs[0], 1);
        let second = frame(&runs[0], 2);
        let bytes = format!("{{\"half-written\":\n{first}\nnot json\n{first}\n{second}\n{{\"tail");
        fs::write(guard.scratch.path().join("state/spool.ndjson"), bytes).unwrap();
        assert_eq!(super::lifecycle::list(&guard).as_array().unwrap().len(), 1);
        let rejected =
            fs::read_to_string(guard.scratch.path().join("state/spool.rejected")).unwrap();
        assert!(rejected.contains("half-written"));
        assert!(rejected.contains("not json"));
        assert!(rejected.contains("tail"));
        super::lifecycle::stop(&guard);
        let signals = history(&guard);
        assert_eq!(signals.len(), 2);
        assert_eq!(signals[0]["event"], "Stop");
        assert_eq!(signals[0]["detail"], "end_turn");
    }
);
e2e_test!(
    five_megabyte_post_tool_use_is_stored_with_bounded_preview,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start();
        let runs = super::lifecycle::list(&guard);
        let pid = runs[0]["agent_pid"].as_u64().unwrap();
        let payload = guard.scratch.path().join("large.json");
        fs::write(&payload,serde_json::to_vec(&json!({"hook_event_name":"PostToolUse","tool_name":"Bash","session_id":"large-fake","cwd":"/fixture","tool_response":{"stdout":"x".repeat(5*1024*1024)}})).unwrap()).unwrap();
        super::hooks::fire(&guard, pid, "claude", &payload);
        until!(|| tmux_sidecar::hook::spool::drained(&guard.scratch.path().join("state")).unwrap());
        super::lifecycle::stop(&guard);
        let signals = history(&guard);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0]["event"], "PostToolUse");
        assert_eq!(signals[0]["detail"], "Bash");
        let raw = signals[0]["payload_json"].as_str().unwrap();
        assert!(raw.len() < 256 * 1024);
        assert!(raw.contains("…truncated"));
    }
);
e2e_test!(
    cached_context_avoids_slow_tmux_and_linux_ps_on_the_hook_path,
    {
        if !enabled() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let guard = TmuxGuard::start();
        let runs = super::lifecycle::list(&guard);
        let pid = runs[0]["agent_pid"].as_u64().unwrap();
        assert!(guard.scratch.path().join("run/server_start").exists());
        let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join("tmux"))
            .find(|p| p.is_file())
            .unwrap();
        let script = guard.scratch.path().join("bin/tmux");
        fs::write(
            &script,
            format!("#!/bin/sh\n[ \"$3\" != display-message ] || touch \"$HOME/hook-probe\"\nsleep 0.4\nexec '{}' \"$@\"\n", real.display()),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        #[cfg(target_os = "linux")]
        {
            let ps = guard.scratch.path().join("bin/ps");
            fs::write(&ps, "#!/bin/sh\nsleep 0.4\nexec /bin/ps \"$@\"\n").unwrap();
            fs::set_permissions(ps, fs::Permissions::from_mode(0o700)).unwrap();
        }
        super::hooks::fire(
            &guard,
            pid,
            "claude",
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks/claude/Stop.json"),
        );
        until!(|| tmux_sidecar::hook::spool::drained(&guard.scratch.path().join("state")).unwrap());
        assert!(
            !guard.scratch.path().join("home/hook-probe").exists(),
            "hook forked tmux despite cached context"
        );
        super::lifecycle::stop(&guard);
        assert_eq!(history(&guard).len(), 1);
    }
);
e2e_test!(symlinked_spool_leaves_target_untouched_and_logs_loss, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let runs = super::lifecycle::list(&guard);
    super::lifecycle::stop(&guard);
    let target = guard.scratch.path().join("target");
    fs::write(&target, "original").unwrap();
    std::os::unix::fs::symlink(&target, guard.scratch.path().join("state/spool.ndjson")).unwrap();
    super::hooks::fire(
        &guard,
        runs[0]["agent_pid"].as_u64().unwrap(),
        "claude",
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks/claude/Stop.json"),
    );
    assert_eq!(fs::read_to_string(target).unwrap(), "original");
    assert!(
        fs::read_to_string(guard.scratch.path().join("state/hook.log"))
            .unwrap()
            .contains("signal lost")
    );
});
e2e_test!(
    rejected_file_errors_do_not_stop_startup_or_count_as_poll_failures,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start();
        let _ = super::lifecycle::list(&guard);
        super::lifecycle::stop(&guard);
        let target = guard.scratch.path().join("target");
        fs::write(&target, "original").unwrap();
        std::os::unix::fs::symlink(&target, guard.scratch.path().join("state/spool.rejected"))
            .unwrap();
        fs::write(
            guard.scratch.path().join("state/spool.ndjson"),
            "not json\n",
        )
        .unwrap();
        let _ = super::lifecycle::list(&guard);
        let pid = super::lifecycle::pid(&guard);
        let started = std::time::Instant::now();
        until!(|| {
            let _ = super::lifecycle::list(&guard);
            started.elapsed() > std::time::Duration::from_millis(800)
        });
        assert_eq!(super::lifecycle::pid(&guard), pid);
        assert_eq!(fs::read_to_string(target).unwrap(), "original");
        super::lifecycle::stop(&guard);
    }
);
