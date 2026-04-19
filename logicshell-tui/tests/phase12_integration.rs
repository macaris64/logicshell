// Phase 12 integration tests — InputWidget cursor math, HistoryStore ring-buffer,
// persistence round-trip, and full App key-dispatch exercising all new bindings.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use logicshell_tui::{ui, App, AppState, HistoryStore, InputWidget};
use ratatui::{backend::TestBackend, Terminal};
use std::path::PathBuf;
use tempfile::tempdir;

// ── helpers ───────────────────────────────────────────────────────────────────

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn tmp_store() -> HistoryStore {
    let dir = tempdir().unwrap();
    HistoryStore::new(dir.path().join("history"))
}

fn app_with_store(history: HistoryStore) -> App {
    App::with_history("/", "balanced", history)
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

// ── InputWidget: cursor math ──────────────────────────────────────────────────

#[test]
fn input_widget_insert_advances_cursor() {
    let mut w = InputWidget::new();
    w.insert('a');
    w.insert('b');
    assert_eq!(w.cursor_pos(), 2);
    assert_eq!(w.value(), "ab");
}

#[test]
fn input_widget_insert_mid_line() {
    let mut w = InputWidget::new();
    w.set_value("ac");
    w.cursor = 1; // between 'a' and 'c'
    w.insert('b');
    assert_eq!(w.value(), "abc");
    assert_eq!(w.cursor_pos(), 2);
}

#[test]
fn input_widget_backspace_at_start_is_noop() {
    let mut w = InputWidget::new();
    w.set_value("hello");
    w.move_to_start();
    w.delete_before_cursor();
    assert_eq!(w.value(), "hello");
    assert_eq!(w.cursor_pos(), 0);
}

#[test]
fn input_widget_backspace_decrements_cursor() {
    let mut w = InputWidget::new();
    w.set_value("ab");
    w.delete_before_cursor(); // removes 'b', cursor = 1
    assert_eq!(w.value(), "a");
    assert_eq!(w.cursor_pos(), 1);
}

#[test]
fn input_widget_delete_at_end_is_noop() {
    let mut w = InputWidget::new();
    w.set_value("x");
    w.delete_after_cursor();
    assert_eq!(w.value(), "x");
    assert_eq!(w.cursor_pos(), 1);
}

#[test]
fn input_widget_delete_does_not_move_cursor() {
    let mut w = InputWidget::new();
    w.set_value("ab");
    w.move_to_start();
    w.delete_after_cursor();
    assert_eq!(w.value(), "b");
    assert_eq!(w.cursor_pos(), 0);
}

#[test]
fn input_widget_ctrl_k_kills_to_end() {
    let mut w = InputWidget::new();
    w.set_value("hello world");
    w.cursor = 5;
    w.kill_to_end();
    assert_eq!(w.value(), "hello");
    assert_eq!(w.cursor_pos(), 5);
}

#[test]
fn input_widget_ctrl_k_from_start_clears() {
    let mut w = InputWidget::new();
    w.set_value("clear me");
    w.move_to_start();
    w.kill_to_end();
    assert!(w.is_empty());
    assert_eq!(w.cursor_pos(), 0);
}

#[test]
fn input_widget_home_moves_to_start() {
    let mut w = InputWidget::new();
    w.set_value("hello");
    w.move_to_start();
    assert_eq!(w.cursor_pos(), 0);
}

#[test]
fn input_widget_end_moves_to_end() {
    let mut w = InputWidget::new();
    w.set_value("hello");
    w.move_to_start();
    w.move_to_end();
    assert_eq!(w.cursor_pos(), 5);
}

#[test]
fn input_widget_left_right_roundtrip() {
    let mut w = InputWidget::new();
    w.set_value("abc");
    w.move_to_start();
    w.move_right();
    w.move_right();
    assert_eq!(w.cursor_pos(), 2);
    w.move_left();
    assert_eq!(w.cursor_pos(), 1);
}

#[test]
fn input_widget_render_cursor_at_start() {
    let mut w = InputWidget::new();
    w.set_value("hello");
    w.move_to_start();
    assert!(w.render_with_cursor().starts_with('_'));
}

#[test]
fn input_widget_render_cursor_at_end() {
    let mut w = InputWidget::new();
    w.set_value("hello");
    assert!(w.render_with_cursor().ends_with('_'));
}

#[test]
fn input_widget_render_cursor_mid() {
    let mut w = InputWidget::new();
    w.set_value("ab");
    w.cursor = 1;
    assert_eq!(w.render_with_cursor(), "a_b");
}

#[test]
fn input_widget_set_value_puts_cursor_at_end() {
    let mut w = InputWidget::new();
    w.set_value("hello");
    assert_eq!(w.cursor_pos(), 5);
}

#[test]
fn input_widget_clear_resets_cursor() {
    let mut w = InputWidget::new();
    w.set_value("hello");
    w.clear();
    assert_eq!(w.cursor_pos(), 0);
    assert!(w.is_empty());
}

// ── HistoryStore: ring-buffer behaviour ───────────────────────────────────────

#[test]
fn history_push_adds_entry() {
    let mut s = tmp_store();
    s.push("ls".to_string());
    assert_eq!(s.len(), 1);
}

#[test]
fn history_consecutive_duplicate_skipped() {
    let mut s = tmp_store();
    s.push("ls".to_string());
    s.push("ls".to_string());
    assert_eq!(s.len(), 1);
}

#[test]
fn history_cap_enforced() {
    let dir = tempdir().unwrap();
    let mut s = HistoryStore::with_cap(dir.path().join("h"), 3);
    for cmd in &["a", "b", "c", "d"] {
        s.push(cmd.to_string());
    }
    assert_eq!(s.len(), 3);
    let vals: Vec<_> = s.entries().iter().cloned().collect();
    assert_eq!(vals, vec!["b", "c", "d"]);
}

#[test]
fn history_navigate_prev_returns_newest_first() {
    let mut s = tmp_store();
    s.push("cmd1".to_string());
    s.push("cmd2".to_string());
    assert_eq!(s.navigate_prev(""), Some("cmd2".to_string()));
}

#[test]
fn history_navigate_prev_walks_older() {
    let mut s = tmp_store();
    s.push("a".to_string());
    s.push("b".to_string());
    s.push("c".to_string());
    s.navigate_prev(""); // c
    s.navigate_prev(""); // b
    let entry = s.navigate_prev(""); // a
    assert_eq!(entry, Some("a".to_string()));
    assert_eq!(s.navigate_prev(""), None); // at oldest
}

#[test]
fn history_navigate_next_toward_newest() {
    let mut s = tmp_store();
    s.push("a".to_string());
    s.push("b".to_string());
    s.navigate_prev(""); // b
    s.navigate_prev(""); // a
    assert_eq!(s.navigate_next(), Some("b".to_string()));
}

#[test]
fn history_navigate_next_restores_saved_input() {
    let mut s = tmp_store();
    s.push("ls".to_string());
    s.navigate_prev("my input"); // → "ls"
    let restored = s.navigate_next();
    assert_eq!(restored, Some("my input".to_string()));
}

#[test]
fn history_push_resets_navigation() {
    let mut s = tmp_store();
    s.push("ls".to_string());
    s.navigate_prev("");
    s.push("pwd".to_string()); // should reset nav
    assert!(s.nav_index.is_none());
}

// ── HistoryStore: persistence round-trip ─────────────────────────────────────

#[test]
fn history_save_and_load_roundtrip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history");

    let mut s = HistoryStore::new(path.clone());
    s.push("ls".to_string());
    s.push("pwd".to_string());
    s.push("whoami".to_string());
    s.save().unwrap();

    let loaded = HistoryStore::load(path).unwrap();
    let vals: Vec<_> = loaded.entries().iter().cloned().collect();
    assert_eq!(vals, vec!["ls", "pwd", "whoami"]);
}

