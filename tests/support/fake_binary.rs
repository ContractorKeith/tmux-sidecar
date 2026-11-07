//! Reuse the same agent inodes: macOS scans each fresh unsigned executable.
use super::BoundedCommand;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};
static CACHE: OnceLock<tempfile::TempDir> = OnceLock::new();
extern "C" fn cleanup() {
    if let Some(root) = CACHE.get() {
        let _ = fs::remove_dir_all(root.path());
    }
}
fn cache() -> &'static Path {
    CACHE
        .get_or_init(|| {
            let root = tempfile::Builder::new()
                .prefix("tsx-agents-")
                .tempdir()
                .unwrap();
            let executable = root.path().join("fake-agent");
            let output = Command::new("rustc")
                .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_agent.rs"))
                .arg("-o")
                .arg(&executable)
                .bounded_output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            for agent in ["claude", "codex", "grok", "opencode", "opencode.exe"] {
                fs::copy(&executable, root.path().join(agent)).unwrap();
            }
            // SAFETY: cleanup takes no arguments and remains valid for the process lifetime.
            assert_eq!(unsafe { libc::atexit(cleanup) }, 0);
            root
        })
        .path()
}
pub fn link_agents(bin: &Path) {
    for agent in ["claude", "codex", "grok", "opencode", "opencode.exe"] {
        // A hard link preserves both the executable's name and its already-scanned inode.
        fs::hard_link(cache().join(agent), bin.join(agent)).unwrap();
    }
}

e2e_test!(fake_agents_reuse_copied_inodes, {
    use std::os::unix::fs::MetadataExt;
    let first = super::Scratch::new();
    let second = super::Scratch::new();
    link_agents(first.path());
    link_agents(second.path());
    for agent in ["claude", "codex", "grok", "opencode", "opencode.exe"] {
        let a = fs::symlink_metadata(first.path().join(agent)).unwrap();
        let b = fs::symlink_metadata(second.path().join(agent)).unwrap();
        assert!(a.is_file() && !a.file_type().is_symlink());
        assert_eq!((a.dev(), a.ino()), (b.dev(), b.ino()));
    }
});
