use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
#[test]
fn optional_agents_skip_and_explicit_or_marked_hooks_fail() {
    use crate::model::AgentKind;
    let root = tempfile::tempdir().unwrap();
    let binary = Path::new("/scratch/current binary");
    let store = root.path().join("projects");
    let stores = [(AgentKind::Claude, store.clone())];
    let checks = agents::checks(root.path(), binary, &[], &stores);
    assert!(checks.iter().all(|c| c.status == Status::Skip));
    assert!(!checks[0].human().contains("fix:"));
    let checks = agents::checks(root.path(), binary, &[AgentKind::Claude], &stores);
    assert_eq!(checks[0].status, Status::Fail);
    assert_eq!(checks[1].status, Status::Skip);
    assert!(checks[0].human().contains("fix:"));
    crate::install::run(
        root.path(),
        binary,
        &[AgentKind::Claude],
        true,
        false,
        false,
    )
    .unwrap();
    let checks = agents::checks(root.path(), binary, &[], &stores);
    assert_eq!(checks[0].status, Status::Ok);
    assert_eq!(checks[1].status, Status::Fail);
    fs::create_dir(&store).unwrap();
    fs::write(store.join("unreadable-child"), "ignored").unwrap();
    fs::set_permissions(
        store.join("unreadable-child"),
        fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    let checks = agents::checks(root.path(), binary, &[], &stores);
    assert!(checks.iter().all(|c| c.status == Status::Ok));
    let alias = root.path().join("alias");
    symlink(&store, &alias).unwrap();
    assert!(inspect::readable(&alias).is_ok());
    fs::set_permissions(&store, fs::Permissions::from_mode(0o000)).unwrap();
    let checks = agents::checks(root.path(), binary, &[], &stores);
    assert_eq!(checks[1].status, Status::Fail);
    crate::install::run(
        root.path(),
        binary,
        &[AgentKind::Claude],
        false,
        false,
        false,
    )
    .unwrap();
    let checks = agents::checks(root.path(), binary, &[], &stores);
    assert_eq!(checks[0].status, Status::Skip);
    assert_eq!(checks[1].status, Status::Fail);
    fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).unwrap();
    let check = Check::skip("store claude", "path with spaces/é");
    assert_eq!(check.human(), "skip store claude: path with spaces/é");
}

#[test]
fn every_agent_is_optional_until_requested_or_marked() {
    use crate::model::AgentKind;
    for agent in [
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Grok,
        AgentKind::Opencode,
    ] {
        let home = tempfile::tempdir().unwrap();
        let binary = Path::new("/current binary");
        let stores = [(agent, home.path().join("missing-store"))];
        assert!(
            agents::checks(home.path(), binary, &[], &stores)
                .iter()
                .all(|c| c.status == Status::Skip)
        );
        let explicit = agents::checks(home.path(), binary, &[agent], &stores);
        assert_eq!(explicit[0].status, Status::Fail);
        crate::install::run(home.path(), binary, &[agent], true, false, false).unwrap();
        let marked = agents::checks(home.path(), binary, &[], &stores);
        assert_eq!(marked[0].status, Status::Ok);
        assert_eq!(marked[1].status, Status::Fail);
        let drifted = agents::checks(home.path(), Path::new("/old binary"), &[], &stores);
        assert_eq!(drifted[0].status, Status::Fail);
    }
}
#[test]
fn broken_store_symlink_is_a_failure_even_for_an_unhooked_agent() {
    use crate::model::AgentKind;
    let home = tempfile::tempdir().unwrap();
    let link = home.path().join("store");
    symlink(home.path().join("absent-target"), &link).unwrap();
    let checks = agents::checks(
        home.path(),
        Path::new("/binary"),
        &[],
        &[(AgentKind::Claude, link)],
    );
    assert_eq!(checks[0].status, Status::Skip);
    assert_eq!(checks[1].status, Status::Fail);
}
