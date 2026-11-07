//! A detached non-agent helper avoids inheriting the coding agent running this test.
use super::{TmuxGuard, process::ChildGuard, watchdog};
use std::{
    fs,
    process::{Command, Stdio},
};
struct Detached(u32);
impl Drop for Detached {
    fn drop(&mut self) {
        watchdog::untrack(self.0);
        // SAFETY: this is the recorded setsid helper's pid and private process group.
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
            libc::kill(self.0 as i32, libc::SIGKILL);
        }
    }
}
pub fn check(guard: &TmuxGuard) {
    let root = guard.scratch.path();
    let script = r#"
import os,sys,time,json,subprocess,pathlib,signal
root=pathlib.Path(sys.argv[1]);pid=os.fork()
if pid:
    (root/'detached.pid.next').write_text(str(pid))
    (root/'detached.pid.next').replace(root/'detached.pid')
    deadline=time.monotonic()+5
    while not (root/'detach').exists() and time.monotonic()<deadline:time.sleep(.01)
    if not (root/'detach').exists():
        os.kill(pid,signal.SIGKILL);os.waitpid(pid,0);sys.exit(1)
    sys.exit(0)
os.setsid()
deadline=time.monotonic()+5
while os.getppid()!=1 and time.monotonic()<deadline:time.sleep(.01)
assert os.getppid()==1
result=subprocess.run([sys.argv[2],'hook','codex-notify',json.dumps({'type':'agent-turn-complete','thread-id':'none','cwd':str(root)})],input=b'',capture_output=True,timeout=2)
(root/'detached-result.next').write_text(json.dumps([result.returncode,result.stdout.decode(),result.stderr.decode()]))
(root/'detached-result.next').replace(root/'detached-result')
os._exit(0)
"#;
    let mut command = Command::new("python3");
    guard.isolate(&mut command);
    let mut parent = ChildGuard::new(
        command
            .args(["-c", script])
            .arg(root)
            .arg(env!("CARGO_BIN_EXE_tmux-sidecar"))
            .env("TMUX", format!("{},0,0", guard.socket().display()))
            .env("TMUX_PANE", "%0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    super::wait_for("detached helper pid", || root.join("detached.pid").exists());
    let pid = fs::read_to_string(root.join("detached.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let _detached = Detached(pid);
    watchdog::track(pid);
    fs::write(root.join("detach"), b"").unwrap();
    super::wait_for("detached helper result", || {
        root.join("detached-result").exists()
    });
    assert_eq!(
        fs::read_to_string(root.join("detached-result")).unwrap(),
        "[0, \"\", \"\"]"
    );
    super::wait_for("helper parent exit", || {
        parent.0.try_wait().unwrap().is_some()
    });
    assert!(parent.0.wait().unwrap().success());
    let log = fs::read_to_string(root.join("state/hook.log")).unwrap();
    assert!(log.ends_with("hook dropped: no agent ancestor\n"), "{log}");
    assert_eq!(&log[10..11], "T", "{log}");
    assert!(
        log.contains("agent=unknown format=codex-notify pane=%0"),
        "{log}"
    );
}
