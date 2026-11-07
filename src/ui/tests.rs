use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
fn fixture() -> App {
    let runs=(0..6).map(|i|serde_json::from_value(serde_json::json!({
        "id":format!("r{}",i+1),"server_start":1,"pane_id":format!("%{i}"),"agent":(["claude","codex","grok","opencode"][i%4]),"agent_pid":i+1,
        "agent_session_id":null,"cwd":"/work","project_root":if i<3 {"/work/alpha"}else{"/work/beta"},"started_at":1000,"ended_at":null,
        "state":(["blocked","error","waiting","working","idle","done"][i]),"state_since":1000,"state_source":"hook","read_at":null,
        "last_assistant_text":"First line\nLatest answer","transcript_path":null,"pane_name":if i==0 {Some("build")}else{None}
    })).unwrap()).collect();
    App {
        data: Snapshot {
            runs,
            groups: vec![Group {
                name: "review".into(),
                server_start: Some(1),
                pane_id: Some("%0".into()),
            }],
            connected: true,
            error: String::new(),
        },
        ..App::default()
    }
}
fn snapshot(name: &str, app: &App, width: u16) {
    let mut terminal = Terminal::new(TestBackend::new(width, 18)).unwrap();
    terminal.draw(|f| render::draw(app, f, 61_000)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut actual = String::new();
    for y in 0..18 {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buffer[(x, y)].symbol());
        }
        actual.push_str(line.trim_end());
        actual.push('\n');
    }
    let actual = format!("{}\n", actual.trim_end());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/ui/{name}.txt"));
    if std::env::var_os("UPDATE_UI_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
    }
    assert_eq!(
        actual,
        std::fs::read_to_string(path).unwrap(),
        "snapshot {name}"
    );
    // NO_COLOR leaves every cell at terminal-default foreground/background.
    if app.color {
        use ratatui::style::Color;
        for (text, color) in [
            ("blocked", Color::Yellow),
            ("error", Color::Red),
            ("working", Color::Green),
        ] {
            let y = (0..18)
                .find(|&y| {
                    (0..width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                        .contains(text)
                })
                .unwrap();
            let cell = (0..width)
                .map(|x| &buffer[(x, y)])
                .find(|c| c.symbol() == &text[..1])
                .unwrap();
            assert_eq!(cell.fg, color, "{text}");
        }
    }
    if !app.color {
        for cell in &buffer.content {
            assert_eq!(cell.fg, ratatui::style::Color::Reset);
            assert_eq!(cell.bg, ratatui::style::Color::Reset);
        }
    }
}
#[test]
fn backend_snapshots() {
    snapshot(
        "empty",
        &App {
            data: Snapshot {
                connected: true,
                ..Snapshot::default()
            },
            ..App::default()
        },
        80,
    );
    let mut app = fixture();
    snapshot("mixed-projects", &app, 80);
    app.manual = true;
    snapshot("manual", &app, 80);
    app.manual = false;
    snapshot("narrow", &app, 40);
    app.data.connected = false;
    snapshot("restarting", &app, 80);
    app.color = true;
    snapshot("color", &app, 80);
}
#[test]
fn decoder_navigation_actions_and_filter_editing() {
    use input::{Action, decode};
    let mut app = fixture();
    let key = |c| KeyEvent::new(c, KeyModifiers::NONE);
    decode(&mut app, key(KeyCode::Char('j')));
    assert_eq!(app.selected_run().unwrap().run.id, "r2");
    decode(&mut app, key(KeyCode::Up));
    assert_eq!(app.selected, 0);
    assert_eq!(decode(&mut app, key(KeyCode::Enter)), Action::Open);
    assert_eq!(decode(&mut app, key(KeyCode::Char('a'))), Action::Ack);
    decode(&mut app, key(KeyCode::Char('g')));
    assert!(app.manual);
    decode(&mut app, key(KeyCode::Char('/')));
    for c in "qjk".chars() {
        assert_eq!(decode(&mut app, key(KeyCode::Char(c))), Action::None);
    }
    assert_eq!(app.filter, "qjk");
    assert!(app.selected_run().is_none());
    decode(&mut app, key(KeyCode::Backspace));
    assert_eq!(app.filter, "qj");
    decode(&mut app, key(KeyCode::Esc));
    assert!(!app.filtering);
    assert_eq!(decode(&mut app, key(KeyCode::Char('q'))), Action::Quit);
    let mut release = key(KeyCode::Enter);
    release.kind = crossterm::event::KeyEventKind::Release;
    assert_eq!(decode(&mut app, release), Action::Ignored);
    assert_eq!(decode(&mut app, key(KeyCode::Char('x'))), Action::Ignored);
}
#[test]
fn selection_survives_reordering_and_clamps_after_removal() {
    let mut app = fixture();
    app.selected = 1;
    let mut data = app.data.clone();
    data.runs[0].run.state = crate::model::AgentState::Working;
    app.replace(data);
    assert_eq!(app.selected_run().unwrap().run.id, "r2");
    app.replace(Snapshot::default());
    assert_eq!(app.selected, 0);
    assert!(app.selected_run().is_none());
}
#[test]
fn project_selection_stays_in_project_after_live_update() {
    let mut app = fixture();
    app.selected = 3; // r1 under /work/alpha, after its attention duplicate.
    assert_eq!(app.selected_run().unwrap().run.id, "r1");
    let mut data = app.data.clone();
    data.runs[0].run.last_assistant_text = "New answer".into();
    app.replace(data);
    assert_eq!(app.selected, 3);
}

#[test]
fn connection_error_is_visible_with_no_runs() {
    let app = App {
        data: Snapshot {
            error: "wrong tmux server".into(),
            ..Snapshot::default()
        },
        ..App::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
    terminal.draw(|f| render::draw(&app, f, 1)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(text.contains("wrong tmux server"));
}
