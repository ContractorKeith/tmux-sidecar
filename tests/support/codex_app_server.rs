use super::{TmuxGuard, enabled, lifecycle::list, support::output};
use std::{fs, path::Path};
e2e_test!(
    codex_app_server_hooks_belong_to_tui_and_beat_done_transcript,
    {
        if !enabled() {
            return;
        }
        let g = TmuxGuard::start();
        assert!(
            output(
                g.command()
                    .args(["respawn-pane", "-k", "-t", "%0"])
                    .arg(g.scratch.path().join("bin/codex"))
                    .arg("--app-server")
            )
            .status
            .success()
        );
        until!(|| g.scratch.path().join("state/plugin-host-pid").exists());
        until!(|| list(&g)[0]["agent"] == "codex");
        let run = list(&g)[0].clone();
        let child: u64 = fs::read_to_string(g.scratch.path().join("state/plugin-host-pid"))
            .unwrap()
            .parse()
            .unwrap();
        assert_ne!(run["agent_pid"].as_u64().unwrap(), child);
        // Stop is a compatibility payload; current Codex installations use Interrupt/notify.
        let stop = g.scratch.path().join("codex-stop.json");
        let mut payload: serde_json::Value = serde_json::from_slice(
            &fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/hooks/codex/PermissionRequest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        payload["hook_event_name"] = "Stop".into();
        fs::write(&stop, serde_json::to_vec(&payload).unwrap()).unwrap();
        for (format, fixture, state) in [
            ("codex", "codex/PermissionRequest", "blocked"),
            ("codex", "codex/Stop", "waiting"),
            ("codex", "codex/PermissionRequest", "blocked"),
            ("codex-notify", "codex/notify", "waiting"),
        ] {
            let path = if fixture == "codex/Stop" {
                stop.clone()
            } else {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("tests/fixtures/hooks/{fixture}.json"))
            };
            super::hooks::fire(&g, child, format, &path);
            until!(|| list(&g)[0]["state"] == state);
            if state == "blocked" {
                super::cli_verbs::cli(&g, &["send", "%0", "refused"], 3);
            }
        }
        super::lifecycle::stop(&g);
        // Exercise the same store seam used by completed scanner results after hook delivery.
        let store =
            tmux_sidecar::store::Store::open(&g.scratch.path().join("state/tmux-sidecar.db"))
                .unwrap();
        let reading = serde_json::from_value(serde_json::json!({"id":"codex:fixture","agent":"codex","cwd":run["cwd"],"project_root":run["cwd"],"title":"fixture","started_at":1,"last_activity_at":3,"status":"done","last_user_text":"","last_assistant_text":"answer","transcript_path":"/fixture.jsonl","resume_hint":"","pane":null})).unwrap();
        store
            .transcript(
                run["id"].as_str().unwrap(),
                &reading,
                tmux_sidecar::daemon::now(),
            )
            .unwrap();
        drop(store);
        let active = list(&g);
        assert_eq!(active.as_array().unwrap().len(), 1);
        assert_eq!(active[0]["id"], run["id"]);
        assert_eq!(active[0]["state"], "waiting");
        assert!(active[0]["ended_at"].is_null());
    }
);

e2e_test!(remain_on_exit_ends_blocked_run, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let run = list(&g)[0].clone();
    super::cli_verbs::fire(&g, &run, "PermissionRequest");
    assert!(
        output(
            g.command()
                .args(["set-window-option", "remain-on-exit", "on"])
        )
        .status
        .success()
    );
    let pid = run["agent_pid"].as_i64().unwrap() as i32;
    // SAFETY: positive pid belongs to the isolated fake agent.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    until!(|| list(&g).as_array().unwrap().is_empty());
    let all = super::state::rpc(&g, "list", serde_json::json!({"all":true}));
    assert_eq!(all[0]["state"], "done");
    assert!(all[0]["ended_at"].is_number());
    assert!(
        super::state::rpc(&g, "attention", serde_json::json!({}))
            .as_array()
            .unwrap()
            .is_empty()
    );
});
