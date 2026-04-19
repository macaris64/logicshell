use crate::event::DispatchEvent;
use crate::history::HistoryStore;
use crate::input::InputWidget;
use crate::output::OutputPanel;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use logicshell_core::{
    config::{Config, SafetyConfig, SafetyMode},
    Decision, SafetyPolicyEngine,
};
use std::path::PathBuf;

/// Lifecycle state of the TUI application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    Running,
    Quitting,
}

/// Input / dialog mode for the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    /// Normal input mode: typing a command.
    Normal,
    /// Awaiting user confirmation before dispatching a medium-risk command.
    Confirming {
        /// The raw command string that requires confirmation.
        command: String,
    },
}

/// Status of the most-recent (or in-flight) dispatch operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchStatus {
    Idle,
    Running,
    Done {
        exit_code: i32,
        duration_ms: u64,
    },
    /// Command was blocked by the safety policy; reason is shown in the UI.
    Denied {
        reason: String,
    },
}

/// Top-level TUI application state.
///
/// All business logic lives here; the struct is fully testable without a real
/// terminal by passing synthetic [`KeyEvent`] values to [`handle_key`].
///
/// [`handle_key`]: App::handle_key
pub struct App {
    pub state: AppState,
    /// Readline-like input line with cursor tracking.
    pub input_widget: InputWidget,
    /// Working directory shown in the prompt.
    pub cwd: String,
    /// Safety mode label displayed in the status bar.
    pub safety_mode: String,
    /// Legacy: submitted command headers (kept for Phase 12 compatibility).
    pub messages: Vec<String>,
    /// Phase 13: scrollable output panel with ring-buffer.
    pub output_panel: OutputPanel,
    /// Current dialog mode (normal input vs. confirm overlay).
    pub mode: AppMode,
    /// Status of the most recent dispatch.
    pub dispatch_status: DispatchStatus,
    /// Command ready for the event loop to spawn as an async task.
    pub(crate) pending_command: Option<Vec<String>>,
    /// Session command history with persistence.
    pub history: HistoryStore,
    /// Safety configuration for evaluating commands.
    safety_eval_mode: SafetyMode,
    safety_eval_config: SafetyConfig,
}

impl App {
    pub fn new(cwd: impl Into<String>, safety_mode: impl Into<String>) -> Self {
        let history_path = dirs_history_path();
        let history = HistoryStore::load(history_path)
            .unwrap_or_else(|_| HistoryStore::new(default_history_path()));
        let sm_str = safety_mode.into();
        let (eval_mode, eval_cfg) = parse_safety(&sm_str);
        Self {
            state: AppState::Running,
            input_widget: InputWidget::new(),
            cwd: cwd.into(),
            safety_mode: sm_str,
            messages: Vec::new(),
            output_panel: OutputPanel::with_default_cap(),
            mode: AppMode::Normal,
            dispatch_status: DispatchStatus::Idle,
            pending_command: None,
            history,
            safety_eval_mode: eval_mode,
            safety_eval_config: eval_cfg,
        }
    }

    /// Create an `App` with an explicit history store (used in tests).
    pub fn with_history(
        cwd: impl Into<String>,
        safety_mode: impl Into<String>,
        history: HistoryStore,
    ) -> Self {
        let sm_str = safety_mode.into();
        let (eval_mode, eval_cfg) = parse_safety(&sm_str);
        Self {
            state: AppState::Running,
            input_widget: InputWidget::new(),
            cwd: cwd.into(),
            safety_mode: sm_str,
            messages: Vec::new(),
            output_panel: OutputPanel::with_default_cap(),
            mode: AppMode::Normal,
            dispatch_status: DispatchStatus::Idle,
            pending_command: None,
            history,
            safety_eval_mode: eval_mode,
            safety_eval_config: eval_cfg,
        }
    }

