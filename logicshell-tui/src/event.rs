use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};
use std::time::Duration;
use tokio::sync::mpsc;

/// Internal event enum for the TUI event loop.
#[derive(Debug, Clone)]
pub enum Event {
    /// A keyboard event forwarded from crossterm.
    Key(KeyEvent),
    /// Periodic tick for animations or polling.
    Tick,
    /// Terminal resize notification.
    Resize(u16, u16),
}

/// Events produced by a running dispatch task and consumed by the event loop.
#[derive(Debug, Clone)]
pub enum DispatchEvent {
    /// A single stdout line streamed from the child process.
    OutputLine(String),
    /// The child process exited with the given code and wall-clock duration.
    Done { exit_code: i32, duration_ms: u64 },
    /// The dispatch failed before or during execution.
    Error(String),
}

/// Spawns a background task that forwards crossterm events and periodic ticks
/// to the returned receiver channel.
///
/// Callers drain `rx` in their event loop and pass `Event::Key` to `App::handle_key`.
/// The returned `EventHandler` must be kept alive for as long as the receiver is in use.
pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<Event>,
    /// Keep the sender alive so the background task is not dropped immediately.
    _tx: mpsc::UnboundedSender<Event>,
}

impl EventHandler {
    /// Start the event forwarding task with the given tick rate.
    ///
    /// `tick_rate` controls how often `Event::Tick` is emitted when no real
    /// terminal events arrive.
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            loop {
                if event::poll(tick_rate).unwrap_or(false) {
                    match event::read() {
                        Ok(CrosstermEvent::Key(key)) => {
                            let _ = tx_clone.send(Event::Key(key));
                        }
                        Ok(CrosstermEvent::Resize(w, h)) => {
                            let _ = tx_clone.send(Event::Resize(w, h));
                        }
                        _ => {}
                    }
                } else {
                    let _ = tx_clone.send(Event::Tick);
                }
            }
        });

        Self { rx, _tx: tx }
    }

    /// Receive the next event, waiting until one is available.
    pub async fn next(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn make_key_event(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn event_key_variant_stores_key_event() {
        let ke = make_key_event(KeyCode::Enter);
        let ev = Event::Key(ke);
        assert!(matches!(ev, Event::Key(_)));
    }

    #[test]
    fn event_tick_variant() {
        let ev = Event::Tick;
        assert!(matches!(ev, Event::Tick));
    }

    #[test]
    fn event_resize_variant_stores_dimensions() {
        let ev = Event::Resize(80, 24);
        match ev {
            Event::Resize(w, h) => {
                assert_eq!(w, 80);
                assert_eq!(h, 24);
            }
            _ => panic!("expected Resize"),
        }
    }

    #[test]
    fn event_clone_produces_equal_tick() {
        let ev = Event::Tick;
        let cloned = ev.clone();
        assert!(matches!(cloned, Event::Tick));
    }

    #[test]
    fn event_key_clone_preserves_key_code() {
        let ke = make_key_event(KeyCode::Char('x'));
        let ev = Event::Key(ke);
        let cloned = ev.clone();
        match cloned {
            Event::Key(k) => assert_eq!(k.code, KeyCode::Char('x')),
            _ => panic!("expected Key"),
        }
    }

    // ── DispatchEvent ─────────────────────────────────────────────────────────

    #[test]
    fn dispatch_event_output_line_stores_string() {
        let ev = DispatchEvent::OutputLine("hello".to_string());
        match ev {
            DispatchEvent::OutputLine(s) => assert_eq!(s, "hello"),
            _ => panic!("expected OutputLine"),
        }
    }

    #[test]
    fn dispatch_event_done_stores_exit_code_and_duration() {
        let ev = DispatchEvent::Done {
            exit_code: 0,
            duration_ms: 42,
        };
        match ev {
            DispatchEvent::Done {
                exit_code,
                duration_ms,
            } => {
                assert_eq!(exit_code, 0);
                assert_eq!(duration_ms, 42);
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn dispatch_event_error_stores_message() {
        let ev = DispatchEvent::Error("oops".to_string());
        match ev {
            DispatchEvent::Error(msg) => assert_eq!(msg, "oops"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn dispatch_event_output_line_clone() {
        let ev = DispatchEvent::OutputLine("x".to_string());
        let cloned = ev.clone();
        assert!(matches!(cloned, DispatchEvent::OutputLine(_)));
    }

    #[test]
    fn dispatch_event_done_clone() {
        let ev = DispatchEvent::Done {
            exit_code: 1,
            duration_ms: 100,
        };
        let cloned = ev.clone();
        assert!(matches!(cloned, DispatchEvent::Done { .. }));
    }
}
