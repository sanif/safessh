//! Rules screen — four-tab view of persistent rules per project.
//!
//! All deletes go through the storage crate's `remove` methods, which
//! hold an exclusive lock + write atomically (SAFETY-INVARIANT-12).

use crate::theme;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, TableState, Tabs},
    Frame,
};
use safessh_core::error::{Error, Result};
use safessh_storage::approvals::{AlwaysStore, BlockedStore, PatternRule, TimedRule, TimedStore};
use safessh_storage::paths::Paths;
use safessh_storage::project::{FileRule, ProjectStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleTab {
    Timed,
    Always,
    Blocked,
    File,
}

impl RuleTab {
    fn label(self) -> &'static str {
        match self {
            Self::Timed => "Timed",
            Self::Always => "Always",
            Self::Blocked => "Blocked",
            Self::File => "File",
        }
    }

    fn idx(self) -> usize {
        match self {
            Self::Timed => 0,
            Self::Always => 1,
            Self::Blocked => 2,
            Self::File => 3,
        }
    }
}

pub struct RulesScreen {
    paths: Paths,
    pub projects: Vec<String>,
    pub project_idx: usize,
    pub tab: RuleTab,
    pub timed: Vec<TimedRule>,
    pub always: Vec<PatternRule>,
    pub blocked: Vec<PatternRule>,
    pub file_rules: Vec<FileRule>,
    pub selected: usize,
}

impl RulesScreen {
    pub fn load(paths: &Paths) -> Result<Self> {
        let store = ProjectStore::new(paths.clone());
        let projects = store.list().unwrap_or_default();
        let mut s = Self {
            paths: paths.clone(),
            projects,
            project_idx: 0,
            tab: RuleTab::Timed,
            timed: vec![],
            always: vec![],
            blocked: vec![],
            file_rules: vec![],
            selected: 0,
        };
        s.reload()?;
        Ok(s)
    }

    pub fn empty(paths: &Paths) -> Self {
        Self {
            paths: paths.clone(),
            projects: vec![],
            project_idx: 0,
            tab: RuleTab::Timed,
            timed: vec![],
            always: vec![],
            blocked: vec![],
            file_rules: vec![],
            selected: 0,
        }
    }

