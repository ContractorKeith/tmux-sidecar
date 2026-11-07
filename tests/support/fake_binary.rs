//! Compile once per test process; parallel rustc jobs distort the hook latency check.
use super::BoundedCommand;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};
static BINARY: OnceLock<Vec<u8>> = OnceLock::new();
pub fn write(path: &Path) {
    let bytes = BINARY.get_or_init(|| {
        let root = crate::support::Scratch::new();
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
        fs::read(executable).unwrap()
    });
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
