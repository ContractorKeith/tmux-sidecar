use super::{TmuxGuard, enabled, lifecycle::list, support::output};
use serde_json::json;
use std::fs;
e2e_test!(
    claude_interruption_transcript_clears_a_hooked_permission_block,
    {
        if !enabled() {
            return;
        }
        let g = TmuxGuard::start();
        let run = list(&g)[0].clone();
        let dir = g.scratch.path().join("home/.claude/projects/fixture");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("interrupted.jsonl");
        let permission = g.scratch.path().join("permission.json");
        fs::write(&permission,json!({"hook_event_name":"PermissionRequest","session_id":"interrupted","cwd":run["cwd"],"transcript_path":path}).to_string()).unwrap();
        super::hooks::fire(
            &g,
            run["agent_pid"].as_u64().unwrap(),
            "claude",
            &permission,
        );
        until!(|| list(&g)[0]["state"] == "blocked");
        let blocked_at = list(&g)[0]["state_since"].as_u64().unwrap();
        // session-board timestamps have second precision; interruption must follow the hook.
        until!(|| tmux_sidecar::daemon::now() / 1000 > blocked_at / 1000);
        let timestamp = String::from_utf8(
            output(std::process::Command::new("date").args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])).stdout,
        )
        .unwrap();
        let record = json!({"type":"user","sessionId":"interrupted","cwd":run["cwd"],"timestamp":timestamp.trim(),"message":{"role":"user","content":"[Request interrupted by user]"}});
        fs::write(path, format!("{record}\n")).unwrap();
        until!(|| list(&g)[0]["state"] == "waiting");
        assert_eq!(list(&g)[0]["state_source"], "transcript");
    }
);
e2e_test!(
    completed_transcript_keeps_a_live_unhooked_run_waiting_then_resumes,
    {
        if !enabled() {
            return;
        }
        let g = TmuxGuard::start();
        let run = list(&g)[0].clone();
        let dir = g.scratch.path().join("home/.claude/projects/fixture");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("complete.jsonl");
        let timestamp = String::from_utf8(
            output(std::process::Command::new("date").args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])).stdout,
        )
        .unwrap();
        let record = json!({"type":"assistant","sessionId":"complete","cwd":run["cwd"],"timestamp":timestamp.trim(),"message":{"role":"assistant","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"}});
        fs::write(&path, format!("{record}\n")).unwrap();
        until!(|| list(&g)[0]["state_source"] == "transcript");
        assert_eq!(list(&g)[0]["state"], "waiting");
        assert_eq!(
            super::state::rpc(&g, "attention", json!({}))[0]["id"],
            run["id"]
        );
        let record = json!({"type":"user","sessionId":"complete","cwd":run["cwd"],"timestamp":timestamp.trim(),"message":{"role":"user","content":"next turn"}});
        use std::io::Write;
        writeln!(
            fs::OpenOptions::new().append(true).open(&path).unwrap(),
            "{record}"
        )
        .unwrap();
        until!(|| list(&g)[0]["state"] == "working");
    }
);
