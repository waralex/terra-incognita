mod app;
mod event;
mod llm;
mod session;
pub mod sql;
mod store;
mod ui;

use std::io;
use std::path::PathBuf;

use crossterm::event::DisableMouseCapture;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::{App, Mode};
use crate::llm::anthropic::AnthropicProvider;
use crate::llm::openai::OpenAiProvider;
use crate::llm::LlmProviderConfig;
use crate::session::SessionChoice;
use crate::store::StoreHandle;

const CONFIG_FILENAME: &str = "terra-incognita.yml";
const CONFIG_ENV: &str = "TERRA_INCOGNITA_CONFIG";
const DEFAULT_DATA_DIR: &str = ".terra-incognita";

fn load_data_dir() -> PathBuf {
    let candidates = config_candidates();
    for path in &candidates {
        if path.exists() {
            if let Ok(contents) = std::fs::read_to_string(path) {
                if let Ok(val) = serde_yaml::from_str::<serde_json::Value>(&contents) {
                    if let Some(dir) = val.get("data_dir").and_then(|v| v.as_str()) {
                        return PathBuf::from(dir);
                    }
                }
            }
        }
    }
    PathBuf::from(DEFAULT_DATA_DIR)
}

fn config_candidates() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from(CONFIG_FILENAME)];
    if let Ok(env_path) = std::env::var(CONFIG_ENV) {
        paths.push(PathBuf::from(env_path));
    }
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(DEFAULT_DATA_DIR).join(CONFIG_FILENAME));
    }
    paths
}

fn main() -> io::Result<()> {
    let data_dir = load_data_dir();
    let db_path = data_dir.join("assertions");

    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir)?;
    }

    let store = StoreHandle::open(&db_path).with_log(data_dir.join("query.log"));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Session picker
    let branch = match session::pick_session(&mut terminal, &store)? {
        SessionChoice::Selected(slug) => slug,
        SessionChoice::Quit => {
            cleanup_terminal(&mut terminal)?;
            return Ok(());
        }
    };

    // Detect mode
    let mode = if let Ok(api_key) = std::env::var("TERRA_LLM_API_KEY") {
        let provider_name = std::env::var("TERRA_LLM_PROVIDER")
            .unwrap_or_else(|_| "openai".into());

        let (default_url, default_model) = match provider_name.as_str() {
            "anthropic" => ("https://api.anthropic.com", "claude-sonnet-4-20250514"),
            _ => ("https://api.openai.com/v1", "gpt-4o"),
        };

        let config = LlmProviderConfig {
            api_key,
            base_url: std::env::var("TERRA_LLM_BASE_URL")
                .unwrap_or_else(|_| default_url.into()),
            model: std::env::var("TERRA_LLM_MODEL")
                .unwrap_or_else(|_| default_model.into()),
            log_path: Some(data_dir.join("llm.log")),
        };

        let provider: Box<dyn crate::llm::LlmProvider> = match provider_name.as_str() {
            "anthropic" => Box::new(AnthropicProvider::new(config)),
            _ => Box::new(OpenAiProvider::new(config)),
        };

        Mode::Llm(provider)
    } else {
        Mode::Direct
    };

    let mut app = App::new(store.clone(), mode, branch);

    // Main loop
    let result = run_loop(&mut terminal, &mut app, &store);

    cleanup_terminal(&mut terminal)?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    store: &StoreHandle,
) -> io::Result<()> {
    while !app.should_quit {
        if app.wants_switch_session {
            app.wants_switch_session = false;
            match session::pick_session(terminal, store)? {
                SessionChoice::Selected(slug) => app.switch_branch(slug),
                SessionChoice::Quit => app.should_quit = true,
            }
            continue;
        }
        terminal.draw(|frame| ui::draw(frame, app))?;
        if let Some(input) = event::handle_events(app)? {
            // Redraw with "thinking..." before blocking on dispatch
            terminal.draw(|frame| ui::draw(frame, app))?;
            app.dispatch_input(&input);
        }
    }
    Ok(())
}

fn cleanup_terminal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}
