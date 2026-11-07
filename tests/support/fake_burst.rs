//! Preserve the original 200-hook concurrency while scaling the number of deliveries.
use super::{ChildGuard, command};
use std::{
    fs,
    process::Stdio,
    sync::{Arc, Barrier},
    time::{Duration, Instant},
};
pub fn burst(binary: &str, fixture: &str, count: usize) {
    for start in (0..count).step_by(200) {
        let wave = (count - start).min(200);
        let barrier = Arc::new(Barrier::new(wave));
        std::thread::scope(|scope| {
            for _ in 0..wave {
                let barrier = barrier.clone();
                scope.spawn(move || {
                    barrier.wait();
                    let mut child = ChildGuard(
                        command(binary)
                            .args(["hook", "claude"])
                            .stdin(fs::File::open(fixture).unwrap())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()
                            .unwrap(),
                    );
                    let deadline = Instant::now() + Duration::from_secs(5);
                    loop {
                        if let Some(status) = child.0.try_wait().unwrap() {
                            assert!(status.success());
                            break;
                        }
                        assert!(Instant::now() < deadline, "hook child exceeded 5 s");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                });
            }
        });
    }
}
