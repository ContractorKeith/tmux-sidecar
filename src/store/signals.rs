use super::*;
use crate::{hook::signal::Signal, model::AgentState};
impl Store {
    pub fn signal_history(&self) -> io::Result<Vec<serde_json::Value>> {
        (|| -> rusqlite::Result<_> {
            self.connection.prepare("SELECT run_id,at,source,kind,payload_json FROM signals ORDER BY id")?
                .query_map([],|r| Ok(serde_json::json!({"run_id":format!("r{}",r.get::<_,i64>(0)?), "at":r.get::<_,i64>(1)?,
                    "source":r.get::<_,String>(2)?,"kind":r.get::<_,String>(3)?,"payload_json":r.get::<_,String>(4)?})))?.collect()
        })().map_err(|e|db_error(&self.path,e))
    }

    /// Receipt and signal commit together; replay can safely repeat after a crash.
    pub fn signal(&self, signal: &Signal) -> io::Result<()> {
        signal.validate()?;
        let payload = serde_json::to_string(&signal.payload)?;
        // Store the canonical payload alongside its hash to make collisions harmless.
        let hash = payload.bytes().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
        let fingerprint = format!(
            "{}:{}:{}:{hash:x}:{payload}",
            signal.server_start, signal.pane_id, signal.agent_pid
        );
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(|e| db_error(&self.path, e))?;
        let result = (|| -> rusqlite::Result<()> {
            if tx.execute(
                "INSERT OR IGNORE INTO signal_receipts VALUES (?1,?2)",
                params![number(signal.at)?, fingerprint],
            )? == 0
            {
                return Ok(());
            }
            let existing: Option<i64> = tx.query_row("SELECT id FROM runs WHERE server_start=?1 AND pane_id=?2 AND agent_pid=?3 ORDER BY id DESC LIMIT 1",
                params![number(signal.server_start)?,signal.pane_id,signal.agent_pid], |r| r.get(0)).optional()?;
            let id = match existing {
                Some(id) => id,
                None => {
                    self.pane(signal.server_start, &signal.pane_id)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    let run = AgentRun {
                        id: String::new(),
                        server_start: signal.server_start,
                        pane_id: signal.pane_id.clone(),
                        agent: signal.agent,
                        agent_pid: signal.agent_pid,
                        agent_session_id: None,
                        cwd: signal.cwd.clone(),
                        project_root: None,
                        started_at: signal.at,
                        ended_at: None,
                        state: AgentState::Waiting,
                        state_since: signal.at,
                        state_source: "process".into(),
                        read_at: None,
                        last_assistant_text: String::new(),
                        transcript_path: None,
                    };
                    self.insert_inner(&run)?;
                    self.connection.last_insert_rowid()
                }
            };
            tx.execute("UPDATE runs SET agent_session_id=COALESCE(?1,agent_session_id), transcript_path=COALESCE(?2,transcript_path) WHERE id=?3",
                params![signal.agent_session_id,signal.transcript_path,id])?;
            tx.execute(
                "INSERT INTO signals(run_id,at,source,kind,payload_json) VALUES (?1,?2,?3,?4,?5)",
                params![id, number(signal.at)?, signal.source, signal.kind, payload],
            )?;
            Ok(())
        })();
        result.map_err(|e| db_error(&self.path, e))?;
        tx.commit().map_err(|e| db_error(&self.path, e))
    }
}
#[cfg(test)]
mod tests {
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
            kind: "Stop".into(),
            agent_session_id: Some("fake".into()),
            cwd: "/work".into(),
            transcript_path: Some("/transcript".into()),
            payload: serde_json::json!({"hook_event_name":"Stop"}),
        };
        store.signal(&signal).unwrap();
        store.signal(&signal).unwrap();
        let run = &store.list().unwrap()[0];
        assert_eq!(run.state, AgentState::Waiting);
        assert_eq!(run.agent_session_id.as_deref(), Some("fake"));
        store.end(&run.id, 2000).unwrap();
        signal.at += 1;
        store.signal(&signal).unwrap();
        let runs = store.list().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].state, AgentState::Done);
        let count: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM signals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}
