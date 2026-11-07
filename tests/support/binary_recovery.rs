use super::{TmuxGuard, enabled, lifecycle::list, support::output};
use std::{fs, os::unix::fs::PermissionsExt};
e2e_test!(hook_still_starts_daemon_without_a_local_tmux, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    list(&g);
    super::lifecycle::stop(&g);
    assert!(
        output(
            g.command()
                .args(["respawn-pane", "-k", "-t", "%0"])
                .arg("/usr/bin/env")
                .arg(format!("PATH={}", g.scratch.path().join("bin").display()))
                .arg(g.scratch.path().join("bin/claude"))
        )
        .status
        .success()
    );
    let run = current_run(&g);
    super::cli_verbs::fire(&g, &serde_json::to_value(&run).unwrap(), "Stop");
    until!(
        || fs::read_to_string(g.scratch.path().join("state/daemon.log"))
            .unwrap_or_default()
            .contains("initial poll failure: tmux not found (looked in PATH=")
    );
    until!(|| g.scratch.path().join("run/sock").exists());
    super::lifecycle::stop(&g);
});

e2e_test!(poll_failure_re_resolves_a_still_executable_broken_pin, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("tmux"))
        .find(|p| p.is_file())
        .unwrap();
    let pinned = g.scratch.path().join("pinned-tmux");
    fs::write(
        &pinned,
        format!(
            "#!/bin/sh\n[ ! -e \"$HOME/break-pin\" ] || {{ echo no server running >&2; exit 1; }}\ntouch \"$HOME/pin-used\"\nexec '{}' \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&pinned, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        output(
            g.sidecar()
                .env("TMUX_SIDECAR_TMUX", &pinned)
                .args(["list", "--json"])
        )
        .status
        .success()
    );
    let original = list(&g)[0]["id"].clone();
    fs::write(g.scratch.path().join("home/break-pin"), b"").unwrap();
    until!(
        || fs::read_to_string(g.scratch.path().join("state/daemon.log"))
            .unwrap_or_default()
            .contains("falling back to PATH")
    );
    until!(|| list(&g)[0]["stale"] == false);
    assert_eq!(list(&g)[0]["id"], original);
    fs::remove_file(g.scratch.path().join("home/pin-used")).unwrap();
    fs::remove_file(g.scratch.path().join("home/break-pin")).unwrap();
    until!(|| g.scratch.path().join("home/pin-used").exists());
});

e2e_test!(hook_fallback_probes_stay_silent, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    list(&g);
    super::lifecycle::stop(&g);
    assert!(
        output(
            g.command()
                .args(["respawn-pane", "-k", "-t", "%0"])
                .arg("/usr/bin/env")
                .arg(format!(
                    "TMUX_SIDECAR_TMUX={}",
                    g.scratch.path().join("missing-tmux").display()
                ))
                .arg(g.scratch.path().join("bin/claude"))
        )
        .status
        .success()
    );
    for file in ["run/server_start", "run/pane_pids"] {
        let _ = fs::remove_file(g.scratch.path().join(file));
    }
    let run = current_run(&g);
    // fire asserts the hook's stdout/stderr are empty as well as its latency and exit code.
    super::cli_verbs::fire(&g, &serde_json::to_value(&run).unwrap(), "Stop");
    // Hook startup is detached; own its lifecycle before scratch removal.
    until!(|| g.scratch.path().join("run/sock").exists());
    super::lifecycle::stop(&g);
});

pub(super) fn current_run(g: &TmuxGuard) -> tmux_sidecar::model::AgentRun {
    use tmux_sidecar::seams::{ProcessTree, Tmux};
    let mut run = None;
    super::support::wait_for("respawned fake agent and valid pane metadata", || {
        let Ok(panes) = g.adapter.list_panes() else {
            return false;
        };
        let Ok(processes) = tmux_sidecar::proc::PsProcessTree.snapshot() else {
            return false;
        };
        run = tmux_sidecar::engine::detect(&panes, &processes, &[], tmux_sidecar::daemon::now())
            .start
            .pop();
        run.is_some()
    });
    run.unwrap()
}