    /// Create an `App` with a fully custom [`Config`] (used in tests / examples).
    pub fn with_config(cwd: impl Into<String>, history: HistoryStore, config: &Config) -> Self {
        let sm_str = format!("{:?}", config.safety_mode).to_lowercase();
        Self {
            state: AppState::Running,
            input_widget: InputWidget::new(),
            cwd: cwd.into(),
            safety_mode: sm_str,
            messages: Vec::new(),
            output_panel: OutputPanel::with_default_cap(),
            mode: AppMode::Normal,
            dispatch_status: DispatchStatus::Idle,
            pending_command: None,
            history,
            safety_eval_mode: config.safety_mode.clone(),
            safety_eval_config: config.safety.clone(),
        }
    }

    /// Returns `true` while the event loop should keep running.
    pub fn is_running(&self) -> bool {
        self.state == AppState::Running
    }

    /// Take the pending argv (if any), leaving `None` in its place.
    ///
    /// The event loop calls this each frame and spawns a dispatch task when
    /// `Some` is returned.
    pub fn take_pending_command(&mut self) -> Option<Vec<String>> {
        self.pending_command.take()
    }

    /// Returns `true` when a command is waiting to be dispatched.
    pub fn has_pending_command(&self) -> bool {
        self.pending_command.is_some()
    }

    /// Push a line of output into the [`OutputPanel`].
    pub fn push_output_line(&mut self, line: impl Into<String>) {
        self.output_panel.push_line(line);
    }

    /// Update state when a dispatch task completes or is cancelled.
    pub fn handle_dispatch_done(&mut self, exit_code: i32, duration_ms: u64) {
        self.dispatch_status = DispatchStatus::Done {
            exit_code,
            duration_ms,
        };
    }

    /// Apply a [`DispatchEvent`] received from a running task.
    pub fn apply_dispatch_event(&mut self, event: DispatchEvent) {
        match event {
            DispatchEvent::OutputLine(line) => {
                self.output_panel.push_line(line);
            }
            DispatchEvent::Done {
                exit_code,
                duration_ms,
            } => {
                self.handle_dispatch_done(exit_code, duration_ms);
            }
            DispatchEvent::Error(msg) => {
                self.output_panel.push_line(format!("error: {msg}"));
                self.handle_dispatch_done(-1, 0);
            }
        }
    }

    /// Transition the dispatch status back to `Idle` (used on task cancellation).
    pub fn cancel_dispatch(&mut self) {
        self.dispatch_status = DispatchStatus::Idle;
        self.output_panel.push_line("[cancelled]");
    }

