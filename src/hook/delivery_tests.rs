use super::*;
#[test]
fn internal_budget_never_exceeds_two_hundred_ms() {
    assert!(HOOK_BUDGET <= Duration::from_millis(200));
    for elapsed in 0..=250 {
        let elapsed = Duration::from_millis(elapsed);
        let budget = live_budget(elapsed);
        assert!(budget <= Duration::from_millis(100));
        if !budget.is_zero() {
            assert!(elapsed + budget < HOOK_BUDGET);
        }
    }
    assert_eq!(live_budget(HOOK_BUDGET), Duration::ZERO);
}
#[tokio::test]
async fn live_delivery_times_out_on_a_daemon_that_never_answers_hello() {
    use tokio::{
        io::{AsyncBufReadExt, BufReader},
        net::UnixListener,
    };
    let root = tempfile::tempdir().unwrap();
    let paths = paths::Paths {
        runtime: root.path().to_owned(),
        state: root.path().join("state"),
        config: root.path().join("config"),
    };
    let listener = UnixListener::bind(paths.runtime.join("sock")).unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        deliver(
            &root.path().join("tmux"),
            &paths,
            b"signal\n",
            live_budget(Duration::ZERO),
        ),
    )
    .await
    .expect("hook delivery must not block on daemon");
    assert!(
        result.is_err(),
        "a stalled hello must hit the internal timer"
    );
    let (stream, _) = tokio::time::timeout(Duration::from_secs(1), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(1), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&line).unwrap()["method"],
        "hello"
    );
}

#[test]
fn notify_uses_last_argument_after_wrapper_arguments() {
    let args = [
        "codex-notify",
        "wrapper-option",
        "{\"type\":\"agent-turn-complete\"}",
    ]
    .map(str::to_owned);
    assert_eq!(notify_payload(&args), args[2].as_bytes());
}
