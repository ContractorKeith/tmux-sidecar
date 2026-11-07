use super::{Cli, Command, execute::Failure};
use crate::rpc;
use serde_json::json;
pub fn prepare(cli: &mut Cli) -> Result<(), Failure> {
    let Command::Send { sel, text, force } = &mut cli.command else {
        return Ok(());
    };
    if text == "-" {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(rpc::MAX_FRAME_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| (1, e.to_string()))?;
        if bytes.len() > rpc::MAX_FRAME_BYTES {
            return Err((2, rpc::frame_error("send").to_string()));
        }
        *text = String::from_utf8(bytes).map_err(|e| (2, format!("prompt must be UTF-8: {e}")))?;
    }
    crate::seams::validate_prompt(text).map_err(|e| (2, e.into()))?;
    rpc::request_frame(
        "send",
        json!({"sel":sel.to_string(),"text":text,"force":force}),
    )
    .map_err(|e| (2, e.to_string()))?;
    Ok(())
}
