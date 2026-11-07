use super::*;
use std::time::Duration;
#[test]
fn connected_send_and_wait_failures_are_errors_not_unreachable() {
    for kind in [
        io::ErrorKind::TimedOut,
        io::ErrorKind::UnexpectedEof,
        io::ErrorKind::ConnectionReset,
    ] {
        let send = response_error(client::connected_error(
            "send",
            io::Error::new(kind, "lost response"),
        ));
        assert_eq!(
            send,
            (
                1,
                "send outcome unknown — check the pane before retrying (-32001)".into()
            )
        );
        let wait = response_error(client::connected_error(
            "wait",
            io::Error::new(kind, "lost response"),
        ));
        assert_eq!(wait, (1, "daemon stopped while waiting (-32001)".into()));
    }
    assert_eq!(
        response_error(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            "no daemon"
        ))
        .0,
        6
    );
}
#[tokio::test(flavor = "current_thread")]
async fn wait_eof_after_handshake_is_exit_one() {
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::UnixListener,
    };
    let root = tempfile::tempdir().unwrap();
    let paths = paths::Paths {
        runtime: root.path().join("run"),
        state: root.path().join("state"),
        config: root.path().join("config"),
    };
    std::fs::create_dir(&paths.runtime).unwrap();
    let socket = root.path().join("tmux");
    let key = paths::server_key(&socket).unwrap();
    let listener = UnixListener::bind(paths.runtime.join("sock")).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        stream
            .get_mut()
            .write_all(
                format!(
                    "{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"server_key\":\"{key}\"}}}}\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        line.clear();
        stream.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&line).unwrap()["method"],
            "wait"
        );
        // Dropping an established daemon connection must not become unreachable.
    });
    let error = tokio::time::timeout(
        Duration::from_secs(1),
        client::request(
            &socket,
            &paths,
            "wait",
            json!({"sel":"r1","until":"blocked"}),
            false,
        ),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(
        response_error(error),
        (1, "daemon stopped while waiting (-32001)".into())
    );
    server.await.unwrap();
}
#[test]
fn wait_shutdown_response_is_general_but_send_timeout_remains_timeout() {
    let error = |code| {
        io::Error::other(crate::rpc::RpcError {
            code,
            message: "daemon stopping".into(),
            data: None,
        })
    };
    assert_eq!(
        response_error(client::connected_error(
            "wait",
            error(crate::rpc::DAEMON_UNREACHABLE)
        ))
        .0,
        1
    );
    assert_eq!(
        response_error(client::connected_error("send", error(crate::rpc::TIMEOUT))).0,
        4
    );
}

#[test]
fn stdout_errors_name_the_output_stream() {
    let error = stdout_error(io::Error::from_raw_os_error(libc::ENOSPC));
    assert_eq!(error.0, 1);
    assert!(error.1.contains("stdout"), "{}", error.1);
}

#[test]
fn non_output_errors_do_not_blame_stdout() {
    let error = output_error(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "settings.json is not writable",
    ));
    assert_eq!(error, (1, "settings.json is not writable".into()));
}

#[tokio::test]
async fn cap_rejection_survives_peer_closing_before_hello_write() {
    use tokio::{io::AsyncWriteExt, net::UnixStream};
    let (peer, mut rejector) = UnixStream::pair().unwrap();
    rejector.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32600,\"message\":\"daemon connection limit reached (256); close idle clients\"}}\n").await.unwrap();
    drop(rejector);
    let error = tokio::time::timeout(
        Duration::from_secs(1),
        client::exchange(&mut tokio::io::BufReader::new(peer), "hello", json!({})),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("daemon connection limit reached (256); close idle clients"),
        "{error}"
    );
}
#[tokio::test]
async fn unrelated_null_or_mismatched_reply_ids_remain_protocol_errors() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    for frame in [
        json!({"jsonrpc":"2.0","id":null,"error":{"code":-32001,"message":"unrelated"}}),
        json!({"jsonrpc":"2.0","id":2,"error":{"code":-32600,"message":"wrong id"}}),
        json!({"jsonrpc":"2.0","id":null,"result":{}}),
    ] {
        let (peer, server) = tokio::io::duplex(4096);
        let server = tokio::spawn(async move {
            let mut server = BufReader::new(server);
            server.read_line(&mut String::new()).await.unwrap();
            server
                .get_mut()
                .write_all(format!("{frame}\n").as_bytes())
                .await
                .unwrap();
        });
        let error = tokio::time::timeout(
            Duration::from_secs(1),
            client::exchange(&mut BufReader::new(peer), "hello", json!({})),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(
            error.to_string().contains("mismatched RPC response id"),
            "{error}"
        );
        server.await.unwrap();
    }
}
