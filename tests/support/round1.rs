use super::{
    enabled,
    lifecycle::{list, ok, stop, until},
    support::TmuxGuard,
};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{ffi::OsStringExt, net::UnixStream},
};

#[test]
fn clean_restart_keeps_live_run_identity() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let before = list(&g);
    stop(&g);
    assert_eq!(list(&g), before);
}
#[test]
fn byte_cwd_is_discovered() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let cwd = g
        .scratch
        .path()
        .join(std::ffi::OsString::from_vec(b"bad-\xff".to_vec()));
    match fs::create_dir(&cwd) {
        Ok(()) => (),
        // APFS rejects invalid UTF-8 at mkdir; parser coverage still runs on macOS.
        Err(error) if cfg!(target_os = "macos") && error.raw_os_error() == Some(libc::EILSEQ) => {
            eprintln!("skip live byte-cwd fixture: filesystem rejects invalid UTF-8 ({error})");
            return;
        }
        Err(error) => panic!("create byte-cwd fixture: {error}"),
    }
    assert!(
        g.command()
            .args(["new-window", "-d", "-c"])
            .arg(&cwd)
            .arg(g.scratch.path().join("bin/codex"))
            .output()
            .unwrap()
            .status
            .success()
    );
    until(|| list(&g).as_array().unwrap().len() == 2);
    assert!(
        list(&g)
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["cwd"].as_str().unwrap().contains("bad-"))
    );
}
#[test]
fn shared_override_rejects_wrong_server() {
    if !enabled() {
        return;
    }
    let a = TmuxGuard::start();
    let b = TmuxGuard::start();
    list(&a);
    let output = b
        .sidecar()
        .env("TMUX_SIDECAR_RUNTIME_DIR", a.scratch.path().join("run"))
        .args(["daemon", "status"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(6));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("TMUX_SIDECAR_RUNTIME_DIR")
    );
}
#[test]
fn deleted_database_stops_daemon_and_is_logged() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    list(&g);
    fs::remove_file(g.scratch.path().join("state/tmux-sidecar.db")).unwrap();
    until(|| !g.scratch.path().join("run/sock").exists());
    let log = fs::read_to_string(g.scratch.path().join("state/daemon.log")).unwrap();
    assert!(
        log.contains("tmux-sidecar.db") && log.contains("history"),
        "{log}"
    );
    assert_eq!(list(&g).as_array().unwrap().len(), 1);
}
#[test]
fn corrupt_database_error_names_path() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let path = g.scratch.path().join("state/tmux-sidecar.db");
    fs::write(&path, b"corrupt database").unwrap();
    let output = g.sidecar().args(["daemon", "run"]).output().unwrap();
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains(path.to_str().unwrap()) && error.contains("move it aside"),
        "{error}"
    );
    let output = g.sidecar().args(["list", "--json"]).output().unwrap();
    let error = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(6));
    assert!(
        error.contains(path.to_str().unwrap()) && error.contains("move it aside"),
        "{error}"
    );
}
#[test]
fn oversized_frame_gets_error_before_close() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    list(&g);
    let mut stream = UnixStream::connect(g.scratch.path().join("run/sock")).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    // Keep writing bounded: daemon closes after reading the first byte over the limit.
    let _ = stream.write_all(&vec![b'x'; 1024 * 1024 + 1]);
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).unwrap();
    let value: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(value["error"]["code"], -32600);
    assert_eq!(value["error"]["message"], "frame exceeds 1 MiB");
}
#[test]
fn ended_runs_need_all_and_limit() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    list(&g);
    assert!(
        g.command()
            .args(["new-window", "-d", "sleep 60"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        g.command()
            .args(["kill-pane", "-t", "%0"])
            .output()
            .unwrap()
            .status
            .success()
    );
    until(|| list(&g).as_array().unwrap().is_empty());
    let history = ok(g
        .sidecar()
        .args(["list", "--all", "--limit", "1", "--json"])
        .output()
        .unwrap());
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(history[0]["state"], "done");
}
