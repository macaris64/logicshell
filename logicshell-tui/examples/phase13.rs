/// Phase 13 interactive demo — TUI dispatch + output panel
///
/// Run with:  cargo run --example phase13 -p logicshell-tui
///
/// Key bindings:
///   (All Phase 12 bindings still work)
///   Enter           — submit command (safety evaluated first)
///   y / Enter       — confirm a medium-risk command in the confirm dialog
///   n / Esc / q     — cancel a pending confirmation
///   PageUp          — scroll output panel up
///   PageDown        — scroll output panel down
///   Ctrl-C / q      — quit (q only when input is empty)
use logicshell_core::LogicShell;
use logicshell_tui::{terminal, App, DispatchEvent, Event, EventHandler};
use ratatui::crossterm::event::KeyEventKind;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[tokio::main]
async fn main() -> logicshell_tui::Result<()> {
    let mut term = terminal::init()?;
    let mut app = App::default();
    let mut events = EventHandler::new(Duration::from_millis(50));

    // Channel for streaming dispatch output back to the TUI event loop.
    let mut dispatch_rx: Option<mpsc::UnboundedReceiver<DispatchEvent>> = None;
    let mut dispatch_handle: Option<JoinHandle<()>> = None;

    while app.is_running() {
        // Drain any pending dispatch events first (non-blocking)
        if let Some(ref mut rx) = dispatch_rx {
            while let Ok(event) = rx.try_recv() {
                app.apply_dispatch_event(event);
            }
        }

        // If a new dispatch is pending, spawn it
        if let Some(argv) = app.take_pending_command() {
            // Abort any currently running task
            if let Some(old) = dispatch_handle.take() {
                old.abort();
            }

            let (event_tx, event_rx) = mpsc::unbounded_channel::<DispatchEvent>();
            dispatch_rx = Some(event_rx);

            let shell = LogicShell::new();
            dispatch_handle = Some(tokio::spawn(async move {
                let (line_tx, mut line_rx) = mpsc::unbounded_channel::<String>();

                // Forward lines from the streaming channel to DispatchEvent
                let tx_clone = event_tx.clone();
                let forward = tokio::spawn(async move {
                    while let Some(line) = line_rx.recv().await {
                        let _ = tx_clone.send(DispatchEvent::OutputLine(line));
                    }
                });

                let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
                match shell.dispatch_streaming(&argv_refs, line_tx).await {
                    Ok((exit_code, duration)) => {
                        let _ = forward.await;
                        let _ = event_tx.send(DispatchEvent::Done {
                            exit_code,
                            duration_ms: duration.as_millis() as u64,
                        });
                    }
                    Err(e) => {
                        let _ = forward.await;
                        let _ = event_tx.send(DispatchEvent::Error(e.to_string()));
                    }
                }
            }));
        }

        // Draw the current frame
        term.draw(|f| logicshell_tui::ui::draw(f, &app))?;

        // Wait for the next user event
        if let Some(Event::Key(key)) = events.next().await {
            if key.kind == KeyEventKind::Press {
                // Cancel in-flight dispatch on Ctrl-C before quitting
                if key.code == ratatui::crossterm::event::KeyCode::Char('c')
                    && key
                        .modifiers
                        .contains(ratatui::crossterm::event::KeyModifiers::CONTROL)
                {
                    if let Some(handle) = dispatch_handle.take() {
                        handle.abort();
                        app.cancel_dispatch();
                        continue;
                    }
                }
                app.handle_key(key);
            }
        }
    }

    terminal::restore(&mut term)?;
    Ok(())
}
