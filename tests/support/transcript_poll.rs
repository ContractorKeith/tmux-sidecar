use super::lifecycle::{list, stop};
use super::state::rpc;
use super::{TmuxGuard, enabled};
use crate::support::{ChildGuard, output};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};
struct Release(PathBuf);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = fs::write(self.0.join("scan-release"), b"");
    }
}
e2e_test!(slow_transcript_scan_does_not_delay_process_exit_poll, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let gate = Release(guard.scratch.path().join("home"));
    let _daemon = ChildGuard::new(
        guard
            .sidecar()
            .env("TMUX_SIDECAR_TEST_SCAN_GATE", &gate.0)
            .args(["daemon", "run"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    until!(|| gate.0.join("scan-started").exists());
    let run = list(&guard)[0].clone();
    assert!(
        output(guard.command().args(["new-window", "-d", "sleep 60"]))
            .status
            .success()
    );
    assert!(
        output(guard.command().args(["kill-pane", "-t", "%0"]))
            .status
            .success()
    );
    let deadline = Instant::now() + Duration::from_millis(900);
    loop {
        let runs = rpc(&guard, "list", json!({"all":true}));
        if runs
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == run["id"] && r["state"] == "done")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "exit detection waited for transcript scan"
        );
        std::thread::sleep(Duration::from_millis(15));
    }
    fs::write(gate.0.join("scan-release"), b"").unwrap();
    stop(&guard);
});
