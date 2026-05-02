//! Projects screen — left list, right detail pane.
//!
//! SAFETY-INVARIANT-12: reads always go through `ProjectStore`, never
//! direct file IO, so atomic + locked storage semantics are preserved.

use crate::theme;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};
use safessh_core::error::Result;
use safessh_storage::paths::Paths;
use safessh_storage::project::{Approvals, OutputCaps, Policy, Project, ProjectStore, Target};
use safessh_storage::ssh_config::{SshAlias, SshConfigSnapshot};

pub struct ProjectsScreen {
    paths: Paths,
    pub names: Vec<String>,
    pub detail: Option<Project>,
    pub selected: usize,
    pub import: Option<ImportDialog>,
}

#[derive(Debug, Clone)]
pub struct ImportEntry {
    pub alias: SshAlias,
    pub checked: bool,
}

#[derive(Debug, Clone)]
pub struct ImportDialog {
    pub entries: Vec<ImportEntry>,
    pub cursor: usize,
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
            import: None,
        })
    }

    pub fn empty(paths: &Paths) -> Self {
        Self {
            paths: paths.clone(),
            names: vec![],
            detail: None,
            selected: 0,
            import: None,
        }
    }

    /// Open the import dialog, populating the alias list from the
    /// ssh-config snapshot.
    pub fn open_import(&mut self) -> Result<()> {
        let snap = SshConfigSnapshot::load(&self.paths)?;
        let entries = snap
            .aliases
            .into_iter()
            .map(|alias| ImportEntry {
                alias,
                checked: false,
            })
            .collect();
        self.import = Some(ImportDialog { entries, cursor: 0 });
        Ok(())
    }

    pub fn close_import(&mut self) {
        self.import = None;
    }

    pub fn import_move(&mut self, delta: i32) {
        if let Some(dlg) = self.import.as_mut() {
            if dlg.entries.is_empty() {
                return;
            }
            let len = dlg.entries.len() as i32;
            dlg.cursor = (dlg.cursor as i32 + delta).rem_euclid(len) as usize;
        }
    }

    pub fn import_toggle(&mut self) {
        if let Some(dlg) = self.import.as_mut() {
            if let Some(e) = dlg.entries.get_mut(dlg.cursor) {
                e.checked = !e.checked;
            }
        }
    }

    /// Materialize a project per checked alias, skipping any whose
    /// alias clashes with an existing project name. Returns the number
    /// of projects created.
    pub fn import_commit(&mut self) -> Result<usize> {
        let Some(dlg) = self.import.take() else {
            return Ok(0);
        };
        let store = ProjectStore::new(self.paths.clone());
        let existing: std::collections::HashSet<String> =
            store.list().unwrap_or_default().into_iter().collect();
        let mut created = 0;
        for entry in dlg.entries.into_iter().filter(|e| e.checked) {
            if existing.contains(&entry.alias.alias) {
                continue;
            }
            let target = Target::Inline {
                name: "default".into(),
                host: entry
                    .alias
                    .hostname
                    .clone()
                    .unwrap_or_else(|| entry.alias.alias.clone()),
                port: entry.alias.port.unwrap_or(22),
                user: entry
                    .alias
                    .user
                    .clone()
                    .unwrap_or_else(|| std::env::var("USER").unwrap_or_default()),
                identity_file: entry.alias.identity_file.clone(),
                proxy_jump: None,
                keychain_secret: None,
            };
            store.save(&Project {
                name: entry.alias.alias.clone(),
                default_target: "default".into(),
                targets: vec![target],
                policy: Policy {
                    allow: vec!["read:safe".into(), "file:read".into()],
                    require_approval: vec![],
                    deny: vec![],
                    file_rules: vec![],
                },
                approvals: Approvals::default(),
                output: OutputCaps::default(),
            })?;
            created += 1;
        }
        self.reload()?;
        Ok(created)
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

        if let Some(dlg) = &self.import {
            render_import(frame, area, dlg);
        }
    }
}

fn render_import(frame: &mut Frame<'_>, area: Rect, dlg: &ImportDialog) {
    let popup = centered_rect(80, 80, area);
    frame.render_widget(Clear, popup);
    if dlg.entries.is_empty() {
        let block = Block::default()
            .title("Import from ~/.ssh/config  (Esc cancel)")
            .borders(Borders::ALL);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        frame.render_widget(
            Paragraph::new(
                "No aliases found in ~/.ssh/config. \
                 Set $SSH_CONFIG_PATH or add a Host entry first.",
            )
            .style(theme::dim()),
            inner,
        );
        return;
    }
    let items: Vec<ListItem> = dlg
        .entries
        .iter()
        .map(|e| {
            let mark = if e.checked { "[x]" } else { "[ ]" };
            let host = e.alias.hostname.as_deref().unwrap_or(&e.alias.alias);
            let user = e.alias.user.as_deref().unwrap_or("(default)");
            ListItem::new(format!("{mark} {}  ->  {user}@{host}", e.alias.alias))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(dlg.cursor));
    let list = List::new(items)
        .block(
            Block::default()
                .title("Import from ~/.ssh/config  (Space toggle, Enter create, Esc cancel)")
                .borders(Borders::ALL),
        )
        .highlight_style(theme::title())
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, popup, &mut state);
}

fn centered_rect(percent_w: u16, percent_h: u16, area: Rect) -> Rect {
    let w = (area.width * percent_w / 100).min(area.width);
    let h = (area.height * percent_h / 100).min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}
