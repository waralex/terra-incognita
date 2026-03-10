use crate::store::StoreHandle;

/// Message role in the chat log.
#[derive(Clone)]
pub enum Role {
    User,
    System,
}

/// A single chat message.
#[derive(Clone)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

/// Application state.
pub struct App {
    pub messages: Vec<Message>,
    pub input: String,
    pub cursor_pos: usize,
    pub branch: String,
    pub show_side_panel: bool,
    pub side_panel_content: String,
    pub should_quit: bool,
    pub scroll_offset: usize,
    store: StoreHandle,
}

impl App {
    /// Creates a new App with the given store handle.
    pub fn new(store: StoreHandle) -> Self {
        Self {
            messages: vec![Message {
                role: Role::System,
                text: "Welcome to terra-agent. Type YAML commands and press Enter.".into(),
            }],
            input: String::new(),
            cursor_pos: 0,
            branch: "main".into(),
            show_side_panel: false,
            side_panel_content: String::new(),
            should_quit: false,
            scroll_offset: 0,
            store,
        }
    }

    /// Submits the current input as a YAML command.
    pub fn submit_input(&mut self) {
        let input = self.input.trim().to_string();
        if input.is_empty() {
            return;
        }

        self.messages.push(Message {
            role: Role::User,
            text: input.clone(),
        });

        let response = match self.store.dispatch(&input, &self.branch) {
            Ok(yaml) => yaml,
            Err(e) => e,
        };

        self.messages.push(Message {
            role: Role::System,
            text: response.trim().to_string(),
        });

        self.input.clear();
        self.cursor_pos = 0;
        self.scroll_offset = 0;

        // Auto-refresh side panel if visible
        if self.show_side_panel {
            self.refresh_side_panel();
        }
    }

    /// Toggles the side panel and refreshes its content.
    pub fn toggle_panel(&mut self) {
        self.show_side_panel = !self.show_side_panel;
        if self.show_side_panel {
            self.refresh_side_panel();
        }
    }

    /// Scrolls chat log up.
    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    /// Scrolls chat log down.
    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Moves cursor left in input.
    pub fn cursor_left(&mut self) {
        self.cursor_pos = self.cursor_pos.saturating_sub(1);
    }

    /// Moves cursor right in input.
    pub fn cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos += 1;
        }
    }

    /// Moves cursor to start of input.
    pub fn cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    /// Moves cursor to end of input.
    pub fn cursor_end(&mut self) {
        self.cursor_pos = self.input.len();
    }

    /// Inserts a character at cursor position.
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    /// Deletes the character before cursor.
    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.input.remove(self.cursor_pos);
        }
    }

    /// Deletes the character at cursor.
    pub fn delete(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.input.remove(self.cursor_pos);
        }
    }

    fn refresh_side_panel(&mut self) {
        self.side_panel_content = match self.store.fetch_state(&self.branch) {
            Ok(yaml) => yaml,
            Err(e) => e,
        };
    }
}
