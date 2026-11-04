use std::{fs, io, path::Path};
mod process;
pub use process::output;
pub fn put(home: &Path, path: &str, text: &str) {
    let path = home.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
pub fn run(home: &Path, install: bool, chain: bool, dry: bool) -> io::Result<()> {
    tmux_sidecar::install::run(
        home,
        Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")),
        &[],
        install,
        chain,
        dry,
    )
}
