// Phase 13 integration tests — OutputPanel scroll math, confirm dialog state
// machine, deny-banner render, dispatch-task cancellation.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use logicshell_tui::{ui, App, AppMode, AppState, DispatchEvent, DispatchStatus, OutputPanel};
use ratatui::{backend::TestBackend, Terminal};
use tempfile::tempdir;

// ── helpers ───────────────────────────────────────────────────────────────────

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn tmp_app() -> App {
    let dir = tempdir().unwrap();
    let history = logicshell_tui::HistoryStore::new(dir.path().join("history"));
    App::with_history("/", "balanced", history)
}

fn loose_app() -> App {
    let dir = tempdir().unwrap();
    let history = logicshell_tui::HistoryStore::new(dir.path().join("history"));
    App::with_history("/", "loose", history)
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

fn all_rows(buf: &ratatui::buffer::Buffer, w: u16, h: u16) -> String {
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
        .collect()
}

// ── OutputPanel: ring-buffer integration ──────────────────────────────────────

#[test]
fn output_panel_enforces_cap() {
    let mut p = OutputPanel::new(5);
    for i in 0..10 {
        p.push_line(format!("line {i}"));
    }
    assert_eq!(p.len(), 5);
    let lines: Vec<_> = p.lines().iter().cloned().collect();
    assert_eq!(lines[0], "line 5"); // oldest retained
    assert_eq!(lines[4], "line 9"); // newest
}

#[test]
fn output_panel_scroll_visible_window_at_bottom() {
    let mut p = OutputPanel::new(100);
    for i in 0..20 {
        p.push_line(format!("{i}"));
    }
    // height=5, offset=0 → should see lines 15..19
    let visible = p.visible_lines(5);
    assert_eq!(visible, vec!["15", "16", "17", "18", "19"]);
}

#[test]
fn output_panel_scroll_up_shows_older() {
    let mut p = OutputPanel::new(100);
    for i in 0..20 {
        p.push_line(format!("{i}"));
    }
    p.scroll_up();
    p.scroll_up();
    p.scroll_up();
    // offset=3, height=5 → end=17, start=12 → lines 12..17
    let visible = p.visible_lines(5);
    assert_eq!(visible, vec!["12", "13", "14", "15", "16"]);
}

#[test]
fn output_panel_scroll_down_from_scrolled_position() {
    let mut p = OutputPanel::new(100);
    for i in 0..10 {
        p.push_line(format!("{i}"));
    }
    p.scroll_up();
    p.scroll_up();
    p.scroll_down();
    assert_eq!(p.scroll_offset(), 1);
}

#[test]
fn output_panel_clear_resets_all() {
    let mut p = OutputPanel::new(100);
    for i in 0..10 {
        p.push_line(i.to_string());
    }
    p.scroll_up();
    p.clear();
    assert!(p.is_empty());
    assert_eq!(p.scroll_offset(), 0);
}

#[test]
fn output_panel_default_cap_is_500() {
    let p = OutputPanel::with_default_cap();
    assert_eq!(p.cap(), 500);
}

// ── Confirm dialog: full state machine ───────────────────────────────────────

#[test]
fn confirm_dialog_enter_with_sudo_triggers_confirm_mode() {
    let mut app = tmp_app();
    for c in "sudo whoami".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(
        matches!(app.mode, AppMode::Confirming { .. }),
        "sudo should trigger Confirming mode; mode={:?}",
        app.mode
    );
}

#[test]
fn confirm_y_executes_command() {
    let mut app = tmp_app();
    app.mode = AppMode::Confirming {
        command: "sudo whoami".to_string(),
    };
    app.handle_key(key(KeyCode::Char('y')));
    assert_eq!(app.mode, AppMode::Normal);
    assert!(app.has_pending_command());
    assert_eq!(app.dispatch_status, DispatchStatus::Running);
}

#[test]
fn confirm_enter_executes_command() {
    let mut app = tmp_app();
    app.mode = AppMode::Confirming {
        command: "sudo whoami".to_string(),
    };
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.mode, AppMode::Normal);
    assert!(app.has_pending_command());
}

#[test]
fn confirm_n_cancels_without_dispatch() {
    let mut app = tmp_app();
    app.mode = AppMode::Confirming {
        command: "sudo whoami".to_string(),
    };
    app.handle_key(key(KeyCode::Char('n')));
    assert_eq!(app.mode, AppMode::Normal);
    assert!(!app.has_pending_command());
    assert_eq!(app.dispatch_status, DispatchStatus::Idle);
}

