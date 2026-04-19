/// Phase 12 interactive demo — readline-like input + session history
///
/// Run with:  cargo run --example phase12 -p logicshell-tui
///
/// Key bindings:
///   Left / Right    — move cursor
///   Home / End      — beginning / end of line
///   Ctrl-A / Ctrl-E — beginning / end of line (readline style)
///   Ctrl-K          — kill from cursor to end of line
///   Up / Down       — history navigation
///   Backspace       — delete character before cursor
///   Delete          — delete character at cursor
///   Enter           — submit command
///   Ctrl-C / q      — quit (q only when input is empty)
use logicshell_tui::{terminal, App, Event, EventHandler};
use ratatui::crossterm::event::KeyEventKind;
use std::time::Duration;

#[tokio::main]
async fn main() -> logicshell_tui::Result<()> {
    let mut term = terminal::init()?;
    let mut app = App::default();
    let mut events = EventHandler::new(Duration::from_millis(100));

    while app.is_running() {
        term.draw(|f| logicshell_tui::ui::draw(f, &app))?;
        if let Some(Event::Key(key)) = events.next().await {
            if key.kind == KeyEventKind::Press {
                app.handle_key(key);
            }
        }
    }

    terminal::restore(&mut term)?;
    Ok(())
}
