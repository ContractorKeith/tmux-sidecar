//! Bound ordinary process storms too: many-core hosts otherwise run every probe at once.
//! Keep the deliberate twenty-server storm separate from hook latency proofs.
use std::{
    sync::{
        RwLock, RwLockReadGuard, RwLockWriteGuard,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
static NORMAL: AtomicUsize = AtomicUsize::new(0);
pub struct Slot;
impl Drop for Slot {
    fn drop(&mut self) {
        NORMAL.fetch_sub(1, Ordering::Relaxed);
    }
}
static LOAD: RwLock<()> = RwLock::new(());
pub enum Guard {
    Normal {
        _slot: Slot,
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
        } else if NORMAL
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < 8).then_some(n + 1)
            })
            .is_ok()
        {
            let slot = Slot;
            if let Ok(guard) = LOAD.try_read() {
                return Guard::Normal {
                    _slot: slot,
                    _guard: guard,
                };
            }
        }
        assert!(Instant::now() < deadline, "test load gate exceeded 50 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}
