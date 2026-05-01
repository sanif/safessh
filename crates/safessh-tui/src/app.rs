//! Top-level App state, screen routing, AppEvent loop, and terminal
//! lifecycle.

use crate::event::{AppEvent, EventStream, FsEvent};
use crate::screens::{
    approvals::ApprovalsScreen,
    audit::{AuditScreen, EditField},
    projects::ProjectsScreen,
    rules::{RuleTab, RulesScreen},
    Screen,
};
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
    pub approvals: ApprovalsScreen,
    pub rules: RulesScreen,
    pub audit: AuditScreen,
    pub help_open: bool,
}

impl App {
    pub fn new(paths: Paths) -> Self {
        let projects =
            ProjectsScreen::load(&paths).unwrap_or_else(|_| ProjectsScreen::empty(&paths));
        let approvals =
            ApprovalsScreen::load(&paths).unwrap_or_else(|_| ApprovalsScreen::empty(&paths));
        let rules = RulesScreen::load(&paths).unwrap_or_else(|_| RulesScreen::empty(&paths));
        let audit = AuditScreen::load(&paths).unwrap_or_else(|_| AuditScreen::empty(&paths));
        Self {
            paths,
            current: Screen::Projects,
            projects,
            approvals,
            rules,
            audit,
            help_open: false,
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
        // Help overlay swallows everything except quit + close.
        if self.help_open {
            match (key.code, key.modifiers) {
                (KeyCode::Char('q'), _) => return AppAction::Quit,
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => return AppAction::Quit,
                (KeyCode::Esc, _) | (KeyCode::Char('?'), _) => self.help_open = false,
                _ => {}
            }
            return AppAction::Redraw;
        }

        // Global keys first.
        match (key.code, key.modifiers) {
            (KeyCode::Char('?'), _) => {
                self.help_open = true;
                return AppAction::Redraw;
            }
            (KeyCode::Tab, _) => {
                self.current = self.current.next();
                return AppAction::Redraw;
            }
            (KeyCode::BackTab, _) => {
                self.current = self.current.prev();
                return AppAction::Redraw;
            }
            (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => return AppAction::Quit,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => return AppAction::Quit,
            _ => {}
        }
        // Per-screen keys.
        match self.current {
            Screen::Projects => {
                if self.projects.import.is_some() {
                    match key.code {
                        KeyCode::Esc => self.projects.close_import(),
                        KeyCode::Up | KeyCode::Char('k') => self.projects.import_move(-1),
                        KeyCode::Down | KeyCode::Char('j') => self.projects.import_move(1),
                        KeyCode::Char(' ') => self.projects.import_toggle(),
                        KeyCode::Enter => {
                            let _ = self.projects.import_commit();
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => self.projects.move_selection(-1),
                        KeyCode::Down | KeyCode::Char('j') => self.projects.move_selection(1),
                        KeyCode::Char('i') => {
                            let _ = self.projects.open_import();
                        }
                        _ => {}
                    }
                }
            }
            Screen::Approvals => {
                if self.approvals.picker_open {
                    match key.code {
                        KeyCode::Esc => self.approvals.close_picker(),
                        KeyCode::Up | KeyCode::Char('k') => self.approvals.picker_move(-1),
                        KeyCode::Down | KeyCode::Char('j') => self.approvals.picker_move(1),
                        KeyCode::Enter => {
                            let action = self.approvals.picker_action();
                            let _ = self.approvals.apply_to_selected(action);
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => self.approvals.move_selection(-1),
                        KeyCode::Down | KeyCode::Char('j') => self.approvals.move_selection(1),
                        KeyCode::Enter => self.approvals.open_picker(),
                        _ => {}
                    }
                }
            }
            Screen::Rules => match key.code {
                KeyCode::Char('<') | KeyCode::Left => self.rules.move_project(-1),
                KeyCode::Char('>') | KeyCode::Right => self.rules.move_project(1),
                KeyCode::Char('1') => self.rules.switch_tab(RuleTab::Timed),
                KeyCode::Char('2') => self.rules.switch_tab(RuleTab::Always),
                KeyCode::Char('3') => self.rules.switch_tab(RuleTab::Blocked),
                KeyCode::Up | KeyCode::Char('k') => self.rules.move_selection(-1),
                KeyCode::Down | KeyCode::Char('j') => self.rules.move_selection(1),
                KeyCode::Char('d') => {
                    let _ = self.rules.apply_delete();
                }
                _ => {}
            },
            Screen::Audit => {
                if self.audit.editing.is_some() {
                    match key.code {
                        KeyCode::Esc => self.audit.cancel_edit(),
                        KeyCode::Enter => self.audit.finish_edit(),
                        KeyCode::Backspace => self.audit.pop_edit_char(),
                        KeyCode::Char(c) => self.audit.push_edit_char(c),
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Up | KeyCode::Char('k') => self.audit.move_selection(-1),
                        KeyCode::Down | KeyCode::Char('j') => self.audit.move_selection(1),
                        KeyCode::Char('g') => self.audit.jump_top(),
                        KeyCode::Char('G') => self.audit.jump_bottom(),
                        KeyCode::Char('p') => self.audit.begin_edit(EditField::Project),
                        KeyCode::Char('t') => self.audit.begin_edit(EditField::Type),
                        KeyCode::Char('/') => self.audit.begin_edit(EditField::Grep),
                        _ => {}
                    }
                }
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
        match self.current {
            Screen::Projects => self.projects.render(frame, chunks[1]),
            Screen::Approvals => self.approvals.render(frame, chunks[1]),
            Screen::Rules => self.rules.render(frame, chunks[1]),
            Screen::Audit => self.audit.render(frame, chunks[1]),
        }
        crate::widgets::footer(frame, chunks[2], self.footer_text());
        if self.help_open {
            crate::help::render_overlay(frame, frame.area());
        }
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
            Some(AppEvent::Fs(FsEvent::ApprovalsChanged)) => {
                let _ = app.approvals.reload();
            }
            Some(AppEvent::Fs(FsEvent::AuditAppended)) => {
                let _ = app.audit.append_tail();
            }
            None => break,
        }
    }
    Ok(())
}