#[test]
fn confirm_esc_cancels() {
    let mut app = tmp_app();
    app.mode = AppMode::Confirming {
        command: "sudo ls".to_string(),
    };
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.mode, AppMode::Normal);
    assert!(!app.has_pending_command());
}

#[test]
fn confirm_dialog_shows_cancelled_in_output_panel() {
    let mut app = tmp_app();
    app.mode = AppMode::Confirming {
        command: "sudo ls".to_string(),
    };
    app.handle_key(key(KeyCode::Char('n')));
    assert!(!app.output_panel.is_empty());
    let lines: Vec<_> = app.output_panel.lines().iter().cloned().collect();
    assert!(lines.iter().any(|l| l.contains("cancelled")));
}

#[test]
fn confirm_y_adds_command_header_to_output_panel() {
    let mut app = tmp_app();
    app.mode = AppMode::Confirming {
        command: "sudo ls".to_string(),
    };
    app.handle_key(key(KeyCode::Char('y')));
    assert!(!app.output_panel.is_empty());
    let lines: Vec<_> = app.output_panel.lines().iter().cloned().collect();
    assert!(lines.iter().any(|l| l.contains("sudo ls")));
}

// ── Deny banner: render integration ──────────────────────────────────────────

#[test]
fn deny_banner_render_shows_denied_in_status_bar() {
    let mut app = tmp_app();
    app.dispatch_status = DispatchStatus::Denied {
        reason: "rm -rf matched".to_string(),
    };
    let buf = render(&app, 80, 10);
    let status = row(&buf, 9, 80);
    assert!(
        status.contains("DENIED"),
        "status bar should show DENIED: {status:?}"
    );
}

#[test]
fn deny_banner_render_shows_reason() {
    let mut app = tmp_app();
    app.dispatch_status = DispatchStatus::Denied {
        reason: "dangerous cmd".to_string(),
    };
    let buf = render(&app, 80, 10);
    let status = row(&buf, 9, 80);
    assert!(
        status.contains("dangerous"),
        "status bar should show reason: {status:?}"
    );
}

#[test]
fn deny_banner_triggered_by_rm_rf_root() {
    let mut app = tmp_app();
    for c in "rm -rf /".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(
        matches!(app.dispatch_status, DispatchStatus::Denied { .. }),
        "rm -rf / should be denied; status={:?}",
        app.dispatch_status
    );
    let buf = render(&app, 80, 10);
    let status = row(&buf, 9, 80);
    assert!(
        status.contains("DENIED"),
        "status bar should show DENIED after rm -rf /: {status:?}"
    );
}

// ── Dispatch done: status bar integration ────────────────────────────────────

#[test]
fn done_status_render_shows_exit_code() {
    let mut app = tmp_app();
    app.dispatch_status = DispatchStatus::Done {
        exit_code: 0,
        duration_ms: 42,
    };
    let buf = render(&app, 80, 10);
    let status = row(&buf, 9, 80);
    assert!(
        status.contains("exit=0"),
        "status bar should show exit=0: {status:?}"
    );
}

#[test]
fn done_status_render_shows_duration() {
    let mut app = tmp_app();
    app.dispatch_status = DispatchStatus::Done {
        exit_code: 0,
        duration_ms: 999,
    };
    let buf = render(&app, 80, 10);
    let status = row(&buf, 9, 80);
    assert!(
        status.contains("999ms"),
        "status bar should show 999ms: {status:?}"
    );
}

#[test]
fn running_status_render() {
    let mut app = tmp_app();
    app.dispatch_status = DispatchStatus::Running;
    let buf = render(&app, 80, 10);
    let status = row(&buf, 9, 80);
    assert!(
        status.contains("Running"),
        "status bar should show Running: {status:?}"
    );
}

// ── Confirm dialog: rendering ─────────────────────────────────────────────────

#[test]
fn confirm_dialog_renders_when_in_confirming_mode() {
    let mut app = tmp_app();
    app.mode = AppMode::Confirming {
        command: "sudo reboot".to_string(),
    };
    let buf = render(&app, 80, 24);
    let all = all_rows(&buf, 80, 24);
    assert!(
        all.contains("Confirm"),
        "confirm dialog should render: <buf>"
    );
}

#[test]
fn confirm_dialog_renders_command_name() {
    let mut app = tmp_app();
    app.mode = AppMode::Confirming {
        command: "sudo reboot".to_string(),
    };
    let buf = render(&app, 80, 24);
    let all = all_rows(&buf, 80, 24);
    assert!(
        all.contains("reboot"),
        "confirm dialog should show command: <buf>"
    );
}

// ── Dispatch-task cancellation ────────────────────────────────────────────────

