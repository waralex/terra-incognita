use std::path::PathBuf;
use std::sync::OnceLock;

use crate::llm::{self, LlmCommand, LlmProvider, LlmResult};
use crate::sql::SqlTool;
use crate::store::StoreHandle;

const MAX_RETRIES: usize = 2;
const MAX_COMMAND_ROUNDS: usize = 3;
const MAX_COMMANDS_PER_ROUND: usize = 3;

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
    pub prompt_name: String,
    store: StoreHandle,
    mode: Mode,
    sql_tool: Option<SqlTool>,
}

impl App {
    /// Creates a new App with the given store handle, mode, branch, and optional SQL tool.
    pub fn new(store: StoreHandle, mode: Mode, branch: String, sql_tool: Option<SqlTool>) -> Self {
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
            prompt_name: prompt_name(),
            store,
            mode,
            sql_tool,
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
    /// Supports a command loop: if LLM returns commands, execute them and call LLM again.
    fn dispatch_llm(&mut self, input: &str) {
        let mut compiled_commands: Vec<serde_json::Value> = Vec::new();

        for round in 0..=MAX_COMMAND_ROUNDS {
            let branch_state = self.store.fetch_state(&self.branch).unwrap_or_default();

            let mut extended_state = branch_state;

            if let Some(ref tool) = self.sql_tool {
                extended_state.push_str(&format!(
                    "\navailable_tools:\n  - type: sql\n    engine: postgresql\n    database: {}\n",
                    tool.database
                ));
            }

            if !compiled_commands.is_empty() {
                let cc_yaml = serde_yaml::to_string(&compiled_commands).unwrap_or_default();
                extended_state.push_str(&format!("\ncompiled_commands:\n{cc_yaml}"));
            }

            let result = {
                let provider: &dyn LlmProvider = match &self.mode {
                    Mode::Llm(p) => p.as_ref(),
                    _ => unreachable!(),
                };
                llm::call_llm_with_retry(
                    provider,
                    system_prompt(),
                    &extended_state,
                    input,
                    MAX_RETRIES,
                )
            };

            match result {
                Ok(LlmResult { answer, transaction_yaml, usage, commands }) => {
                    let token_info = match &usage {
                        Some(u) => {
                            self.total_tokens += u.total_tokens;
                            format!(" [{}+{}={}tok]", u.prompt_tokens, u.completion_tokens, u.total_tokens)
                        }
                        None => String::new(),
                    };

                    if !answer.is_empty() {
                        self.messages.push(Message {
                            role: Role::Agent,
                            text: answer,
                        });
                    }

                    // Dispatch transaction if it has content
                    if !transaction_yaml.is_empty() {
                        let dispatch_yaml = inject_commands(&transaction_yaml, &compiled_commands);
                        match self.store.dispatch(&dispatch_yaml, &self.branch) {
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
                    } else if !token_info.is_empty() {
                        self.messages.push(Message {
                            role: Role::System,
                            text: format!("no transaction{token_info}"),
                        });
                    }

                    // Execute commands if any
                    if commands.is_empty() || round == MAX_COMMAND_ROUNDS {
                        break;
                    }

                    compiled_commands = self.execute_commands(&commands);

                    self.messages.push(Message {
                        role: Role::System,
                        text: format!("executed {} command(s), calling LLM again...", compiled_commands.len()),
                    });
                }
                Err(e) => {
                    self.messages.push(Message {
                        role: Role::System,
                        text: format!("LLM error: {e}"),
                    });
                    break;
                }
            }
        }
    }

    /// Executes LLM commands (up to MAX_COMMANDS_PER_ROUND) and returns compiled results.
    fn execute_commands(&mut self, commands: &[LlmCommand]) -> Vec<serde_json::Value> {
        commands
            .iter()
            .take(MAX_COMMANDS_PER_ROUND)
            .map(|cmd| {
                let mut compiled = serde_json::to_value(cmd).unwrap_or_default();

                match cmd.command_type.as_str() {
                    "sql" => {
                        let query = cmd.query.as_deref().unwrap_or("");
                        self.messages.push(Message {
                            role: Role::System,
                            text: format!("sql> {query}"),
                        });

                        match &self.sql_tool {
                            Some(tool) => match tool.execute(query) {
                                Ok(result) => {
                                    self.messages.push(Message {
                                        role: Role::System,
                                        text: format!(
                                            "  {} rows, {}ms{}",
                                            result.row_count,
                                            result.elapsed_ms,
                                            if result.truncated { " (truncated)" } else { "" }
                                        ),
                                    });
                                    compiled["result"] = serde_json::to_value(&result).unwrap_or_default();
                                }
                                Err(e) => {
                                    self.messages.push(Message {
                                        role: Role::System,
                                        text: format!("  error: {e}"),
                                    });
                                    compiled["error"] = serde_json::Value::String(e);
                                }
                            },
                            None => {
                                let err = "SQL tool not available (no database configured)";
                                self.messages.push(Message {
                                    role: Role::System,
                                    text: format!("  error: {err}"),
                                });
                                compiled["error"] = serde_json::Value::String(err.into());
                            }
                        }
                    }
                    other => {
                        let err = format!("unknown command type: {other}");
                        self.messages.push(Message {
                            role: Role::System,
                            text: format!("  error: {err}"),
                        });
                        compiled["error"] = serde_json::Value::String(err);
                    }
                }

                compiled
            })
            .collect()
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

    /// Copies branch state to system clipboard.
    pub fn copy_state_to_clipboard(&mut self) {
        let state = self.store.fetch_state(&self.branch).unwrap_or_default();
        let ok = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(state.as_bytes())?;
                }
                child.wait()
            })
            .is_ok();

        self.messages.push(Message {
            role: Role::System,
            text: if ok { "State copied to clipboard".into() } else { "Failed to copy to clipboard".into() },
        });
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

/// Strips result data from compiled commands, keeping only metadata for audit trail.
fn strip_command_results(commands: &[serde_json::Value]) -> Vec<serde_json::Value> {
    commands
        .iter()
        .map(|cmd| {
            let mut stripped = serde_json::Map::new();
            for key in ["reasoning", "type", "query", "command_type"] {
                if let Some(v) = cmd.get(key) {
                    stripped.insert(key.into(), v.clone());
                }
            }
            // Keep stats but not full result data
            if let Some(result) = cmd.get("result") {
                let mut stats = serde_json::Map::new();
                for key in ["row_count", "elapsed_ms", "truncated"] {
                    if let Some(v) = result.get(key) {
                        stats.insert(key.into(), v.clone());
                    }
                }
                if !stats.is_empty() {
                    stripped.insert("stats".into(), serde_json::Value::Object(stats));
                }
            }
            if let Some(err) = cmd.get("error") {
                stripped.insert("error".into(), err.clone());
            }
            serde_json::Value::Object(stripped)
        })
        .collect()
}

/// Injects compiled commands metadata into transaction YAML before dispatch.
fn inject_commands(transaction_yaml: &str, commands: &[serde_json::Value]) -> String {
    if commands.is_empty() {
        return transaction_yaml.to_string();
    }
    let stripped = strip_command_results(commands);
    let Ok(mut val) = serde_yaml::from_str::<serde_json::Value>(transaction_yaml) else {
        return transaction_yaml.to_string();
    };
    if let Some(obj) = val.as_object_mut() {
        obj.insert("commands".into(), serde_json::Value::Array(stripped));
    }
    serde_yaml::to_string(&val).unwrap_or_else(|_| transaction_yaml.to_string())
}

/// Returns the human-readable prompt name from `TERRA_AGENT_PROMPT` (default: "system").
fn prompt_name() -> String {
    match std::env::var("TERRA_AGENT_PROMPT") {
        Ok(val) if val.contains('/') || val.contains('.') => {
            PathBuf::from(&val)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or(val)
        }
        Ok(name) => name,
        Err(_) => "system".into(),
    }
}

/// Loads system prompt from a file.
///
/// Resolution order:
/// 1. `TERRA_AGENT_PROMPT` env var — exact file path or bare name (resolved to `prompts/{name}.md`)
/// 2. Default: `prompts/system.md`
fn system_prompt() -> &'static str {
    static PROMPT: OnceLock<String> = OnceLock::new();
    PROMPT.get_or_init(|| {
        let prompt_file = match std::env::var("TERRA_AGENT_PROMPT") {
            Ok(val) if val.contains('/') || val.contains('.') => val,
            Ok(name) => format!("prompts/{name}.md"),
            Err(_) => "prompts/system.md".into(),
        };

        let candidates = [
            // Next to the executable (cargo run sets this to target/debug/)
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .map(|d| d.join(&prompt_file)),
            // Relative to cwd (typical for `cargo run -p terra-agent`)
            Some(PathBuf::from(format!("crates/terra-agent/{prompt_file}"))),
            // Direct relative
            Some(PathBuf::from(&prompt_file)),
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
