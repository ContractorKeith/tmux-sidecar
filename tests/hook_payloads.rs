use std::{fs, path::Path};
use tmux_sidecar::hook::{signal, spool};
#[test]
fn fixture_metadata_and_event_kinds_parse_without_losing_raw_fields() {
    for agent in ["claude", "codex", "grok", "opencode"] {
        for file in fs::read_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/hooks")
                .join(agent),
        )
        .unwrap()
        {
            let file = file.unwrap().path();
            let payload: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
            let format = if file.file_stem().unwrap() == "notify" {
                "codex-notify"
            } else {
                agent
            };
            let expected_transcript = payload
                .get("transcript_path")
                .or_else(|| payload.get("transcriptPath"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let (kind, session, cwd, transcript) = signal::parse(format, payload).unwrap();
            assert!(!kind.is_empty());
            assert!(session.is_some(), "{}", file.display());
            assert_eq!(cwd, "/home/user/project");
            assert_eq!(transcript, expected_transcript);
        }
    }
}
#[test]
fn spool_ack_retains_concurrent_suffix() {
    let root = tempfile::tempdir().unwrap();
    let paths = tmux_sidecar::paths::Paths {
        runtime: root.path().join("run"),
        state: root.path().join("state"),
        config: root.path().join("config"),
    };
    spool::append(&paths, b"first\n").unwrap();
    let prefix = spool::read(&paths.state).unwrap();
    spool::append(&paths, b"second\n").unwrap();
    spool::acknowledge(&paths.state, &prefix).unwrap();
    assert_eq!(spool::read(&paths.state).unwrap(), b"second\n");
}
#[tokio::test(flavor = "current_thread")]
async fn wrong_server_hello_is_rejected_before_signal_delivery() {
    // The hook's client verifier uses the same identity check as ordinary clients.
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let task = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        stream
            .get_mut()
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"server_key\":\"wrong\"}}\n")
            .await
            .unwrap();
    });
    let paths = tmux_sidecar::paths::Paths {
        runtime: root.path().to_owned(),
        state: root.path().join("state"),
        config: root.path().join("config"),
    };
    let result =
        tmux_sidecar::client::request(&socket, &paths, "hello", serde_json::json!({}), false).await;
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("wrong tmux server")
    );
    task.await.unwrap();
}
