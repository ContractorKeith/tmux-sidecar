use super::{
    TmuxGuard,
    cli_verbs::fire,
    enabled,
    lifecycle::list,
    support::{ChildGuard, output},
};
use std::{
    fs,
    process::Stdio,
    time::{Duration, Instant},
};
struct Resume(i32);
impl Drop for Resume {
    fn drop(&mut self) {
        // SAFETY: this pid belongs to the isolated tmux fixture.
        unsafe {
            libc::kill(self.0, libc::SIGCONT);
        }
    }
}
e2e_test!(
    stopped_tmux_send_times_out_without_blocking_hooks_or_late_paste,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start_capture();
        let run = list(&guard)[0].clone();
        let pid: i32 =
            String::from_utf8(output(guard.command().args(["display", "-p", "#{pid}"])).stdout)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
        let resume = Resume(pid);
        // SAFETY: only this test's isolated tmux server is stopped.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);
        let began = Instant::now();
        let mut send = ChildGuard::new(
            guard
                .sidecar()
                .args(["send", "%0", "must not arrive", "--force"])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        // Wait until send has begun probing, then deliver independent hook evidence.
        // The daemon's first scan starts immediately; hook retries also cover scheduler order.
        fire(&guard, &run, "UserPromptSubmit");
        let deadline = Instant::now() + Duration::from_secs(1);
        while list(&guard)[0]["state"] != "working" {
            assert!(
                Instant::now() < deadline,
                "hook must commit while send probes a stopped tmux"
            );
            std::thread::yield_now();
        }
        let deadline = began + Duration::from_secs(4);
        while send.0.try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < deadline,
                "send exceeded its daemon deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(send.0.wait().unwrap().code(), Some(4));
        drop(resume);
        assert!(output(guard.command().arg("list-panes")).status.success());
        let input = guard
            .scratch
            .path()
            .join(format!("state/input-{}", run["agent_pid"]));
        assert_eq!(fs::read(&input).unwrap(), b"");
        super::cli_verbs::cli(&guard, &["send", "%0", "fresh", "--force"], 0);
        until!(|| fs::read(&input).unwrap() == b"\x1b[200~fresh\x1b[201~\r");
    }
);
