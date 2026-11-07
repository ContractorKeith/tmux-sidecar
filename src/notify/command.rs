use std::{io, time::Duration};
pub(super) async fn desktop(message: &str) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = tokio::process::Command::new("notify-send");
        c.args(["--app-name=tmux-sidecar", "--", "tmux-sidecar", message]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = tokio::process::Command::new("osascript");
        c.args(["-e","on run argv\ndisplay notification (item 1 of argv) with title \"tmux-sidecar\"\nend run","--",message]);
        c
    };
    execute(&mut command).await
}
async fn execute(command: &mut tokio::process::Command) -> io::Result<()> {
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        command
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status(),
    )
    .await
    .map_err(io::Error::other)??;
    if result.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("notifier exited {result}")))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn hung_notifier_is_killed_within_deadline() {
        let start = std::time::Instant::now();
        let root = tempfile::tempdir().unwrap();
        assert!(
            execute(
                tokio::process::Command::new("sleep")
                    .arg("30")
                    .env("HOME", root.path())
                    .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
                    .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"))
            )
            .await
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}

#[cfg(test)]
mod fake_tests {
    use super::*;
    #[tokio::test]
    async fn fake_notifier_receives_literal_message_and_reports_failure() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("received");
        let mut fake = tokio::process::Command::new("sh");
        fake.env("HOME", root.path())
            .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
            .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"));
        fake.args([
            "-c",
            "printf '%s' \"$1\" > \"$2\"",
            "fake",
            "Agent run r1 is blocked",
        ])
        .arg(&path);
        execute(&mut fake).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "Agent run r1 is blocked"
        );
        assert!(
            execute(tokio::process::Command::new("sh").args(["-c", "exit 7"]))
                .await
                .is_err()
        );
    }
}
