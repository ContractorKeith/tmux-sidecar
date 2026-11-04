use super::*;
impl Store {
    pub fn insert(&self, run: &AgentRun) -> io::Result<String> {
        self.pane(run.server_start, &run.pane_id)?;
        self.insert_inner(run).map_err(|e| db_error(&self.path, e))
    }
    fn insert_inner(&self, run: &AgentRun) -> rusqlite::Result<String> {
        self.connection.execute("INSERT INTO runs(server_start,pane_id,agent,agent_pid,agent_session_id,
            cwd,project_root,started_at,ended_at,state,state_since,state_source,read_at,
            last_assistant_text,transcript_path) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![number(run.server_start)?, run.pane_id, enum_name(&run.agent), run.agent_pid,
                run.agent_session_id, run.cwd, run.project_root, number(run.started_at)?, run.ended_at.map(number).transpose()?,
                enum_name(&run.state), number(run.state_since)?, run.state_source, run.read_at.map(number).transpose()?,
                run.last_assistant_text, run.transcript_path])?;
        Ok(format!("r{}", self.connection.last_insert_rowid()))
    }
    pub fn end(&self, id: &str, now: u64) -> io::Result<()> {
        (|| -> rusqlite::Result<_> {
            self.connection.execute(
                "UPDATE runs SET ended_at=?1,state='done',state_since=?1,state_source='process'
            WHERE id=?2 AND ended_at IS NULL",
                params![number(now)?, run_id(id)?],
            )?;
            Ok(())
        })()
        .map_err(|e| db_error(&self.path, e))
    }
    pub fn end_all(&self, now: u64) -> io::Result<()> {
        (|| -> rusqlite::Result<_> {
            self.connection.execute(
                "UPDATE runs SET ended_at=?1,state='done',state_since=?1,state_source='process'
            WHERE ended_at IS NULL",
                [number(now)?],
            )?;
            Ok(())
        })()
        .map_err(|e| db_error(&self.path, e))
    }
    pub fn list(&self) -> io::Result<Vec<AgentRun>> {
        (|| -> rusqlite::Result<_> {
            self.connection
                .prepare("SELECT * FROM runs ORDER BY id")?
                .query_map([], row)?
                .collect()
        })()
        .map_err(|e| db_error(&self.path, e))
    }
    pub fn active(&self) -> io::Result<Vec<AgentRun>> {
        (|| -> rusqlite::Result<_> {
            self.connection
                .prepare("SELECT * FROM runs WHERE ended_at IS NULL ORDER BY id")?
                .query_map([], row)?
                .collect()
        })()
        .map_err(|e| db_error(&self.path, e))
    }
    pub fn get(&self, selector: RunSelector, server_start: u64) -> io::Result<Option<AgentRun>> {
        (|| -> rusqlite::Result<_> {
            match selector {
                RunSelector::Run(id) => self
                    .connection
                    .query_row("SELECT * FROM runs WHERE id=?1", [number(id)?], row)
                    .optional(),
                RunSelector::Pane(id) => self
                    .connection
                    .query_row(
                        "SELECT * FROM runs WHERE server_start=?1 AND pane_id=?2
                AND ended_at IS NULL ORDER BY id DESC LIMIT 1",
                        params![number(server_start)?, format!("%{id}")],
                        row,
                    )
                    .optional(),
            }
        })()
        .map_err(|e| db_error(&self.path, e))
    }
    pub fn pane(&self, server_start: u64, pane_id: &str) -> io::Result<()> {
        (|| -> rusqlite::Result<_> {
            self.connection.execute(
                "INSERT OR IGNORE INTO panes(server_start,pane_id) VALUES (?1,?2)",
                params![number(server_start)?, pane_id],
            )?;
            Ok(())
        })()
        .map_err(|e| db_error(&self.path, e))
    }
    pub fn name_pane(
        &self,
        server_start: u64,
        pane_id: &str,
        name: Option<&str>,
    ) -> io::Result<()> {
        self.pane(server_start, pane_id)?;
        (|| -> rusqlite::Result<_> {
            self.connection.execute(
                "UPDATE panes SET name=?3 WHERE server_start=?1 AND pane_id=?2",
                params![number(server_start)?, pane_id, name],
            )?;
            Ok(())
        })()
        .map_err(|e| db_error(&self.path, e))
    }
    pub fn pane_name(&self, server_start: u64, pane_id: &str) -> io::Result<Option<String>> {
        (|| -> rusqlite::Result<_> {
            self.connection
                .query_row(
                    "SELECT name FROM panes WHERE server_start=?1 AND pane_id=?2",
                    params![number(server_start)?, pane_id],
                    |r| r.get(0),
                )
                .optional()
                .map(Option::flatten)
        })()
        .map_err(|e| db_error(&self.path, e))
    }
}
