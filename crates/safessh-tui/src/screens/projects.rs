//! Projects screen — left list, right detail pane.
//!
//! SAFETY-INVARIANT-12: reads always go through `ProjectStore`, never
//! direct file IO, so atomic + locked storage semantics are preserved.

use crate::theme;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use safessh_core::error::Result;
use safessh_storage::paths::Paths;
use safessh_storage::project::{Project, ProjectStore, Target};

pub struct ProjectsScreen {
    paths: Paths,
    pub names: Vec<String>,
    pub detail: Option<Project>,
    pub selected: usize,
}

impl ProjectsScreen {
    pub fn load(paths: &Paths) -> Result<Self> {
        let store = ProjectStore::new(paths.clone());
        let names = store.list().unwrap_or_default();
        let detail = names.first().and_then(|n| store.load(n).ok());
        Ok(Self {
            paths: paths.clone(),
            names,
            detail,
            selected: 0,
        })
    }

    pub fn empty(paths: &Paths) -> Self {
        Self {
            paths: paths.clone(),
            names: vec![],
            detail: None,
            selected: 0,
        }
    }

    pub fn reload(&mut self) -> Result<()> {
        let store = ProjectStore::new(self.paths.clone());
        self.names = store.list().unwrap_or_default();
        if self.names.is_empty() {
            self.selected = 0;
            self.detail = None;
        } else {
            self.selected = self.selected.min(self.names.len() - 1);
            self.detail = store.load(&self.names[self.selected]).ok();
        }
        Ok(())
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.names.is_empty() {
            return;
        }
        let len = self.names.len() as i32;
        let next = (self.selected as i32 + delta).rem_euclid(len) as usize;
        self.selected = next;
        let store = ProjectStore::new(self.paths.clone());
        self.detail = self
            .names
            .get(self.selected)
            .and_then(|n| store.load(n).ok());
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        if self.names.is_empty() {
            let msg = Paragraph::new(
                "No projects. Run `safessh project add <name> --alias <alias>` to add one.",
            )
            .style(theme::dim());
            frame.render_widget(msg, area);
            return;
        }
        let cols = Layout::horizontal([Constraint::Percentage(30), Constraint::Min(0)]).split(area);
        let items: Vec<ListItem> = self
            .names
            .iter()
            .map(|n| ListItem::new(n.clone()))
            .collect();
        let mut state = ListState::default();
        state.select(Some(self.selected));
        let list = List::new(items)
            .block(Block::default().title("Projects").borders(Borders::ALL))
            .highlight_style(theme::title())
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, cols[0], &mut state);

        let detail_block = Block::default().title("Detail").borders(Borders::ALL);
        let inner = detail_block.inner(cols[1]);
        frame.render_widget(detail_block, cols[1]);
        if let Some(p) = &self.detail {
            let mut lines: Vec<Line> = vec![
                Line::from(vec![
                    Span::styled("name: ", theme::dim()),
                    Span::raw(&p.name),
                ]),
                Line::from(vec![
                    Span::styled("default target: ", theme::dim()),
                    Span::raw(&p.default_target),
                ]),
                Line::raw(""),
                Line::styled("targets", theme::title()),
            ];
            for t in &p.targets {
                let marker = if t.name() == p.default_target {
                    " [default]"
                } else {
                    ""
                };
                let detail = match t {
                    Target::SshConfigAlias {
                        ssh_config_alias, ..
                    } => format!("alias={ssh_config_alias}"),
                    Target::Inline {
                        host, port, user, ..
                    } => format!("{user}@{host}:{port}"),
                };
                lines.push(Line::raw(format!("  {}{marker}  {}", t.name(), detail)));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled("policy", theme::title()));
            lines.push(Line::raw(format!(
                "  allow:            {}",
                p.policy.allow.join(", ")
            )));
            lines.push(Line::raw(format!(
                "  require_approval: {}",
                p.policy.require_approval.join(", ")
            )));
            lines.push(Line::raw(format!(
                "  deny:             {}",
                p.policy.deny.join(", ")
            )));
            frame.render_widget(Paragraph::new(lines), inner);
        }
    }
}
