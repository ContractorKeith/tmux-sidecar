use super::App;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Quit,
    Open,
    Ack,
}
// Decode only key presses; editing mode owns ordinary characters, including q/j/k.
pub fn decode(app: &mut App, key: KeyEvent) -> Action {
    if key.kind == KeyEventKind::Release {
        return Action::None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::Quit;
    }
    if app.filtering {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => app.filtering = false,
            KeyCode::Backspace => {
                app.filter.pop();
                app.selected = 0;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                app.filter.push(c);
                app.selected = 0;
            }
            _ => (),
        }
        return Action::None;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
        KeyCode::Char('j') | KeyCode::Down => {
            app.selected = app.selected.saturating_add(1);
            app.clamp();
        }
        KeyCode::Char('k') | KeyCode::Up => app.selected = app.selected.saturating_sub(1),
        KeyCode::Char('g') => {
            app.manual = !app.manual;
            app.selected = 0;
        }
        KeyCode::Char('/') => app.filtering = true,
        KeyCode::Enter => return Action::Open,
        KeyCode::Char('a') => return Action::Ack,
        _ => (),
    }
    Action::None
}
