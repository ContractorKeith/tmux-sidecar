use super::store_worker::StoreWorker;
use crate::{
    engine,
    proc::PsProcessTree,
    seams::{ProcessTree, Tmux},
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

enum Snapshot {
    Alive(crate::tmux::PaneScan, crate::seams::ProcessSnapshot),
    Gone,
}
pub async fn tick(socket: PathBuf, worker: &StoreWorker) -> Result<bool, Failure> {
    worker
        .with(|store| store.verify_identity())
        .await?
        .map_err(|error| Failure { error, fatal: true })?;
    // External commands can be slow. Do not occupy the SQLite owner while collecting.
    let snapshot = tokio::task::spawn_blocking(move || -> io::Result<Snapshot> {
        let tmux = RealTmux {
            socket: Socket::Path(socket),
        };
        let scan = match tmux.scan() {
            Ok(scan) => scan,
            Err(error) if tmux.confirmed_gone(&error) => return Ok(Snapshot::Gone),
            Err(error) => return Err(error),
        };
        Ok(Snapshot::Alive(scan, PsProcessTree.snapshot()?))
    })
    .await
    .map_err(|error| Failure::from(io::Error::other(error)))??;
    worker
        .with(move |store| -> Result<bool, Failure> {
            store
                .verify_identity()
                .map_err(|error| Failure { error, fatal: true })?;
            match snapshot {
                Snapshot::Gone => {
                    store.end_all(super::now())?;
                    Ok(false)
                }
                Snapshot::Alive(scan, processes) => {
                    let now = super::now();
                    let changes = engine::detect_scan(
                        &scan.panes,
                        &scan.skipped,
                        &processes,
                        &store.active()?,
                        now,
                    );
                    store.apply(&scan.panes, &changes, now)?;
                    Ok(true)
                }
            }
        })
        .await?
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
