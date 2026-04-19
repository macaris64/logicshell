// logicshell-tui: Ratatui-powered interactive shell TUI — Phase 13

pub mod app;
pub mod error;
pub mod event;
pub mod history;
pub mod input;
pub mod output;
pub mod terminal;
pub mod ui;

pub use app::{App, AppMode, AppState, DispatchStatus};
pub use error::{Result, TuiError};
pub use event::{DispatchEvent, Event, EventHandler};
pub use history::HistoryStore;
pub use input::InputWidget;
pub use output::OutputPanel;
