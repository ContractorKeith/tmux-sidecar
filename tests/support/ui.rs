use super::lifecycle::{list, stop};
use super::{TmuxGuard, enabled};
use crate::support::{ChildGuard, output};
use std::process::Stdio;
fn screen(guard: &TmuxGuard, pane: &str) -> String {
    String::from_utf8(output(guard.command().args(["capture-pane", "-p", "-t", pane])).stdout)
        .unwrap()
}
e2e_test!(
    ui_subscribes_refreshes_reconnects_and_enter_switches_pane,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start();
        let run = list(&guard)[0].clone();
        assert!(
            output(
                guard
                    .command()
                    .args(["split-window", "-t", "fixture", "sleep 60"])
            )
            .status
            .success()
        );
        assert!(
            output(
                guard
                    .command()
                    .args(["new-session", "-d", "-s", "dashboard", "sleep 60"])
            )
            .status
            .success()
        );
        let _client = ChildGuard::new(
            guard
                .command()
                .args(["-C", "attach-session", "-t", "dashboard"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let command = guard.sidecar();
        let socket = command
            .get_args()
            .nth(1)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let ui_command = format!(
            "{} --tmux-socket {} ui",
            tmux_sidecar::install::quote(env!("CARGO_BIN_EXE_tmux-sidecar")),
            tmux_sidecar::install::quote(&socket)
        );
        let out = output(
            guard
                .command()
                .args([
                    "new-window",
                    "-P",
                    "-F",
                    "#{pane_id}",
                    "-t",
                    "dashboard",
                    "-n",
                    "ui",
                ])
                .arg(ui_command),
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let pane = String::from_utf8(out.stdout).unwrap().trim().to_string();
        until!(|| screen(&guard, &pane).contains("claude"));
        super::state::rpc(
            &guard,
            "name",
            serde_json::json!({"sel":run["id"],"name":"renamed-live"}),
        );
        until!(|| screen(&guard, &pane).contains("renamed-live"));
        super::state::rpc(
            &guard,
            "group.add",
            serde_json::json!({"sel":run["id"],"group":"review-live"}),
        );
        output(guard.command().args(["send-keys", "-t", &pane, "g"]));
        until!(|| screen(&guard, &pane).contains("review-live"));
        super::state::rpc(
            &guard,
            "signal",
            serde_json::json!({"server_start":run["server_start"],"pane_id":run["pane_id"],"agent":"claude","agent_pid":run["agent_pid"],"at":tmux_sidecar::daemon::now(),"source":"hook:claude","nonce":"55555555555555555555555555555555","event":"PermissionRequest","cwd":run["cwd"],"payload":{}}),
        );
        until!(|| screen(&guard, &pane).contains("Attention (1)"));
        output(guard.command().args(["send-keys", "-t", &pane, "a"]));
        until!(|| screen(&guard, &pane).contains("Attention (0)"));
        let old_pid = super::lifecycle::pid(&guard);
        stop(&guard);
        until!(
            || std::fs::read_to_string(guard.scratch.path().join("run/daemon.pid"))
                .ok()
                .and_then(|p| p.parse::<i32>().ok())
                .is_some_and(|pid| pid != old_pid)
        );
        until!(|| screen(&guard, &pane).contains("claude")
            && !screen(&guard, &pane).contains("daemon restarting"));
        output(guard.command().args(["send-keys", "-t", &pane, "Enter"]));
        until!(|| String::from_utf8_lossy(
            &output(
                guard
                    .command()
                    .args(["display-message", "-p", "-t", "fixture", "#{pane_id}"])
            )
            .stdout
        )
        .trim()
            == run["pane_id"].as_str().unwrap());
        until!(|| String::from_utf8_lossy(
            &output(
                guard
                    .command()
                    .args(["list-clients", "-F", "#{session_name}"])
            )
            .stdout
        )
        .trim()
            == "fixture");
        stop(&guard);
    }
);
