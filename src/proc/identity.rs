//! Resolve exact agent identities without starting subprocesses.
use super::agent_kind;
use std::path::Path;
fn basename(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let name = Path::new(text).file_name()?.to_str()?;
    agent_kind(name).map(|_| name.to_owned())
}
pub(super) fn comm(pid: u32, comm: &str) -> String {
    if agent_kind(comm).is_some() {
        return comm.to_owned();
    }
    names(pid)
        .into_iter()
        .flatten()
        .find_map(|s| basename(&s))
        .unwrap_or_else(|| comm.to_owned())
}
#[cfg(target_os = "linux")]
fn names(pid: u32) -> [Option<Vec<u8>>; 2] {
    use std::os::unix::ffi::OsStrExt;
    let argv = std::fs::read(format!("/proc/{pid}/cmdline"))
        .ok()
        .map(|v| v.split(|b| *b == 0).next().unwrap_or_default().to_vec());
    let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|p| p.as_os_str().as_bytes().to_vec());
    [argv, exe]
}
#[cfg(target_os = "macos")]
fn names(pid: u32) -> [Option<Vec<u8>>; 2] {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
    let mut bytes = vec![0u8; 1024 * 1024];
    let mut size = bytes.len();
    // SAFETY: mib and bytes own buffers of the supplied lengths; sysctl retains neither.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return [None, None];
    }
    bytes.truncate(size);
    mac_names(&bytes)
}
#[cfg(any(test, target_os = "macos"))]
fn mac_names(bytes: &[u8]) -> [Option<Vec<u8>>; 2] {
    // KERN_PROCARGS2: argc, executable path, NUL padding, then argv[0].
    let Some(rest) = bytes.get(std::mem::size_of::<libc::c_int>()..) else {
        return [None, None];
    };
    let Some(end) = rest.iter().position(|b| *b == 0) else {
        return [None, None];
    };
    let exe = rest[..end].to_vec();
    let tail = &rest[end..];
    let argv = tail.iter().position(|b| *b != 0).map(|start| {
        tail[start..]
            .split(|b| *b == 0)
            .next()
            .unwrap_or_default()
            .to_vec()
    });
    [argv, Some(exe)]
}
#[cfg(test)]
mod tests {
    #[test]
    fn mac_procargs_preserves_symlink_argv0() {
        let mut bytes = 2i32.to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"/opt/grok-1.0.77-macos-aarch64\0\0\0/usr/local/bin/grok\0arg\0");
        let names = super::mac_names(&bytes);
        assert_eq!(
            super::basename(names[0].as_ref().unwrap()).as_deref(),
            Some("grok")
        );
        assert_eq!(super::basename(names[1].as_ref().unwrap()), None);
        assert_eq!(super::basename(b"/bin/not-grok"), None);
    }
}
