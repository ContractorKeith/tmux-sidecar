//! Stream UI-relevant columns into a digest; never allocate or deep-compare JSON.
use super::*;
use std::hash::{DefaultHasher, Hash, Hasher};
impl Store {
    pub(crate) fn total_changes(&self) -> u64 {
        self.connection.total_changes()
    }
    pub(crate) fn view_digest(&self) -> io::Result<u64> {
        let mut digest = DefaultHasher::new();
        for sql in [
            "SELECT r.id,r.server_start,r.pane_id,r.agent,r.cwd,r.project_root,r.started_at,r.ended_at,r.state,r.state_since,r.state_source,r.read_at,r.last_assistant_text,p.name FROM runs r LEFT JOIN panes p ON p.server_start=r.server_start AND p.pane_id=r.pane_id WHERE r.ended_at IS NULL ORDER BY r.id",
            "SELECT g.name,m.server_start,m.pane_id FROM groups g LEFT JOIN group_members m ON m.group_id=g.id ORDER BY g.name,m.server_start,m.pane_id",
        ] {
            let mut stmt = self.connection.prepare(sql).map_err(io::Error::other)?;
            let count = stmt.column_count();
            let mut rows = stmt.query([]).map_err(io::Error::other)?;
            while let Some(row) = rows.next().map_err(io::Error::other)? {
                count.hash(&mut digest);
                for column in 0..count {
                    use rusqlite::types::ValueRef;
                    let value = row.get_ref(column).map_err(io::Error::other)?;
                    std::mem::discriminant(&value).hash(&mut digest);
                    match value {
                        ValueRef::Null => (),
                        ValueRef::Integer(v) => v.hash(&mut digest),
                        ValueRef::Real(v) => v.to_bits().hash(&mut digest),
                        ValueRef::Text(v) | ValueRef::Blob(v) => v.hash(&mut digest),
                    }
                }
            }
        }
        Ok(digest.finish())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn digest_tracks_visible_columns_and_ignores_signal_history() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(&root.path().join("db")).unwrap();
        store.connection.execute_batch("INSERT INTO runs(id,server_start,pane_id,agent,agent_pid,cwd,started_at,state,state_since,state_source,last_assistant_text) VALUES(1,1,'%0','claude',1,'/work',1,'waiting',1,'process','');").unwrap();
        let initial = store.view_digest().unwrap();
        store.connection.execute_batch("INSERT INTO signals(run_id,at,source,event,payload_json) VALUES(1,1,'hook:claude','unknown','{}')").unwrap();
        assert_eq!(initial, store.view_digest().unwrap());
        let mut before = initial;
        for sql in [
            "UPDATE runs SET state_source='hook:claude'",
            "UPDATE runs SET last_assistant_text='new answer'",
            "UPDATE runs SET read_at=2",
            "UPDATE runs SET cwd='/other'",
            "UPDATE runs SET state='blocked'",
            "INSERT INTO panes(server_start,pane_id,name) VALUES(1,'%0','build')",
            "INSERT INTO groups(id,name) VALUES(1,'review'); INSERT INTO group_members VALUES(1,1,'%0')",
            "UPDATE runs SET ended_at=3",
        ] {
            store.connection.execute_batch(sql).unwrap();
            let after = store.view_digest().unwrap();
            assert_ne!(before, after, "{sql}");
            before = after;
        }
    }
}
