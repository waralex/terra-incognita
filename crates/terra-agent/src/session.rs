use std::io;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Terminal;

use crate::store::StoreHandle;

/// Result of the session selection screen.
pub enum SessionChoice {
    Selected(String),
    Quit,
}

/// Creation step: first name, then goal.
enum CreateStep {
    Name,
    Goal,
}

/// Session selection state.
struct SessionScreen {
    sessions: Vec<String>,
    selected: usize,
    creating: Option<CreateStep>,
    new_name: String,
    new_goal: String,
    cursor_pos: usize,
    error: Option<String>,
}

impl SessionScreen {
    fn new(sessions: Vec<String>) -> Self {
        Self {
            sessions,
            selected: 0,
            creating: None,
            new_name: String::new(),
            new_goal: String::new(),
            cursor_pos: 0,
            error: None,
        }
    }

    fn active_input(&self) -> &str {
        match &self.creating {
            Some(CreateStep::Name) => &self.new_name,
            Some(CreateStep::Goal) => &self.new_goal,
            None => "",
        }
    }

    fn insert_char(&mut self, c: char) {
        match &self.creating {
            Some(CreateStep::Name) => {
                self.new_name.insert(self.cursor_pos, c);
            }
            Some(CreateStep::Goal) => {
                self.new_goal.insert(self.cursor_pos, c);
            }
            None => {}
        }
        self.cursor_pos += c.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            match &self.creating {
                Some(CreateStep::Name) => { self.new_name.remove(self.cursor_pos); }
                Some(CreateStep::Goal) => { self.new_goal.remove(self.cursor_pos); }
                None => {}
            }
        }
    }

    fn reset_create(&mut self) {
        self.creating = None;
        self.new_name.clear();
        self.new_goal.clear();
        self.cursor_pos = 0;
        self.error = None;
    }
}

/// Shows session picker, returns chosen branch slug or Quit.
pub fn pick_session(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    store: &StoreHandle,
) -> io::Result<SessionChoice> {
    let sessions = store.list_branches().unwrap_or_default();
    let mut screen = SessionScreen::new(sessions);

    loop {
        terminal.draw(|frame| draw_session_screen(frame, &screen))?;

        if let Event::Key(key) = event::read()? {
            let creating = screen.creating.is_some();
            match (key.code, key.modifiers, creating) {
                // Quit
                (KeyCode::Esc, _, false) | (KeyCode::Char('c'), KeyModifiers::CONTROL, _) => {
                    return Ok(SessionChoice::Quit);
                }
                // Cancel create
                (KeyCode::Esc, _, true) => {
                    screen.reset_create();
                }
                // Navigate
                (KeyCode::Up, _, false) => {
                    if screen.selected > 0 {
                        screen.selected -= 1;
                    }
                }
                (KeyCode::Down, _, false) => {
                    if screen.selected < screen.sessions.len() {
                        screen.selected += 1;
                    }
                }
                // Select / enter create
                (KeyCode::Enter, _, false) => {
                    if screen.selected < screen.sessions.len() {
                        let slug = screen.sessions[screen.selected].clone();
                        return Ok(SessionChoice::Selected(slug));
                    } else {
                        screen.creating = Some(CreateStep::Name);
                        screen.error = None;
                    }
                }
                // Create: submit step
                (KeyCode::Enter, _, true) => {
                    match &screen.creating {
                        Some(CreateStep::Name) => {
                            let name = screen.new_name.trim().to_string();
                            if name.is_empty() {
                                screen.error = Some("Name cannot be empty".into());
                            } else {
                                screen.creating = Some(CreateStep::Goal);
                                screen.cursor_pos = 0;
                                screen.error = None;
                            }
                        }
                        Some(CreateStep::Goal) => {
                            let name = screen.new_name.trim().to_string();
                            let goal = screen.new_goal.trim().to_string();
                            let reasoning = if goal.is_empty() {
                                format!("Agent session: {name}")
                            } else {
                                goal
                            };
                            match store.create_branch(&name, &reasoning) {
                                Ok(()) => return Ok(SessionChoice::Selected(name)),
                                Err(e) => screen.error = Some(e),
                            }
                        }
                        None => {}
                    }
                }
                // Type
                (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT, true) => {
                    screen.insert_char(c);
                    screen.error = None;
                }
                (KeyCode::Backspace, _, true) => screen.backspace(),
                (KeyCode::Left, _, true) => {
                    screen.cursor_pos = screen.cursor_pos.saturating_sub(1);
                }
                (KeyCode::Right, _, true) => {
                    if screen.cursor_pos < screen.active_input().len() {
                        screen.cursor_pos += 1;
                    }
                }
                _ => {}
            }
        }
    }
}

fn draw_session_screen(frame: &mut Frame, screen: &SessionScreen) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // title
            Constraint::Min(5),    // list
            Constraint::Length(3), // input or hint
            Constraint::Length(1), // error
        ])
        .split(area);

    // Title
    let title = Paragraph::new(Line::from(vec![
        Span::styled(" terra-agent", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" \u{00b7} select session"),
    ]))
    .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(title, chunks[0]);

    // Session list
    let mut items: Vec<ListItem> = screen.sessions.iter().enumerate().map(|(i, slug)| {
        let is_selected = i == screen.selected && screen.creating.is_none();
        let style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let marker = if is_selected { "> " } else { "  " };
        ListItem::new(Line::from(Span::styled(format!("{marker}{slug}"), style)))
    }).collect();

    // "New session" item
    let new_idx = screen.sessions.len();
    let is_new_selected = screen.selected == new_idx && screen.creating.is_none();
    let new_style = if is_new_selected {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let new_marker = if is_new_selected { "> " } else { "  " };
    items.push(ListItem::new(Line::from(Span::styled(
        format!("{new_marker}+ New session"), new_style,
    ))));

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Sessions "));
    frame.render_widget(list, chunks[1]);

    // Input or hint
    match &screen.creating {
        Some(CreateStep::Name) => {
            let input = Paragraph::new(screen.new_name.as_str())
                .block(Block::default().borders(Borders::ALL).title(" Session name (slug) "));
            frame.render_widget(input, chunks[2]);
            frame.set_cursor_position((
                chunks[2].x + screen.cursor_pos as u16 + 1,
                chunks[2].y + 1,
            ));
        }
        Some(CreateStep::Goal) => {
            let input = Paragraph::new(screen.new_goal.as_str())
                .block(Block::default().borders(Borders::ALL).title(
                    format!(" Session goal for \"{}\" (Enter to skip) ", screen.new_name)
                ));
            frame.render_widget(input, chunks[2]);
            frame.set_cursor_position((
                chunks[2].x + screen.cursor_pos as u16 + 1,
                chunks[2].y + 1,
            ));
        }
        None => {
            let hint = Paragraph::new(Line::from(vec![
                Span::styled(" Enter", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(": select  "),
                Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(": quit"),
            ]));
            frame.render_widget(hint, chunks[2]);
        }
    }

    // Error
    if let Some(ref err) = screen.error {
        let error = Paragraph::new(Span::styled(
            format!(" {err}"),
            Style::default().fg(Color::Red),
        ));
        frame.render_widget(error, chunks[3]);
    }
}
