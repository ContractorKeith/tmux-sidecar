use super::*;
#[test]
fn unknown_run_metadata_dedupe_and_late_replay_preserve_state() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let mut signal = Signal {
        server_start: 42,
        pane_id: "%0".into(),
        agent: crate::model::AgentKind::Grok,
        agent_pid: 100,
        at: 1000,
        source: "hook:claude".into(),
        nonce: "00000000000000000000000000000001".into(),
        event: "Stop".into(),
        detail: None,
        agent_session_id: Some("fake".into()),
        cwd: "/work".into(),
        transcript_path: Some("/transcript".into()),
        orphaned: false,
        parent_agent_pid: None,
        payload: serde_json::json!({"hook_event_name":"Stop"}),
    };
    store.signal(&signal).unwrap();
    store.signal(&signal).unwrap();
    let run = &store.list().unwrap()[0];
    assert_eq!(run.state, AgentState::Waiting);
    assert_eq!(run.agent_session_id.as_deref(), Some("fake"));
    store.end(&run.id, 2000).unwrap();
    signal.nonce = "00000000000000000000000000000002".into();
    store.signal(&signal).unwrap();
    let runs = store.list().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].state, AgentState::Done);
    let count: i64 = store
        .connection
        .query_row("SELECT COUNT(*) FROM signals", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);
    let longest: i64 = store
        .connection
        .query_row("SELECT MAX(length(nonce)) FROM signal_receipts", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(longest, 32);
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn draft_v2_history_migrates_kind_and_detail_without_losing_rows() {
        let root = tempfile::tempdir().unwrap();
        let db = root.path().join("db");
        let store = Store::open(&db).unwrap();
        let signal = Signal {
            server_start: 1,
            pane_id: "%0".into(),
            agent: crate::model::AgentKind::Claude,
            agent_pid: 42,
            at: 10,
            nonce: "00000000000000000000000000000001".into(),
            source: "hook:claude".into(),
            event: "Stop".into(),
            detail: None,
            agent_session_id: None,
            cwd: "/work".into(),
            transcript_path: None,
            orphaned: false,
            parent_agent_pid: None,
            payload: serde_json::json!({"hook_event_name":"Stop"}),
        };
        store.signal(&signal).unwrap();
        store
            .connection
            .execute_batch(
                "UPDATE signals SET event='Stop:end_turn';
            ALTER TABLE signals DROP COLUMN detail;
            ALTER TABLE signals RENAME COLUMN event TO kind;
            DROP TABLE signal_receipts;
            CREATE TABLE signal_receipts(at INTEGER,fingerprint TEXT,PRIMARY KEY(at,fingerprint));
            UPDATE meta SET schema_version=2;",
            )
            .unwrap();
        drop(store);
        let reopened = Store::open(&db).unwrap();
        let rows = reopened.signal_history().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["event"], "Stop");
        assert_eq!(rows[0]["detail"], "end_turn");
    }
}
