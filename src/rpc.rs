//! M0: JSON-RPC 2.0 wire types; socket transport arrives in M1.
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub const RPC_VERSION: u32 = 1;
pub const METHODS: &[&str] = &[
    "hello",
    "list",
    "attention",
    "history",
    "wait",
    "send",
    "ack",
    "name",
    "group.add",
    "group.rm",
    "group.ls",
    "subscribe",
    "signal",
    "daemon.status",
    "daemon.stop",
];
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;
pub const ERROR: i32 = -32001;
pub const USAGE: i32 = -32002;
pub const REFUSED: i32 = -32003;
pub const TIMEOUT: i32 = -32004;
pub const NOT_FOUND: i32 = -32005;
pub const DAEMON_UNREACHABLE: i32 = -32006;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Version {
    #[serde(rename = "2.0")]
    V2,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    Number(i64),
    String(String),
    Null,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: Version,
    pub id: Id,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: Version,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Outcome {
    Success { result: Value },
    Failure { error: RpcError },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: Version,
    pub id: Option<Id>,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn round_trip<T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
        value: T,
    ) {
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<T>(&json).unwrap(), value);
    }
    #[test]
    fn wire_round_trips() {
        for method in METHODS {
            round_trip(Request {
                jsonrpc: Version::V2,
                id: Id::Number(42),
                method: (*method).into(),
                params: Some(serde_json::json!({"sel":"%7"})),
            });
        }
        round_trip(Request {
            jsonrpc: Version::V2,
            id: Id::Null,
            method: "hello".into(),
            params: None,
        });
        round_trip(Notification {
            jsonrpc: Version::V2,
            method: "state_changed".into(),
            params: None,
        });
        round_trip(Response {
            jsonrpc: Version::V2,
            id: Some(Id::String("one".into())),
            outcome: Outcome::Success {
                result: Value::Null,
            },
        });
        round_trip(Response {
            jsonrpc: Version::V2,
            id: None,
            outcome: Outcome::Failure {
                error: RpcError {
                    code: PARSE_ERROR,
                    message: "invalid JSON".into(),
                    data: None,
                },
            },
        });
        assert!(
            serde_json::from_str::<Request>(r#"{"jsonrpc":"1.0","id":1,"method":"list"}"#).is_err()
        );
    }
}
