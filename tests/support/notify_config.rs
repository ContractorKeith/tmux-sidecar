//! Tests never deliver desktop notifications to the host's notification service.
pub fn disable(root: &std::path::Path) {
    for relative in [
        "config/tmux-sidecar",
        "home/Library/Application Support/tmux-sidecar",
    ] {
        let path = root.join(relative);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("config.toml"),
            "[notify]\ndesktop=false\ntmux=false\n",
        )
        .unwrap();
    }
}
