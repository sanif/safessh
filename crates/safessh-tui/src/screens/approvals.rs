//! Approvals screen — pending queue + 5-action picker.
//!
//! All store mutations go through the same `safessh_storage::approvals`
//! API the CLI uses, so atomic + locked write semantics carry over
//! (SAFETY-INVARIANT-12).

use crate::theme;
use chrono::{Duration, Utc};
use ratatui::{
    layout::{Constraint, Rect},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
    },
    Frame,
};
use safessh_core::error::{Error, Result};
use safessh_core::types::ApprovalToken;
use safessh_storage::approvals::{
    AlwaysStore, BlockedStore, PatternRule, PendingRequest, PendingStore, TimedRule, TimedStore,
};
use safessh_storage::paths::Paths;

#[derive(Debug, Clone, Copy)]
pub enum PickerAction {
    Once,
    Timed(u32),
    Always,
    Deny,
    Block,
}

pub struct ApprovalsScreen {
    paths: Paths,
    pub pending: Vec<PendingRequest>,
    pub selected: usize,
    pub picker_open: bool,
    pub picker_idx: usize,
    pub timed_default_minutes: u32,
}

impl ApprovalsScreen {
    pub fn load(paths: &Paths) -> Result<Self> {
        let pending = list_pending(paths)?;
        Ok(Self {
            paths: paths.clone(),
            pending,
            selected: 0,
            picker_open: false,
            picker_idx: 0,
            timed_default_minutes: 30,
        })
    }

    pub fn empty(paths: &Paths) -> Self {
        Self {
            paths: paths.clone(),
            pending: vec![],
            selected: 0,
            picker_open: false,
            picker_idx: 0,
            timed_default_minutes: 30,
        }
    }

    pub fn reload(&mut self) -> Result<()> {
        self.pending = list_pending(&self.paths)?;
        if self.pending.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.pending.len() - 1);
        }
        Ok(())
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.pending.is_empty() {
            return;
        }
        let len = self.pending.len() as i32;
        self.selected = (self.selected as i32 + delta).rem_euclid(len) as usize;
    }

    pub fn open_picker(&mut self) {
        if !self.pending.is_empty() {
            self.picker_open = true;
            self.picker_idx = 0;
        }
    }

    pub fn close_picker(&mut self) {
        self.picker_open = false;
    }

    pub fn picker_move(&mut self, delta: i32) {
        self.picker_idx = (self.picker_idx as i32 + delta).rem_euclid(5) as usize;
    }

    pub fn picker_action(&self) -> PickerAction {
        match self.picker_idx {
            0 => PickerAction::Once,
            1 => PickerAction::Timed(self.timed_default_minutes),
            2 => PickerAction::Always,
            3 => PickerAction::Deny,
            _ => PickerAction::Block,
        }
    }

    pub fn apply_to_selected(&mut self, action: PickerAction) -> Result<()> {
        let req = self
            .pending
            .get(self.selected)
            .ok_or_else(|| Error::Usage("nothing selected".into()))?
            .clone();
        // SAFETY-INVARIANT-12: every store write below holds the storage
        // crate's advisory lock and writes atomically; the TUI never bypasses
        // the API the CLI uses.
        let pending = PendingStore::new(&self.paths);
        let _ = pending.take(&ApprovalToken::from_str(&req.token));

        let pattern = PatternRule {
            rule_id: format!("rule-{}", Utc::now().timestamp_millis()),
            binary: req.parsed.binary.clone(),
            flags: req.parsed.flags.clone(),
            args_pattern: None,
            categories: req.categories.clone(),
            created_at: Utc::now(),
        };
        match action {
            PickerAction::Once => {}
            PickerAction::Timed(min) => {
                TimedStore::new(&self.paths).add(
                    &req.project,
                    TimedRule {
                        pattern,
                        expires_at: Utc::now() + Duration::minutes(min as i64),
                    },
                )?;
            }
            PickerAction::Always => {
                AlwaysStore::new(&self.paths).add(&req.project, pattern)?;
            }
            PickerAction::Deny => {
                // Removing pending IS deny-once.
            }
            PickerAction::Block => {
                BlockedStore::new(&self.paths).add(&req.project, pattern)?;
            }
        }
        self.picker_open = false;
        self.reload()?;
        Ok(())
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        if self.pending.is_empty() {
            frame.render_widget(
                Paragraph::new("No pending approvals.").style(theme::dim()),
                area,
            );
            return;
        }
        let rows: Vec<Row> = self
            .pending
            .iter()
            .map(|r| {
                let age = Utc::now() - r.created_at;
                let age_str = if age.num_seconds() < 60 {
                    "now".to_string()
                } else if age.num_minutes() < 60 {
                    format!("{}m ago", age.num_minutes())
                } else {
                    format!("{}h ago", age.num_hours())
                };
                Row::new(vec![
                    r.token.clone(),
                    r.project.clone(),
                    r.parsed.binary.clone(),
                    r.categories.join(","),
                    age_str,
                ])
            })
            .collect();
        let widths = [
            Constraint::Length(8),
            Constraint::Length(15),
            Constraint::Length(10),
            Constraint::Min(20),
            Constraint::Length(10),
        ];
        let mut state = TableState::default();
        state.select(Some(self.selected));
        let table = Table::new(rows, widths)
            .header(
                Row::new(vec!["TOKEN", "PROJECT", "BIN", "CATEGORIES", "AGE"])
                    .style(theme::title()),
            )
            .block(
                Block::default()
                    .title("Pending approvals")
                    .borders(Borders::ALL),
            )
            .highlight_style(theme::title())
            .highlight_symbol("> ");
        frame.render_stateful_widget(table, area, &mut state);

        if self.picker_open {
            let popup = centered_rect(50, 11, area);
            frame.render_widget(Clear, popup);
            let timed_label = format!("Timed — allow for {} minutes", self.timed_default_minutes);
            let labels: [&str; 5] = [
                "Once — allow this single retry",
                &timed_label,
                "Always — persist allow-rule for this pattern",
                "Deny — refuse this request",
                "Block — persist deny for this pattern",
            ];
            let items: Vec<ListItem> = labels
                .iter()
                .map(|l| ListItem::new(l.to_string()))
                .collect();
            let mut s = ListState::default();
            s.select(Some(self.picker_idx));
            let list = List::new(items)
                .block(
                    Block::default()
                        .title("Choose action  (Up/Down Enter Esc)")
                        .borders(Borders::ALL),
                )
                .highlight_style(theme::title())
                .highlight_symbol("> ");
            frame.render_stateful_widget(list, popup, &mut s);
        }
    }
}

fn list_pending(paths: &Paths) -> Result<Vec<PendingRequest>> {
    let dir = paths.approvals_dir().join("pending");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        if let Ok(raw) = std::fs::read_to_string(entry.path()) {
            if let Ok(req) = toml::from_str::<PendingRequest>(&raw) {
                out.push(req);
            }
        }
    }
    out.sort_by_key(|r| r.created_at);
    Ok(out)
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let pop_w = area.width * percent_x / 100;
    let x = area.x + (area.width.saturating_sub(pop_w)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: pop_w,
        height,
    }
}
