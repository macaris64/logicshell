// logicshell-tui: Ratatui-powered interactive shell TUI — Phase 11 foundation

pub mod app;
pub mod error;
pub mod event;
pub mod terminal;
pub mod ui;

pub use app::{App, AppState};
pub use error::{Result, TuiError};
pub use event::{Event, EventHandler};
