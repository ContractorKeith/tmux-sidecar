//! Exercise cleanup in a real pane, where raw mode and the alternate screen matter.
use super::lifecycle::list;
use super::{TmuxGuard, enabled};
use crate::support::output;
use std::{fs, path::Path};
fn quote(path: &Path) -> String {
    tmux_sidecar::install::quote(&path.to_string_lossy())
}
e2e_test!(ui_signals_restore_raw_mode_and_alternate_screen, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    list(&guard);
    for (name, signal) in [("term", libc::SIGTERM), ("hup", libc::SIGHUP)] {
        let pidfile = guard.scratch.path().join(format!("ui-{name}.pid"));
        let modes = guard.scratch.path().join(format!("ui-{name}.modes"));
        // Foreground exec records the UI pid without a background terminal job.
        let inner = format!(
            "echo $$ > {}; exec {} --tmux-socket {} ui",
            quote(&pidfile),
            quote(Path::new(env!("CARGO_BIN_EXE_tmux-sidecar"))),
            quote(&guard.socket())
        );
        let command = format!(
            "/bin/sh -c {}; stty -a > {}; printf 'terminal restored\\n'; sleep 30",
            tmux_sidecar::install::quote(&inner),
            quote(&modes)
        );
        assert_eq!(
            super::state::rpc(&guard, "hello", serde_json::json!({}))["rpc_version"],
            1
        );
        let launched = std::time::Instant::now();
        let result = output(
            guard
                .command()
                .args(["new-window", "-P", "-F", "#{pane_id}", "-t", "fixture"])
                .arg(command),
        );
        assert!(result.status.success());
        let pane = String::from_utf8(result.stdout).unwrap().trim().to_string();
        let dimensions = String::from_utf8(
            output(guard.command().args([
                "display-message",
                "-p",
                "-t",
                &pane,
                "#{pane_width} #{pane_height}",
            ]))
            .stdout,
        )
        .unwrap();
        let size: Vec<u16> = dimensions
            .split_whitespace()
            .map(|n| n.parse().unwrap())
            .collect();
        assert!(
            size[0] >= 40 && size[1] >= 10,
            "unexpected UI pane size: {dimensions}"
        );
        crate::support::stage(&format!(
            "signal UI {name}: daemon hello ready, pane {pane} size {dimensions}"
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        loop {
            let text = String::from_utf8(
                output(guard.command().args(["capture-pane", "-p", "-t", &pane])).stdout,
            )
            .unwrap();
            if text.contains("Projects") {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "UI did not render: {text}"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        crate::support::stage(&format!(
            "signal UI {name}: first frame ready after {:?}",
            launched.elapsed()
        ));
        let pid: i32 = fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // SAFETY: this positive pid was written by our scratch pane's shell for its UI child.
        assert_eq!(unsafe { libc::kill(pid, signal) }, 0);
        until!(|| modes.exists());
        let text = fs::read_to_string(&modes).unwrap();
        assert!(
            !text
                .split_whitespace()
                .any(|word| word.trim_end_matches(';') == "-icanon"),
            "{text}"
        );
        assert!(
            !text
                .split_whitespace()
                .any(|word| word.trim_end_matches(';') == "-echo"),
            "{text}"
        );
        let captured = String::from_utf8(
            output(guard.command().args(["capture-pane", "-p", "-t", &pane])).stdout,
        )
        .unwrap();
        assert!(captured.contains("terminal restored"), "{captured}");
        assert!(
            !captured.contains("Projects"),
            "alternate screen remained active: {captured}"
        );
    }
});
e2e_test!(ui_enter_without_attached_client_has_actionable_error, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let run = list(&guard)[0].clone();
    let result = output(
        guard
            .command()
            .args(["new-window", "-P", "-F", "#{pane_id}", "-t", "fixture"])
            .arg(format!(
                "{} --tmux-socket {} ui",
                quote(Path::new(env!("CARGO_BIN_EXE_tmux-sidecar"))),
                quote(&guard.socket())
            )),
    );
    assert!(result.status.success());
    let pane = String::from_utf8(result.stdout).unwrap().trim().to_string();
    until!(|| String::from_utf8_lossy(
        &output(guard.command().args(["capture-pane", "-p", "-t", &pane])).stdout
    )
    .contains("claude"));
    output(guard.command().args(["send-keys", "-t", &pane, "Enter"]));
    until!(|| String::from_utf8_lossy(
        &output(guard.command().args(["capture-pane", "-p", "-t", &pane])).stdout
    )
    .contains("run the ui inside tmux to switch panes"));
    assert_eq!(run["pane_id"], "%0");
    output(guard.command().args(["send-keys", "-t", &pane, "q"]));
});
