use super::*;
fn paths(root: &Path) -> Paths {
    Paths {
        runtime: root.join("run"),
        state: root.join("state"),
        config: root.join("config"),
    }
}
#[test]
fn stale_open_descriptor_retries_after_claim_was_read_and_deleted() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    append(&paths, b"first\n").unwrap();
    append_inner(&paths, b"late\n", || {
        let claims = claim(&paths.state)?;
        assert_eq!(read_claim(&claims[0])?, b"first\n");
        acknowledge(&claims[0])
    })
    .unwrap();
    assert_eq!(read(&paths.state).unwrap(), b"late\n");
}
#[test]
fn leftover_claims_are_returned_before_new_live_data() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    append(&paths, b"old\n").unwrap();
    let first = claim(&paths.state).unwrap();
    append(&paths, b"new\n").unwrap();
    let recovered = claim(&paths.state).unwrap();
    assert_eq!(recovered.len(), 2);
    assert_eq!(recovered[0], first[0]);
    assert_eq!(read_claim(&recovered[0]).unwrap(), b"old\n");
    assert_eq!(read_claim(&recovered[1]).unwrap(), b"new\n");
}
#[test]
fn two_hundred_parallel_appenders_need_no_lock_and_preserve_whole_frames() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    crate::paths::prepare(&paths).unwrap();
    let _lock = crate::client::FileLock::try_acquire(&paths.state.join("spool.lock")).unwrap();
    std::thread::scope(|scope| {
        for n in 0..200 {
            let paths = &paths;
            scope.spawn(move || {
                append(paths, format!("{n:03}:{}\n", "x".repeat(4096)).as_bytes()).unwrap()
            });
        }
    });
    let bytes = read(&paths.state).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let unique: std::collections::HashSet<_> = text.lines().filter(|s| !s.is_empty()).collect();
    assert_eq!(unique.len(), 200);
    assert_eq!(text.lines().filter(|s| !s.is_empty()).count(), 200);
    assert!(unique.iter().all(|s| s.len() == 4100));
}
