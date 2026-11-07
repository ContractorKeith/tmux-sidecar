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
    let config = Config::default();
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
    std::fs::write(&path, "[notify]\ndesktop=false\ndebounce_secs=3\n").unwrap();
    let config = Config::read(&path).unwrap();
    assert!(!config.desktop);
    assert_eq!(config.debounce_secs, 3);
    std::fs::write(&path, "[notify]\ndebounce_secs=-1").unwrap();
    assert!(Config::read(&path).is_err());
}
