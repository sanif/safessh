//! Top-level App state, screen routing, AppEvent loop, and terminal
//! lifecycle.

use crate::event::{AppEvent, EventStream, FsEvent};
use crate::screens::{projects::ProjectsScreen, Screen};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    Frame, Terminal,
};
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
    pub projects: ProjectsScreen,
}

impl App {
    pub fn new(paths: Paths) -> Self {
        let projects =
            ProjectsScreen::load(&paths).unwrap_or_else(|_| ProjectsScreen::empty(&paths));
        Self {
            paths,
            current: Screen::Projects,
            projects,
        }
    }

    fn header_text(&self) -> &'static str {
        match self.current {
            Screen::Projects => "safessh — Projects",
            Screen::Approvals => "safessh — Approvals",
            Screen::Rules => "safessh — Rules",
            Screen::Audit => "safessh — Audit",
        }
    }

    fn footer_text(&self) -> &'static str {
        "q quit  Tab next  ↑↓/jk move"
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AppAction {
        // Global keys first.
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => return AppAction::Quit,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => return AppAction::Quit,
            _ => {}
        }
        // Per-screen keys.
        if self.current == Screen::Projects {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.projects.move_selection(-1),
                KeyCode::Down | KeyCode::Char('j') => self.projects.move_selection(1),
                _ => {}
            }
        }
        AppAction::Redraw
    }

    pub fn render(&self, frame: &mut Frame<'_>) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());
        crate::widgets::header(frame, chunks[0], self.header_text());
        // Approvals/Rules/Audit screens land in Tasks 8-10; the
        // single-arm match becomes wider then.
        #[allow(clippy::single_match)]
        match self.current {
            Screen::Projects => self.projects.render(frame, chunks[1]),
            _ => {}
        }
        crate::widgets::footer(frame, chunks[2], self.footer_text());
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
    let _watcher = crate::watcher::start_watcher(&app.paths, events.fs_tx.clone())?;

    loop {
        tui.terminal.draw(|f| app.render(f)).map_err(Error::Io)?;
        match events.next().await {
            Some(AppEvent::Key(k)) => {
                if let AppAction::Quit = app.handle_key(k) {
                    break;
                }
            }
            Some(AppEvent::Tick) => {}
            Some(AppEvent::Fs(FsEvent::ProjectsChanged)) => {
                let _ = app.projects.reload();
            }
            Some(AppEvent::Fs(_)) => {}
            None => break,
        }
    }
    Ok(())
}