#[tokio::test]
async fn dispatch_task_cancellation_aborts_task() {
    // Start a long-running task and abort the JoinHandle.
    // Verifies that abort() prevents further output from arriving.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let handle = tokio::spawn(async move {
        // This task would run for 10 seconds without cancellation.
        for i in 0..1000 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let _ = tx.send(format!("line {i}"));
        }
    });

    // Give it a head start
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    // Abort the task
    handle.abort();

    // Wait for abort to propagate
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Drain any lines already sent
    let mut count_before = 0usize;
    while rx.try_recv().is_ok() {
        count_before += 1;
    }

    // After abort, no further lines should arrive
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let mut count_after = 0usize;
    while rx.try_recv().is_ok() {
        count_after += 1;
    }

    assert_eq!(count_after, 0, "no output should arrive after abort");
    let _ = count_before; // some lines may have arrived before abort
}

#[tokio::test]
async fn cancel_dispatch_transitions_to_idle() {
    let dir = tempdir().unwrap();
    let history = logicshell_tui::HistoryStore::new(dir.path().join("history"));
    let mut app = App::with_history("/", "loose", history);
    app.dispatch_status = DispatchStatus::Running;
    app.cancel_dispatch();
    assert_eq!(app.dispatch_status, DispatchStatus::Idle);
}

// ── DispatchEvent application ─────────────────────────────────────────────────

#[test]
fn apply_multiple_output_lines_accumulate_in_panel() {
    let mut app = tmp_app();
    for i in 0..5 {
        app.apply_dispatch_event(DispatchEvent::OutputLine(format!("line {i}")));
    }
    assert_eq!(app.output_panel.len(), 5);
}

#[test]
fn apply_done_event_updates_status() {
    let mut app = tmp_app();
    app.apply_dispatch_event(DispatchEvent::Done {
        exit_code: 42,
        duration_ms: 100,
    });
    assert!(matches!(
        app.dispatch_status,
        DispatchStatus::Done { exit_code: 42, .. }
    ));
}

#[test]
fn apply_error_event_shows_in_panel_and_sets_done() {
    let mut app = tmp_app();
    app.apply_dispatch_event(DispatchEvent::Error("spawn failed".to_string()));
    assert_eq!(
        app.dispatch_status,
        DispatchStatus::Done {
            exit_code: -1,
            duration_ms: 0
        }
    );
    assert!(!app.output_panel.is_empty());
}

// ── Full round-trip: type, submit, output streams ─────────────────────────────

#[test]
fn full_round_trip_allowed_command_starts_dispatch() {
    let mut app = loose_app();
    for c in "echo hello world".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.dispatch_status, DispatchStatus::Running);
    assert!(app.has_pending_command());

    let argv = app.take_pending_command().unwrap();
    assert_eq!(argv, vec!["echo", "hello", "world"]);

    // Simulate dispatch done
    app.apply_dispatch_event(DispatchEvent::OutputLine("hello world".to_string()));
    app.apply_dispatch_event(DispatchEvent::Done {
        exit_code: 0,
        duration_ms: 10,
    });

    assert_eq!(
        app.dispatch_status,
        DispatchStatus::Done {
            exit_code: 0,
            duration_ms: 10
        }
    );
    // output panel has "$ echo hello world" + "hello world" = 2 lines
    assert!(app.output_panel.len() >= 2);
}

#[test]
fn full_round_trip_denied_command_no_dispatch() {
    let mut app = tmp_app();
    for c in "rm -rf /".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(
        matches!(app.dispatch_status, DispatchStatus::Denied { .. }),
        "should be denied"
    );
    assert!(!app.has_pending_command(), "no pending dispatch");
}

// ── Phase 12 compatibility ────────────────────────────────────────────────────

#[test]
fn phase12_ctrl_c_still_quits() {
    let mut app = tmp_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert_eq!(app.state, AppState::Quitting);
}

#[test]
fn phase12_q_quits_on_empty_input() {
    let mut app = tmp_app();
    app.handle_key(key(KeyCode::Char('q')));
    assert_eq!(app.state, AppState::Quitting);
}

#[test]
fn phase12_history_navigation_still_works() {
    let dir = tempdir().unwrap();
    let mut history = logicshell_tui::HistoryStore::new(dir.path().join("history"));
    history.push("ls".to_string());
    history.push("pwd".to_string());
    let mut app = App::with_history("/", "loose", history);
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.input_widget.value(), "pwd");
}

#[test]
fn phase12_messages_still_populated_on_submit() {
    let mut app = loose_app();
    for c in "echo hi".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.messages.len(), 1);
    assert!(app.messages[0].contains("echo hi"));
}
