//! Public worker seam: a queued burst must flush even without another event.
use super::*;
use serde_json::json;
use std::sync::{Arc, Mutex};
struct Recording(Arc<Mutex<Vec<String>>>);
impl worker::Backend for Recording {
    async fn desktop(&mut self, message: &str) -> io::Result<()> {
        self.0.lock().unwrap().push(format!("desktop:{message}"));
        Ok(())
    }
    async fn waiting(&mut self, _: &str, _: u64, message: &str) -> io::Result<()> {
        self.0.lock().unwrap().push(format!("tmux:{message}"));
        Ok(())
    }
}
fn event(id: &str, to: &str, agent: &str, project: &str, pane: &str) -> serde_json::Value {
    json!({"run_id":id,"from":"working","to":to,"agent":agent,"project_root":project,"pane_id":pane,"server_start":42})
}
#[tokio::test]
async fn queued_desktop_burst_flushes_on_timer_and_waiting_does_not_block_it() {
    let root = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (send, receive) = broadcast::channel(16);
    let task = worker::run(
        Recording(calls.clone()),
        root.path().into(),
        Config::default(),
        receive,
    );
    let drive = async {
        send.send(event("r0", "waiting", "codex", "/work/other", "%1"))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        send.send(event("r2", "blocked", "claude", "/work/proj", "%3"))
            .unwrap();
        send.send(event("r3", "blocked", "grok", "/work/api", "%7"))
            .unwrap();
        send.send(event("r2", "blocked", "claude", "/work/proj", "%3"))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                if calls.lock().unwrap().len() >= 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("queued summary never delivered");
        drop(send);
    };
    tokio::join!(task, drive);
    assert_eq!(
        *calls.lock().unwrap(),
        [
            "tmux:Agent run r0 is waiting",
            "desktop:2 runs blocked: claude in proj (%3), grok in api (%7)",
        ]
    );
}

#[tokio::test]
async fn a_disappeared_waiting_pane_does_not_discard_other_recipients() {
    struct Vanished(Arc<Mutex<Vec<String>>>);
    impl worker::Backend for Vanished {
        async fn desktop(&mut self, _: &str) -> io::Result<()> {
            panic!("waiting only")
        }
        async fn waiting(&mut self, pane: &str, _: u64, _: &str) -> io::Result<()> {
            self.0.lock().unwrap().push(pane.into());
            if pane == "%1" {
                Err(io::Error::other("pane no longer exists"))
            } else {
                Ok(())
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (send, receive) = broadcast::channel(4);
    send.send(event("r1", "waiting", "claude", "/work/proj", "%1"))
        .unwrap();
    send.send(event("r2", "waiting", "grok", "/work/api", "%2"))
        .unwrap();
    drop(send);
    tokio::time::timeout(
        Duration::from_secs(1),
        worker::run(
            Vanished(calls.clone()),
            root.path().into(),
            Config::default(),
            receive,
        ),
    )
    .await
    .unwrap();
    assert_eq!(*calls.lock().unwrap(), ["%1", "%2"]);
}
