use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{stdout, Stdout};

use crate::error::TuiError;

pub type CrosstermTerminal = Terminal<CrosstermBackend<Stdout>>;

/// Enter alternate screen and raw mode, returning a configured terminal.
///
/// Must be paired with [`restore`] on the same terminal to leave the alternate
/// screen and restore line-buffered mode.
pub fn init() -> Result<CrosstermTerminal, TuiError> {
    enable_raw_mode().map_err(TuiError::Io)?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen).map_err(TuiError::Io)?;
    let backend = CrosstermBackend::new(out);
    Terminal::new(backend).map_err(TuiError::Io)
}

/// Leave alternate screen, disable raw mode, and show the cursor.
pub fn restore(terminal: &mut CrosstermTerminal) -> Result<(), TuiError> {
    disable_raw_mode().map_err(TuiError::Io)?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(TuiError::Io)?;
    terminal.show_cursor().map_err(TuiError::Io)
}
