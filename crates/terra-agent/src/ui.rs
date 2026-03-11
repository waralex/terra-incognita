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
        Span::styled("[Ctrl+C: quit]", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(format!("{}tok", app.total_tokens), Style::default().fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled(format!("state: ~{}tok", app.state_tokens), Style::default().fg(Color::Magenta)),
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
    if app.side_panel_content.is_empty() {
        let panel = Paragraph::new("(empty)")
            .block(Block::default().borders(Borders::ALL).title(" Branch State "))
            .wrap(Wrap { trim: false });
        frame.render_widget(panel, area);
        return;
    }

    let lines: Vec<Line> = app.side_panel_content.lines().map(highlight_yaml).collect();

    let total_lines = lines.len() as u16;
    let visible = area.height.saturating_sub(2);
    let max_scroll = total_lines.saturating_sub(visible);
    let scroll = max_scroll.saturating_sub(app.panel_scroll as u16);

    let panel = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title(" Branch State [C-u/C-d] "))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(panel, area);
}

/// Simple YAML syntax highlighting for a single line.
fn highlight_yaml(line: &str) -> Line<'_> {
    let trimmed = line.trim_start();

    // Comment
    if trimmed.starts_with('#') {
        return Line::from(Span::styled(line, Style::default().fg(Color::DarkGray)));
    }

    // List item marker
    if trimmed.starts_with("- ") {
        let indent = line.len() - trimmed.len();
        let rest = &trimmed[2..];
        if let Some((key, val)) = rest.split_once(": ") {
            return Line::from(vec![
                Span::raw(&line[..indent]),
                Span::styled("- ", Style::default().fg(Color::Yellow)),
                Span::styled(key, Style::default().fg(Color::Cyan)),
                Span::styled(": ", Style::default().fg(Color::White)),
                highlight_value(val),
            ]);
        }
        return Line::from(vec![
            Span::raw(&line[..indent]),
            Span::styled("- ", Style::default().fg(Color::Yellow)),
            highlight_value(rest),
        ]);
    }

    // Key: value
    if let Some((key, val)) = trimmed.split_once(": ") {
        let indent = line.len() - trimmed.len();
        return Line::from(vec![
            Span::raw(&line[..indent]),
            Span::styled(key, Style::default().fg(Color::Cyan)),
            Span::styled(": ", Style::default().fg(Color::White)),
            highlight_value(val),
        ]);
    }

    // Key with no value (section header like "entities:")
    if trimmed.ends_with(':') {
        let indent = line.len() - trimmed.len();
        return Line::from(vec![
            Span::raw(&line[..indent]),
            Span::styled(trimmed, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]);
    }

    Line::from(line)
}

fn highlight_value(val: &str) -> Span<'_> {
    if val == "true" || val == "false" {
        Span::styled(val, Style::default().fg(Color::Yellow))
    } else if val == "null" || val == "~" {
        Span::styled(val, Style::default().fg(Color::DarkGray))
    } else if val.starts_with('"') || val.starts_with('\'') {
        Span::styled(val, Style::default().fg(Color::Green))
    } else if val.parse::<f64>().is_ok() {
        Span::styled(val, Style::default().fg(Color::Magenta))
    } else {
        Span::styled(val, Style::default().fg(Color::White))
    }
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
