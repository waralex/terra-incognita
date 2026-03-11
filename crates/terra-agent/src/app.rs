use std::path::PathBuf;
use std::sync::OnceLock;

use crate::llm::{self, LlmProvider, LlmResult};
use crate::store::StoreHandle;

const MAX_RETRIES: usize = 2;

/// Message role in the chat log.
#[derive(Clone)]
pub enum Role {
    User,
    Agent,
    System,
}

/// A single chat message.
#[derive(Clone)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

/// Operating mode.
pub enum Mode {
    /// Direct YAML command dispatch (no LLM).
    Direct,
    /// LLM-assisted: user types natural language, LLM returns transactions.
    Llm(Box<dyn LlmProvider>),
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
    pub wants_switch_session: bool,
    pub scroll_offset: usize,
    pub total_tokens: usize,
    pub state_tokens: usize,
    pub panel_scroll: usize,
    store: StoreHandle,
    mode: Mode,
}

impl App {
    /// Creates a new App with the given store handle, mode, and branch.
    pub fn new(store: StoreHandle, mode: Mode, branch: String) -> Self {
        let welcome = match &mode {
            Mode::Direct => format!("Direct mode · branch: {branch}. Type YAML commands and press Enter."),
            Mode::Llm(_) => format!("LLM mode · branch: {branch}. Type natural language, agent will create transactions."),
        };
        let mut messages = vec![Message {
            role: Role::System,
            text: welcome,
        }];

        // Load recent conversation history from branch transactions
        let history = store.fetch_history(&branch, 10);
        if !history.is_empty() {
            messages.push(Message {
                role: Role::System,
                text: format!("── last {} exchanges ──", history.len()),
            });
            for (q, a) in &history {
                messages.push(Message { role: Role::User, text: q.clone() });
                messages.push(Message { role: Role::Agent, text: a.clone() });
            }
            messages.push(Message {
                role: Role::System,
                text: "── end of history ──".into(),
            });
        }

        Self {
            messages,
            input: String::new(),
            cursor_pos: 0,
            branch,
            show_side_panel: false,
            side_panel_content: String::new(),
            should_quit: false,
            wants_switch_session: false,
            scroll_offset: 0,
            total_tokens: 0,
            state_tokens: 0,
            panel_scroll: 0,
            store,
            mode,
        }
    }

    /// Takes input from the buffer. Returns Some(input) if non-empty.
    /// Moves the message to chat immediately so UI can redraw before dispatch.
    pub fn take_input(&mut self) -> Option<String> {
        let input = self.input.trim().to_string();
        if input.is_empty() {
            return None;
        }

        self.messages.push(Message {
            role: Role::User,
            text: input.clone(),
        });
        self.input.clear();
        self.cursor_pos = 0;
        self.scroll_offset = 0;

        // Add pending indicator for LLM mode
        if matches!(self.mode, Mode::Llm(_)) {
            self.messages.push(Message {
                role: Role::System,
                text: "thinking...".into(),
            });
        }

        Some(input)
    }

    /// Dispatches the input after UI has redrawn.
    pub fn dispatch_input(&mut self, input: &str) {
        // Remove pending message if present
        if matches!(self.mode, Mode::Llm(_)) {
            if let Some(last) = self.messages.last() {
                if last.text == "thinking..." {
                    self.messages.pop();
                }
            }
        }

        match &self.mode {
            Mode::Direct => self.dispatch_direct(input),
            Mode::Llm(_) => self.dispatch_llm(input),
        }

        self.update_state_tokens();
        if self.show_side_panel {
            self.refresh_side_panel();
        }
    }

    /// Direct mode: dispatch YAML command as-is.
    fn dispatch_direct(&mut self, input: &str) {
        let response = match self.store.dispatch(input, &self.branch) {
            Ok(yaml) => yaml,
            Err(e) => e,
        };
        self.messages.push(Message {
            role: Role::System,
            text: response.trim().to_string(),
        });
    }

