use super::*;
use crate::{
    engine::{Evidence, transition},
    model::AgentState,
};
use serde_json::{Value, json};
impl Store {
    pub fn has_hooks(&self, id: &str) -> io::Result<bool> {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM signals WHERE run_id=?1 AND source LIKE 'hook:%')",
                [run_id(id).map_err(io::Error::other)?],
                |r| r.get(0),
            )
            .map_err(io::Error::other)
    }
    /// Caller owns the transaction, so history and run state become visible together.
    pub(super) fn change(
        &self,
        run: &AgentRun,
        state: AgentState,
        at: u64,
        source: &str,
        cause: Option<i64>,
    ) -> rusqlite::Result<()> {
        if state == run.state {
            return Ok(());
        }
        self.connection.execute(
            "UPDATE runs SET state=?1,state_since=?2,state_source=?3,read_at=NULL WHERE id=?4",
            params![enum_name(&state), number(at)?, source, run_id(&run.id)?],
        )?;
        let payload = json!({"run_id":run.id,"from":run.state,"to":state,"at":at,"cause":cause});
        self.connection.execute("INSERT INTO signals(run_id,at,source,event,payload_json) VALUES (?1,?2,'engine','state_changed',?3)",
            params![run_id(&run.id)?,number(at)?,payload.to_string()])?;
        Ok(())
    }
    pub fn changes_since(&self, cursor: i64) -> io::Result<Vec<(i64, Value)>> {
        self.connection.prepare("SELECT id,payload_json FROM signals WHERE source='engine' AND event='state_changed' AND id>?1 ORDER BY id").map_err(io::Error::other)?
            .query_map([cursor], |r| {let payload: String = r.get(1)?; Ok((r.get(0)?,payload))}).map_err(io::Error::other)?
            .map(|r| {let (id,payload)=r.map_err(io::Error::other)?; Ok((id,serde_json::from_str(&payload)?))}).collect()
    }
    pub fn maintain(
        &self,
        panes: &[crate::seams::PaneSnapshot],
        now: u64,
        idle_after_ms: u64,
    ) -> io::Result<()> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(io::Error::other)?;
        for run in self.active()? {
            if panes.iter().any(|p| {
                p.server_start == run.server_start
                    && p.pane_id == run.pane_id
                    && p.pane_active
                    && p.window_active
                    && p.session_attached
            }) {
                self.ack(&run, now)?;
            }
            let mut run = run;
            run.read_at = self
                .get(run.id.parse().map_err(io::Error::other)?, run.server_start)?
                .and_then(|r| r.read_at);
            let (state, _) = transition(&run, Evidence::Tick { idle_after_ms }, now);
            self.change(&run, state, now, "clock", None)
                .map_err(io::Error::other)?;
        }
        tx.commit().map_err(io::Error::other)
    }
    pub fn transcript(
        &self,
        id: &str,
        reading: &session_board::model::Session,
        now: u64,
    ) -> io::Result<()> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(io::Error::other)?;
        if let Some(run) = self.get(id.parse().map_err(io::Error::other)?, 0)? {
            use session_board::model::Status;
            let state = match reading.status {
                Status::Working => AgentState::Working,
                Status::Waiting => AgentState::Waiting,
                Status::Idle => AgentState::Idle,
                Status::Error => AgentState::Error,
                Status::Done => AgentState::Done,
            };
            let (state, _) = transition(
                &run,
                Evidence::Transcript {
                    state,
                    has_hooks: self.has_hooks(id)?,
                    at: u64::try_from(reading.last_activity_at)
                        .unwrap_or(0)
                        .saturating_mul(1000),
                },
                now,
            );
            self.connection.execute("INSERT INTO signals(run_id,at,source,event,payload_json) VALUES (?1,?2,'transcript','transcript_reading',?3)",
                params![run_id(id).map_err(io::Error::other)?,number(now).map_err(io::Error::other)?,serde_json::to_string(reading)?]).map_err(io::Error::other)?;
            let cause = self.connection.last_insert_rowid();
            self.change(&run, state, now, "transcript", Some(cause))
                .map_err(io::Error::other)?;
            self.connection
                .execute(
                    "UPDATE runs SET last_assistant_text=?1,transcript_path=?2 WHERE id=?3",
                    params![
                        reading.last_assistant_text,
                        reading.transcript_path.to_string_lossy(),
                        run_id(id).map_err(io::Error::other)?
                    ],
                )
                .map_err(io::Error::other)?;
        }
        tx.commit().map_err(io::Error::other)
    }
    pub fn ack(&self, run: &AgentRun, at: u64) -> io::Result<()> {
        self.connection
            .execute(
                "UPDATE runs SET read_at=?1 WHERE id=?2",
                params![
                    number(at).map_err(io::Error::other)?,
                    run_id(&run.id).map_err(io::Error::other)?
                ],
            )
            .map_err(io::Error::other)?;
        Ok(())
    }
}
