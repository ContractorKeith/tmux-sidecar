use crate::rpc::{self, Id, Outcome, Request, Response, RpcError, Version};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{mpsc, oneshot},
};

pub struct Call {
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
}
pub fn failure(id: Option<Id>, code: i32, message: impl Into<String>) -> Response {
    Response {
        jsonrpc: Version::V2,
        id,
        outcome: Outcome::Failure {
            error: RpcError {
                code,
                message: message.into(),
                data: None,
            },
        },
    }
}

pub async fn serve(
    stream: UnixStream,
    sender: mpsc::Sender<Call>,
    mut events: tokio::sync::broadcast::Receiver<Value>,
    mut views: tokio::sync::watch::Receiver<u64>,
) -> std::io::Result<()> {
    if !super::peer::authorize(&stream)? {
        return Ok(());
    }
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut subscribed = false;
    let mut view_subscribed = false;
    loop {
        // Limit a single frame so a local client cannot grow the daemon without bound.
        let mut bytes = Vec::new();
        use tokio::io::AsyncReadExt;
        let count = {
            let read = async {
                (&mut reader)
                    .take(1024 * 1024 + 1)
                    .read_until(b'\n', &mut bytes)
                    .await
            };
            tokio::pin!(read);
            loop {
                tokio::select! {
                    count=&mut read=>break count?,
                    changed=views.changed(), if view_subscribed => {
                        if changed.is_err() {return Ok(());}
                        let frame=b"{\"jsonrpc\":\"2.0\",\"method\":\"view_changed\",\"params\":{}}\n";
                        match tokio::time::timeout(std::time::Duration::from_millis(250),writer.write_all(frame)).await {
                            Ok(result)=>result?, Err(_)=>return Ok(()),
                        }
                    },
                    event=events.recv(), if subscribed=> {
                        let (frame,close)=match event {
                            Ok(payload)=>(serde_json::json!({"jsonrpc":"2.0","method":"state_changed","params":payload}),false),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>(serde_json::json!({"jsonrpc":"2.0","method":"subscription_error","params":{"code":-32001,"message":"slow subscriber exceeded 256 changes"}}),true),
                            Err(_)=>return Ok(()),
                        };
                        let mut encoded=serde_json::to_vec(&frame)?;encoded.push(b'\n');
                        // A blocked socket never holds the daemon; bound even the final write.
                        match tokio::time::timeout(std::time::Duration::from_millis(250),writer.write_all(&encoded)).await {
                            Ok(result)=>result?, Err(_)=>return Ok(()),
                        }
                        if close {return Ok(());}
                    }
                }
            }
        };
        if count == 0 {
            return Ok(());
        }
        if count - usize::from(bytes.last() == Some(&b'\n')) > 1024 * 1024 {
            let response = failure(None, rpc::INVALID_REQUEST, "frame exceeds 1 MiB");
            let mut bytes = serde_json::to_vec(&response)?;
            bytes.push(b'\n');
            writer.write_all(&bytes).await?;
            return Ok(());
        }
        let response = match serde_json::from_slice::<Value>(&bytes) {
            Err(_) => failure(None, rpc::PARSE_ERROR, "invalid JSON"),
            Ok(value) => {
                // JSON-RPC notifications receive no response and do not execute M1's request methods.
                if value.get("jsonrpc") == Some(&Value::String("2.0".into()))
                    && value.get("method").is_some_and(Value::is_string)
                    && value.get("id").is_none()
                {
                    if value["method"] == "signal" {
                        let mut request = value;
                        request["id"] = Value::from(0);
                        if let Ok(request) = serde_json::from_value::<Request>(request) {
                            let (reply, receive) = oneshot::channel();
                            if sender.send(Call { request, reply }).await.is_err() {
                                return Ok(());
                            }
                            let _ = receive.await;
                        }
                    }
                    continue;
                }
                match serde_json::from_value::<Request>(value.clone()) {
                    Err(_) => failure(
                        value
                            .get("id")
                            .and_then(|id| serde_json::from_value(id.clone()).ok()),
                        rpc::INVALID_REQUEST,
                        "invalid request",
                    ),
                    Ok(request) => {
                        let hello = matches!(request.method.as_str(), "hello" | "daemon.status");
                        let subscribing = request.method == "subscribe";
                        let wants_views =
                            request.params.as_ref().is_some_and(|p| p["views"] == true);
                        let waiting = request.method == "wait";
                        if subscribing && !subscribed {
                            events = events.resubscribe();
                        }
                        let (reply, receive) = oneshot::channel();
                        if sender.send(Call { request, reply }).await.is_err() {
                            return Ok(());
                        }
                        let mut receive = receive;
                        // Ordinary RPCs own their reply before the next read. Only a
                        // long-lived wait needs a concurrent disconnect monitor.
                        let response = if waiting {
                            tokio::select! {
                                biased;
                                response = &mut receive => response,
                                buffered = reader.fill_buf() => {
                                    if buffered?.is_empty() && super::peer::disconnected(writer.as_ref()) { return Ok(()); }
                                    // Preserve pipelined requests; process them after this reply.
                                    receive.await
                                }
                            }
                        } else {
                            receive.await
                        };
                        let Ok(mut response) = response else {
                            return Ok(());
                        };
                        if hello && let Outcome::Success { result } = &mut response.outcome {
                            result["build_id"] =
                                Value::String(env!("TMUX_SIDECAR_BUILD_ID").into());
                        }
                        if subscribing && matches!(response.outcome, Outcome::Success { .. }) {
                            subscribed = true;
                            view_subscribed |= wants_views;
                        }
                        response
                    }
                }
            }
        };
        let mut bytes = serde_json::to_vec(&response)?;
        bytes.push(b'\n');
        writer.write_all(&bytes).await?;
    }
}
