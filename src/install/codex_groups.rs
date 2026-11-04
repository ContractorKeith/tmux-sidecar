use std::io;
use toml_edit::{Array, ArrayOfTables, InlineTable, Item, Table, Value, value};
const MARKER: &str = "# tmux-sidecar";
pub fn edit(item: &mut Item, install: bool, binary: &str) -> io::Result<bool> {
    let command = format!("{} hook codex {MARKER}", super::quote(binary));
    if let Some(groups) = item.as_array_of_tables_mut() {
        groups.retain(|group| {
            !group
                .get("hooks")
                .is_some_and(|h| h.to_string().contains(MARKER))
        });
        if install {
            let mut handler = Table::new();
            handler["type"] = value("command");
            handler["command"] = value(command);
            let mut handlers = ArrayOfTables::new();
            handlers.push(handler);
            let mut group = Table::new();
            group["hooks"] = Item::ArrayOfTables(handlers);
            groups.push(group);
        }
        Ok(groups.is_empty())
    } else if let Some(groups) = item.as_array_mut() {
        groups.retain(|g| !g.to_string().contains(MARKER));
        if install {
            let mut handler = InlineTable::new();
            handler.insert("type", Value::from("command"));
            handler.insert("command", Value::from(command));
            let mut handlers = Array::new();
            handlers.push(handler);
            let mut group = InlineTable::new();
            group.insert("hooks", Value::Array(handlers));
            groups.push(group);
        }
        Ok(groups.is_empty())
    } else {
        Err(io::Error::other("hook groups must be arrays"))
    }
}
