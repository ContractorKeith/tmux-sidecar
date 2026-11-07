use super::{
    TmuxGuard,
    cli_verbs::{cli, fire},
    enabled,
    lifecycle::list,
    support::ChildGuard,
};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    process::Stdio,
    sync::mpsc::{self, Receiver},
};
fn watch(guard: &TmuxGuard, json: bool) -> (ChildGuard, Receiver<String>) {
    let mut command = guard.sidecar();
    command.arg("watch");
    if json {
        command.arg("--json");
    }
    let mut child = ChildGuard::new(command.stdout(Stdio::piped()).spawn().unwrap());
    let stdout = child.0.stdout.take().unwrap();
    let (send, recv) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if send.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    (child, recv)
}
e2e_test!(cli_wait_and_watch_follow_committed_changes, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let run = list(&guard)[0].clone();
    fire(&guard, &run, "UserPromptSubmit");
    let (mut watch, json_lines) = watch(&guard, true);
    until!(|| {
        fire(&guard, &run, "UserPromptSubmit");
        fire(&guard, &run, "Stop");
        json_lines
            .try_iter()
            .any(|line| serde_json::from_str::<Value>(&line).unwrap()["to"] == "waiting")
    });
    let mut wait = ChildGuard::new(
        guard
            .sidecar()
            .args([
                "wait",
                "%0",
                "--until",
                "blocked",
                "--timeout",
                "10",
                "--json",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    until!(|| {
        fire(&guard, &run, "UserPromptSubmit");
        fire(&guard, &run, "PermissionRequest");
        wait.0.try_wait().unwrap().is_some()
    });
    assert!(wait.0.wait().unwrap().success());
    let mut output = String::new();
    use std::io::Read;
    wait.0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&output).unwrap()["state"],
        "blocked"
    );
    let (mut human, human_lines) = self::watch(&guard, false);
    until!(|| {
        fire(&guard, &run, "UserPromptSubmit");
        fire(&guard, &run, "PermissionRequest");
        human_lines.try_iter().any(|line| line.contains("blocked"))
    });
    until!(|| json_lines
        .try_iter()
        .any(|line| serde_json::from_str::<Value>(&line).unwrap()["to"] == "blocked"));
    cli(&guard, &["daemon", "stop"], 0);
    until!(|| human.0.try_wait().unwrap().is_some());
    until!(|| watch.0.try_wait().unwrap().is_some());
    assert_eq!(human.0.wait().unwrap().code(), Some(1));
    assert_eq!(watch.0.wait().unwrap().code(), Some(1));
});
