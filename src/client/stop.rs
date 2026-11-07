use super::*;
use std::os::unix::fs::MetadataExt;
fn identity(path: &Path) -> Option<(u64, u64)> {
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}
/// Stop is idempotent and waits on the endpoint of the verified connection.
pub async fn stop(socket: &Path, paths: &Paths) -> io::Result<bool> {
    let (mut stream, endpoint) = match identified(socket, paths, false).await {
        Ok(connection) => connection,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(false);
        }
        Err(e) => return Err(e),
    };
    let original = identity(&endpoint);
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        exchange(&mut stream, "daemon.stop", serde_json::json!({})),
    )
    .await
    .map_err(|_| io::Error::other("daemon.stop exceeded 5 s"))??;
    let pid = response["pid"]
        .as_i64()
        .and_then(|p| i32::try_from(p).ok())
        .filter(|p| *p > 0)
        .ok_or_else(|| io::Error::other("daemon.stop response did not identify its pid"))?;
    drop(stream);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while crate::proc::running(pid)? || (original.is_some() && identity(&endpoint) == original) {
        if tokio::time::Instant::now() >= deadline {
            return Err(io::Error::other(format!(
                "waiting for daemon pid {pid} and socket {} cleanup exceeded 5 s",
                endpoint.display()
            )));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(true)
}
