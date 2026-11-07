use super::*;
use serde_json::json;
fn event(id: &str, state: &str) -> serde_json::Value {
    json!({"run_id":id,"from":"working","to":state,"pane_id":"%3","server_start":1})
}
#[test]
fn bursts_queue_all_runs_and_repeats_collapse() {
    let mut queue = Debounce::default();
    let now = Instant::now();
    let config = Config::default();
    queue.push(&config, event("r1", "blocked"), now);
    assert!(queue.drain(now).is_empty());
    queue.push(
        &config,
        event("r2", "blocked"),
        now + Duration::from_millis(400),
    );
    queue.push(
        &config,
        event("r3", "blocked"),
        now + Duration::from_millis(800),
    );
    queue.push(
        &config,
        event("r2", "blocked"),
        now + Duration::from_secs(1),
    );
    assert!(queue.drain(now + Duration::from_secs(1)).is_empty());
    let batches = queue.drain(now + Duration::from_secs(2));
    assert_eq!(batches[0].len(), 3);
    assert_eq!(batches[0][0]["run_id"], "r1");
    assert_eq!(batches[0][2]["run_id"], "r3");
    queue.push(
        &config,
        event("r1", "blocked"),
        now + Duration::from_secs(6),
    );
    assert!(queue.drain(now + Duration::from_secs(6)).is_empty());
    queue.push(
        &config,
        event("r1", "blocked"),
        now + Duration::from_secs(10),
    );
    assert!(queue.drain(now + Duration::from_secs(12)).is_empty());
}
#[test]
fn waiting_has_its_own_window_and_disabled_channels_do_not_queue() {
    let mut queue = Debounce::default();
    let now = Instant::now();
    queue.push(&Config::default(), event("r1", "waiting"), now);
    assert_eq!(queue.drain(now + Duration::from_secs(2)).len(), 1);
    queue.push(
        &Config::default(),
        event("r2", "blocked"),
        now + Duration::from_secs(3),
    );
    assert_eq!(queue.drain(now + Duration::from_secs(5)).len(), 1);
    let config = Config {
        desktop: false,
        tmux: false,
        ..Config::default()
    };
    queue.push(&config, event("r3", "blocked"), now);
    queue.push(&config, event("r4", "waiting"), now);
    assert!(queue.drain(now + Duration::from_secs(10)).is_empty());
}
#[test]
fn config_defaults_and_validation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    assert!(Config::read(&path).unwrap().desktop);
    std::fs::write(
        &path,
        "[notify]\ndesktop=false\ntmux=false\ndebounce_secs=3\n",
    )
    .unwrap();
    let config = Config::read(&path).unwrap();
    assert!(!config.desktop && !config.tmux);
    assert_eq!(config.debounce_secs, 3);
    std::fs::write(&path, "[notify]\ndebounce_secs=-1").unwrap();
    assert!(Config::read(&path).is_err());
}

#[test]
fn each_block_episode_notifies_once_even_inside_debounce_window() {
    let mut queue = Debounce::default();
    let now = Instant::now();
    let mut first = event("r1", "blocked");
    first["at"] = json!(100);
    queue.push(&Config::default(), first.clone(), now);
    assert_eq!(queue.drain(now + Duration::from_secs(2))[0].len(), 1);
    queue.push(&Config::default(), first, now + Duration::from_secs(3));
    assert!(queue.drain(now + Duration::from_secs(3)).is_empty());
    let mut next = event("r1", "blocked");
    next["at"] = json!(200);
    queue.push(&Config::default(), next, now + Duration::from_secs(4));
    assert_eq!(queue.drain(now + Duration::from_secs(6))[0].len(), 1);
}

#[test]
fn config_errors_keep_filename_and_reason() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("broken.toml");
    for (text, reason) in [
        ("[notify]\ndesktop='yes'", "invalid type"),
        ("idle_after_secs=18446744073709551615", "too large"),
    ] {
        std::fs::write(&path, text).unwrap();
        let error = Config::read(&path).unwrap_err().to_string();
        assert!(
            error.contains(path.to_str().unwrap()) && error.contains(reason),
            "{error}"
        );
    }
}
