#[test]
fn database_write_error_names_wal_and_shm_permissions() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("absent/history.db");
    let error = tmux_sidecar::store::Store::open(&path)
        .err()
        .unwrap()
        .to_string();
    for expected in [
        path.to_str().unwrap(),
        "-shm",
        "-wal",
        "permissions",
        "directory",
    ] {
        assert!(error.contains(expected), "missing {expected}: {error}");
    }
}
