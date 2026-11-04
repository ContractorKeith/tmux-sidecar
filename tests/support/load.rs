//! Keep the deliberate twenty-server storm separate from hook latency proofs.
use std::{
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::{Duration, Instant},
};
static LOAD: RwLock<()> = RwLock::new(());
pub enum Guard {
    Normal {
        _guard: RwLockReadGuard<'static, ()>,
    },
    Stress {
        _guard: RwLockWriteGuard<'static, ()>,
    },
}
pub fn enter(stress: bool) -> Guard {
    let deadline = Instant::now() + Duration::from_secs(50);
    loop {
        // There is no protected data; one failed test must not poison other bodies.
        LOAD.clear_poison();
        if stress {
            if let Ok(guard) = LOAD.try_write() {
                return Guard::Stress { _guard: guard };
            }
        } else if let Ok(guard) = LOAD.try_read() {
            return Guard::Normal { _guard: guard };
        }
        assert!(Instant::now() < deadline, "test load gate exceeded 50 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}
