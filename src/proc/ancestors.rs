use crate::seams::Process;
#[cfg(target_os = "linux")]
use std::collections::HashSet;
use std::io;
#[cfg(target_os = "linux")]
pub fn hook_ancestors() -> io::Result<Vec<(u32, Process)>> {
    let mut current = std::process::id();
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    while current > 1 && seen.insert(current) {
        let text = std::fs::read_to_string(format!("/proc/{current}/stat"))?;
        let (_, tail) = text
            .rsplit_once(") ")
            .ok_or_else(|| io::Error::other("invalid process stat"))?;
        let parent = tail
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| io::Error::other("missing ppid"))?
            .parse::<u32>()
            .map_err(io::Error::other)?;
        let parent_text = match std::fs::read_to_string(format!("/proc/{parent}/stat")) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => break,
            Err(e) => return Err(e),
        };
        let (_, tail) = parent_text
            .rsplit_once(") ")
            .ok_or_else(|| io::Error::other("invalid parent stat"))?;
        let ppid = tail
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| io::Error::other("missing parent ppid"))?
            .parse()
            .map_err(io::Error::other)?;
        let comm = parent_text
            .split_once('(')
            .and_then(|(_, rest)| rest.rsplit_once(") ").map(|(name, _)| name))
            .ok_or_else(|| io::Error::other("missing process comm"))?;
        result.push((
            parent,
            Process {
                ppid,
                comm: comm.to_owned(),
            },
        ));
        current = parent;
    }
    Ok(result)
}
#[cfg(not(target_os = "linux"))]
pub fn hook_ancestors() -> io::Result<Vec<(u32, Process)>> {
    use crate::seams::ProcessTree;
    super::PsProcessTree.ancestors(std::process::id())
}
