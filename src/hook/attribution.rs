use crate::{model::AgentKind, proc, seams::Process};
use std::{io, path::Path};

pub fn ancestor(chain: &[(u32, Process)], pane_pid: u32) -> Option<(u32, AgentKind)> {
    let end = chain.iter().position(|(pid, _)| *pid == pane_pid)?;
    chain[..=end]
        .iter()
        .rev()
        .find_map(|(pid, p)| proc::agent_kind(&p.comm).map(|agent| (*pid, agent)))
}
pub async fn pane_pid(runtime: &Path, socket: &Path, pane: &str, start: u64) -> io::Result<u32> {
    if let Ok(bytes) = super::files::read(&runtime.join("pane_pids"))
        && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
        && value["server_start"] == start
        && let Some(pid) = value["panes"][pane]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
    {
        return Ok(pid);
    }
    crate::tmux::hook_pane_pid(socket, pane).await
}
pub fn cache(runtime: &Path, panes: &[crate::seams::PaneSnapshot]) -> io::Result<()> {
    let Some(first) = panes.first() else {
        return Ok(());
    };
    let pids: std::collections::BTreeMap<_, _> =
        panes.iter().map(|p| (&p.pane_id, p.pane_pid)).collect();
    let next = runtime.join("pane_pids.next");
    use std::io::Write;
    let mut file = super::files::regular(
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true),
        &next,
    )?;
    file.write_all(&serde_json::to_vec(
        &serde_json::json!({"server_start":first.server_start,"panes":pids}),
    )?)?;
    std::fs::rename(next, runtime.join("pane_pids"))
}

pub async fn resolve(
    signal: &mut super::signal::Signal,
    chain: &[(u32, Process)],
    runtime: &Path,
    socket: &Path,
) {
    let mut pane_pid = pane_pid(runtime, socket, &signal.pane_id, signal.server_start)
        .await
        .ok();
    if pane_pid.is_some_and(|root| !chain.iter().any(|(pid, _)| *pid == root)) {
        pane_pid = crate::tmux::hook_pane_pid(socket, &signal.pane_id)
            .await
            .ok();
    }
    apply(signal, chain, pane_pid);
    if signal.orphaned
        && let Ok(cwd) = Path::new(&signal.cwd).canonicalize()
    {
        signal.cwd = cwd.to_string_lossy().into_owned();
    }
}
fn apply(signal: &mut super::signal::Signal, chain: &[(u32, Process)], pane_pid: Option<u32>) {
    if let Some(identity) = pane_pid.and_then(|root| ancestor(chain, root)) {
        (signal.agent_pid, signal.agent) = identity;
        signal.pane_verified = true;
    } else {
        let nearest = chain
            .iter()
            .position(|(_, p)| proc::agent_kind(&p.comm).is_some());
        // Failed lookups do not prove orphaning. A direct child of init does.
        signal.orphaned = pane_pid.is_some() || nearest.is_some_and(|i| chain[i].1.ppid == 1);
        if signal.orphaned
            && let Some(i) = nearest
        {
            signal.parent_agent_pid = chain[i + 1..]
                .iter()
                .find(|(_, p)| proc::agent_kind(&p.comm) == Some(signal.agent))
                .map(|(pid, _)| *pid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_host_maps_to_outermost_agent_inside_the_pane() {
        let p = |ppid, comm: &str| Process {
            ppid,
            comm: comm.into(),
        };
        let chain = vec![
            (30, p(20, "opencode.exe")),
            (20, p(10, "opencode.exe")),
            (10, p(1, "bash")),
            (1, p(0, "init")),
        ];
        assert_eq!(ancestor(&chain, 10), Some((20, AgentKind::Opencode)));
        assert_eq!(ancestor(&chain, 20), Some((20, AgentKind::Opencode)));
        assert_eq!(ancestor(&chain, 99), None);
    }
    #[test]
    fn init_parent_proves_orphaning_but_failed_lookup_alone_does_not() {
        let mut signal: super::super::signal::Signal=serde_json::from_value(serde_json::json!({"server_start":1,"pane_id":"%0","agent":"opencode","agent_pid":30,"at":100,"nonce":format!("{:032x}",100),"source":"hook:opencode","event":"permission.asked","cwd":"/work","payload":{}})).unwrap();
        let mut chain = vec![
            (
                30,
                Process {
                    ppid: 20,
                    comm: "opencode.exe".into(),
                },
            ),
            (
                20,
                Process {
                    ppid: 1,
                    comm: "bash".into(),
                },
            ),
            (
                1,
                Process {
                    ppid: 0,
                    comm: "init".into(),
                },
            ),
        ];
        apply(&mut signal, &chain, None);
        assert!(!signal.orphaned);
        chain[0].1.ppid = 1;
        chain.remove(1);
        apply(&mut signal, &chain, None);
        assert!(signal.orphaned);
        assert_eq!(signal.payload, serde_json::json!({}));
    }
}
