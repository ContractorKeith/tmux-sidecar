use std::io;
use toml_edit::{Array, ArrayOfTables, InlineTable, Item, Table, Value, value};
const MARKER: &str = "# tmux-sidecar";
fn owned(command: Option<&str>) -> bool {
    command.is_some_and(|s| s.trim_end().ends_with(MARKER))
}
fn table(group: &mut Table) -> bool {
    let Some(item) = group.get_mut("hooks") else {
        return false;
    };
    if let Some(handlers) = item.as_array_of_tables_mut() {
        let before = handlers.len();
        handlers.retain(|h| !owned(h.get("command").and_then(Item::as_str)));
        return before > 0 && handlers.is_empty();
    }
    if let Some(handlers) = item.as_array_mut() {
        return inline_handlers(handlers);
    }
    false
}
fn inline_handlers(handlers: &mut Array) -> bool {
    let before = handlers.len();
    handlers.retain(|h| {
        !owned(
            h.as_inline_table()
                .and_then(|h| h.get("command"))
                .and_then(Value::as_str),
        )
    });
    before > 0 && handlers.is_empty()
}
pub fn edit(item: &mut Item, install: bool, binary: &str) -> io::Result<bool> {
    let command = format!("{} hook codex {MARKER}", super::quote(binary));
    if let Some(groups) = item.as_array_of_tables_mut() {
        let remove: Vec<_> = groups
            .iter_mut()
            .enumerate()
            .filter_map(|(i, g)| table(g).then_some(i))
            .collect();
        for index in remove.into_iter().rev() {
            groups.remove(index);
        }
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
        let remove: Vec<_> = groups
            .iter_mut()
            .enumerate()
            .filter_map(|(i, g)| {
                g.as_inline_table_mut()
                    .and_then(|g| g.get_mut("hooks"))
                    .and_then(Value::as_array_mut)
                    .is_some_and(inline_handlers)
                    .then_some(i)
            })
            .collect();
        for index in remove.into_iter().rev() {
            groups.remove(index);
        }
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
