// Phase 11 integration tests — App + UI pipeline without a real terminal
// (updated for Phase 12: App.input → App.input_widget)

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use logicshell_tui::{ui, App, AppState};
use ratatui::{backend::TestBackend, Terminal};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn render(app: &App, w: u16, h: u16) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| ui::draw(f, app)).unwrap();
    term.backend().buffer().clone()
}

fn row(buf: &ratatui::buffer::Buffer, y: u16, w: u16) -> String {
    (0..w)
        .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
        .collect()
}

fn rows(buf: &ratatui::buffer::Buffer, y0: u16, y1: u16, w: u16) -> String {
    (y0..y1)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
        .collect()
}

// ── state machine ─────────────────────────────────────────────────────────────

#[test]
fn initial_state_is_running() {
    let app = App::new("/tmp", "balanced");
    assert_eq!(app.state, AppState::Running);
}

#[test]
fn ctrl_c_transitions_to_quitting() {
    let mut app = App::new("/tmp", "balanced");
    app.handle_key(ctrl('c'));
    assert_eq!(app.state, AppState::Quitting);
    assert!(!app.is_running());
}

#[test]
fn q_key_on_empty_input_transitions_to_quitting() {
    let mut app = App::new("/tmp", "balanced");
    app.handle_key(key(KeyCode::Char('q')));
    assert_eq!(app.state, AppState::Quitting);
}

#[test]
fn quit_is_idempotent() {
    let mut app = App::new("/tmp", "balanced");
    app.handle_key(ctrl('c'));
    app.handle_key(ctrl('c'));
    assert_eq!(app.state, AppState::Quitting);
}

// ── event routing ─────────────────────────────────────────────────────────────

#[test]
fn typing_sequence_builds_correct_input() {
    let mut app = App::new("/", "balanced");
    for c in "git status".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    assert_eq!(app.input_widget.value(), "git status");
}

#[test]
fn backspace_corrects_input() {
    let mut app = App::new("/", "balanced");
    for c in "gii".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Backspace));
    app.handle_key(key(KeyCode::Char('t')));
    assert_eq!(app.input_widget.value(), "git");
}

#[test]
fn enter_clears_input_after_submit() {
    let mut app = App::new("/", "balanced");
    for c in "ls".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.input_widget.value(), "");
}

#[test]
fn enter_appends_to_messages() {
    let mut app = App::new("/home", "balanced");
    for c in "echo hello".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.messages.len(), 1);
    assert!(app.messages[0].contains("echo hello"));
}

#[test]
fn multiple_commands_accumulate_in_order() {
    let mut app = App::new("/", "balanced");
    let cmds = ["ls", "pwd", "whoami"];
    for cmd in &cmds {
        for c in cmd.chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        app.handle_key(key(KeyCode::Enter));
    }
    assert_eq!(app.messages.len(), 3);
    assert!(app.messages[0].contains("ls"));
    assert!(app.messages[1].contains("pwd"));
    assert!(app.messages[2].contains("whoami"));
}

#[test]
fn q_mid_word_does_not_quit() {
    let mut app = App::new("/", "balanced");
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Char('q')));
    assert_eq!(app.state, AppState::Running);
    assert_eq!(app.input_widget.value(), "sq");
}

// ── layout rendering to buffer ────────────────────────────────────────────────

#[test]
fn rendered_title_contains_logicshell_and_phase() {
    let app = App::new("/home/user", "balanced");
    let buf = render(&app, 80, 10);
    let title = row(&buf, 0, 80);
    assert!(title.contains("LogicShell"), "title: {title:?}");
    assert!(
        title.contains("12"),
        "title should mention phase 12: {title:?}"
    );
}

#[test]
fn rendered_status_bar_shows_all_fields() {
    let app = App::new("/", "strict");
    let buf = render(&app, 80, 10);
    let status = row(&buf, 9, 80);
    assert!(status.contains("Phase"), "status: {status:?}");
    assert!(status.contains("Safety"), "status: {status:?}");
    assert!(status.contains("strict"), "status: {status:?}");
}

#[test]
fn welcome_shown_before_any_commands() {
    let app = App::new("/", "balanced");
    let buf = render(&app, 80, 10);
    let body = rows(&buf, 1, 9, 80);
    assert!(body.contains("Welcome"), "body: {body:?}");
}

#[test]
fn prompt_renders_cwd() {
    let app = App::new("/var/log", "balanced");
    let buf = render(&app, 80, 10);
    let prompt = row(&buf, 8, 80);
    assert!(prompt.contains("/var/log"), "prompt: {prompt:?}");
}

#[test]
fn prompt_renders_current_input() {
    let mut app = App::new("/", "balanced");
    app.input_widget.set_value("cat foo.txt");
    let buf = render(&app, 80, 10);
    let prompt = row(&buf, 8, 80);
    assert!(prompt.contains("cat foo.txt"), "prompt: {prompt:?}");
}

#[test]
fn messages_area_shows_submitted_commands() {
    let mut app = App::new("/", "balanced");
    app.messages.push("/ > ls -la".to_string());
    let buf = render(&app, 80, 10);
    let body = rows(&buf, 1, 9, 80);
    assert!(body.contains("ls -la"), "body: {body:?}");
    assert!(
        !body.contains("Welcome"),
        "Welcome should be gone: {body:?}"
    );
}

#[test]
fn render_survives_narrow_terminal() {
    let app = App::new("/long/path/here", "balanced");
    let backend = TestBackend::new(15, 5);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| ui::draw(f, &app)).unwrap();
}

#[test]
fn render_survives_wide_terminal() {
    let app = App::new("/", "loose");
    let backend = TestBackend::new(300, 60);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| ui::draw(f, &app)).unwrap();
}

// ── full simulate-and-render round trip ───────────────────────────────────────

#[test]
fn full_interaction_round_trip() {
    let mut app = App::new("/home/aero", "balanced");

    for c in "ls -la".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    assert_eq!(app.input_widget.value(), "ls -la");

    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.input_widget.value(), "");
    assert_eq!(app.messages.len(), 1);

    let buf = render(&app, 80, 10);
    let body = rows(&buf, 1, 9, 80);
    assert!(body.contains("ls -la"), "body after submit: {body:?}");

    app.handle_key(ctrl('c'));
    assert_eq!(app.state, AppState::Quitting);
}
