// logicshell-tui: Ratatui-powered interactive shell TUI — Phase 12

pub mod app;
pub mod error;
pub mod event;
pub mod history;
pub mod input;
pub mod terminal;
pub mod ui;

pub use app::{App, AppState};
pub use error::{Result, TuiError};
pub use event::{Event, EventHandler};
pub use history::HistoryStore;
pub use input::InputWidget;
