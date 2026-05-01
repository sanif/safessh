//! Top-level App state, screen routing, AppEvent loop, and terminal
//! lifecycle.
//!
//! Concrete screens (projects/approvals/rules/audit) land in Tasks 7-10;
//! this skeleton renders a placeholder header/footer and handles
//! q/Ctrl-C/Esc → quit.

use crate::event::{AppEvent, EventStream};
use crate::screens::Screen;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, prelude::*, Terminal};
use safessh_core::error::{Error, Result};
use safessh_storage::paths::Paths;
use std::io::Stdout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    None,
    Quit,
    Redraw,
}

pub struct App {
    pub paths: Paths,
    pub current: Screen,
}

impl App {
    pub fn new(paths: Paths) -> Self {
        Self {
            paths,
            current: Screen::Projects,
        }
    }

    /// Translate a key event into an action. The placeholder skeleton in
    /// Task 5 handles only the global quit keys; per-screen routing and
    /// the `?` overlay are wired in Task 11.
    pub fn handle_key(&mut self, key: KeyEvent) -> AppAction {
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) => AppAction::Quit,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => AppAction::Quit,
            (KeyCode::Esc, _) => AppAction::Quit,
            _ => AppAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());
        crate::widgets::header(frame, chunks[0], "safessh");
        crate::widgets::footer(frame, chunks[2], "q quit  ?  help  Tab next screen");
    }
}

/// Owns the underlying `Terminal` and restores it via `Drop` so panics
/// don't leave the user's terminal in raw mode / on the alternate screen.
pub struct Tui {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Tui {
    pub fn enter() -> Result<Self> {
        enable_raw_mode().map_err(Error::Io)?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture).map_err(Error::Io)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).map_err(Error::Io)?;
        Ok(Self { terminal })
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    }
}

pub async fn run(paths: Paths) -> Result<()> {
    let mut tui = Tui::enter()?;
    let mut app = App::new(paths);
    let mut events = EventStream::new()?;

    loop {
        tui.terminal.draw(|f| app.render(f)).map_err(Error::Io)?;
        match events.next().await {
            Some(AppEvent::Key(k)) => {
                if let AppAction::Quit = app.handle_key(k) {
                    break;
                }
            }
            Some(AppEvent::Tick) => {}
            Some(AppEvent::Fs(_)) => {} // wired in Task 6+
            None => break,
        }
    }
    Ok(())
}
