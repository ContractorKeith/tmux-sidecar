use super::*;
e2e_test!(fixture_start_waits_for_a_transient_writer, {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("fixture");
    fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let writer = fs::OpenOptions::new()
        .write(true)
        .open(&executable)
        .unwrap();
    {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            drop(writer);
        });
        let mut command = Command::new(&executable);
        command
            .env("HOME", root.path())
            .env_remove("BASH_ENV")
            .env_remove("ENV")
            .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
            .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"));
        let mut child = ChildGuard(spawn_fixture(&mut command));
        wait_for("transient-writer fixture to exit", || {
            child.0.try_wait().unwrap().is_some()
        });
        assert!(child.0.try_wait().unwrap().unwrap().success());
    }
});
