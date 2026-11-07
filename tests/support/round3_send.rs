use super::{TmuxGuard, enabled, lifecycle::list, support::output};
use serde_json::json;
use std::fs;
e2e_test!(notify_only_send_is_refused_in_cli_and_mcp, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start_capture();
    assert!(
        output(
            g.command()
                .args(["respawn-pane", "-k", "-t", "%0"])
                .arg(g.scratch.path().join("bin/codex"))
        )
        .status
        .success()
    );
    let run = list(&g)[0].clone();
    let fixture = g.scratch.path().join("notify.json");
    fs::write(
        &fixture,
        json!({"type":"agent-turn-complete","thread-id":"turn","last-assistant-message":"ok"})
            .to_string(),
    )
    .unwrap();
    super::hooks::fire(
        &g,
        run["agent_pid"].as_u64().unwrap(),
        "codex-notify",
        &fixture,
    );
    until!(|| list(&g)[0]["last_assistant_text"] == "ok"
        || output(g.sidecar().args(["history", "%0", "--json"]))
            .stdout
            .windows(19)
            .any(|s| s == b"agent-turn-complete"));
    assert_eq!(list(&g)[0]["state_source"], "hook:codex-notify");
    let out = output(g.sidecar().args(["send", "%0", "n"]));
    assert_eq!(out.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Codex notify can't see approval prompts")
    );
    let mut mcp = super::mcp_client::Mcp::start(&g, true);
    let reply = mcp.call("send_prompt", json!({"selector":"%0","text":"n"}));
    assert_eq!(reply["isError"], true);
    assert!(
        reply
            .to_string()
            .contains("Codex notify can't see approval prompts")
    );
    assert!(
        super::lifecycle::ok(output(g.sidecar().args(["attention", "--json"])))
            .as_array()
            .unwrap()
            .is_empty()
    );
});
e2e_test!(
    send_refuses_terminal_controls_before_any_paste_in_cli_and_mcp,
    {
        if !enabled() {
            return;
        }
        let g = TmuxGuard::start_capture();
        let run = list(&g)[0].clone();
        super::cli_verbs::fire(&g, &run, "Stop");
        let mut mcp = super::mcp_client::Mcp::start(&g, true);
        for text in [
            "paste\n\x1b[201~INJECTED\x03\rnext",
            "a\rb",
            "a\0b",
            "a\x07b",
        ] {
            if !text.contains('\0') {
                let out = output(g.sidecar().args(["send", "%0", text, "--force"]));
                assert_eq!(out.status.code(), Some(2), "{text:?}");
                assert!(String::from_utf8_lossy(&out.stderr).contains("control"));
            }
            let reply = mcp.call("send_prompt", json!({"selector":"%0","text":text}));
            assert_eq!(reply["isError"], true, "{reply}");
            assert!(reply.to_string().contains("control"), "{reply}");
        }
        assert!(
            fs::read(
                g.scratch
                    .path()
                    .join(format!("state/input-{}", run["agent_pid"]))
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            output(g.sidecar().args(["send", "%0", "allowed\ntext\t✓"]))
                .status
                .success()
        );
    }
);
e2e_test!(stop_uses_recorded_endpoint_and_is_idempotent, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    list(&g);
    let alternate = g.scratch.path().join("alternate-run");
    fs::create_dir(&alternate).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&alternate, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(alternate.join("sock"), b"unrelated stale endpoint").unwrap();
    let out = output(
        g.sidecar()
            .env("TMUX_SIDECAR_RUNTIME_DIR", &alternate)
            .args(["daemon", "stop"]),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = output(g.sidecar().args(["daemon", "stop"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("daemon not running"));
});
