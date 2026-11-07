use std::{ffi::OsString, fs, os::unix::ffi::OsStringExt, process::Command};
#[path = "install_support/process.rs"]
mod process;
#[test]
fn poll_environment_errors_name_the_variable_without_a_config_path() {
    for value in [
        OsString::from("not-a-number"),
        OsString::from("49"),
        OsString::from_vec(vec![0xff]),
    ] {
        let root = tempfile::tempdir().unwrap();
        let config = if cfg!(target_os = "macos") {
            root.path()
                .join("Library/Application Support/tmux-sidecar/config.toml")
        } else {
            root.path().join(".config/tmux-sidecar/config.toml")
        };
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, "[notify]\ndesktop=false\ntmux=false\n").unwrap();
        let out = process::output(
            Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
                .env("HOME", root.path())
                .env("XDG_CONFIG_HOME", root.path().join(".config"))
                .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
                .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"))
                .env("TMUX_SIDECAR_POLL_MS", value)
                .arg("--tmux-socket")
                .arg(root.path().join("tmux"))
                .args(["daemon", "run"]),
        );
        let message = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success());
        assert!(
            message.contains("TMUX_SIDECAR_POLL_MS")
                && message.contains("poll_ms")
                && !message.contains(config.to_str().unwrap()),
            "{message}"
        );
    }
}
