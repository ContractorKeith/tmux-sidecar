//! Deadline-bound probes kill their child on cancellation and drain both pipes.
use std::{
    io,
    process::{Command, Output},
    time::Duration,
};
use tokio::time::Instant;
pub(crate) async fn output(command: Command, deadline: Instant) -> io::Result<Output> {
    let deadline = deadline.min(Instant::now() + Duration::from_secs(1));
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "probe deadline exceeded",
        ));
    }
    let mut command = tokio::process::Command::from(command);
    let output = tokio::time::timeout_at(deadline, command.kill_on_drop(true).output())
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "probe exceeded 1 s or send deadline",
            )
        })??;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn timed_out_probe_kills_its_process() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("pid");
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "printf '%s' \"$$\" > \"$TSX_MARKER\"; exec /bin/sleep 30",
            ])
            .env("TSX_MARKER", &marker);
        let began = Instant::now();
        let error = output(command, began + Duration::from_millis(200))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(began.elapsed() < Duration::from_secs(1));
        let pid: i32 = std::fs::read_to_string(marker).unwrap().parse().unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while crate::proc::running(pid).unwrap() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
    }
}
