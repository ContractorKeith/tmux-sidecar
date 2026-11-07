use super::{App, Row};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
pub fn draw(app: &App, frame: &mut Frame, now: u64) {
    let narrow = frame.area().width < 60;
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(if narrow { 3 } else { 2 }),
    ])
    .split(frame.area());
    frame.render_widget(
        Paragraph::new(if app.data.connected {
            "tmux-sidecar"
        } else {
            "tmux-sidecar · daemon restarting"
        }),
        areas[0],
    );
    let rows = app.rows();
    let mut index = 0;
    let mut selected = None;
    let items: Vec<_> = rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| match row {
            Row::Heading(name) => ListItem::new(Line::styled(
                name.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Row::Run(r) => {
                if index == app.selected {
                    selected = Some(row_index);
                }
                index += 1;
                let state = format!("{:?}", r.run.state).to_lowercase();
                let agent = format!("{:?}", r.run.agent).to_lowercase();
                let age = now.saturating_sub(r.run.state_since) / 1000;
                let age = if age >= 3600 {
                    format!("{}h", age / 3600)
                } else if age >= 60 {
                    format!("{}m", age / 60)
                } else {
                    format!("{age}s")
                };
                let name = r.pane_name.as_deref().unwrap_or(&r.run.pane_id);
                let last = r
                    .run
                    .last_assistant_text
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("");
                let text = format!("{state:<7} {agent:<8} {name} · {age} · {last}");
                let color = match r.run.state {
                    crate::model::AgentState::Blocked => Color::Yellow,
                    crate::model::AgentState::Error => Color::Red,
                    crate::model::AgentState::Working => Color::Green,
                    _ => Color::Reset,
                };
                ListItem::new(text).style(if app.color {
                    Style::default().fg(color)
                } else {
                    Style::default()
                })
            }
        })
        .collect();
    let mut state = ListState::default().with_selected(selected);
    let title = if app.manual {
        "Manual groups"
    } else {
        "Projects"
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_symbol("› ")
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, areas[1], &mut state);
    let status = if app.filtering {
        format!("/{}▏", app.filter)
    } else if !app.filter.is_empty() {
        format!("filter: {}", app.filter)
    } else if app.data.runs.is_empty() {
        "No agent runs".into()
    } else {
        app.message.clone()
    };
    frame.render_widget(
        Paragraph::new(if narrow {
            format!("j/k move · Enter open · a ack\ng group · / filter · q quit\n{status}")
        } else {
            format!("j/k move · Enter open · a ack · g group · / filter · q quit\n{status}")
        }),
        areas[2],
    );
}
