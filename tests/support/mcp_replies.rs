//! Shared fake replies, checked against the real daemon by mcp_contract.
use serde_json::{Value, json};
pub fn waiting(run_id: Value, state: Value, at: Value) -> Value {
    json!({"run_id":run_id,"state":state,"at":at})
}
pub fn sent() -> Value {
    json!({})
}