    pub fn reload(&mut self) -> Result<()> {
        if let Some(project) = self.projects.get(self.project_idx) {
            self.timed = TimedStore::new(&self.paths)
                .list_active(project)
                .unwrap_or_default();
            self.always = AlwaysStore::new(&self.paths)
                .list(project)
                .unwrap_or_default();
            self.blocked = BlockedStore::new(&self.paths)
                .list(project)
                .unwrap_or_default();
            self.file_rules = ProjectStore::new(self.paths.clone())
                .load(project)
                .map(|p| p.policy.file_rules)
                .unwrap_or_default();
        } else {
            self.timed.clear();
            self.always.clear();
            self.blocked.clear();
            self.file_rules.clear();
        }
        let len = self.current_len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(len - 1);
        }
        Ok(())
    }

    pub fn switch_tab(&mut self, tab: RuleTab) {
        self.tab = tab;
        self.selected = 0;
    }

    pub fn cycle_tab(&mut self, delta: i32) {
        let new_idx = ((self.tab.idx() as i32 + delta).rem_euclid(4)) as usize;
        let tab = match new_idx {
            0 => RuleTab::Timed,
            1 => RuleTab::Always,
            2 => RuleTab::Blocked,
            _ => RuleTab::File,
        };
        self.switch_tab(tab);
    }

    pub fn move_project(&mut self, delta: i32) {
        if self.projects.is_empty() {
            return;
        }
        let len = self.projects.len() as i32;
        self.project_idx = (self.project_idx as i32 + delta).rem_euclid(len) as usize;
        let _ = self.reload();
    }

    pub fn move_selection(&mut self, delta: i32) {
        let len = self.current_len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected as i32 + delta).rem_euclid(len as i32) as usize;
    }

    pub fn apply_delete(&mut self) -> Result<()> {
        let project = self
            .projects
            .get(self.project_idx)
            .ok_or_else(|| Error::Usage("no project selected".into()))?
            .clone();
        // SAFETY-INVARIANT-12: every store::remove call below holds the
        // storage crate's advisory lock and writes atomically; the TUI
        // never bypasses that path.
        match self.tab {
            RuleTab::Timed => {
                if let Some(r) = self.timed.get(self.selected) {
                    TimedStore::new(&self.paths).remove(&project, &r.pattern.rule_id)?;
                }
            }
            RuleTab::Always => {
                if let Some(r) = self.always.get(self.selected) {
                    AlwaysStore::new(&self.paths).remove(&project, &r.rule_id)?;
                }
            }
            RuleTab::Blocked => {
                if let Some(r) = self.blocked.get(self.selected) {
                    BlockedStore::new(&self.paths).remove(&project, &r.rule_id)?;
                }
            }
            RuleTab::File => {
                // SAFETY-INVARIANT-12: mutate project in-memory then persist
                // atomically via ProjectStore::save, which uses tempfile +
                // persist() under the hood.
                if self.selected < self.file_rules.len() {
                    let store = ProjectStore::new(self.paths.clone());
                    let mut proj = store.load(&project)?;
                    proj.policy.file_rules.remove(self.selected);
                    store.save(&proj)?;
                }
            }
        }
        self.reload()
    }

    fn current_len(&self) -> usize {
        match self.tab {
            RuleTab::Timed => self.timed.len(),
            RuleTab::Always => self.always.len(),
            RuleTab::Blocked => self.blocked.len(),
            RuleTab::File => self.file_rules.len(),
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

        // Row 1: project selector.
        let proj_label = if self.projects.is_empty() {
            "(no projects)".to_string()
        } else {
            format!(
                "< {} >  ({}/{})",
                self.projects[self.project_idx],
                self.project_idx + 1,
                self.projects.len()
            )
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("project: ", theme::dim()),
                Span::styled(proj_label, theme::title()),
            ])),
            layout[0],
        );

        // Row 2: tabs.
        let titles: Vec<Line> = [RuleTab::Timed, RuleTab::Always, RuleTab::Blocked, RuleTab::File]
            .iter()
            .map(|t| Line::raw(t.label()))
            .collect();
        let tabs = Tabs::new(titles)
            .block(Block::default().borders(Borders::ALL))
            .select(self.tab.idx())
            .highlight_style(theme::title());
        frame.render_widget(tabs, layout[1]);

        // Row 3: rules table for the active tab.
        if self.projects.is_empty() {
            frame.render_widget(
                Paragraph::new("No projects. Add one with `safessh project add`.")
                    .style(theme::dim()),
                layout[2],
            );
            return;
        }
        let project = &self.projects[self.project_idx];
        let (rows, header_cols, body_empty_msg): (Vec<Row>, Vec<&str>, String) = match self.tab {
            RuleTab::Timed => (
                self.timed
                    .iter()
                    .map(|r| {
                        let mins = (r.expires_at - chrono::Utc::now()).num_minutes().max(0);
                        Row::new(vec![
                            r.pattern.rule_id.clone(),
                            r.pattern.binary.clone(),
                            r.pattern.flags.join(" "),
                            r.pattern.categories.join(","),
                            format!("expires {mins}m"),
                        ])
                    })
                    .collect(),
                vec!["RULE", "BIN", "FLAGS", "CATEGORIES", "EXPIRES"],
                format!("No timed rules for {project}."),
            ),
            RuleTab::Always => (
                self.always
                    .iter()
                    .map(|r| {
                        Row::new(vec![
                            r.rule_id.clone(),
                            r.binary.clone(),
                            r.flags.join(" "),
                            r.categories.join(","),
                            String::new(),
                        ])
                    })
                    .collect(),
                vec!["RULE", "BIN", "FLAGS", "CATEGORIES", ""],
                format!("No always rules for {project}."),
            ),
            RuleTab::Blocked => (
                self.blocked
                    .iter()
                    .map(|r| {
                        Row::new(vec![
                            r.rule_id.clone(),
                            r.binary.clone(),
                            r.flags.join(" "),
                            r.categories.join(","),
                            String::new(),
                        ])
                    })
                    .collect(),
                vec!["RULE", "BIN", "FLAGS", "CATEGORIES", ""],
                format!("No blocked rules for {project}."),
            ),
            RuleTab::File => (
                self.file_rules
                    .iter()
                    .enumerate()
                    .map(|(idx, r)| {
                        Row::new(vec![
                            idx.to_string(),
                            r.category.clone(),
                            r.paths.join(" "),
                            format!("{:?}", r.decision),
                        ])
                    })
                    .collect(),
                vec!["IDX", "CATEGORY", "PATHS", "DECISION"],
                format!("No file rules for {project}."),
            ),
        };

        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new(body_empty_msg).style(theme::dim()),
                layout[2],
            );
            return;
        }

        let widths: Vec<Constraint> = if self.tab == RuleTab::File {
            vec![
                Constraint::Length(4),
                Constraint::Length(20),
                Constraint::Min(20),
                Constraint::Length(10),
            ]
        } else {
            vec![
                Constraint::Length(20),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Min(20),
                Constraint::Length(14),
            ]
        };
        let mut state = TableState::default();
        state.select(Some(self.selected));
        let table = Table::new(rows, widths)
            .header(Row::new(header_cols).style(theme::title()))
            .block(
                Block::default()
                    .title(format!("{} rules", self.tab.label()))
                    .borders(Borders::ALL),
            )
            .highlight_style(theme::title())
            .highlight_symbol("> ");
        frame.render_stateful_widget(table, layout[2], &mut state);
    }
}
