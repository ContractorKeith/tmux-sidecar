//! Delivery checks live state; each new approval episode remains eligible.
use super::{TmuxGuard, cli_verbs::fire, enabled, lifecycle::list};
use crate::support::wait_for_timeout;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

e2e_test!(
    desktop_drops_resumed_blocks_and_notifies_the_next_episode,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start();
        for name in ["notify-send", "osascript"] {
            let path = guard.scratch.path().join("bin").join(name);
            fs::write(&path, "#!/bin/sh\nprintf 'notification\\n' >> \"$TMUX_SIDECAR_STATE_DIR/desktop.calls\"\n").unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let config = if cfg!(target_os = "macos") {
            guard
                .scratch
                .path()
                .join("home/Library/Application Support/tmux-sidecar/config.toml")
        } else {
            guard.scratch.path().join("config/tmux-sidecar/config.toml")
        };
        fs::write(
            config,
            "[notify]\ndesktop=true\ntmux=false\ndebounce_secs=10\n",
        )
        .unwrap();
        let run = list(&guard)[0].clone();
        let calls = guard.scratch.path().join("state/desktop.calls");
        fire(&guard, &run, "PermissionRequest");
        until!(|| list(&guard)[0]["state"] == "blocked");
        fire(&guard, &run, "PostToolUse");
        until!(|| list(&guard)[0]["state"] == "working");
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            assert!(!calls.exists(), "resumed block raised a stale notification");
            std::thread::sleep(Duration::from_millis(25));
        }
        for expected in [1, 2] {
            if expected == 2 {
                fire(&guard, &run, "PostToolUse");
                until!(|| list(&guard)[0]["state"] == "working");
            }
            fire(&guard, &run, "PermissionRequest");
            wait_for_timeout(
                "current block notification",
                Duration::from_secs(10),
                || {
                    fs::read_to_string(&calls)
                        .unwrap_or_default()
                        .lines()
                        .count()
                        == expected
                },
            );
        }
    }
);
