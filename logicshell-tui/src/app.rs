use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Lifecycle state of the TUI application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Running,
    Quitting,
}

/// Top-level TUI application state — owns the input buffer and rendered message history.
///
/// Designed to be fully testable without a real terminal: all transitions happen
/// through `handle_key` which takes plain `KeyEvent` values.
pub struct App {
    pub state: AppState,
    /// Current line the user is typing.
    pub input: String,
    /// Working directory shown in the prompt.
    pub cwd: String,
    /// Safety mode label displayed in the status bar.
    pub safety_mode: String,
    /// Submitted commands and output lines shown in the main panel.
    pub messages: Vec<String>,
}

impl App {
    pub fn new(cwd: impl Into<String>, safety_mode: impl Into<String>) -> Self {
        Self {
            state: AppState::Running,
            input: String::new(),
            cwd: cwd.into(),
            safety_mode: safety_mode.into(),
            messages: Vec::new(),
        }
    }

    /// Returns `true` while the event loop should keep running.
    pub fn is_running(&self) -> bool {
        self.state == AppState::Running
    }

    /// Process a single key event, updating state in place.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            // Ctrl-C always quits, regardless of modifier state.
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.state = AppState::Quitting;
            }
            // 'q' quits when input is empty (not mid-typing).
            KeyCode::Char('q') if self.input.is_empty() => {
                self.state = AppState::Quitting;
            }
            // Enter submits the current input line.
            KeyCode::Enter => {
                let line = self.input.trim().to_string();
                if !line.is_empty() {
                    self.messages.push(format!("{} > {}", self.cwd, line));
                    self.input.clear();
                }
            }
            // Backspace removes the last character.
            KeyCode::Backspace => {
                self.input.pop();
            }
            // Printable characters append to the input buffer.
            KeyCode::Char(c) => {
                self.input.push(c);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    // ── lifecycle ──────────────────────────────────────────────────────────────

    #[test]
    fn new_app_is_running() {
        let app = App::new("/home/user", "balanced");
        assert_eq!(app.state, AppState::Running);
        assert!(app.is_running());
    }

    #[test]
    fn ctrl_c_quits() {
        let mut app = App::new("/", "balanced");
        app.handle_key(ctrl('c'));
        assert_eq!(app.state, AppState::Quitting);
        assert!(!app.is_running());
    }

    #[test]
    fn q_quits_when_input_empty() {
        let mut app = App::new("/", "balanced");
        app.handle_key(key(KeyCode::Char('q')));
        assert_eq!(app.state, AppState::Quitting);
    }

    #[test]
    fn q_does_not_quit_when_input_non_empty() {
        let mut app = App::new("/", "balanced");
        app.handle_key(key(KeyCode::Char('l')));
        app.handle_key(key(KeyCode::Char('s')));
        // 'q' should append, not quit, because input is non-empty
        app.handle_key(key(KeyCode::Char('q')));
        assert_eq!(app.state, AppState::Running);
        assert_eq!(app.input, "lsq");
    }

    // ── input buffer ───────────────────────────────────────────────────────────

    #[test]
    fn char_keys_append_to_input() {
        let mut app = App::new("/", "balanced");
        for c in "ls -la".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(app.input, "ls -la");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut app = App::new("/", "balanced");
        app.handle_key(key(KeyCode::Char('l')));
        app.handle_key(key(KeyCode::Char('s')));
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.input, "l");
    }

    #[test]
    fn backspace_on_empty_input_is_noop() {
        let mut app = App::new("/", "balanced");
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.input, "");
        assert_eq!(app.state, AppState::Running);
    }

    // ── enter / submit ─────────────────────────────────────────────────────────

    #[test]
    fn enter_submits_input_and_clears_buffer() {
        let mut app = App::new("/home", "balanced");
        app.handle_key(key(KeyCode::Char('l')));
        app.handle_key(key(KeyCode::Char('s')));
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.input, "");
        assert_eq!(app.messages.len(), 1);
        assert!(app.messages[0].contains("ls"));
    }

    #[test]
    fn enter_on_empty_input_adds_no_message() {
        let mut app = App::new("/", "balanced");
        app.handle_key(key(KeyCode::Enter));
        assert!(app.messages.is_empty());
    }

    #[test]
    fn enter_whitespace_only_adds_no_message() {
        let mut app = App::new("/", "balanced");
        app.handle_key(key(KeyCode::Char(' ')));
        app.handle_key(key(KeyCode::Enter));
        assert!(app.messages.is_empty());
    }

    #[test]
    fn multiple_submits_accumulate_messages() {
        let mut app = App::new("/", "balanced");
        for cmd in &["ls", "pwd", "echo hello"] {
            for c in cmd.chars() {
                app.handle_key(key(KeyCode::Char(c)));
            }
            app.handle_key(key(KeyCode::Enter));
        }
        assert_eq!(app.messages.len(), 3);
    }

    // ── miscellaneous ──────────────────────────────────────────────────────────

    #[test]
    fn app_stores_cwd_and_safety_mode() {
        let app = App::new("/var/log", "strict");
        assert_eq!(app.cwd, "/var/log");
        assert_eq!(app.safety_mode, "strict");
    }

    #[test]
    fn default_app_is_running_with_balanced_mode() {
        let app = App::default();
        assert!(app.is_running());
        assert_eq!(app.safety_mode, "balanced");
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let mut app = App::new("/", "balanced");
        app.handle_key(key(KeyCode::F(1)));
        app.handle_key(key(KeyCode::Null));
        assert_eq!(app.state, AppState::Running);
        assert_eq!(app.input, "");
    }
}
