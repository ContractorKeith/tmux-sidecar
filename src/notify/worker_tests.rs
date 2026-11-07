use super::*;
use std::sync::{Arc, Mutex};
struct Recording(Arc<Mutex<Vec<String>>>);
impl Backend for Recording {
    async fn desktop(&mut self, message: &str) -> io::Result<()> {
        self.0.lock().unwrap().push(message.into());
        Ok(())
    }
    async fn waiting(&mut self, _: &str, _: u64, _: &str) -> io::Result<()> {
        panic!("desktop transition must not send a tmux message")
    }
}
#[tokio::test]
async fn desktop_uses_project_basename_and_name_or_pane_id() {
    for (name, pane) in [(Some("review"), "review"), (None, "%7")] {
        let root = tempfile::tempdir().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (send, receive) = broadcast::channel(1);
        send.send(serde_json::json!({"run_id":"r1","from":"working","to":"blocked", "agent":"grok", "project_root":"/work/project", "pane_name":name, "pane_id":"%7"})).unwrap();
        drop(send);
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            run(
                Recording(calls.clone()),
                root.path().into(),
                Config::default(),
                receive,
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            [format!("grok in project ({pane}) is blocked")]
        );
    }
}

#[tokio::test]
async fn stale_episode_is_dropped_at_delivery() {
    struct Resumed;
    impl Backend for Resumed {
        async fn current(&mut self, _: &serde_json::Value) -> bool {
            false
        }
        async fn desktop(&mut self, _: &str) -> io::Result<()> {
            panic!("stale notification delivered")
        }
        async fn waiting(&mut self, _: &str, _: u64, _: &str) -> io::Result<()> {
            panic!("desktop only")
        }
    }
    let event = serde_json::json!({"run_id":"r1","from":"working","to":"blocked","at":100});
    deliver(&mut Resumed, &[event], false).await.unwrap();
}
