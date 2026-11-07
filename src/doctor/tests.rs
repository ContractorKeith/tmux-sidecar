use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
#[test]
fn real_tmux_version_forms_and_missing_or_old_versions() {
    for version in ["tmux 3.3a", "tmux next-3.6", "tmux openbsd-7.5", "tmux 3.8"] {
        assert!(inspect::tmux_version(version).is_ok(), "{version}");
    }
    for version in ["", "tmux 3.2a", "tmux openbsd-7.2", "garbage"] {
        assert!(inspect::tmux_version(version).is_err(), "{version}");
    }
    assert!(!Check::new("tmux", Err(io::Error::other("not found")), "install").ok);
}
#[test]
fn daemon_unreachable_skew_and_matching() {
    assert!(!daemon_check(Err(io::Error::other("unreachable"))).ok);
    let skew = daemon_check(Ok(serde_json::json!({"daemon_version":"older"})));
    assert!(!skew.ok);
    assert!(skew.fix.contains("daemon stop"));
    assert!(
        daemon_check(Ok(
            serde_json::json!({"daemon_version": env!("CARGO_PKG_VERSION")})
        ))
        .ok
    );
}
#[test]
fn readable_stores_notifier_and_private_directories() {
    let root = tempfile::tempdir().unwrap();
    assert!(inspect::readable(root.path()).is_ok());
    let file = root.path().join("store");
    assert!(inspect::readable(&file).is_err());
    assert!(!inspect::executable(&file));
    fs::write(&file, "fixture").unwrap();
    assert!(inspect::readable(&file).is_ok());
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
    assert!(inspect::readable(&file).is_err());
    fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(inspect::executable(&file));
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(inspect::permissions(root.path(), paths::uid()).is_ok());
    assert!(inspect::permissions(root.path(), paths::uid().wrapping_add(1)).is_err());
    let alias = root.path().join("link");
    symlink(root.path(), &alias).unwrap();
    assert!(inspect::permissions(&alias, paths::uid()).is_err());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(inspect::permissions(root.path(), paths::uid()).is_err());
}
#[test]
fn all_hooks_missing_installed_and_drifted() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let binary = Path::new("/scratch/current binary");
    for agent in ["claude", "codex", "grok", "opencode"] {
        assert!(hooks::check(home, binary, agent).is_err());
    }
    crate::install::run(home, binary, &[], true, false, false).unwrap();
    for agent in ["claude", "codex", "grok", "opencode"] {
        assert!(hooks::check(home, binary, agent).is_ok(), "{agent}");
        assert!(
            hooks::check(home, Path::new("/different"), agent).is_err(),
            "{agent}"
        );
    }
    let path = home.join(".claude/settings.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    value["hooks"]["Stop"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"hooks":[{"type":"command","command":"my-user-hook"}]}));
    fs::write(path, serde_json::to_string(&value).unwrap()).unwrap();
    assert!(hooks::check(home, binary, "claude").is_ok());
    crate::install::run(home, binary, &[], false, false, false).unwrap();
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(
        home.join(".codex/config.toml"),
        "notify = ['old-program', 'arg']\n",
    )
    .unwrap();
    crate::install::run(home, binary, &[], true, true, false).unwrap();
    assert!(hooks::check(home, binary, "codex").is_ok());
    assert!(hooks::check(home, Path::new("/different"), "codex").is_err());
    fs::write(
        home.join(".grok/config.toml"),
        "claude_hooks_enabled = false\n",
    )
    .unwrap();
    assert!(hooks::check(home, binary, "grok").is_err());
    crate::install::run(home, binary, &[], true, true, false).unwrap();
    assert!(hooks::check(home, binary, "grok").is_ok());
}
