//! The daemon's SQLite boundary. History is never pruned.
use crate::model::{AgentRun, RunSelector};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

mod groups;
mod lists;
mod queries;
mod routing;
mod row;
mod sends;
mod signal_filter;
mod signals;
mod state;
mod view;
use row::{enum_name, number, row, run_id};
fn db_error(path: &Path, error: impl std::fmt::Display) -> io::Error {
    let error = error.to_string();
    let hint = if error.contains("newer than this binary") {
        ""
    } else {
        "; move it aside to start fresh"
    };
    io::Error::other(format!("database {}: {error}{hint}", path.display()))
}
pub struct Store {
    connection: Connection,
    path: PathBuf,
    identity: (u64, u64),
}
impl Store {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::open_inner(path).map_err(|e| db_error(path, e))
    }
    fn open_inner(path: &Path) -> rusqlite::Result<Self> {
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
        if version > 3 {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                io::Error::other(format!(
                    "database schema {version} is newer than this binary (3); upgrade tmux-sidecar"
                )),
            )));
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
        if version < 2 {
            tx.execute_batch("CREATE TABLE signal_receipts(at INTEGER NOT NULL, fingerprint TEXT NOT NULL, PRIMARY KEY(at,fingerprint)); UPDATE meta SET schema_version=2;")?;
        }
        if version < 3 {
            tx.execute_batch("ALTER TABLE signals RENAME COLUMN kind TO event;
                ALTER TABLE signals ADD COLUMN detail TEXT;
                UPDATE signals SET detail=substr(event,instr(event,':')+1),event=substr(event,1,instr(event,':')-1) WHERE instr(event,':')>0;
                DROP TABLE signal_receipts;
                CREATE TABLE signal_receipts(nonce TEXT PRIMARY KEY NOT NULL);
                UPDATE meta SET schema_version=3;")?;
        }
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_sends(run_id INTEGER PRIMARY KEY REFERENCES runs(id), at INTEGER NOT NULL, prior_json TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS signals_changes ON signals(event,id);
            CREATE INDEX IF NOT EXISTS signals_run ON signals(run_id,source);
            CREATE INDEX IF NOT EXISTS hook_time ON signals(run_id,at DESC) WHERE source GLOB 'hook:*';",
        )?;
        let has_cursor = tx
            .prepare("PRAGMA table_info(pending_sends)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|name| name == "signal_id");
        if !has_cursor {
            tx.execute_batch(
                "ALTER TABLE pending_sends ADD COLUMN signal_id INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        tx.commit()?;
        let meta = std::fs::metadata(path)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        Ok(Self {
            connection,
            path: path.to_owned(),
            identity: (meta.dev(), meta.ino()),
        })
    }
}

impl Store {
    pub fn verify_identity(&self) -> io::Result<()> {
        let meta = std::fs::metadata(&self.path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    e.kind(),
                    format!("history database was deleted: {}", self.path.display()),
                )
            } else {
                db_error(&self.path, e)
            }
        })?;
        if (meta.dev(), meta.ino()) != self.identity {
            return Err(db_error(
                &self.path,
                "history file was replaced while the daemon was running",
            ));
        }
        Ok(())
    }
    /// All changes from one snapshot commit together; a failure rolls back the tick.
    pub fn apply(
        &self,
        panes: &[crate::seams::PaneSnapshot],
        changes: &crate::engine::Changes,
        now: u64,
    ) -> io::Result<()> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(|e| db_error(&self.path, e))?;
        for pane in panes {
            self.pane(pane.server_start, &pane.pane_id)?;
        }
        for id in &changes.end {
            self.end_inner(id, now)?;
        }
        for run in &changes.start {
            self.insert(run)?;
        }
        tx.commit().map_err(|e| db_error(&self.path, e))
    }
    pub fn query_runs(
        &self,
        all: bool,
        state: Option<crate::model::AgentState>,
        limit: u32,
    ) -> io::Result<Vec<AgentRun>> {
        self.list_page(all, state, all.then_some(limit), None, None)
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
