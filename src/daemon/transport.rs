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

pub async fn serve(stream: UnixStream, sender: mpsc::Sender<Call>) -> std::io::Result<()> {
    if !super::peer::authorize(&stream)? {
        return Ok(());
    }
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    loop {
        // Limit a single frame so a local client cannot grow the daemon without bound.
        let mut bytes = Vec::new();
        use tokio::io::AsyncReadExt;
        let count = (&mut reader)
            .take(1024 * 1024 + 1)
            .read_until(b'\n', &mut bytes)
            .await?;
        if count == 0 {
            return Ok(());
        }
        if count > 1024 * 1024 {
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
                        let (reply, receive) = oneshot::channel();
                        if sender.send(Call { request, reply }).await.is_err() {
                            return Ok(());
                        }
                        let Ok(response) = receive.await else {
                            return Ok(());
                        };
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
