/// Phase 11 demo — run the TUI shell interactively.
///
/// Usage: cargo run -p logicshell-tui --example phase11
///
/// Controls:
///   Type characters    — append to the input buffer
///   Backspace          — delete last character
///   Enter              — submit the current line
///   q (empty input)    — quit
///   Ctrl-C             — quit immediately
use logicshell_tui::{terminal, App, Event, EventHandler};
use std::time::Duration;

#[tokio::main]
async fn main() -> logicshell_tui::Result<()> {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "?".to_string());

    let mut app = App::new(cwd, "balanced");
    let mut term = terminal::init()?;
    let mut events = EventHandler::new(Duration::from_millis(250));

    while app.is_running() {
        term.draw(|f| logicshell_tui::ui::draw(f, &app))?;

        if let Some(event) = events.next().await {
            if let Event::Key(key) = event {
                app.handle_key(key);
            }
        }
    }

    terminal::restore(&mut term)?;
    Ok(())
}
