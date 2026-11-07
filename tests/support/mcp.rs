use super::lifecycle::{list, stop};
use super::mcp_client::Mcp;
use super::{TmuxGuard, enabled};
use serde_json::json;
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn fire(guard: &TmuxGuard, run: &serde_json::Value, event: &str) {
    super::hooks::fire(
        guard,
        run["agent_pid"].as_u64().unwrap(),
        "claude",
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/hooks/claude/{event}.json")),
    );
}

e2e_test!(mcp_stdio_tools_read_fixtures_and_guard_registration, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let run = list(&guard)[0].clone();
    let mut mcp = Mcp::start(&guard, false);
    let tools = mcp.request("tools/list", json!({}));
    let names: Vec<_> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["list_runs", "attention", "run_history", "wait_for_state"]
    );
    fire(&guard, &run, "Notification-permission_prompt");
    until!(|| list(&guard)[0]["state"] == "blocked");
    assert_eq!(
        mcp.json("list_runs", json!({"state":"blocked"})),
        list(&guard)
    );
    assert_eq!(mcp.json("list_runs", json!({"state":"working"})), json!([]));
    super::state::rpc(&guard, "group.add", json!({"group":"mcp","sel":run["id"]}));
    assert_eq!(
        mcp.json("list_runs", json!({"group":"manual:mcp"}))[0]["id"],
        run["id"]
    );
    assert_eq!(mcp.json("attention", json!({}))[0]["id"], run["id"]);
    let history = mcp.json("run_history", json!({"selector":run["id"],"limit":1}));
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(
        mcp.call("run_history", json!({"selector":"r999999"}))["isError"],
        true
    );
    // A tool absent from discovery is also unavailable by direct invocation.
    let id = mcp.begin(
        "tools/call",
        json!({"name":"send_prompt","arguments":{"selector":run["id"],"text":"no"}}),
    );
    let response = mcp.receive();
    assert_eq!(response["id"], id);
    assert_eq!(response["error"]["code"], -32602);
    let mut writable = Mcp::start(&guard, true);
    assert!(
        writable.request("tools/list", json!({}))["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "send_prompt")
    );
    assert_eq!(
        writable.call(
            "send_prompt",
            json!({"selector":run["id"],"text":"no","force":true})
        )["isError"],
        true
    );
    drop(writable);
    drop(mcp);
    stop(&guard);
});

e2e_test!(mcp_wait_timeout_and_transition, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let run = list(&guard)[0].clone();
    let mut mcp = Mcp::start(&guard, false);
    assert_eq!(
        mcp.json(
            "wait_for_state",
            json!({"selector":run["id"],"state":"waiting","timeout":1})
        )["run_id"],
        run["id"]
    );
    fire(&guard, &run, "UserPromptSubmit");
    until!(|| list(&guard)[0]["state"] == "working");
    let zero = mcp.call(
        "wait_for_state",
        json!({"selector":run["id"],"state":"waiting","timeout":0}),
    );
    assert_eq!(zero["isError"], true);
    assert!(zero.to_string().contains("-32004"), "{zero}");
    let start = Instant::now();
    let result = mcp.call(
        "wait_for_state",
        json!({"selector":run["id"],"state":"waiting","timeout":1}),
    );
    assert_eq!(result["isError"], true, "{result}");
    assert!(result.to_string().contains("-32004"), "{result}");
    assert!(start.elapsed() >= Duration::from_secs(1));
    assert!(start.elapsed() < Duration::from_secs(4));
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(250));
            fire(&guard, &run, "Stop");
        });
        assert_eq!(
            mcp.json(
                "wait_for_state",
                json!({"selector":run["id"],"state":"waiting","timeout":3})
            )["state"],
            "waiting"
        );
    });
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(250));
            fire(&guard, &run, "SessionEnd");
        });
        let ended = mcp.call(
            "wait_for_state",
            json!({"selector":run["id"],"state":"blocked","timeout":3}),
        );
        assert_eq!(ended["isError"], true);
        assert!(ended.to_string().contains("ended in done"), "{ended}");
        assert!(ended.to_string().contains("-32001"), "{ended}");
    });
    drop(mcp);
    stop(&guard);
});

e2e_test!(mcp_send_delivers_only_to_waiting_run, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start_capture();
    let run = list(&guard)[0].clone();
    let mut mcp = Mcp::start(&guard, true);
    mcp.json(
        "send_prompt",
        json!({"selector":run["id"],"text":"mcp-delivered"}),
    );
    let input = guard
        .scratch
        .path()
        .join(format!("state/input-{}", run["agent_pid"]));
    let delivered = b"\x1b[200~mcp-delivered\x1b[201~\r";
    until!(|| std::fs::read(&input).unwrap() == delivered);
    fire(&guard, &run, "UserPromptSubmit");
    until!(|| list(&guard)[0]["state"] == "working");
    let result = mcp.call(
        "send_prompt",
        json!({"selector":run["id"],"text":"mcp-refused"}),
    );
    assert_eq!(result["isError"], true);
    assert!(result.to_string().contains("working"), "{result}");
    assert!(result.to_string().contains("-32003"), "{result}");
    assert_eq!(std::fs::read(&input).unwrap(), delivered);
    drop(mcp);
    stop(&guard);
});
