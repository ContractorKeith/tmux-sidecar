use crate::seams::Process;
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
#[cfg(target_os = "macos")]
pub fn hook_ancestors() -> io::Result<Vec<(u32, Process)>> {
    let mut current = std::process::id();
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    while current > 1 && seen.insert(current) {
        let Some(process) = mac_process(current)? else {
            break;
        };
        let parent = process.ppid;
        if parent == 0 || seen.contains(&parent) {
            break;
        }
        let Some(process) = mac_process(parent)? else {
            break;
        };
        result.push((parent, process));
        current = parent;
    }
    Ok(result)
}
#[cfg(target_os = "macos")]
fn mac_process(pid: u32) -> io::Result<Option<Process>> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: info owns a writable buffer of exactly size bytes. PROC_PIDTBSDINFO
    // writes a proc_bsdinfo; it receives no other pointers and retains none.
    let count = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if count != size {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(None);
        }
        return Err(error);
    }
    // SAFETY: only a full-size successful response initializes the entire structure.
    let info = unsafe { info.assume_init() };
    let bytes: Vec<u8> = info
        .pbi_comm
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    Ok(Some(Process {
        ppid: info.pbi_ppid,
        comm: String::from_utf8_lossy(&bytes).into_owned(),
    }))
}
#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn hook_walk_has_no_process_spawn_path() {
        let walk = super::hook_ancestors().unwrap();
        // SAFETY: getppid has no arguments or pointers and only queries our parent.
        let parent = unsafe { libc::getppid() };
        assert_eq!(walk.first().map(|p| p.0), Some(parent as u32));
        // This module uses only libproc. Keep subprocess APIs out of the hook walk.
        let source = include_str!("ancestors.rs")
            .split("#[cfg(all(test")
            .next()
            .unwrap();
        assert!(!source.contains("Command"));
        assert!(!source.contains("PsProcessTree"));
        assert!(!source.contains(".spawn("));
    }
}
