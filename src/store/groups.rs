use super::*;
use serde_json::{Value, json};
impl Store {
    pub fn group_edit(&self, name: &str, runs: &[AgentRun], add: bool) -> io::Result<()> {
        let tx = self
            .connection
            .unchecked_transaction()
            .map_err(io::Error::other)?;
        if add {
            tx.execute("INSERT OR IGNORE INTO groups(name) VALUES (?1)", [name])
                .map_err(io::Error::other)?;
        }
        for run in runs {
            self.pane(run.server_start, &run.pane_id)?;
            let sql = if add {
                "INSERT OR IGNORE INTO group_members SELECT id,?2,?3 FROM groups WHERE name=?1"
            } else {
                "DELETE FROM group_members WHERE group_id=(SELECT id FROM groups WHERE name=?1) AND server_start=?2 AND pane_id=?3"
            };
            tx.execute(
                sql,
                params![
                    name,
                    number(run.server_start).map_err(io::Error::other)?,
                    run.pane_id
                ],
            )
            .map_err(io::Error::other)?;
        }
        tx.commit().map_err(io::Error::other)
    }
    pub fn group_contains(&self, name: &str, run: &AgentRun) -> io::Result<bool> {
        self.connection.query_row("SELECT EXISTS(SELECT 1 FROM group_members JOIN groups ON groups.id=group_id WHERE name=?1 AND server_start=?2 AND pane_id=?3)",params![name,number(run.server_start).map_err(io::Error::other)?,run.pane_id],|r|r.get(0)).map_err(io::Error::other)
    }
    pub fn groups(&self) -> io::Result<Value> {
        let mut result = Vec::new();
        let mut stmt=self.connection.prepare("SELECT name,server_start,pane_id FROM groups LEFT JOIN group_members ON groups.id=group_id ORDER BY name,server_start,pane_id").map_err(io::Error::other)?;
        for row in stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(io::Error::other)?
        {
            let (name, start, pane) = row.map_err(io::Error::other)?;
            result.push(json!({"name":name,"server_start":start,"pane_id":pane}));
        }
        Ok(json!(result))
    }
    pub fn history(&self, run: &AgentRun, limit: u32) -> io::Result<Value> {
        let mut stmt=self.connection.prepare("SELECT id,at,source,event,detail,payload_json FROM signals WHERE run_id=?1 ORDER BY at DESC,id DESC LIMIT ?2").map_err(io::Error::other)?;
        let records = stmt
            .query_map(
                params![run_id(&run.id).map_err(io::Error::other)?, limit],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                },
            )
            .map_err(io::Error::other)?;
        let mut result = Vec::new();
        for record in records {
            let (id, at, source, event, detail, payload) = record.map_err(io::Error::other)?;
            let payload: Value = serde_json::from_str(&payload)?;
            result.push(json!({"id":id,"run_id":run.id,"at":at,"source":source,"event":event,"detail":detail,"payload":payload}));
        }
        Ok(json!(result))
    }
}
