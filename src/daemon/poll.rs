use super::store_worker::StoreWorker;
use crate::{
    engine,
    proc::PsProcessTree,
    seams::{ProcessTree, Tmux},
    store::Store,
    tmux::{RealTmux, Socket},
};
use std::{io, path::PathBuf};
pub struct Failure {
    pub error: io::Error,
    pub fatal: bool,
}
impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self {
            error,
            fatal: false,
        }
    }
}

pub async fn tick(socket: PathBuf, worker: &StoreWorker) -> Result<bool, Failure> {
    worker
        .with(move |store| {
            store
                .verify_identity()
                .map_err(|error| Failure { error, fatal: true })?;
            let tmux = RealTmux {
                socket: Socket::Path(socket),
            };
            let scan = match tmux.scan() {
                Ok(scan) => scan,
                Err(error) if tmux.confirmed_gone(&error) => {
                    store.end_all(super::now())?;
                    return Ok(false);
                }
                Err(error) => return Err(error.into()),
            };
            apply_scan(scan, &PsProcessTree, store)?;
            Ok(true)
        })
        .await
        .map_err(Failure::from)?
}
fn apply_scan(
    scan: crate::tmux::PaneScan,
    tree: &impl ProcessTree,
    store: &Store,
) -> io::Result<()> {
    let processes = tree.snapshot()?;
    let now = super::now();
    let changes = engine::detect_scan(
        &scan.panes,
        &scan.skipped,
        &processes,
        &store.active()?,
        now,
    );
    store.apply(&scan.panes, &changes, now)
}
/// A good tick resets the failure budget; transient errors do not alter persisted runs.
#[derive(Default)]
pub struct Health {
    failures: u8,
}
impl Health {
    pub fn record(&mut self, result: &Result<bool, Failure>) -> bool {
        match result {
            Ok(_) => {
                self.failures = 0;
                false
            }
            Err(f) => {
                self.failures += 1;
                f.fatal || self.failures >= 5
            }
        }
    }
    pub fn failures(&self) -> u8 {
        self.failures
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transient_failures_recover_and_five_failures_stop() {
        let mut health = Health::default();
        let fail = || Err(Failure::from(io::Error::other("ps failed")));
        assert!(!health.record(&fail()));
        assert!(!health.record(&fail()));
        assert!(!health.record(&Ok(true)));
        assert_eq!(health.failures(), 0);
        for _ in 0..4 {
            assert!(!health.record(&fail()));
        }
        assert!(health.record(&fail()));
    }
}
