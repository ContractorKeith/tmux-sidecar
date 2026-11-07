use super::transport::{self, Call};
use crate::rpc::{Outcome, Response, Version};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{broadcast, mpsc},
};
#[tokio::test(flavor = "current_thread")]
async fn lagged_subscriber_receives_final_error_and_disconnects() {
    let (server, client) = UnixStream::pair().unwrap();
    let (calls, mut receive) = mpsc::channel::<Call>(1);
    let (events, _) = broadcast::channel(256);
    let listener = tokio::spawn(transport::serve(
        server,
        calls,
        events.subscribe(),
        tokio::sync::watch::channel(0).1,
    ));
    let (reader, mut writer) = client.into_split();
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"subscribe\"}\n")
        .await
        .unwrap();
    let call = tokio::time::timeout(std::time::Duration::from_secs(1), receive.recv())
        .await
        .unwrap()
        .unwrap();
    call.reply
        .send(Response {
            jsonrpc: Version::V2,
            id: Some(call.request.id),
            outcome: Outcome::Success {
                result: json!({"subscribed":true}),
            },
        })
        .unwrap();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        reader.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(line.contains("subscribed"));
    // Current-thread runtime: the consumer cannot drain until all 257 sends finish.
    for n in 0..257 {
        events.send(json!({"at":n})).unwrap();
    }
    line.clear();
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        reader.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    let frame: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(frame["method"], "subscription_error");
    tokio::time::timeout(std::time::Duration::from_secs(1), listener)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn half_closed_request_still_gets_its_reply() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let (calls, mut receive) = mpsc::channel::<Call>(1);
    let (events, _) = broadcast::channel(256);
    let listener = tokio::spawn(transport::serve(
        server,
        calls,
        events.subscribe(),
        tokio::sync::watch::channel(0).1,
    ));
    client
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"hello\"}\n")
        .await
        .unwrap();
    client.shutdown().await.unwrap();
    let call = receive.recv().await.unwrap();
    tokio::task::yield_now().await;
    assert!(
        call.reply
            .send(Response {
                jsonrpc: Version::V2,
                id: Some(call.request.id),
                outcome: Outcome::Success {
                    result: json!({"ok":true})
                }
            })
            .is_ok()
    );
    let mut reader = BufReader::new(client);
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        reader.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["result"]["ok"],
        true
    );
    drop(reader);
    tokio::time::timeout(std::time::Duration::from_secs(1), listener)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn disconnected_client_cancels_an_unfinished_reply() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let (calls, mut receive) = mpsc::channel::<Call>(1);
    let (events, _) = broadcast::channel(256);
    let listener = tokio::spawn(transport::serve(
        server,
        calls,
        events.subscribe(),
        tokio::sync::watch::channel(0).1,
    ));
    client
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"wait\"}\n")
        .await
        .unwrap();
    let mut call = receive.recv().await.unwrap();
    drop(client);
    tokio::time::timeout(std::time::Duration::from_secs(1), call.reply.closed())
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), listener)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn foreign_uid_never_reaches_dispatch() {
    let (server, mut client) = UnixStream::pair().unwrap();
    let (calls, mut receive) = mpsc::channel::<Call>(1);
    let (events, _) = broadcast::channel(256);
    let task = tokio::spawn(transport::serve_with_peer(
        server,
        calls,
        events.subscribe(),
        tokio::sync::watch::channel(0).1,
        |_| Ok(crate::paths::uid().wrapping_add(1)),
    ));
    let _ = client
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"hello\"}\n")
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        matches!(
            receive.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ),
        "foreign uid reached dispatcher"
    );
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        client.read_to_end(&mut bytes),
    )
    .await
    .unwrap();
    assert!(read.is_err() || bytes.is_empty());
}
