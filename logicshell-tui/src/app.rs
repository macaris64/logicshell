use crate::history::HistoryStore;
use crate::input::InputWidget;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

/// Lifecycle state of the TUI application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Running,
    Quitting,
}

/// Top-level TUI application state.
///
/// All business logic lives here; the struct is fully testable without a real
/// terminal by passing synthetic `KeyEvent` values to `handle_key`.
pub struct App {
    pub state: AppState,
    /// Readline-like input line with cursor tracking.
    pub input_widget: InputWidget,
    /// Working directory shown in the prompt.
    pub cwd: String,
    /// Safety mode label displayed in the status bar.
    pub safety_mode: String,
    /// Submitted commands and output lines shown in the main panel.
    pub messages: Vec<String>,
    /// Session command history with persistence.
    pub history: HistoryStore,
}

impl App {
    pub fn new(cwd: impl Into<String>, safety_mode: impl Into<String>) -> Self {
        let history_path = dirs_history_path();
        let history = HistoryStore::load(history_path)
            .unwrap_or_else(|_| HistoryStore::new(default_history_path()));
        Self {
            state: AppState::Running,
            input_widget: InputWidget::new(),
            cwd: cwd.into(),
            safety_mode: safety_mode.into(),
            messages: Vec::new(),
            history,
        }
    }

    /// Create an `App` with an explicit history store (used in tests to inject
    /// a temp-dir-backed store without touching the real home directory).
    pub fn with_history(
        cwd: impl Into<String>,
        safety_mode: impl Into<String>,
        history: HistoryStore,
    ) -> Self {
        Self {
            state: AppState::Running,
            input_widget: InputWidget::new(),
            cwd: cwd.into(),
            safety_mode: safety_mode.into(),
            messages: Vec::new(),
            history,
        }
    }

    /// Returns `true` while the event loop should keep running.
    pub fn is_running(&self) -> bool {
        self.state == AppState::Running
    }

    /// Process a single key event, updating state in place.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            // ── quit ─────────────────────────────────────────────────────────
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.state = AppState::Quitting;
            }
            KeyCode::Char('q') if self.input_widget.is_empty() => {
                self.state = AppState::Quitting;
            }

            // ── submit ────────────────────────────────────────────────────────
            KeyCode::Enter => {
                let line = self.input_widget.value();
                let line = line.trim().to_string();
                if !line.is_empty() {
                    self.history.push(line.clone());
                    self.history.reset_navigation();
                    self.messages.push(format!("{} > {}", self.cwd, line));
                    let _ = self.history.save();
                }
                self.input_widget.clear();
            }

            // ── readline shortcuts ────────────────────────────────────────────
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_widget.move_to_start();
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_widget.move_to_end();
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_widget.kill_to_end();
            }

            // ── cursor movement ───────────────────────────────────────────────
            KeyCode::Left => {
                self.input_widget.move_left();
            }
            KeyCode::Right => {
                self.input_widget.move_right();
            }
            KeyCode::Home => {
                self.input_widget.move_to_start();
            }
            KeyCode::End => {
                self.input_widget.move_to_end();
            }

            // ── deletion ──────────────────────────────────────────────────────
            KeyCode::Backspace => {
                self.input_widget.delete_before_cursor();
            }
            KeyCode::Delete => {
                self.input_widget.delete_after_cursor();
            }

            // ── history navigation ────────────────────────────────────────────
            KeyCode::Up => {
                let current = self.input_widget.value();
                if let Some(entry) = self.history.navigate_prev(&current) {
                    self.input_widget.set_value(&entry);
                }
            }
            KeyCode::Down => {
                if let Some(entry) = self.history.navigate_next() {
                    self.input_widget.set_value(&entry);
                }
            }

            // ── printable characters ──────────────────────────────────────────
            KeyCode::Char(c) => {
                self.input_widget.insert(c);
            }

            _ => {}
        }
    }
}

impl Default for App {
    fn default() -> Self {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "?".to_string());
        Self::new(cwd, "balanced")
    }
}

