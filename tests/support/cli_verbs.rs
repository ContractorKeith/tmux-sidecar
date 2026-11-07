use super::{
    TmuxGuard, enabled,
    lifecycle::list,
    support::{ChildGuard, output},
};
use serde_json::Value;
use std::{fs, path::Path, process::Stdio};
pub(super) fn cli(guard: &TmuxGuard, args: &[&str], code: i32) -> String {
    let result = output(guard.sidecar().args(args));
    assert_eq!(
        result.status.code(),
        Some(code),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}
pub(super) fn fire(guard: &TmuxGuard, run: &Value, fixture: &str) {
    super::hooks::fire(
        guard,
        run["agent_pid"].as_u64().unwrap(),
        "claude",
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/hooks/claude/{fixture}.json")),
    );
}
e2e_test!(cli_verbs_and_every_exit_code, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start_capture();
    let run = list(&guard)[0].clone();
    let sel = run["id"].as_str().unwrap();
    fire(&guard, &run, "UserPromptSubmit");
    cli(&guard, &["send", sel, "refused"], 3);
    let input = guard
        .scratch
        .path()
        .join(format!("state/input-{}", run["agent_pid"]));
    assert_eq!(fs::read(&input).unwrap(), b"");
    cli(
        &guard,
        &["wait", sel, "--until", "waiting", "--timeout", "1"],
        4,
    );
    cli(&guard, &["wait", "r999999", "--until", "waiting"], 5);
    cli(&guard, &["history", "bad"], 2);
    cli(&guard, &["name", sel, "bad\nname"], 2);
    for args in [
        vec!["list"],
        vec!["list", "--json"],
        vec!["attention"],
        vec!["attention", "--json"],
        vec!["history", sel],
        vec!["history", sel, "--limit", "1", "--json"],
    ] {
        assert!(!cli(&guard, &args, 0).is_empty());
    }
    cli(&guard, &["name", sel, "build"], 0);
    cli(&guard, &["name", sel, "--clear"], 0);
    cli(&guard, &["group", "add", "test", sel, "%0"], 0);
    assert!(cli(&guard, &["group", "ls"], 0).contains("test"));
    assert_eq!(
        serde_json::from_str::<Value>(&cli(&guard, &["group", "ls", "--json"], 0)).unwrap()[0]["name"],
        "test"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&cli(&guard, &["list", "--group", "test", "--json"], 0))
            .unwrap()[0]["id"],
        sel
    );
    cli(&guard, &["group", "rm", "test", sel], 0);
    fire(&guard, &run, "Stop");
    cli(
        &guard,
        &["wait", sel, "--until", "waiting", "--timeout", "0"],
        0,
    );
    assert!(!cli(&guard, &["wait", sel, "--until", "waiting", "--json"], 0).is_empty());
    cli(&guard, &["ack", sel, "%0"], 0);
    assert_eq!(cli(&guard, &["attention", "--json"], 0).trim(), "[]");
    let prompt = "literal C-c and Enter\nsecond line";
    cli(&guard, &["send", sel, prompt], 0);
    until!(|| fs::read(&input).unwrap() == format!("\x1b[200~{prompt}\x1b[201~\r").as_bytes());
    assert_eq!(list(&guard)[0]["state"], "working");
    cli(&guard, &["send", sel, "forced", "--force"], 0);
    until!(|| fs::read(&input)
        .unwrap()
        .ends_with(b"\x1b[200~forced\x1b[201~\r"));
    // Done in a different state is a general error, not a timeout.
    output(guard.command().args(["new-window", "-d", "sleep 30"]));
    output(guard.command().args(["kill-pane", "-t", "%0"]));
    until!(|| list(&guard).as_array().unwrap().is_empty());
    cli(
        &guard,
        &["wait", sel, "--until", "waiting", "--timeout", "1"],
        1,
    );
    cli(&guard, &["wait", sel, "--until", "done"], 0);
    cli(&guard, &["daemon", "stop"], 0);
    let result = output(guard.sidecar().args(["daemon", "status"]));
    assert_eq!(result.status.code(), Some(6));
});
e2e_test!(cli_stdin_send_and_concurrent_guard, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start_capture();
    let run = list(&guard)[0].clone();
    fire(&guard, &run, "Stop");
    let mut senders = Vec::new();
    for _ in 0..2 {
        senders.push(ChildGuard(
            guard
                .sidecar()
                .args(["send", "%0", "literal Enter C-c"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        ));
    }
    for child in &mut senders {
        until!(|| child.0.try_wait().unwrap().is_some());
    }
    let mut codes: Vec<_> = senders
        .iter_mut()
        .map(|c| c.0.wait().unwrap().code().unwrap())
        .collect();
    codes.sort();
    assert_eq!(codes, vec![0, 3]);
    let path = guard.scratch.path().join("prompt.txt");
    fs::write(&path, "stdin\nC-c Enter").unwrap();
    let mut child = ChildGuard(
        guard
            .sidecar()
            .args(["send", "%0", "-", "--force"])
            .stdin(fs::File::open(path).unwrap())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    until!(|| child.0.try_wait().unwrap().is_some());
    assert!(child.0.wait().unwrap().success());
    let input = guard
        .scratch
        .path()
        .join(format!("state/input-{}", run["agent_pid"]));
    until!(|| fs::read(&input)
        .unwrap()
        .ends_with(b"\x1b[200~stdin\nC-c Enter\x1b[201~\r"));
    let buffers = output(guard.command().arg("list-buffers"));
    assert!(!String::from_utf8_lossy(&buffers.stdout).contains("tmux-sidecar-"));
    assert!(cli(&guard, &["-q", "list"], 0).is_empty());
});
