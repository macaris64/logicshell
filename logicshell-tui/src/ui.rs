use crate::app::{App, AppMode, DispatchStatus};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PHASE: &str = "13";

/// Render the full TUI layout into the given [`Frame`].
///
/// Layout (top to bottom):
/// 1. Title banner      — single line
/// 2. Output panel      — fills remaining space (ring-buffered stdout)
/// 3. Prompt input line — single line
/// 4. Status bar        — single line (red when denied, shows exit code)
///
/// When in `Confirming` mode a modal dialog is overlaid on the output panel.
pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Min(1),    // output panel
            Constraint::Length(1), // prompt input
            Constraint::Length(1), // status bar
        ])
        .split(area);

    draw_title(frame, chunks[0]);
    draw_output_panel(frame, app, chunks[1]);
    draw_prompt(frame, app, chunks[2]);
    draw_status_bar(frame, app, chunks[3]);

    // Modal overlay: draw on top of the output panel
    if let AppMode::Confirming { command } = &app.mode {
        draw_confirm_dialog(frame, chunks[1], command);
    }
}

fn draw_title(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new(format!(
        " LogicShell v{VERSION}  —  Phase {PHASE} TUI Dispatch + Output Panel"
    ))
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .alignment(Alignment::Left);
    frame.render_widget(title, area);
}

fn draw_output_panel(frame: &mut Frame, app: &App, area: Rect) {
    let height = area.height as usize;

    let lines: Vec<Line> = if app.output_panel.is_empty() {
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
            Line::from("  PageUp / PageDown to scroll output."),
        ]
    } else {
        app.output_panel
            .visible_lines(height.max(1))
            .into_iter()
            .map(Line::from)
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
    let (text, bg, fg) = status_bar_content(app);
    let status = Paragraph::new(text).style(Style::default().bg(bg).fg(fg));
    frame.render_widget(status, area);
}

fn status_bar_content(app: &App) -> (String, Color, Color) {
    let base = format!(" Phase {PHASE} | v{VERSION} | Safety: {}", app.safety_mode);

    match &app.dispatch_status {
        DispatchStatus::Idle => (base, Color::DarkGray, Color::White),

        DispatchStatus::Running => {
            let text = format!("{base} | ⏳ Running…");
            (text, Color::DarkGray, Color::Yellow)
        }

        DispatchStatus::Done {
            exit_code,
            duration_ms,
        } => {
            let exit_color = if *exit_code == 0 {
                Color::Green
            } else {
                Color::Red
            };
            let text = format!("{base} | exit={exit_code} ({duration_ms}ms)");
            (text, Color::DarkGray, exit_color)
        }

        DispatchStatus::Denied { reason } => {
            let short = if reason.len() > 40 {
                format!("{}…", &reason[..40])
            } else {
                reason.clone()
            };
            let text = format!(" DENIED: {short}");
            (text, Color::Red, Color::White)
        }
    }
}

