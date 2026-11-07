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
    Alive(
        crate::tmux::PaneScan,
        crate::seams::ProcessSnapshot,
        std::collections::BTreeMap<u32, PathBuf>,
    ),
    Gone,
}
pub async fn tick(
    socket: PathBuf,
    worker: &StoreWorker,
    runtime: PathBuf,
    idle_after_ms: u64,
    send_confirm_ms: u64,
) -> Result<bool, Failure> {
    worker
        .with(|store| store.verify_identity())
        .await?
        .map_err(|error| Failure { error, fatal: true })?;
    // External commands can be slow. Do not occupy the SQLite owner while collecting.
    let snapshot = tokio::task::spawn_blocking(move || -> io::Result<Snapshot> {
        let tmux = RealTmux {
            socket: Socket::Path(socket.clone()),
        };
        let scan = match tmux.scan() {
            Ok(scan) => scan,
            Err(error) => {
                crate::tmux::retry_path();
                match tmux.scan() {
                    Ok(scan) => scan,
                    Err(second) if tmux.confirmed_gone(&error) || tmux.confirmed_gone(&second) => {
                        return Ok(Snapshot::Gone);
                    }
                    Err(_) => return Err(error),
                }
            }
        };
        if let Some(pane) = scan.panes.first() {
            crate::hook::context::write(&runtime, pane.server_start, &socket)?;
        }
        crate::hook::attribution::cache(&runtime, &scan.panes)?;
        let processes = PsProcessTree.snapshot()?;
        let cwds = processes
            .iter()
            .filter(|(_, p)| crate::proc::agent_kind(&p.comm).is_some())
            .filter_map(|(&pid, _)| PsProcessTree.cwd(pid).ok().map(|cwd| (pid, cwd)))
            .collect();
        Ok(Snapshot::Alive(scan, processes, cwds))
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
                Snapshot::Alive(scan, processes, cwds) => {
                    let now = super::now();
                    let mut changes = engine::detect_scan(
                        &scan.panes,
                        &scan.skipped,
                        &processes,
                        &store.active()?,
                        now,
                    );
                    for run in &mut changes.start {
                        if let Some(cwd) = cwds.get(&run.agent_pid) {
                            run.cwd = cwd.to_string_lossy().into_owned();
                        }
                    }
                    store.apply(&scan.panes, &changes, now)?;
                    store.process_cwds(&cwds)?;
                    store.maintain_with_confirmation(
                        &scan.panes,
                        now,
                        idle_after_ms,
                        send_confirm_ms,
                    )?;
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
    resource_failures: u8,
}
impl Health {
    pub fn record(&mut self, result: &Result<bool, Failure>) -> bool {
        match result {
            Ok(_) => {
                self.failures = 0;
                self.resource_failures = 0;
                false
            }
            Err(f) => {
                let exhausted = matches!(f.error.raw_os_error(), Some(libc::EMFILE | libc::ENFILE));
                if exhausted {
                    self.failures = 0;
                    self.resource_failures = self.resource_failures.saturating_add(1);
                } else {
                    self.resource_failures = 0;
                    self.failures = self.failures.saturating_add(1);
                }
                f.fatal || self.failures >= 5 || self.resource_failures >= 60
            }
        }
    }
    pub fn failures(&self) -> u8 {
        self.failures.max(self.resource_failures)
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

#[cfg(test)]
mod resource_tests {
    use super::*;
    #[test]
    fn descriptor_exhaustion_has_a_separate_sixty_tick_budget() {
        let mut health = Health::default();
        let exhausted = || Err(Failure::from(io::Error::from_raw_os_error(libc::EMFILE)));
        for _ in 0..59 {
            assert!(!health.record(&exhausted()));
        }
        assert!(health.record(&exhausted()));
        assert!(!health.record(&Ok(true)));
        for _ in 0..4 {
            assert!(!health.record(&Err(Failure::from(io::Error::other("ps failed")))));
        }
        assert!(health.record(&Err(Failure::from(io::Error::other("ps failed")))));
    }
}

pub(super) fn publish(worker: &StoreWorker, result: &Result<bool, Failure>) {
    let next = result.as_ref().err().map(|f| {
        let reason = f.error.to_string();
        if reason.is_empty() {
            "pane/process poll failed".into()
        } else {
            reason
        }
    });
    if *worker.poll_error.borrow() != next {
        worker.poll_error.send_replace(next);
        worker
            .views
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
}