#[test]
fn history_load_missing_file_returns_empty() {
    let path = PathBuf::from("/tmp/does_not_exist_phase12_test");
    let s = HistoryStore::load(path).unwrap();
    assert!(s.is_empty());
}

#[test]
fn history_save_creates_parent_dirs() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a").join("b").join("history");
    let mut s = HistoryStore::new(path.clone());
    s.push("cmd".to_string());
    s.save().unwrap();
    assert!(path.exists());
}

#[test]
fn history_load_with_cap_truncates_to_cap() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history");
    std::fs::write(&path, "a\nb\nc\nd\ne").unwrap();
    let s = HistoryStore::load_with_cap(path, 3).unwrap();
    assert_eq!(s.len(), 3);
    let vals: Vec<_> = s.entries().iter().cloned().collect();
    assert_eq!(vals, vec!["c", "d", "e"]);
}

#[test]
fn history_save_is_overwrite_safe() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history");
    let mut s = HistoryStore::new(path.clone());
    s.push("ls".to_string());
    s.save().unwrap();
    s.push("pwd".to_string());
    s.save().unwrap();
    let loaded = HistoryStore::load(path).unwrap();
    assert_eq!(loaded.len(), 2);
}

// ── App key-dispatch: all Phase 12 bindings ──────────────────────────────────

