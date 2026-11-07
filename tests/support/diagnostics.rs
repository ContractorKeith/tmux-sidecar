//! Diagnostics preserve the object, filename and reason needed to act on failures.
use super::{TmuxGuard, enabled, lifecycle::list};
use crate::support::output;
use std::path::PathBuf;

fn config(guard: &TmuxGuard) -> PathBuf {
    if cfg!(target_os = "macos") {
        guard
            .scratch
            .path()
            .join("home/Library/Application Support/tmux-sidecar/config.toml")
    } else {
        guard.scratch.path().join("config/tmux-sidecar/config.toml")
    }
}
e2e_test!(startup_errors_keep_config_path_and_toml_overflow_reason, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let config = config(&guard);
    std::fs::write(&config, "idle_after_secs = 18446744073709551615\n").unwrap();
    let result = output(guard.sidecar().args(["list", "--json"]));
    assert!(!result.status.success());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(error.contains(config.to_str().unwrap()), "{error}");
    assert!(
        error.contains("too large") || error.contains("out of range"),
        "{error}"
    );
});
e2e_test!(poll_override_errors_name_the_environment_variable, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    for value in ["abc", "0", "60001", "18446744073709551615", ""] {
        let result = output(
            guard
                .sidecar()
                .env("TMUX_SIDECAR_POLL_MS", value)
                .args(["list", "--json"]),
        );
        assert!(!result.status.success());
        let error = String::from_utf8(result.stderr).unwrap();
        assert!(error.contains("TMUX_SIDECAR_POLL_MS"), "{error}");
        assert!(
            !error.contains(config(&guard).to_str().unwrap()),
            "environment error blamed a config file: {error}"
        );
    }
});
e2e_test!(
    foreground_duplicate_daemon_has_a_sentence_in_stderr_and_log,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start();
        list(&guard);
        let result = output(guard.sidecar().args(["daemon", "run"]));
        assert!(!result.status.success());
        let error = String::from_utf8(result.stderr).unwrap();
        assert!(
            error.contains("daemon") && error.contains("already running"),
            "{error}"
        );
        let log = std::fs::read_to_string(guard.scratch.path().join("state/daemon.log")).unwrap();
        assert!(
            log.contains("daemon") && log.contains("already running"),
            "{log}"
        );
    }
);

e2e_test!(runtime_socket_label_tracks_the_actual_override, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    for mode in ["unset", "empty", "set"] {
        let mut command = guard.sidecar();
        command
            .env("XDG_RUNTIME_DIR", guard.scratch.path().join("xdg-runtime"))
            .env("TMPDIR", guard.scratch.path().join("tmp"));
        match mode {
            "unset" => {
                command.env_remove("TMUX_SIDECAR_RUNTIME_DIR");
            }
            "empty" => {
                command.env("TMUX_SIDECAR_RUNTIME_DIR", "");
            }
            _ => (),
        }
        let result = output(command.args(["daemon", "status", "--json"]));
        assert!(!result.status.success());
        let error = String::from_utf8(result.stderr).unwrap();
        if mode == "set" {
            assert!(
                error.contains("runtime (TMUX_SIDECAR_RUNTIME_DIR) socket"),
                "{error}"
            );
        } else {
            assert!(
                error.contains("default runtime socket")
                    && !error.contains("TMUX_SIDECAR_RUNTIME_DIR"),
                "{error}"
            );
        }
    }
});
