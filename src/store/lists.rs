use super::*;
impl Store {
    pub fn list_page(
        &self,
        all: bool,
        state: Option<crate::model::AgentState>,
        limit: Option<u32>,
        before: Option<u64>,
        manual: Option<&str>,
    ) -> io::Result<Vec<AgentRun>> {
        (|| -> rusqlite::Result<_> {
            self.connection
                .prepare(
                    "SELECT * FROM runs WHERE (?1 OR ended_at IS NULL)
                AND (?2 IS NULL OR state=?2) AND (?3 IS NULL OR id<?3)
                AND (?4 IS NULL OR EXISTS(SELECT 1 FROM group_members m JOIN groups g ON g.id=m.group_id
                    WHERE g.name=?4 AND m.server_start=runs.server_start AND m.pane_id=runs.pane_id))
                ORDER BY id DESC LIMIT ?5",
                )?
                .query_map(
                    params![all,state.as_ref().map(enum_name),before.map(number).transpose()?,manual,limit.map(i64::from).unwrap_or(-1)],
                    row,
                )?
                .collect()
        })()
        .map_err(|e| db_error(&self.path,e))
    }
}

impl Store {
    pub fn attention(&self) -> io::Result<Vec<AgentRun>> {
        let mut runs: Vec<_> = self
            .active()?
            .into_iter()
            .filter(|r| {
                matches!(
                    r.state,
                    crate::model::AgentState::Blocked
                        | crate::model::AgentState::Error
                        | crate::model::AgentState::Waiting
                ) && r.read_at.is_none_or(|at| at < r.state_since)
            })
            .collect();
        runs.sort_by_key(|r| {
            (
                match r.state {
                    crate::model::AgentState::Blocked => 0,
                    crate::model::AgentState::Error => 1,
                    _ => 2,
                },
                r.state_since,
                r.id[1..].parse::<u64>().expect("stored numeric run id"),
            )
        });
        Ok(runs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn group_query_filters_before_limit_and_does_not_decode_ended_history() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(&root.path().join("db")).unwrap();
        let pane = crate::seams::PaneSnapshot {
            server_start: 1,
            pane_id: "%1".into(),
            pane_pid: 1,
            cwd: "/work".into(),
            current_command: "claude".into(),
            pane_active: false,
            window_active: false,
            session_attached: false,
        };
        let processes = [(
            1,
            crate::seams::Process {
                ppid: 0,
                comm: "claude".into(),
            },
        )]
        .into();
        let mut run = crate::engine::detect(&[pane], &processes, &[], 1000)
            .start
            .remove(0);
        run.id = store.insert(&run).unwrap();
        store.group_edit("chosen", &[run.clone()], true).unwrap();
        let old = store
            .insert(&crate::model::AgentRun {
                agent_pid: 2,
                ..run.clone()
            })
            .unwrap();
        store.end(&old, 2000).unwrap();
        // If the caller walks all history, this old malformed record fails decoding.
        store
            .connection
            .execute(
                "UPDATE runs SET state='bad' WHERE id=?1",
                [run_id(&old).unwrap()],
            )
            .unwrap();
        assert!(store.list().is_err());
        let selected = store
            .list_page(false, None, Some(1), None, Some("chosen"))
            .unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, run.id);
        assert!(
            store
                .list_page(false, None, Some(1), None, Some("absent"))
                .unwrap()
                .is_empty()
        );
    }
}
