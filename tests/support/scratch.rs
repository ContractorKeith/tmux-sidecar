use std::path::Path;
pub struct Scratch(tempfile::TempDir);
impl Scratch {
    pub fn new() -> Self {
        let root = tempfile::Builder::new().prefix("tsx-").tempdir().unwrap();
        super::watchdog::register(root.path());
        Self(root)
    }
    pub fn path(&self) -> &Path {
        self.0.path()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        super::watchdog::unregister(self.path());
        if let Err(error) = super::cleanup::remove_tree(self.path()) {
            super::stage(&format!(
                "scratch removal {}: {error}",
                self.path().display()
            ));
            if !std::thread::panicking() {
                panic!("scratch removal failed: {error}");
            }
        }
    }
}
