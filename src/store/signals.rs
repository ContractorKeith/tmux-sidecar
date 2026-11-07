use super::*;
use crate::{hook::signal::Signal, model::AgentState};
impl Store {
    pub fn signal_history(&self) -> io::Result<Vec<serde_json::Value>> {
        (|| -> rusqlite::Result<_> {
            self.connection.prepare("SELECT run_id,at,source,event,payload_json,detail FROM signals WHERE source LIKE 'hook:%' ORDER BY id")?
                .query_map([],|r| Ok(serde_json::json!({"run_id":format!("r{}",r.get::<_,i64>(0)?), "at":r.get::<_,i64>(1)?,
                    "source":r.get::<_,String>(2)?,"event":r.get::<_,String>(3)?,"detail":r.get::<_,Option<String>>(5)?,"payload_json":r.get::<_,String>(4)?})))?.collect()
        })().map_err(|e|db_error(&self.path,e))
    }

    /// Receipt and signal commit together; replay can safely repeat after a crash.
    pub fn signal(&self, signal: &Signal) -> io::Result<()> {
        self.record_signal(signal).map(|_| ())
    }
    pub(crate) fn record_signal(&self, signal: &Signal) -> io::Result<bool> {
        let mut dropped = false;
        signal.validate()?;
        let payload = serde_json::to_string(&signal.payload)?;
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(|e| db_error(&self.path, e))?;
        let result = (|| -> rusqlite::Result<()> {
            if tx.execute(
                "INSERT OR IGNORE INTO signal_receipts VALUES (?1)",
                params![signal.nonce],
            )? == 0
            {
                return Ok(());
            }
            let existing: Option<i64> = if signal.orphaned {
                let Some(owner) = self
                    .orphan_owner(signal)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
                else {
                    dropped = true;
                    return Ok(());
                };
                Some(run_id(&owner.id)?)
            } else {
                tx.query_row("SELECT id FROM runs WHERE server_start=?1 AND pane_id=?2 AND agent_pid=?3 ORDER BY id DESC LIMIT 1",
                params![number(signal.server_start)?,signal.pane_id,signal.agent_pid], |r| r.get(0)).optional()?
            };
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
            let original = tx.query_row("SELECT * FROM runs WHERE id=?1", [id], row)?;
            if super::signal_filter::ignore(&original, signal) {
                return Ok(());
            }
            tx.execute(
                "UPDATE runs SET started_at=MIN(started_at,?1) WHERE id=?2 AND NOT EXISTS(SELECT 1 FROM signals WHERE run_id=?2)",
                params![number(signal.at)?, id],
            )?;
            let session_id = if signal.agent == crate::model::AgentKind::Codex
                && signal.source != "hook:codex"
            {
                None
            } else {
                signal.agent_session_id.as_ref()
            };
            tx.execute("UPDATE runs SET agent_session_id=COALESCE(?1,agent_session_id), transcript_path=COALESCE(?2,transcript_path) WHERE id=?3",
                params![session_id,signal.transcript_path,id])?;
            let run = tx.query_row("SELECT * FROM runs WHERE id=?1", [id], row)?;
            let last_signal_at = self
                .latest_hook_at(&run.id)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            tx.execute(
                "INSERT INTO signals(run_id,at,source,event,payload_json,detail) VALUES (?1,?2,?3,?4,?5,?6)",
                params![id, number(signal.at)?, signal.source, signal.event, payload,signal.detail],
            )?;
            let cause = tx.last_insert_rowid();
            tx.execute(
                "DELETE FROM pending_sends WHERE run_id=?1 AND signal_id<?2",
                params![id, cause],
            )?;
            let (state, _) = crate::engine::transition(
                &original,
                crate::engine::Evidence::Hook {
                    session_id: signal.agent_session_id.as_deref(),
                    event: &signal.event,
                    detail: signal.detail.as_deref(),
                    at: signal.at,
                    last_signal_at,
                },
                signal.at,
            );
            // Detached plugin hosts can update a turn but cannot end the pane's process.
            let state = if state == AgentState::Done && !signal.pane_verified {
                original.state
            } else {
                state
            };
            self.change(&run, state, signal.at, &signal.source, Some(cause))?;
            if let Some(text) = signal
                .payload
                .get("last_assistant_message")
                .and_then(serde_json::Value::as_str)
            {
                tx.execute(
                    "UPDATE runs SET last_assistant_text=?1 WHERE id=?2",
                    params![text, id],
                )?;
            }
            Ok(())
        })();
        result.map_err(|e| db_error(&self.path, e))?;
        tx.commit().map_err(|e| db_error(&self.path, e))?;
        Ok(dropped)
    }
}
#[cfg(test)]
#[path = "signal_tests.rs"]
mod tests;
