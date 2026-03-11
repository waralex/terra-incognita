use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, Role};

/// Renders the entire UI.
pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // status bar
            Constraint::Min(3),    // chat area
            Constraint::Length(3), // input
        ])
        .split(frame.area());

    draw_status_bar(frame, app, chunks[0]);

    if app.show_side_panel {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);
        draw_chat(frame, app, h_chunks[0]);
        draw_side_panel(frame, app, h_chunks[1]);
    } else {
        draw_chat(frame, app, chunks[1]);
    }

    draw_input(frame, app, chunks[2]);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let toggle_hint = if app.show_side_panel { "Tab: hide panel" } else { "Tab: show panel" };
    let text = Line::from(vec![
        Span::styled(" terra-agent", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(format!(" \u{00b7} branch: {}", app.branch)),
        Span::raw("  "),
        Span::styled(format!("[{toggle_hint}]"), Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled("[Ctrl+B: switch session]", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled("[Esc: quit]", Style::default().fg(Color::DarkGray)),
    ]);
    let bar = Paragraph::new(text).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(bar, area);
}

fn draw_chat(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    for msg in &app.messages {
        let (prefix, style) = match msg.role {
            Role::User => (
                "you> ",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Role::Agent => (
                "agent> ",
                Style::default().fg(Color::Cyan),
            ),
            Role::System => (
                "sys> ",
                Style::default().fg(Color::DarkGray),
            ),
        };
        for text_line in msg.text.lines() {
            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(text_line, style),
            ]));
        }
        lines.push(Line::from(""));
    }

    let total_lines = lines.len() as u16;
    let visible = area.height.saturating_sub(2); // borders
    let max_scroll = total_lines.saturating_sub(visible);
    let scroll = (max_scroll as usize).saturating_sub(app.scroll_offset) as u16;

    let chat = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title(" Chat "))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(chat, area);
}

fn draw_side_panel(frame: &mut Frame, app: &App, area: Rect) {
    let content = if app.side_panel_content.is_empty() {
        "(empty)".to_string()
    } else {
        app.side_panel_content.clone()
    };
    let panel = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title(" Branch State "))
        .wrap(Wrap { trim: false });
    frame.render_widget(panel, area);
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title(" Input (YAML) "));
    frame.render_widget(input, area);

    // Place cursor (display columns, not byte offset)
    frame.set_cursor_position((
        area.x + app.cursor_display_col() as u16 + 1,
        area.y + 1,
    ));
}
