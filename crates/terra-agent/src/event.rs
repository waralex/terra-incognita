use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

use crate::app::App;

/// Polls for a crossterm event and updates app state accordingly.
/// Returns Some(input) if Enter was pressed and input was taken (needs dispatch after redraw).
pub fn handle_events(app: &mut App) -> std::io::Result<Option<String>> {
    if event::poll(Duration::from_millis(50))? {
        if let Event::Key(key) = event::read()? {
            return Ok(handle_key(app, key));
        }
    }
    Ok(None)
}

fn handle_key(app: &mut App, key: KeyEvent) -> Option<String> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        (KeyCode::Char('b'), KeyModifiers::CONTROL) => {
            app.wants_switch_session = true;
        }
        (KeyCode::Enter, _) => return app.take_input(),
        (KeyCode::Tab, _) => app.toggle_panel(),
        (KeyCode::Up, _) => app.scroll_up(),
        (KeyCode::Down, _) => app.scroll_down(),
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => app.panel_up(),
        (KeyCode::Char('d'), KeyModifiers::CONTROL) => app.panel_down(),
        (KeyCode::Left, _) => app.cursor_left(),
        (KeyCode::Right, _) => app.cursor_right(),
        (KeyCode::Home, _) => app.cursor_home(),
        (KeyCode::End, _) => app.cursor_end(),
        (KeyCode::Backspace, _) => app.backspace(),
        (KeyCode::Delete, _) => app.delete(),
        (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => app.insert_char(c),
        _ => {}
    }
    None
}
