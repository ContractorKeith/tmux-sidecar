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
}
