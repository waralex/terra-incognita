mod app;
mod event;
mod llm;
mod store;
mod ui;

use std::io;
use std::path::PathBuf;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::{App, Mode};
use crate::llm::LlmConfig;
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

    let store = StoreHandle::open(&db_path);

    // Detect mode: if TERRA_LLM_API_KEY is set, use LLM mode
    let mode = match LlmConfig::from_env() {
        Some(config) => Mode::Llm(config),
        None => Mode::Direct,
    };

    let mut app = App::new(store, mode);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    let result = run_loop(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;
        event::handle_events(app)?;
    }
    Ok(())
}