    /// Process a single key event, updating state in place.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl-C always quits (or cancels in-flight dispatch signalled externally).
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.state = AppState::Quitting;
            return;
        }

        match &self.mode {
            AppMode::Confirming { .. } => self.handle_key_confirming(key),
            AppMode::Normal => self.handle_key_normal(key),
        }
    }

    // ── Normal mode ───────────────────────────────────────────────────────────

    fn handle_key_normal(&mut self, key: KeyEvent) {
        match key.code {
            // quit
            KeyCode::Char('q') if self.input_widget.is_empty() => {
                self.state = AppState::Quitting;
            }

            // submit
            KeyCode::Enter => {
                let raw = self.input_widget.value();
                let line = raw.trim().to_string();
                if !line.is_empty() {
                    self.history.push(line.clone());
                    self.history.reset_navigation();
                    // Phase 12 compat: record in messages
                    self.messages.push(format!("{} > {}", self.cwd, line));
                    let _ = self.history.save();
                    self.submit_command(line);
                }
                self.input_widget.clear();
            }

            // readline shortcuts
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_widget.move_to_start();
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_widget.move_to_end();
            }
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_widget.kill_to_end();
            }

            // cursor movement
            KeyCode::Left => self.input_widget.move_left(),
            KeyCode::Right => self.input_widget.move_right(),
            KeyCode::Home => self.input_widget.move_to_start(),
            KeyCode::End => self.input_widget.move_to_end(),

            // deletion
            KeyCode::Backspace => self.input_widget.delete_before_cursor(),
            KeyCode::Delete => self.input_widget.delete_after_cursor(),

            // history navigation
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

            // output panel scrolling
            KeyCode::PageUp => self.output_panel.scroll_up(),
            KeyCode::PageDown => self.output_panel.scroll_down(),

            // printable characters
            KeyCode::Char(c) => self.input_widget.insert(c),

            _ => {}
        }
    }

    /// Evaluate safety and transition to the correct state for `command`.
    fn submit_command(&mut self, command: String) {
        let argv: Vec<String> = command.split_whitespace().map(|s| s.to_string()).collect();
        if argv.is_empty() {
            return;
        }
        let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        let engine =
            SafetyPolicyEngine::new(self.safety_eval_mode.clone(), &self.safety_eval_config);
        let (assessment, decision) = engine.evaluate(&argv_refs);

        match decision {
            Decision::Deny => {
                let reason = assessment.reasons.join("; ");
                let msg = format!("[DENIED] {reason}");
                self.output_panel.push_line(msg);
                self.dispatch_status = DispatchStatus::Denied { reason };
            }
            Decision::Confirm => {
                self.mode = AppMode::Confirming { command };
            }
            Decision::Allow => {
                self.output_panel.push_line(format!("$ {command}"));
                self.dispatch_status = DispatchStatus::Running;
                self.pending_command = Some(argv);
            }
        }
    }

    // ── Confirming mode ───────────────────────────────────────────────────────

    fn handle_key_confirming(&mut self, key: KeyEvent) {
        let command = match &self.mode {
            AppMode::Confirming { command } => command.clone(),
            _ => return,
        };

        match key.code {
            // confirm
            KeyCode::Char('y') | KeyCode::Enter => {
                let argv: Vec<String> = command.split_whitespace().map(|s| s.to_string()).collect();
                self.output_panel.push_line(format!("$ {command}"));
                self.dispatch_status = DispatchStatus::Running;
                self.pending_command = Some(argv);
                self.mode = AppMode::Normal;
            }

            // cancel
            KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {
                self.output_panel.push_line("[cancelled]");
                self.dispatch_status = DispatchStatus::Idle;
                self.mode = AppMode::Normal;
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

// ── helpers ───────────────────────────────────────────────────────────────────

fn parse_safety(mode_str: &str) -> (SafetyMode, SafetyConfig) {
    let mode = match mode_str.to_lowercase().as_str() {
        "strict" => SafetyMode::Strict,
        "loose" => SafetyMode::Loose,
        _ => SafetyMode::Balanced,
    };
    (mode, SafetyConfig::default())
}

fn dirs_history_path() -> PathBuf {
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

    fn loose_app() -> App {
        App::with_history("/", "loose", tmp_history())
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
    fn ctrl_c_quits_from_confirming_mode() {
        let mut a = app();
        a.mode = AppMode::Confirming {
            command: "sudo rm something".to_string(),
        };
        a.handle_key(ctrl('c'));
        assert_eq!(a.state, AppState::Quitting);
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
        let mut a = loose_app();
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
        let mut a = loose_app();
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
        a.input_widget.cursor = 5;
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
        a.handle_key(key(KeyCode::Up));
        a.handle_key(key(KeyCode::Up));
        assert_eq!(a.input_widget.value(), "ls");
    }

    #[test]
    fn down_arrow_navigates_back_to_newer_entry() {
        let mut a = app();
        a.history.push("ls".to_string());
        a.history.push("pwd".to_string());
        a.handle_key(key(KeyCode::Up));
        a.handle_key(key(KeyCode::Up));
        a.handle_key(key(KeyCode::Down));
        assert_eq!(a.input_widget.value(), "pwd");
    }

    #[test]
    fn down_arrow_past_newest_restores_original_input() {
        let mut a = app();
        a.history.push("ls".to_string());
        a.input_widget.set_value("partial");
        a.handle_key(key(KeyCode::Up));
        a.handle_key(key(KeyCode::Down));
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
        let mut a = loose_app();
        for c in "ls -la".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(a.history.len(), 1);
        assert_eq!(a.history.entries()[0], "ls -la");
    }

    #[test]
    fn enter_resets_history_navigation() {
        let mut a = loose_app();
        a.history.push("ls".to_string());
        a.handle_key(key(KeyCode::Up));
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
        let mut a = loose_app();
        for c in "lss".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Left));
        a.handle_key(key(KeyCode::Backspace));
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(a.messages.len(), 1);
        assert!(a.messages[0].contains("ls"));
        assert!(!a.messages[0].contains("lss"));
    }

    // ── Phase 13: safety dispatch integration ────────────────────────────────

    #[test]
    fn enter_allowed_command_sets_pending_dispatch() {
        let mut a = loose_app();
        for c in "echo hello".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert!(
            a.pending_command.is_some(),
            "allowed command should set pending_command"
        );
        assert_eq!(
            a.pending_command.as_ref().unwrap(),
            &vec!["echo".to_string(), "hello".to_string()]
        );
    }

    #[test]
    fn enter_denied_command_sets_denied_status() {
        let mut a = app();
        for c in "rm -rf /".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(a.dispatch_status, DispatchStatus::Denied { .. }),
            "denied command should set Denied status; got {:?}",
            a.dispatch_status
        );
        assert!(a.pending_command.is_none());
    }

    #[test]
    fn enter_denied_command_adds_deny_line_to_output_panel() {
        let mut a = app();
        for c in "rm -rf /".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert!(
            !a.output_panel.is_empty(),
            "deny should add to output panel"
        );
    }

    #[test]
    fn enter_confirm_command_shows_confirm_dialog() {
        let mut a = app();
        // "sudo ls" is medium-risk → Confirm in balanced mode
        for c in "sudo ls".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(a.mode, AppMode::Confirming { .. }),
            "confirm command should switch to Confirming mode; got {:?}",
            a.mode
        );
        assert!(a.pending_command.is_none());
    }

    #[test]
    fn confirm_y_sets_pending_command() {
        let mut a = app();
        a.mode = AppMode::Confirming {
            command: "sudo ls".to_string(),
        };
        a.handle_key(key(KeyCode::Char('y')));
        assert!(
            a.pending_command.is_some(),
            "confirming with 'y' should set pending_command"
        );
        assert_eq!(a.mode, AppMode::Normal);
    }

    #[test]
    fn confirm_enter_sets_pending_command() {
        let mut a = app();
        a.mode = AppMode::Confirming {
            command: "sudo ls".to_string(),
        };
        a.handle_key(key(KeyCode::Enter));
        assert!(a.pending_command.is_some());
        assert_eq!(a.mode, AppMode::Normal);
    }

    #[test]
    fn confirm_n_cancels_and_returns_to_normal() {
        let mut a = app();
        a.mode = AppMode::Confirming {
            command: "sudo ls".to_string(),
        };
        a.handle_key(key(KeyCode::Char('n')));
        assert_eq!(a.mode, AppMode::Normal);
        assert!(a.pending_command.is_none());
    }

    #[test]
    fn confirm_esc_cancels() {
        let mut a = app();
        a.mode = AppMode::Confirming {
            command: "sudo ls".to_string(),
        };
        a.handle_key(key(KeyCode::Esc));
        assert_eq!(a.mode, AppMode::Normal);
        assert!(a.pending_command.is_none());
    }

    #[test]
    fn confirm_q_cancels() {
        let mut a = app();
        a.mode = AppMode::Confirming {
            command: "sudo ls".to_string(),
        };
        a.handle_key(key(KeyCode::Char('q')));
        assert_eq!(a.mode, AppMode::Normal);
        assert!(a.pending_command.is_none());
    }

    #[test]
    fn confirm_y_sets_running_status() {
        let mut a = app();
        a.mode = AppMode::Confirming {
            command: "sudo ls".to_string(),
        };
        a.handle_key(key(KeyCode::Char('y')));
        assert_eq!(a.dispatch_status, DispatchStatus::Running);
    }

    #[test]
    fn take_pending_command_clears_it() {
        let mut a = loose_app();
        for c in "echo hi".chars() {
            a.handle_key(key(KeyCode::Char(c)));
        }
        a.handle_key(key(KeyCode::Enter));
        let cmd = a.take_pending_command();
        assert!(cmd.is_some());
        assert!(a.pending_command.is_none());
    }

    #[test]
    fn take_pending_command_none_when_idle() {
        let mut a = app();
        assert!(a.take_pending_command().is_none());
    }

    // ── Phase 13: output panel integration ──────────────────────────────────

    #[test]
    fn push_output_line_adds_to_panel() {
        let mut a = app();
        a.push_output_line("hello from output");
        assert_eq!(a.output_panel.len(), 1);
    }

    #[test]
    fn handle_dispatch_done_updates_status() {
        let mut a = app();
        a.handle_dispatch_done(0, 123);
        assert_eq!(
            a.dispatch_status,
            DispatchStatus::Done {
                exit_code: 0,
                duration_ms: 123
            }
        );
    }

    #[test]
    fn handle_dispatch_done_nonzero_exit() {
        let mut a = app();
        a.handle_dispatch_done(1, 50);
        assert!(matches!(
            a.dispatch_status,
            DispatchStatus::Done { exit_code: 1, .. }
        ));
    }

    #[test]
    fn cancel_dispatch_sets_idle() {
        let mut a = app();
        a.dispatch_status = DispatchStatus::Running;
        a.cancel_dispatch();
        assert_eq!(a.dispatch_status, DispatchStatus::Idle);
    }

    #[test]
    fn cancel_dispatch_adds_cancelled_line() {
        let mut a = app();
        a.dispatch_status = DispatchStatus::Running;
        a.cancel_dispatch();
        assert!(!a.output_panel.is_empty());
    }

    #[test]
    fn apply_dispatch_event_output_line() {
        let mut a = app();
        a.apply_dispatch_event(DispatchEvent::OutputLine("stdout line".to_string()));
        assert_eq!(a.output_panel.len(), 1);
    }

    #[test]
    fn apply_dispatch_event_done() {
        let mut a = app();
        a.apply_dispatch_event(DispatchEvent::Done {
            exit_code: 0,
            duration_ms: 200,
        });
        assert_eq!(
            a.dispatch_status,
            DispatchStatus::Done {
                exit_code: 0,
                duration_ms: 200
            }
        );
    }

    #[test]
    fn apply_dispatch_event_error() {
        let mut a = app();
        a.apply_dispatch_event(DispatchEvent::Error("command not found".to_string()));
        assert_eq!(
            a.dispatch_status,
            DispatchStatus::Done {
                exit_code: -1,
                duration_ms: 0
            }
        );
        assert!(!a.output_panel.is_empty());
    }

    // ── Phase 13: output panel scroll in App ─────────────────────────────────

    #[test]
    fn page_up_scrolls_output_panel() {
        let mut a = app();
        for i in 0..10 {
            a.output_panel.push_line(i.to_string());
        }
        a.handle_key(key(KeyCode::PageUp));
        assert!(a.output_panel.scroll_offset() > 0);
    }

    #[test]
    fn page_down_scrolls_down_output_panel() {
        let mut a = app();
        for i in 0..10 {
            a.output_panel.push_line(i.to_string());
        }
        a.output_panel.scroll_up();
        a.output_panel.scroll_up();
        let before = a.output_panel.scroll_offset();
        a.handle_key(key(KeyCode::PageDown));
        assert!(a.output_panel.scroll_offset() < before);
    }
}
