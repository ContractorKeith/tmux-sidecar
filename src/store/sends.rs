use super::*;
use crate::model::AgentState;
impl Store {
    pub fn sent(&self, run: &AgentRun, at: u64) -> io::Result<()> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(io::Error::other)?;
        self.connection.execute("INSERT INTO pending_sends(run_id,at,prior_json) VALUES(?1,?2,?3) ON CONFLICT(run_id) DO UPDATE SET at=excluded.at",
            params![run_id(&run.id).map_err(io::Error::other)?,number(at).map_err(io::Error::other)?,serde_json::to_string(run)?]).map_err(io::Error::other)?;
        self.change(run, AgentState::Working, at, "send", None)
            .map_err(io::Error::other)?;
        tx.commit().map_err(io::Error::other)
    }
    pub(super) fn expire_sends(&self, now: u64, confirm_ms: u64) -> io::Result<()> {
        let pending = self
            .connection
            .prepare("SELECT run_id,at,prior_json FROM pending_sends")
            .map_err(io::Error::other)?
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    row::unsigned(r, 1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(io::Error::other)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(io::Error::other)?;
        for (id, at, prior) in pending {
            if now.saturating_sub(at) < confirm_ms {
                continue;
            }
            let prior: AgentRun = serde_json::from_str(&prior)?;
            if let Some(run) = self.get(prior.id.parse().map_err(io::Error::other)?, 0)?
                && run.ended_at.is_none()
                && run.state == AgentState::Working
            {
                self.change(&run, prior.state, now, "send_timeout", None)
                    .map_err(io::Error::other)?;
            }
            self.connection
                .execute("DELETE FROM pending_sends WHERE run_id=?1", [id])
                .map_err(io::Error::other)?;
        }
        Ok(())
    }
}
