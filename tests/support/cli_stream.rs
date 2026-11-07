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
};
e2e_test!(cli_wait_and_watch_follow_committed_changes, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let run = list(&guard)[0].clone();
    fire(&guard, &run, "UserPromptSubmit");
    let mut watch = ChildGuard(
        guard
            .sidecar()
            .args(["watch", "--json"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = watch.0.stdout.take().unwrap();
    let (send, recv) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if send.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let mut wait = ChildGuard(
        guard
            .sidecar()
            .args([
                "wait",
                "%0",
                "--until",
                "waiting",
                "--timeout",
                "4",
                "--json",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    // watch has no replay: allow its handshake to finish before firing the fixture.
    std::thread::sleep(std::time::Duration::from_millis(2250));
    fire(&guard, &run, "Stop");
    let event: Value = serde_json::from_str(
        &recv
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(event["to"], "waiting");
    until!(|| wait.0.try_wait().unwrap().is_some());
    assert!(wait.0.wait().unwrap().success());
    // A human watch emits a compact state table and exits 1 when the daemon closes.
    let mut human = ChildGuard(
        guard
            .sidecar()
            .arg("watch")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    fire(&guard, &run, "PermissionRequest");
    assert_eq!(
        serde_json::from_str::<Value>(
            &recv
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap()
        )
        .unwrap()["to"],
        "blocked"
    );
    cli(&guard, &["daemon", "stop"], 0);
    until!(|| human.0.try_wait().unwrap().is_some());
    until!(|| watch.0.try_wait().unwrap().is_some());
    assert_eq!(human.0.wait().unwrap().code(), Some(1));
    assert_eq!(watch.0.wait().unwrap().code(), Some(1));
    use std::io::Read;
    let mut text = String::new();
    human
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    assert!(text.contains("blocked"));
});