fn dirs_history_path() -> PathBuf {
    // XDG_DATA_HOME or ~/.local/share
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_home().join(".local").join("share"));
    base.join("logicshell").join("history")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn default_history_path() -> PathBuf {
    dirs_history_path()
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn tmp_history() -> HistoryStore {
        let dir = tempdir().unwrap();
        HistoryStore::new(dir.path().join("history"))
    }

    fn app() -> App {
        App::with_history("/", "balanced", tmp_history())
    }

    // ── lifecycle ──────────────────────────────────────────────────────────────

    #[test]
    fn new_app_is_running() {
        let a = app();
        assert_eq!(a.state, AppState::Running);
        assert!(a.is_running());
    }

    #[test]
    fn ctrl_c_quits() {
        let mut a = app();
        a.handle_key(ctrl('c'));
        assert_eq!(a.state, AppState::Quitting);
        assert!(!a.is_running());
    }

    #[test]
    fn q_quits_when_input_empty() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('q')));
        assert_eq!(a.state, AppState::Quitting);
    }

    #[test]
    fn q_does_not_quit_when_input_non_empty() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('l')));
        a.handle_key(key(KeyCode::Char('s')));
        a.handle_key(key(KeyCode::Char('q')));
        assert_eq!(a.state, AppState::Running);
        assert_eq!(a.input_widget.value(), "lsq");
    }

    // ── input buffer ──────────────────────────────────────────────────────────

    #[test]
    fn char_keys_append_to_input() {
        let mut a = app();
        for c in "ls -la".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(a.input_widget.value(), "ls -la");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('l')));
        a.handle_key(key(KeyCode::Char('s')));
        a.handle_key(key(KeyCode::Backspace));
        assert_eq!(a.input_widget.value(), "l");
    }

    #[test]
    fn backspace_on_empty_input_is_noop() {
        let mut a = app();
        a.handle_key(key(KeyCode::Backspace));
        assert_eq!(a.input_widget.value(), "");
        assert_eq!(a.state, AppState::Running);
    }

    #[test]
    fn delete_key_removes_char_at_cursor() {
        let mut a = app();
        a.input_widget.set_value("ls");
        a.input_widget.move_to_start();
        a.handle_key(key(KeyCode::Delete));
        assert_eq!(a.input_widget.value(), "s");
    }

    // ── enter / submit ────────────────────────────────────────────────────────

    #[test]
    fn enter_submits_input_and_clears_buffer() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('l')));
        a.handle_key(key(KeyCode::Char('s')));
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(a.input_widget.value(), "");
        assert_eq!(a.messages.len(), 1);
        assert!(a.messages[0].contains("ls"));
    }

    #[test]
    fn enter_on_empty_input_adds_no_message() {
        let mut a = app();
        a.handle_key(key(KeyCode::Enter));
        assert!(a.messages.is_empty());
    }

    #[test]
    fn enter_whitespace_only_adds_no_message() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char(' ')));
        a.handle_key(key(KeyCode::Enter));
        assert!(a.messages.is_empty());
    }

    #[test]
    fn multiple_submits_accumulate_messages() {
        let mut a = app();
        for cmd in &["ls", "pwd", "echo hello"] {
            for c in cmd.chars() {
                a.handle_key(key(KeyCode::Char(c)));
            }
            a.handle_key(key(KeyCode::Enter));
        }
        assert_eq!(a.messages.len(), 3);
    }

    // ── cursor movement ───────────────────────────────────────────────────────

    #[test]
    fn left_arrow_moves_cursor_left() {
        let mut a = app();
        a.input_widget.set_value("hello");
        a.handle_key(key(KeyCode::Left));
        assert_eq!(a.input_widget.cursor_pos(), 4);
    }

    #[test]
    fn right_arrow_moves_cursor_right() {
        let mut a = app();
        a.input_widget.set_value("hello");
        a.input_widget.move_to_start();
        a.handle_key(key(KeyCode::Right));
        assert_eq!(a.input_widget.cursor_pos(), 1);
    }

    #[test]
    fn home_key_moves_cursor_to_start() {
        let mut a = app();
        a.input_widget.set_value("hello");
        a.handle_key(key(KeyCode::Home));
        assert_eq!(a.input_widget.cursor_pos(), 0);
    }

    #[test]
    fn end_key_moves_cursor_to_end() {
        let mut a = app();
        a.input_widget.set_value("hello");
        a.input_widget.move_to_start();
        a.handle_key(key(KeyCode::End));
        assert_eq!(a.input_widget.cursor_pos(), 5);
    }

    // ── readline shortcuts ────────────────────────────────────────────────────

    #[test]
    fn ctrl_a_moves_cursor_to_start() {
        let mut a = app();
        a.input_widget.set_value("hello");
        a.handle_key(ctrl('a'));
        assert_eq!(a.input_widget.cursor_pos(), 0);
    }

    #[test]
    fn ctrl_e_moves_cursor_to_end() {
        let mut a = app();
        a.input_widget.set_value("hello");
        a.input_widget.move_to_start();
        a.handle_key(ctrl('e'));
        assert_eq!(a.input_widget.cursor_pos(), 5);
    }

    #[test]
    fn ctrl_k_kills_from_cursor_to_end() {
        let mut a = app();
        a.input_widget.set_value("hello world");
        a.input_widget.cursor = 5; // after "hello"
        a.handle_key(ctrl('k'));
        assert_eq!(a.input_widget.value(), "hello");
    }

    #[test]
    fn ctrl_k_on_empty_input_is_noop() {
        let mut a = app();
        a.handle_key(ctrl('k'));
        assert_eq!(a.input_widget.value(), "");
    }

    // ── history navigation ────────────────────────────────────────────────────

    #[test]
    fn up_arrow_recalls_most_recent_command() {
        let mut a = app();
        a.history.push("ls".to_string());
        a.history.push("pwd".to_string());
        a.handle_key(key(KeyCode::Up));
        assert_eq!(a.input_widget.value(), "pwd");
    }

    #[test]
    fn up_arrow_walks_older_entries() {
        let mut a = app();
        a.history.push("ls".to_string());
        a.history.push("pwd".to_string());
        a.handle_key(key(KeyCode::Up)); // pwd
        a.handle_key(key(KeyCode::Up)); // ls
        assert_eq!(a.input_widget.value(), "ls");
    }

    #[test]
    fn down_arrow_navigates_back_to_newer_entry() {
        let mut a = app();
        a.history.push("ls".to_string());
        a.history.push("pwd".to_string());
        a.handle_key(key(KeyCode::Up)); // pwd
        a.handle_key(key(KeyCode::Up)); // ls
        a.handle_key(key(KeyCode::Down)); // pwd
        assert_eq!(a.input_widget.value(), "pwd");
    }

    #[test]
    fn down_arrow_past_newest_restores_original_input() {
        let mut a = app();
        a.history.push("ls".to_string());
        // Type some partial input
        a.input_widget.set_value("partial");
        a.handle_key(key(KeyCode::Up)); // → "ls", saved "partial"
        a.handle_key(key(KeyCode::Down)); // → restored "partial"
        assert_eq!(a.input_widget.value(), "partial");
    }

    #[test]
    fn up_arrow_on_empty_history_is_noop() {
        let mut a = app();
        a.handle_key(key(KeyCode::Up));
        assert_eq!(a.input_widget.value(), "");
    }

    #[test]
    fn enter_adds_command_to_history() {
        let mut a = app();
        for c in "ls -la".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(a.history.len(), 1);
        assert_eq!(a.history.entries()[0], "ls -la");
    }

    #[test]
    fn enter_resets_history_navigation() {
        let mut a = app();
        a.history.push("ls".to_string());
        a.handle_key(key(KeyCode::Up)); // start navigating
                                        // Type new command and submit
        for c in "pwd".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert!(a.history.nav_index.is_none());
    }

    // ── miscellaneous ─────────────────────────────────────────────────────────

    #[test]
    fn app_stores_cwd_and_safety_mode() {
        let a = App::with_history("/var/log", "strict", tmp_history());
        assert_eq!(a.cwd, "/var/log");
        assert_eq!(a.safety_mode, "strict");
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let mut a = app();
        a.handle_key(key(KeyCode::F(1)));
        a.handle_key(key(KeyCode::Null));
        assert_eq!(a.state, AppState::Running);
        assert_eq!(a.input_widget.value(), "");
    }

    #[test]
    fn insert_mid_line_then_submit_produces_correct_message() {
        let mut a = app();
        // Type "lss", move left once, backspace to get "ls", press enter
        for c in "lss".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Left)); // cursor before last 's'
        a.handle_key(key(KeyCode::Backspace)); // remove middle 's'
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(a.messages.len(), 1);
        assert!(a.messages[0].contains("ls"));
        assert!(!a.messages[0].contains("lss"));
    }
}
