use super::*;
#[derive(Default)]
struct Fake {
    sent: Vec<String>,
    debounce: Debounce,
}
impl Fake {
    fn transition(&mut self, config: &Config, id: &str, to: &str, at: Instant) {
        if self.debounce.allow(config, id, to, at) {
            self.sent.push(format!("{id}:{to}"));
        }
    }
}
#[test]
fn only_blocked_and_error_fire_with_per_run_and_global_debounce() {
    let mut fake = Fake::default();
    let config = Config {
        tmux: false,
        ..Config::default()
    };
    let now = Instant::now();
    for state in ["working", "waiting", "idle", "done"] {
        fake.transition(&config, "r1", state, now);
    }
    assert!(fake.sent.is_empty());
    fake.transition(&config, "r1", "blocked", now);
    fake.transition(&config, "r2", "error", now + Duration::from_secs(1));
    fake.transition(&config, "r2", "error", now + Duration::from_secs(2));
    fake.transition(&config, "r1", "error", now + Duration::from_secs(4));
    fake.transition(&config, "r1", "error", now + Duration::from_secs(10));
    assert_eq!(fake.sent, ["r1:blocked", "r2:error", "r1:error"]);
    fake.transition(
        &Config {
            desktop: false,
            ..config
        },
        "r3",
        "blocked",
        now + Duration::from_secs(30),
    );
    assert_eq!(fake.sent.len(), 3);
}
#[test]
fn config_defaults_and_validation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    assert!(Config::read(&path).unwrap().desktop);
    assert!(Config::read(&path).unwrap().tmux);
    std::fs::write(
        &path,
        "[notify]\ndesktop=false\ntmux=false\ndebounce_secs=3\n",
    )
    .unwrap();
    let config = Config::read(&path).unwrap();
    assert!(!config.desktop);
    assert!(!config.tmux);
    assert_eq!(config.debounce_secs, 3);
    std::fs::write(&path, "[notify]\ndebounce_secs=-1").unwrap();
    assert!(Config::read(&path).is_err());
}

#[test]
fn waiting_bursts_share_global_debounce_and_can_be_disabled() {
    let mut fake = Fake::default();
    let now = Instant::now();
    let config = Config {
        desktop: false,
        ..Config::default()
    };
    for n in 0..10 {
        fake.transition(
            &config,
            &format!("r{n}"),
            "waiting",
            now + Duration::from_millis(n * 100),
        );
    }
    assert_eq!(fake.sent.len(), 1);
    fake.transition(&config, "r1", "waiting", now + Duration::from_secs(2));
    assert_eq!(fake.sent.len(), 2);
    let mut disabled = Fake::default();
    let config = Config {
        tmux: false,
        ..config
    };
    for n in 0..10 {
        disabled.transition(
            &config,
            &format!("r{n}"),
            "waiting",
            now + Duration::from_millis(n * 100),
        );
    }
    assert!(disabled.sent.is_empty());
}

#[test]
fn waiting_and_desktop_share_global_limit() {
    let now = Instant::now();
    let config = Config::default();
    let mut fake = Fake::default();
    fake.transition(&config, "r1", "waiting", now);
    fake.transition(&config, "r2", "blocked", now + Duration::from_secs(1));
    assert_eq!(fake.sent, ["r1:waiting"]);
    fake.transition(&config, "r2", "blocked", now + Duration::from_secs(2));
    assert_eq!(fake.sent, ["r1:waiting", "r2:blocked"]);
}
#[tokio::test]
async fn worker_uses_event_pane_debounces_bursts_and_disables_tmux() {
    use std::sync::{Arc, Mutex};
    struct Recording(Arc<Mutex<Vec<String>>>);
    impl worker::Backend for Recording {
        async fn desktop(&mut self, _: &str) -> io::Result<()> {
            panic!("desktop disabled")
        }
        async fn waiting(&mut self, pane: &str, start: u64, message: &str) -> io::Result<()> {
            self.0
                .lock()
                .unwrap()
                .push(format!("{pane}:{start}:{message}"));
            Ok(())
        }
    }
    for (tmux, expected) in [(true, 1), (false, 0)] {
        let root = tempfile::tempdir().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (send, receive) = broadcast::channel(16);
        for n in 0..10 {
            send.send(serde_json::json!({"run_id":format!("r{n}"),"from":"working","to":"waiting","pane_id":"%7","server_start":42})).unwrap();
        }
        drop(send);
        tokio::time::timeout(
            Duration::from_secs(1),
            worker::run(
                Recording(calls.clone()),
                root.path().to_owned(),
                Config {
                    desktop: false,
                    tmux,
                    ..Config::default()
                },
                receive,
            ),
        )
        .await
        .unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), expected);
        if tmux {
            assert_eq!(calls[0], "%7:42:Agent run r0 is waiting");
        }
    }
}