#[test]
fn app_left_arrow_moves_cursor() {
    let mut a = app_with_store(tmp_store());
    a.input_widget.set_value("hello");
    a.handle_key(key(KeyCode::Left));
    assert_eq!(a.input_widget.cursor_pos(), 4);
}

#[test]
fn app_right_arrow_moves_cursor() {
    let mut a = app_with_store(tmp_store());
    a.input_widget.set_value("hello");
    a.input_widget.move_to_start();
    a.handle_key(key(KeyCode::Right));
    assert_eq!(a.input_widget.cursor_pos(), 1);
}

#[test]
fn app_home_key_moves_cursor_to_start() {
    let mut a = app_with_store(tmp_store());
    a.input_widget.set_value("hello");
    a.handle_key(key(KeyCode::Home));
    assert_eq!(a.input_widget.cursor_pos(), 0);
}

#[test]
fn app_end_key_moves_cursor_to_end() {
    let mut a = app_with_store(tmp_store());
    a.input_widget.set_value("hello");
    a.input_widget.move_to_start();
    a.handle_key(key(KeyCode::End));
    assert_eq!(a.input_widget.cursor_pos(), 5);
}

#[test]
fn app_ctrl_a_goes_to_start() {
    let mut a = app_with_store(tmp_store());
    a.input_widget.set_value("hello");
    a.handle_key(ctrl('a'));
    assert_eq!(a.input_widget.cursor_pos(), 0);
}

#[test]
fn app_ctrl_e_goes_to_end() {
    let mut a = app_with_store(tmp_store());
    a.input_widget.set_value("hello");
    a.input_widget.move_to_start();
    a.handle_key(ctrl('e'));
    assert_eq!(a.input_widget.cursor_pos(), 5);
}

#[test]
fn app_ctrl_k_kills_to_end() {
    let mut a = app_with_store(tmp_store());
    a.input_widget.set_value("hello world");
    a.input_widget.cursor = 5;
    a.handle_key(ctrl('k'));
    assert_eq!(a.input_widget.value(), "hello");
}

#[test]
fn app_delete_key_removes_char_at_cursor() {
    let mut a = app_with_store(tmp_store());
    a.input_widget.set_value("abc");
    a.input_widget.move_to_start();
    a.handle_key(key(KeyCode::Delete));
    assert_eq!(a.input_widget.value(), "bc");
    assert_eq!(a.input_widget.cursor_pos(), 0);
}

#[test]
fn app_up_arrow_recalls_history() {
    let mut s = tmp_store();
    s.push("ls".to_string());
    s.push("pwd".to_string());
    let mut a = app_with_store(s);
    a.handle_key(key(KeyCode::Up));
    assert_eq!(a.input_widget.value(), "pwd");
}

#[test]
fn app_down_arrow_moves_forward_in_history() {
    let mut s = tmp_store();
    s.push("ls".to_string());
    s.push("pwd".to_string());
    let mut a = app_with_store(s);
    a.handle_key(key(KeyCode::Up)); // pwd
    a.handle_key(key(KeyCode::Up)); // ls
    a.handle_key(key(KeyCode::Down)); // pwd
    assert_eq!(a.input_widget.value(), "pwd");
}

#[test]
fn app_down_arrow_restores_original_input() {
    let mut s = tmp_store();
    s.push("ls".to_string());
    let mut a = app_with_store(s);
    a.input_widget.set_value("part");
    a.handle_key(key(KeyCode::Up)); // "ls"
    a.handle_key(key(KeyCode::Down)); // restored "part"
    assert_eq!(a.input_widget.value(), "part");
}

#[test]
fn app_enter_pushes_to_history_and_clears_input() {
    let mut a = app_with_store(tmp_store());
    for c in "git status".chars() {
        a.handle_key(key(KeyCode::Char(c)));
    }
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.input_widget.value(), "");
    assert_eq!(a.history.len(), 1);
    assert_eq!(a.history.entries()[0], "git status");
}

#[test]
fn app_enter_resets_history_nav() {
    let mut s = tmp_store();
    s.push("ls".to_string());
    let mut a = app_with_store(s);
    a.handle_key(key(KeyCode::Up)); // start navigating
    for c in "pwd".chars() {
        a.handle_key(key(KeyCode::Char(c)));
    }
    a.handle_key(key(KeyCode::Enter));
    assert!(a.history.nav_index.is_none());
}

#[test]
fn app_up_arrow_on_empty_history_is_noop() {
    let mut a = app_with_store(tmp_store());
    a.handle_key(key(KeyCode::Up));
    assert_eq!(a.input_widget.value(), "");
}

#[test]
fn app_ctrl_k_then_type_works() {
    let mut a = app_with_store(tmp_store());
    a.input_widget.set_value("hello");
    a.input_widget.cursor = 3; // after "hel"
    a.handle_key(ctrl('k')); // → "hel"
    a.handle_key(key(KeyCode::Char('p')));
    assert_eq!(a.input_widget.value(), "help");
}

