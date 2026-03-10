use crate::llm::{self, ChatMessage, LlmConfig, LlmResult};
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
    Llm(LlmConfig),
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
    store: StoreHandle,
    mode: Mode,
    /// Conversation history for LLM context (last N exchanges).
    llm_history: Vec<ChatMessage>,
}

impl App {
    /// Creates a new App with the given store handle, mode, and branch.
    pub fn new(store: StoreHandle, mode: Mode, branch: String) -> Self {
        let welcome = match &mode {
            Mode::Direct => format!("Direct mode · branch: {branch}. Type YAML commands and press Enter."),
            Mode::Llm(_) => format!("LLM mode · branch: {branch}. Type natural language, agent will create transactions."),
        };
        Self {
            messages: vec![Message {
                role: Role::System,
                text: welcome,
            }],
            input: String::new(),
            cursor_pos: 0,
            branch,
            show_side_panel: false,
            side_panel_content: String::new(),
            should_quit: false,
            wants_switch_session: false,
            scroll_offset: 0,
            store,
            mode,
            llm_history: Vec::new(),
        }
    }

    /// Submits the current input.
    pub fn submit_input(&mut self) {
        let input = self.input.trim().to_string();
        if input.is_empty() {
            return;
        }

        self.messages.push(Message {
            role: Role::User,
            text: input.clone(),
        });
        self.input.clear();
        self.cursor_pos = 0;
        self.scroll_offset = 0;

        match &self.mode {
            Mode::Direct => self.dispatch_direct(&input),
            Mode::Llm(_) => self.dispatch_llm(&input),
        }

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
        // Grab config reference - need to clone for borrow checker
        let config = match &self.mode {
            Mode::Llm(c) => LlmConfig {
                api_key: c.api_key.clone(),
                base_url: c.base_url.clone(),
                model: c.model.clone(),
            },
            _ => unreachable!(),
        };

        let branch_state = self.store.fetch_state(&self.branch).unwrap_or_default();

        let result = llm::call_llm_with_retry(
            &config,
            system_prompt(),
            &branch_state,
            &self.llm_history,
            input,
            MAX_RETRIES,
        );

        match result {
            Ok(LlmResult { answer, transaction_json }) => {
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
                            text: format!("tx committed\n{}", yaml.trim()),
                        });
                    }
                    Err(e) => {
                        self.messages.push(Message {
                            role: Role::System,
                            text: format!("tx failed: {e}"),
                        });
                    }
                }

                // Update history (keep last 10 exchanges)
                self.llm_history.push(ChatMessage {
                    role: "user".into(),
                    content: input.into(),
                });
                self.llm_history.push(ChatMessage {
                    role: "assistant".into(),
                    content: answer,
                });
                if self.llm_history.len() > 20 {
                    self.llm_history.drain(..2);
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

    /// Switches to a different branch (session).
    pub fn switch_branch(&mut self, branch: String) {
        self.branch = branch.clone();
        self.messages.push(Message {
            role: Role::System,
            text: format!("Switched to session: {branch}"),
        });
        self.llm_history.clear();
        self.scroll_offset = 0;
        if self.show_side_panel {
            self.refresh_side_panel();
        }
    }

    fn refresh_side_panel(&mut self) {
        self.side_panel_content = match self.store.fetch_state(&self.branch) {
            Ok(yaml) => yaml,
            Err(e) => e,
        };
    }
}

/// System prompt placeholder — instructs the LLM on how to use terra-incognita.
fn system_prompt() -> &'static str {
    // TODO: write a comprehensive system prompt with terra-incognita API docs,
    // examples, and response format requirements.
    r#"You are a knowledge management agent backed by terra-incognita, an append-only epistemic store.

Your EVERY response MUST be a valid JSON object that is a terra-incognita transaction.
The transaction MUST contain:
- "answer": your text response to the user (required)
- "reasoning": why you are making these changes (required)

The transaction MAY also contain data operations:
- "entity_types": create new entity types
- "properties": create new properties
- "attach": attach properties to entity types
- "introduce": create new entities with assertions
- "asserts": make assertions on existing entities
- "hide" / "unhide": visibility changes

Property value formats:
- Set: {"contains": [...], "not_contains": [...]}
- Range: {"eq": value} or {"from": v1, "to": v2}
- Struct: any JSON value

Example response (creating a person):
{
  "answer": "Created person entity for John with age and city.",
  "reasoning": "User described a person, decomposing into structured data.",
  "entity_types": [{"slug": "person"}],
  "properties": [
    {"slug": "name", "value_type": "set"},
    {"slug": "age", "value_type": "range"},
    {"slug": "city", "value_type": "set"}
  ],
  "attach": [
    {"entity_type": "person", "slug": "name"},
    {"entity_type": "person", "slug": "age"},
    {"entity_type": "person", "slug": "city"}
  ],
  "introduce": [{
    "entity": "john",
    "facts": [{
      "entity_type": "person",
      "properties": {
        "name": {"contains": ["John"]},
        "age": {"eq": 30},
        "city": {"contains": ["Moscow"]}
      },
      "reasoning": "User stated these facts directly."
    }]
  }]
}

If no data changes are needed, you MUST still provide reasoning explaining why.
The current branch state is provided below — use it to understand what already exists."#
}
