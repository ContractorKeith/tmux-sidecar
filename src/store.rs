//! The daemon's SQLite boundary. History is never pruned.
use crate::model::{AgentRun, RunSelector};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub struct Store(Connection);
impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        let tx = connection.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta(schema_version INTEGER NOT NULL);
            INSERT INTO meta SELECT 0 WHERE NOT EXISTS (SELECT 1 FROM meta);",
        )?;
        let version: u32 = tx.query_row("SELECT schema_version FROM meta", [], |r| r.get(0))?;
        if version > 1 {
            return Err(rusqlite::Error::InvalidQuery);
        }
        if version == 0 {
            tx.execute_batch("CREATE TABLE runs(
                id INTEGER PRIMARY KEY, server_start INTEGER NOT NULL, pane_id TEXT NOT NULL,
                agent TEXT NOT NULL, agent_pid INTEGER NOT NULL, agent_session_id TEXT,
                cwd TEXT NOT NULL, project_root TEXT, started_at INTEGER NOT NULL, ended_at INTEGER,
                state TEXT NOT NULL, state_since INTEGER NOT NULL, state_source TEXT NOT NULL,
                read_at INTEGER, last_assistant_text TEXT NOT NULL, transcript_path TEXT);
                CREATE UNIQUE INDEX active_run ON runs(server_start,pane_id,agent_pid) WHERE ended_at IS NULL;
                CREATE TABLE signals(id INTEGER PRIMARY KEY, run_id INTEGER REFERENCES runs(id),
                    at INTEGER, source TEXT, kind TEXT, payload_json TEXT);
                CREATE TABLE panes(server_start INTEGER, pane_id TEXT, name TEXT,
                    PRIMARY KEY(server_start,pane_id));
                CREATE TABLE groups(id INTEGER PRIMARY KEY, name TEXT UNIQUE);
                CREATE TABLE group_members(group_id INTEGER REFERENCES groups(id), server_start INTEGER,
                    pane_id TEXT, PRIMARY KEY(group_id,server_start,pane_id),
                    FOREIGN KEY(server_start,pane_id) REFERENCES panes(server_start,pane_id));
                UPDATE meta SET schema_version=1;")?;
        }
        tx.commit()?;
        Ok(Self(connection))
    }
    pub fn insert(&self, run: &AgentRun) -> rusqlite::Result<String> {
        self.pane(run.server_start, &run.pane_id)?;
        self.0.execute("INSERT INTO runs(server_start,pane_id,agent,agent_pid,agent_session_id,
            cwd,project_root,started_at,ended_at,state,state_since,state_source,read_at,
            last_assistant_text,transcript_path) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![number(run.server_start)?, run.pane_id, enum_name(&run.agent), run.agent_pid,
                run.agent_session_id, run.cwd, run.project_root, number(run.started_at)?, run.ended_at.map(number).transpose()?,
                enum_name(&run.state), number(run.state_since)?, run.state_source, run.read_at.map(number).transpose()?,
                run.last_assistant_text, run.transcript_path])?;
        Ok(format!("r{}", self.0.last_insert_rowid()))
    }
    pub fn end(&self, id: &str, now: u64) -> rusqlite::Result<()> {
        self.0.execute(
            "UPDATE runs SET ended_at=?1,state='done',state_since=?1,state_source='process'
            WHERE id=?2 AND ended_at IS NULL",
            params![number(now)?, id.strip_prefix('r')],
        )?;
        Ok(())
    }
    pub fn end_all(&self, now: u64) -> rusqlite::Result<()> {
        self.0.execute(
            "UPDATE runs SET ended_at=?1,state='done',state_since=?1,state_source='process'
            WHERE ended_at IS NULL",
            [number(now)?],
        )?;
        Ok(())
    }
    pub fn list(&self) -> rusqlite::Result<Vec<AgentRun>> {
        self.0
            .prepare("SELECT * FROM runs ORDER BY id")?
            .query_map([], row)?
            .collect()
    }
    pub fn active(&self) -> rusqlite::Result<Vec<AgentRun>> {
        self.0
            .prepare("SELECT * FROM runs WHERE ended_at IS NULL ORDER BY id")?
            .query_map([], row)?
            .collect()
    }
    pub fn get(
        &self,
        selector: RunSelector,
        server_start: u64,
    ) -> rusqlite::Result<Option<AgentRun>> {
        match selector {
            RunSelector::Run(id) => self
                .0
                .query_row("SELECT * FROM runs WHERE id=?1", [number(id)?], row)
                .optional(),
            RunSelector::Pane(id) => self
                .0
                .query_row(
                    "SELECT * FROM runs WHERE server_start=?1 AND pane_id=?2
                AND ended_at IS NULL ORDER BY id DESC LIMIT 1",
                    params![number(server_start)?, format!("%{id}")],
                    row,
                )
                .optional(),
        }
    }
    pub fn pane(&self, server_start: u64, pane_id: &str) -> rusqlite::Result<()> {
        self.0.execute(
            "INSERT OR IGNORE INTO panes(server_start,pane_id) VALUES (?1,?2)",
            params![number(server_start)?, pane_id],
        )?;
        Ok(())
    }
    pub fn name_pane(
        &self,
        server_start: u64,
        pane_id: &str,
        name: Option<&str>,
    ) -> rusqlite::Result<()> {
        self.pane(server_start, pane_id)?;
        self.0.execute(
            "UPDATE panes SET name=?3 WHERE server_start=?1 AND pane_id=?2",
            params![number(server_start)?, pane_id, name],
        )?;
        Ok(())
    }
    pub fn pane_name(&self, server_start: u64, pane_id: &str) -> rusqlite::Result<Option<String>> {
        self.0
            .query_row(
                "SELECT name FROM panes WHERE server_start=?1 AND pane_id=?2",
                params![number(server_start)?, pane_id],
                |r| r.get(0),
            )
            .optional()
            .map(Option::flatten)
    }
}
fn number(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}
fn unsigned(r: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = r.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
fn enum_name(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .expect("fieldless enum serializes")
        .as_str()
        .expect("enum is a string")
        .into()
}
fn enum_column<T: serde::de::DeserializeOwned>(
    r: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let value: String = r.get(index)?;
    serde_json::from_value(value.into()).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(e))
    })
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRun> {
    Ok(AgentRun {
        id: format!("r{}", r.get::<_, i64>(0)?),
        server_start: unsigned(r, 1)?,
        pane_id: r.get(2)?,
        agent: enum_column(r, 3)?,
        agent_pid: r.get(4)?,
        agent_session_id: r.get(5)?,
        cwd: r.get(6)?,
        project_root: r.get(7)?,
        started_at: unsigned(r, 8)?,
        ended_at: r
            .get::<_, Option<i64>>(9)?
            .map(|v| u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(9, v)))
            .transpose()?,
        state: enum_column(r, 10)?,
        state_since: unsigned(r, 11)?,
        state_source: r.get(12)?,
        read_at: r
            .get::<_, Option<i64>>(13)?
            .map(|v| u64::try_from(v).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(13, v)))
            .transpose()?,
        last_assistant_text: r.get(14)?,
        transcript_path: r.get(15)?,
    })
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
