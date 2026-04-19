use crate::app::App;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PHASE: &str = "12";

/// Render the full TUI layout into the given [`Frame`].
///
/// Layout (top to bottom):
/// 1. Title banner  — single line
/// 2. Welcome / message area — fills remaining space
/// 3. Prompt input line — single line
/// 4. Status bar — single line
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Min(1),    // messages
            Constraint::Length(1), // prompt input
            Constraint::Length(1), // status bar
        ])
        .split(area);

    draw_title(frame, chunks[0]);
    draw_messages(frame, app, chunks[1]);
    draw_prompt(frame, app, chunks[2]);
    draw_status_bar(frame, app, chunks[3]);
}

fn draw_title(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new(format!(
        " LogicShell v{VERSION}  —  Phase {PHASE} TUI Foundation"
    ))
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .alignment(Alignment::Left);
    frame.render_widget(title, area);
}

fn draw_messages(frame: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = if app.messages.is_empty() {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Welcome to LogicShell!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  Type a command and press Enter to submit."),
            Line::from("  Press 'q' or Ctrl-C to quit."),
        ]
    } else {
        app.messages
            .iter()
            .map(|m| Line::from(m.as_str()))
            .collect()
    };

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn draw_prompt(frame: &mut Frame, app: &App, area: Rect) {
    let prompt_text = format!("  {} > {}", app.cwd, app.input_widget.render_with_cursor());
    let prompt = Paragraph::new(prompt_text).style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(prompt, area);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let status = Paragraph::new(format!(
        " Phase {PHASE} | v{VERSION} | Safety: {}",
        app.safety_mode
    ))
    .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(status, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use ratatui::{backend::TestBackend, Terminal};

    fn render_to_buffer(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn buf_row(buf: &ratatui::buffer::Buffer, y: u16, width: u16) -> String {
        (0..width)
            .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    fn buf_rows(buf: &ratatui::buffer::Buffer, y_start: u16, y_end: u16, width: u16) -> String {
        (y_start..y_end)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .map(|(x, y)| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    #[test]
    fn title_row_contains_logicshell() {
        let app = App::new("/home/user", "balanced");
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 0, 80);
        assert!(row.contains("LogicShell"), "title row: {row:?}");
    }

    #[test]
    fn status_bar_contains_phase_and_version() {
        let app = App::new("/home/user", "balanced");
        let buf = render_to_buffer(&app, 80, 10);
        // Status bar is the last row (index 9)
        let row = buf_row(&buf, 9, 80);
        assert!(row.contains("Phase"), "status bar row: {row:?}");
        assert!(row.contains("Safety"), "status bar row: {row:?}");
    }

    #[test]
    fn status_bar_shows_safety_mode() {
        let app = App::new("/", "strict");
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 9, 80);
        assert!(
            row.contains("strict"),
            "status bar should show safety mode: {row:?}"
        );
    }

    #[test]
    fn welcome_message_shown_when_no_messages() {
        let app = App::new("/", "balanced");
        let buf = render_to_buffer(&app, 80, 10);
        let interior = buf_rows(&buf, 1, 9, 80);
        assert!(interior.contains("Welcome"), "interior: {interior:?}");
    }

    #[test]
    fn prompt_shows_cwd_and_input() {
        let mut app = App::new("/home/aero", "balanced");
        app.input_widget.set_value("ls -la");
        let buf = render_to_buffer(&app, 80, 10);
        // Prompt is row index 8 (height=10: title=0, msgs=1..7, prompt=8, status=9)
        let row = buf_row(&buf, 8, 80);
        assert!(row.contains("/home/aero"), "prompt row: {row:?}");
        assert!(row.contains("ls -la"), "prompt row: {row:?}");
    }

    #[test]
    fn submitted_messages_replace_welcome_text() {
        let mut app = App::new("/", "balanced");
        app.messages.push("/  > ls".to_string());
        let buf = render_to_buffer(&app, 80, 10);
        let interior = buf_rows(&buf, 1, 9, 80);
        assert!(interior.contains("ls"), "messages area: {interior:?}");
    }

    #[test]
    fn title_contains_phase_number() {
        let app = App::new("/", "balanced");
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 0, 80);
        assert!(row.contains("12"), "title should contain phase 12: {row:?}");
    }

    #[test]
    fn render_does_not_panic_on_small_terminal() {
        let app = App::new("/", "balanced");
        // Minimum viable terminal: 20x5
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        // Should not panic
        terminal.draw(|frame| draw(frame, &app)).unwrap();
    }

    #[test]
    fn render_does_not_panic_on_large_terminal() {
        let app = App::new("/very/long/path/that/might/overflow", "loose");
        let backend = TestBackend::new(200, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
    }
}
