//! Both settings formats retain CRLF after install and edited uninstall.
use std::{fs, path::Path};
use tmux_sidecar::{install, model::AgentKind};
#[test]
fn crlf_settings_survive_install_and_user_edits_before_uninstall() {
    for (agent, file, before) in [
        (
            AgentKind::Claude,
            ".claude/settings.json",
            "{\r\n  \"model\": \"user\"\r\n}\r\n",
        ),
        (
            AgentKind::Codex,
            ".codex/config.toml",
            "# user comment\r\nmodel = 'user'\r\n",
        ),
    ] {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, before).unwrap();
        install::run(
            home.path(),
            Path::new("/bin/sidecar"),
            &[agent],
            true,
            false,
            false,
        )
        .unwrap();
        let installed = fs::read_to_string(&path).unwrap();
        assert!(
            !installed.replace("\r\n", "").contains('\n'),
            "{file} lost CRLF"
        );
        fs::write(&path, installed.replace("user", "edited")).unwrap();
        install::run(
            home.path(),
            Path::new("/bin/sidecar"),
            &[agent],
            false,
            false,
            false,
        )
        .unwrap();
        let removed = fs::read_to_string(&path).unwrap();
        assert!(removed.contains("edited"));
        assert!(
            !removed.replace("\r\n", "").contains('\n'),
            "{file} lost CRLF on uninstall"
        );
    }
}