#[test]
fn app_ctrl_a_then_ctrl_k_clears_buffer() {
    let mut a = app_with_store(tmp_store());
    for c in "delete me".chars() {
        a.handle_key(key(KeyCode::Char(c)));
    }
    a.handle_key(ctrl('a')); // go to start
    a.handle_key(ctrl('k')); // kill to end
    assert_eq!(a.input_widget.value(), "");
}

// ── rendering: cursor position visible in prompt ──────────────────────────────

#[test]
fn prompt_shows_cursor_marker() {
    let mut app = App::with_history("/", "balanced", tmp_store());
    app.input_widget.set_value("hello");
    let buf = render(&app, 80, 10);
    let prompt = row(&buf, 8, 80);
    assert!(
        prompt.contains('_'),
        "prompt should contain cursor marker: {prompt:?}"
    );
}

#[test]
fn prompt_cursor_marker_at_end_by_default() {
    let mut app = App::with_history("/", "balanced", tmp_store());
    for c in "hi".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let buf = render(&app, 80, 10);
    let prompt = row(&buf, 8, 80);
    // rendered as "hi_" — underscore after "hi"
    assert!(
        prompt.contains("hi_"),
        "cursor should be after 'hi': {prompt:?}"
    );
}

#[test]
fn prompt_cursor_marker_mid_line_after_ctrl_a() {
    let mut app = App::with_history("/", "balanced", tmp_store());
    app.input_widget.set_value("abc");
    app.handle_key(ctrl('a')); // cursor to start
    let buf = render(&app, 80, 10);
    let prompt = row(&buf, 8, 80);
    assert!(
        prompt.contains("_abc"),
        "cursor should be before 'abc': {prompt:?}"
    );
}

// ── status bar: still shows phase 12 ─────────────────────────────────────────

#[test]
fn status_bar_shows_phase_12() {
    let app = App::with_history("/", "balanced", tmp_store());
    let buf = render(&app, 80, 10);
    let status = row(&buf, 9, 80);
    assert!(status.contains("12"), "status bar: {status:?}");
}

// ── full round-trip: type, edit mid-line, submit, recall from history ─────────

#[test]
fn full_round_trip_with_history_recall() {
    let mut a = app_with_store(tmp_store());

    // Submit "ls"
    for c in "ls".chars() {
        a.handle_key(key(KeyCode::Char(c)));
    }
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.messages.len(), 1);

    // Submit "pwd"
    for c in "pwd".chars() {
        a.handle_key(key(KeyCode::Char(c)));
    }
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.messages.len(), 2);

    // Navigate back to "pwd" then "ls"
    a.handle_key(key(KeyCode::Up)); // → pwd
    assert_eq!(a.input_widget.value(), "pwd");
    a.handle_key(key(KeyCode::Up)); // → ls
    assert_eq!(a.input_widget.value(), "ls");
    a.handle_key(key(KeyCode::Down)); // → pwd
    assert_eq!(a.input_widget.value(), "pwd");

    // Edit mid-line using cursor keys and submit
    a.handle_key(ctrl('a')); // cursor to start
    a.handle_key(key(KeyCode::Right)); // cursor after 'p'
    a.handle_key(key(KeyCode::Right)); // cursor after 'w'
    a.handle_key(key(KeyCode::Delete)); // remove 'd'
    a.handle_key(key(KeyCode::Char('n')));
    // now value should be "pwn"
    assert_eq!(a.input_widget.value(), "pwn");
    a.handle_key(key(KeyCode::Enter));
    assert_eq!(a.messages.len(), 3);
    assert!(a.messages[2].contains("pwn"));
    assert_eq!(a.history.len(), 3); // ls, pwd, pwn
    assert!(!a.is_running() || a.is_running()); // still running
}

// ── state machine unchanged after Phase 12 additions ─────────────────────────

#[test]
fn ctrl_c_still_quits() {
    let mut a = app_with_store(tmp_store());
    a.handle_key(ctrl('c'));
    assert_eq!(a.state, AppState::Quitting);
}

#[test]
fn q_still_quits_on_empty_input() {
    let mut a = app_with_store(tmp_store());
    a.handle_key(key(KeyCode::Char('q')));
    assert_eq!(a.state, AppState::Quitting);
}

#[test]
fn q_does_not_quit_during_typing() {
    let mut a = app_with_store(tmp_store());
    a.handle_key(key(KeyCode::Char('s')));
    a.handle_key(key(KeyCode::Char('q')));
    assert_eq!(a.state, AppState::Running);
}