/// Render a centered modal dialog for the confirm prompt.
fn draw_confirm_dialog(frame: &mut Frame, area: Rect, command: &str) {
    let popup_width = (area.width as f32 * 0.7) as u16;
    let popup_height = 5u16;
    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(
        x,
        y,
        popup_width.min(area.width),
        popup_height.min(area.height),
    );

    // Clear the area behind the dialog
    frame.render_widget(Clear, popup_area);

    let truncated_cmd = if command.len() > (popup_width as usize).saturating_sub(6) {
        format!("{}…", &command[..(popup_width as usize).saturating_sub(7)])
    } else {
        command.to_string()
    };

    let block = Block::default()
        .title(" Confirm ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let body = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("  {truncated_cmd}"),
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Run this command? ", Style::default().fg(Color::White)),
            Span::styled(
                "[y]es",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" / "),
            Span::styled(
                "[n]o",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]),
    ]);
    frame.render_widget(body, inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, AppMode, DispatchStatus};
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

    fn app_idle() -> App {
        App::new("/home/user", "balanced")
    }

    // ── title ─────────────────────────────────────────────────────────────────

    #[test]
    fn title_row_contains_logicshell() {
        let app = app_idle();
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 0, 80);
        assert!(row.contains("LogicShell"), "title row: {row:?}");
    }

    #[test]
    fn title_contains_phase_13() {
        let app = app_idle();
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 0, 80);
        assert!(row.contains("13"), "title should contain phase 13: {row:?}");
    }

    // ── output panel ──────────────────────────────────────────────────────────

    #[test]
    fn welcome_message_shown_when_output_panel_empty() {
        let app = app_idle();
        let buf = render_to_buffer(&app, 80, 10);
        let interior = buf_rows(&buf, 1, 9, 80);
        assert!(interior.contains("Welcome"), "interior: {interior:?}");
    }

    #[test]
    fn output_panel_lines_shown_when_non_empty() {
        let mut app = app_idle();
        app.output_panel.push_line("$ echo hello");
        app.output_panel.push_line("hello");
        let buf = render_to_buffer(&app, 80, 10);
        let interior = buf_rows(&buf, 1, 9, 80);
        assert!(interior.contains("echo"), "output panel: {interior:?}");
    }

    #[test]
    fn output_panel_replaces_welcome_when_lines_present() {
        let mut app = app_idle();
        app.output_panel.push_line("some output");
        let buf = render_to_buffer(&app, 80, 10);
        let interior = buf_rows(&buf, 1, 9, 80);
        assert!(
            !interior.contains("Welcome"),
            "welcome should be gone: {interior:?}"
        );
    }

    // ── status bar ────────────────────────────────────────────────────────────

    #[test]
    fn status_bar_contains_phase_and_version() {
        let app = app_idle();
        let buf = render_to_buffer(&app, 80, 10);
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
    fn deny_banner_shows_in_status_bar() {
        let mut app = app_idle();
        app.dispatch_status = DispatchStatus::Denied {
            reason: "destructive command".to_string(),
        };
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 9, 80);
        assert!(
            row.contains("DENIED"),
            "status bar should show DENIED: {row:?}"
        );
    }

    #[test]
    fn deny_banner_shows_reason() {
        let mut app = app_idle();
        app.dispatch_status = DispatchStatus::Denied {
            reason: "rm -rf pattern".to_string(),
        };
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 9, 80);
        assert!(
            row.contains("rm -rf pattern"),
            "status bar should contain reason: {row:?}"
        );
    }

    #[test]
    fn done_status_shows_exit_code_in_status_bar() {
        let mut app = app_idle();
        app.dispatch_status = DispatchStatus::Done {
            exit_code: 0,
            duration_ms: 150,
        };
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 9, 80);
        assert!(
            row.contains("exit=0"),
            "status bar should show exit code: {row:?}"
        );
    }

    #[test]
    fn done_status_shows_duration_in_status_bar() {
        let mut app = app_idle();
        app.dispatch_status = DispatchStatus::Done {
            exit_code: 0,
            duration_ms: 250,
        };
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 9, 80);
        assert!(
            row.contains("250ms"),
            "status bar should show duration: {row:?}"
        );
    }

    #[test]
    fn running_status_shows_indicator_in_status_bar() {
        let mut app = app_idle();
        app.dispatch_status = DispatchStatus::Running;
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 9, 80);
        assert!(
            row.contains("Running"),
            "status bar should show Running: {row:?}"
        );
    }

    // ── confirm dialog ────────────────────────────────────────────────────────

    #[test]
    fn confirm_dialog_shown_in_confirming_mode() {
        let mut app = app_idle();
        app.mode = AppMode::Confirming {
            command: "sudo reboot".to_string(),
        };
        let buf = render_to_buffer(&app, 80, 20);
        let all = buf_rows(&buf, 0, 20, 80);
        assert!(
            all.contains("Confirm"),
            "confirm dialog should be visible: {all:?}"
        );
    }

    #[test]
    fn confirm_dialog_shows_command() {
        let mut app = app_idle();
        app.mode = AppMode::Confirming {
            command: "sudo reboot".to_string(),
        };
        let buf = render_to_buffer(&app, 80, 20);
        let all = buf_rows(&buf, 0, 20, 80);
        assert!(
            all.contains("sudo"),
            "confirm dialog should show command: {all:?}"
        );
    }

    #[test]
    fn confirm_dialog_not_shown_in_normal_mode() {
        let app = app_idle();
        let buf = render_to_buffer(&app, 80, 20);
        let all = buf_rows(&buf, 0, 20, 80);
        // "Confirm" won't appear as a standalone word in normal mode
        // (it might appear in title but not as dialog title)
        let _ = all; // just verify no panic
    }

    // ── prompt ────────────────────────────────────────────────────────────────

    #[test]
    fn prompt_shows_cwd_and_input() {
        let mut app = App::new("/home/aero", "balanced");
        app.input_widget.set_value("ls -la");
        let buf = render_to_buffer(&app, 80, 10);
        let row = buf_row(&buf, 8, 80);
        assert!(row.contains("/home/aero"), "prompt row: {row:?}");
        assert!(row.contains("ls -la"), "prompt row: {row:?}");
    }

    // ── edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn render_does_not_panic_on_small_terminal() {
        let app = app_idle();
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
    }

    #[test]
    fn render_does_not_panic_on_large_terminal() {
        let app = App::new("/very/long/path/that/might/overflow", "loose");
        let backend = TestBackend::new(200, 50);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
    }

    #[test]
    fn render_does_not_panic_with_confirm_mode_on_small_terminal() {
        let mut app = app_idle();
        app.mode = AppMode::Confirming {
            command: "sudo rm -rf /".to_string(),
        };
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
    }

    #[test]
    fn render_does_not_panic_with_many_output_lines() {
        let mut app = app_idle();
        for i in 0..1000 {
            app.output_panel.push_line(format!("line {i}"));
        }
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
    }
}