    /// LLM mode: send to LLM, extract answer, dispatch transaction.
    fn dispatch_llm(&mut self, input: &str) {
        let provider: &dyn LlmProvider = match &self.mode {
            Mode::Llm(p) => p.as_ref(),
            _ => unreachable!(),
        };

        let branch_state = self.store.fetch_state(&self.branch).unwrap_or_default();

        let result = llm::call_llm_with_retry(
            provider,
            system_prompt(),
            &branch_state,
            input,
            MAX_RETRIES,
        );

        match result {
            Ok(LlmResult { answer, transaction_json, usage }) => {
                let token_info = match &usage {
                    Some(u) => {
                        self.total_tokens += u.total_tokens;
                        format!(" [{}+{}={}tok]", u.prompt_tokens, u.completion_tokens, u.total_tokens)
                    }
                    None => String::new(),
                };

                // Show answer immediately
                if !answer.is_empty() {
                    self.messages.push(Message {
                        role: Role::Agent,
                        text: answer.clone(),
                    });
                }

                // Dispatch transaction
                match self.store.dispatch(&transaction_json, &self.branch) {
                    Ok(yaml) => {
                        self.messages.push(Message {
                            role: Role::System,
                            text: format!("tx committed{token_info}\n{}", yaml.trim()),
                        });
                    }
                    Err(e) => {
                        self.messages.push(Message {
                            role: Role::System,
                            text: format!("tx failed{token_info}: {e}"),
                        });
                    }
                }

            }
            Err(e) => {
                self.messages.push(Message {
                    role: Role::System,
                    text: format!("LLM error: {e}"),
                });
            }
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

    /// Scrolls side panel up (half-page).
    pub fn panel_up(&mut self) {
        self.panel_scroll = self.panel_scroll.saturating_add(10);
    }

    /// Scrolls side panel down (half-page).
    pub fn panel_down(&mut self) {
        self.panel_scroll = self.panel_scroll.saturating_sub(10);
    }

    /// Moves cursor left in input (by one char boundary).
    pub fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.input[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Moves cursor right in input (by one char boundary).
    pub fn cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos = self.input[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.input.len());
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
            let prev = self.input[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.drain(prev..self.cursor_pos);
            self.cursor_pos = prev;
        }
    }

    /// Deletes the character at cursor.
    pub fn delete(&mut self) {
        if self.cursor_pos < self.input.len() {
            let next = self.input[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.input.len());
            self.input.drain(self.cursor_pos..next);
        }
    }

    /// Returns the display width (in columns) of text before cursor.
    pub fn cursor_display_col(&self) -> usize {
        self.input[..self.cursor_pos].chars().count()
    }

    /// Switches to a different branch (session).
    pub fn switch_branch(&mut self, branch: String) {
        self.branch = branch.clone();
        self.messages.push(Message {
            role: Role::System,
            text: format!("Switched to session: {branch}"),
        });

        let history = self.store.fetch_history(&self.branch, 10);
        if !history.is_empty() {
            self.messages.push(Message {
                role: Role::System,
                text: format!("── last {} exchanges ──", history.len()),
            });
            for (q, a) in &history {
                self.messages.push(Message { role: Role::User, text: q.clone() });
                self.messages.push(Message { role: Role::Agent, text: a.clone() });
            }
            self.messages.push(Message {
                role: Role::System,
                text: "── end of history ──".into(),
            });
        }

        self.scroll_offset = 0;
        if self.show_side_panel {
            self.refresh_side_panel();
        }
    }

    fn update_state_tokens(&mut self) {
        let state = self.store.fetch_state(&self.branch).unwrap_or_default();
        self.state_tokens = llm::estimate_tokens(&state);
    }

    fn refresh_side_panel(&mut self) {
        self.side_panel_content = match self.store.fetch_state(&self.branch) {
            Ok(yaml) => yaml,
            Err(e) => e,
        };
        self.state_tokens = llm::estimate_tokens(&self.side_panel_content);
    }
}

/// Loads system prompt from `prompts/system.md` relative to the executable,
/// falling back to a few common locations.
fn system_prompt() -> &'static str {
    static PROMPT: OnceLock<String> = OnceLock::new();
    PROMPT.get_or_init(|| {
        let candidates = [
            // Next to the executable (cargo run sets this to target/debug/)
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .map(|d| d.join("prompts/system.md")),
            // Relative to cwd (typical for `cargo run -p terra-agent`)
            Some(PathBuf::from("crates/terra-agent/prompts/system.md")),
            // Direct relative
            Some(PathBuf::from("prompts/system.md")),
        ];
        for candidate in candidates.into_iter().flatten() {
            if let Ok(content) = std::fs::read_to_string(&candidate) {
                return content;
            }
        }
        // Minimal fallback if file not found
        "You are a knowledge management agent. Respond with a JSON transaction containing \"answer\" and \"reasoning\".".into()
    })
}
