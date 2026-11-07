use super::*;
use std::io;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub fn frame_error(method: &str) -> io::Error {
    io::Error::other(RpcError {
        code: INVALID_PARAMS,
        message: format!(
            "{} exceeds maximum 1 MiB encoded request frame (1048576 bytes including JSON and newline)",
            if method == "send" {
                "prompt"
            } else {
                "request"
            }
        ),
        data: None,
    })
}
pub fn request_frame(method: &str, params: Value) -> io::Result<Vec<u8>> {
    let request = Request {
        jsonrpc: Version::V2,
        id: Id::Number(1),
        method: method.into(),
        params: Some(params),
    };
    let mut bytes = serde_json::to_vec(&request)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(frame_error(method));
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prompt_limit_counts_json_escaping_and_frame_overhead() {
        let params = |text: String| serde_json::json!({"sel":"%0","text":text,"force":false});
        let overhead = request_frame("send", params(String::new())).unwrap().len();
        let max = MAX_FRAME_BYTES - overhead;
        assert_eq!(
            request_frame("send", params("x".repeat(max)))
                .unwrap()
                .len(),
            MAX_FRAME_BYTES
        );
        for text in [
            "x".repeat(max + 1),
            "\"".repeat(max / 2 + 1),
            "\n".repeat(max / 2 + 1),
        ] {
            assert!(
                request_frame("send", params(text))
                    .unwrap_err()
                    .to_string()
                    .contains("prompt exceeds maximum 1 MiB encoded request frame")
            );
        }
    }
}
