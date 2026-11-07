//! Committed UI data; queried only on the SQLite owner.
use super::*;
use serde_json::{Value, json};
impl Store {
    pub(crate) fn total_changes(&self) -> u64 {
        self.connection.total_changes()
    }
    pub(crate) fn view(&self) -> io::Result<Value> {
        let mut runs = Vec::new();
        for run in self.active()? {
            let name = self.pane_name(run.server_start, &run.pane_id)?;
            let mut value = serde_json::to_value(run)?;
            value["pane_name"] = json!(name);
            runs.push(value);
        }
        Ok(json!({"runs":runs,"groups":self.groups()?}))
    }
}
