use super::*;
use std::{
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    process::{Command, Stdio},
    time::{Duration, Instant},
};
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let until = Instant::now() + Duration::from_secs(1);
        while self.0.try_wait().ok().flatten().is_none() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
#[test]
fn panic_fixture() {
    if std::env::var_os("TMUX_SIDECAR_PANIC_FIXTURE").is_none() {
        return;
    }
    let _restore = Restore::enter().unwrap();
    let detail = "context ".repeat(8192);
    panic!("visible panic fixture: {detail}");
}
#[test]
fn panic_restores_terminal_before_printing_diagnostic() {
    let root = tempfile::tempdir().unwrap();
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: valid output pointers; null name/termios/winsize request defaults.
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(opened, 0);
    // SAFETY: openpty returned two distinct owned descriptors.
    let (mut master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
        // SAFETY: each descriptor is live; FD_CLOEXEC uses no pointers.
        let flags = unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
        assert_eq!(flags, 0);
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "ui::terminal::tests::panic_fixture",
            "--nocapture",
        ])
        .env("TMUX_SIDECAR_PANIC_FIXTURE", "1")
        .env("HOME", root.path())
        .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
        .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"))
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave.try_clone().unwrap()));
    // SAFETY: after fork these libc calls use no allocation or Rust synchronization;
    // stdin is our valid PTY slave and becomes this child's controlling terminal.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    // SAFETY: the master is owned and live; fcntl only changes its status flags.
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
    assert_eq!(flags, 0);
    let mut child = Child(command.spawn().unwrap());
    let until = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let status = loop {
        // PTY output buffers can be small (notably on macOS). Drain while the
        // child runs, otherwise printing its diagnostic can prevent exit.
        drain(&mut master, &mut bytes, until);
        if let Some(status) = child.0.try_wait().unwrap() {
            drain(&mut master, &mut bytes, until);
            break status;
        }
        assert!(
            Instant::now() < until,
            "panic fixture did not exit: {}",
            String::from_utf8_lossy(&bytes)
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(!status.success());
    let output = String::from_utf8_lossy(&bytes);
    let restored = output
        .find("\x1b[?1049l")
        .expect("alternate screen was not restored");
    let panic = output
        .find("visible panic fixture")
        .expect("panic diagnostic missing");
    assert!(restored < panic, "{output}");
    let mut modes = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: the slave remains open and modes points to writable termios storage.
    let read_modes = unsafe { libc::tcgetattr(slave.as_raw_fd(), modes.as_mut_ptr()) };
    assert_eq!(read_modes, 0);
    // SAFETY: successful tcgetattr initialized every field.
    let modes = unsafe { modes.assume_init() };
    assert_ne!(modes.c_lflag & libc::ICANON, 0);
    assert_ne!(modes.c_lflag & libc::ECHO, 0);
}

fn drain(master: &mut File, bytes: &mut Vec<u8>, deadline: Instant) {
    let mut buffer = [0; 4096];
    loop {
        assert!(Instant::now() < deadline, "PTY drain deadline exceeded");
        assert!(
            bytes.len() < 1024 * 1024,
            "panic fixture output exceeded 1 MiB"
        );
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&buffer[..n]),
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.raw_os_error() == Some(libc::EIO) =>
            {
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => panic!("{e}"),
        }
    }
}
