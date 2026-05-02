//! Audit screen — live tail of `state/audit.log` with project / type /
//! grep filters.
//!
//! Tracks a byte offset into the log so `FsEvent::AuditAppended` only
//! reads the new tail, never the whole file. If the log shrinks
//! (truncation/rotation), falls back to a full reload.

use crate::theme;
use chrono::{DateTime, Utc};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Paragraph, Row, Table, TableState},
    Frame,
};
use safessh_core::error::{Error, Result};
use safessh_storage::paths::Paths;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

/// Cap the in-memory tail at 200 events; older history can be queried
/// via `safessh audit query`.
const TAIL_LIMIT: usize = 200;

#[derive(Clone, Debug)]
pub struct AuditRow {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub project: Option<String>,
    pub summary: String,
    pub raw_json: String,
}

#[derive(Default, Clone, Debug)]
pub struct Filters {
    pub project: Option<String>,
    pub event_type: Option<String>,
    pub grep: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EditField {
    Project,
    Type,
    Grep,
}

pub struct AuditScreen {
    paths: Paths,
    rows: Vec<AuditRow>,
    offset_bytes: u64,
    selected: usize,
    auto_scroll: bool,
    pub filters: Filters,
    pub editing: Option<EditField>,
    pub edit_buf: String,
}

impl AuditScreen {
    pub fn load(paths: &Paths) -> Result<Self> {
        let mut s = Self {
            paths: paths.clone(),
            rows: vec![],
            offset_bytes: 0,
            selected: 0,
            auto_scroll: true,
            filters: Filters::default(),
            editing: None,
            edit_buf: String::new(),
        };
        s.full_reload()?;
        Ok(s)
    }

    pub fn empty(paths: &Paths) -> Self {
        Self {
            paths: paths.clone(),
            rows: vec![],
            offset_bytes: 0,
            selected: 0,
            auto_scroll: true,
            filters: Filters::default(),
            editing: None,
            edit_buf: String::new(),
        }
    }

    /// Read the entire audit log from disk and keep only the trailing
    /// `TAIL_LIMIT` events. Sets `offset_bytes` to the file's current
    /// length so subsequent `append_tail` calls only see new bytes.
    pub fn full_reload(&mut self) -> Result<()> {
        let path = self.paths.audit_log();
        if !path.exists() {
            self.rows.clear();
            self.offset_bytes = 0;
            return Ok(());
        }
        let total_bytes = std::fs::metadata(&path).map_err(Error::Io)?.len();
        let file = File::open(&path).map_err(Error::Io)?;
        let reader = BufReader::new(file);
        let mut all: Vec<AuditRow> = Vec::new();
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            if let Some(row) = parse_row(&line) {
                all.push(row);
            }
        }
        let start = all.len().saturating_sub(TAIL_LIMIT);
        self.rows = all.split_off(start);
        self.offset_bytes = total_bytes;
        self.selected = self.filtered_indices().last().copied().unwrap_or(0);
        Ok(())
    }

    /// Read only the bytes appended since the last call. Falls back to
    /// `full_reload` if the file shrunk (rotation).
    pub fn append_tail(&mut self) -> Result<()> {
        let path = self.paths.audit_log();
        if !path.exists() {
            return Ok(());
        }
        let new_len = std::fs::metadata(&path).map_err(Error::Io)?.len();
        if new_len < self.offset_bytes {
            // Rotation / truncation — start over.
            return self.full_reload();
        }
        if new_len == self.offset_bytes {
            return Ok(());
        }
        let mut file = File::open(&path).map_err(Error::Io)?;
        file.seek(SeekFrom::Start(self.offset_bytes))
            .map_err(Error::Io)?;
        let mut tail = String::new();
        file.read_to_string(&mut tail).map_err(Error::Io)?;
        for line in tail.lines() {
            if let Some(row) = parse_row(line) {
                self.rows.push(row);
            }
        }
        if self.rows.len() > TAIL_LIMIT {
            let drop = self.rows.len() - TAIL_LIMIT;
            self.rows.drain(0..drop);
        }
        self.offset_bytes = new_len;
        if self.auto_scroll {
            self.selected = self.filtered_indices().last().copied().unwrap_or(0);
        }
        Ok(())
    }

    pub fn jump_top(&mut self) {
        self.auto_scroll = false;
        self.selected = 0;
    }

    pub fn jump_bottom(&mut self) {
        self.auto_scroll = true;
        self.selected = self.filtered_indices().last().copied().unwrap_or(0);
    }

    pub fn move_selection(&mut self, delta: i32) {
        let visible = self.filtered_indices();
        if visible.is_empty() {
            return;
        }
        let cur_pos = visible
            .iter()
            .position(|&i| i == self.selected)
            .unwrap_or(0) as i32;
        let next = (cur_pos + delta).rem_euclid(visible.len() as i32) as usize;
        self.selected = visible[next];
        self.auto_scroll = false;
    }

    pub fn begin_edit(&mut self, field: EditField) {
        self.editing = Some(field);
        self.edit_buf.clear();
    }

    pub fn finish_edit(&mut self) {
        if let Some(field) = self.editing.take() {
            let value = if self.edit_buf.is_empty() {
                None
            } else {
                Some(self.edit_buf.clone())
            };
            match field {
                EditField::Project => self.filters.project = value,
                EditField::Type => self.filters.event_type = value,
                EditField::Grep => self.filters.grep = value,
            }
        }
        self.edit_buf.clear();
    }

    pub fn cancel_edit(&mut self) {
        self.editing = None;
        self.edit_buf.clear();
    }

    pub fn push_edit_char(&mut self, c: char) {
        if self.editing.is_some() {
            self.edit_buf.push(c);
        }
    }

    pub fn pop_edit_char(&mut self) {
        if self.editing.is_some() {
            self.edit_buf.pop();
        }
    }

    pub fn rows(&self) -> &[AuditRow] {
        &self.rows
    }

    pub fn filtered_rows(&self) -> Vec<&AuditRow> {
        self.filtered_indices()
            .into_iter()
            .map(|i| &self.rows[i])
            .collect()
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let f = &self.filters;
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                if let Some(p) = &f.project {
                    if r.project.as_deref() != Some(p.as_str()) {
                        return false;
                    }
                }
                if let Some(t) = &f.event_type {
                    if r.event_type != *t {
                        return false;
                    }
                }
                if let Some(g) = &f.grep {
                    if !r.raw_json.contains(g) {
                        return false;
                    }
                }
                true
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let layout = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);

        let filter_label = format!(
            "filters:  /p={}  /t={}  /g={}{}",
            self.filters.project.as_deref().unwrap_or("-"),
            self.filters.event_type.as_deref().unwrap_or("-"),
            self.filters.grep.as_deref().unwrap_or("-"),
            match self.editing {
                Some(EditField::Project) => format!("    editing /p: {}_", self.edit_buf),
                Some(EditField::Type) => format!("    editing /t: {}_", self.edit_buf),
                Some(EditField::Grep) => format!("    editing /g: {}_", self.edit_buf),
                None => String::new(),
            }
        );
        frame.render_widget(
            Paragraph::new(Line::raw(filter_label)).style(theme::dim()),
            layout[0],
        );

        let visible = self.filtered_rows();
        if visible.is_empty() {
            let msg = if self.rows.is_empty() {
                "No audit events yet."
            } else {
                "No events match current filters."
            };
            frame.render_widget(Paragraph::new(msg).style(theme::dim()), layout[1]);
            return;
        }

        let rows: Vec<Row> = visible
            .iter()
            .map(|r| {
                Row::new(vec![
                    r.timestamp.format("%H:%M:%S").to_string(),
                    r.event_type.clone(),
                    r.project.clone().unwrap_or_default(),
                    r.summary.clone(),
                ])
            })
            .collect();
        let widths = [
            Constraint::Length(8),
            Constraint::Length(20),
            Constraint::Length(15),
            Constraint::Min(20),
        ];
        let mut state = TableState::default();
        let visible_indices = self.filtered_indices();
        if let Some(p) = visible_indices.iter().position(|&i| i == self.selected) {
            state.select(Some(p));
        }
        let table = Table::new(rows, widths)
            .header(Row::new(vec!["TIME", "TYPE", "PROJECT", "SUMMARY"]).style(theme::title()))
            .block(
                Block::default()
                    .title(format!(
                        "Audit  ({} events{})",
                        visible.len(),
                        if self.auto_scroll { ", live" } else { "" }
                    ))
                    .borders(Borders::ALL),
            )
            .highlight_style(theme::title())
            .highlight_symbol("> ");
        frame.render_stateful_widget(table, layout[1], &mut state);
    }
}

fn parse_row(line: &str) -> Option<AuditRow> {
    let v: Value = serde_json::from_str(line).ok()?;
    let timestamp = v.get("timestamp").and_then(|t| t.as_str())?;
    let timestamp = DateTime::parse_from_rfc3339(timestamp)
        .ok()?
        .with_timezone(&Utc);
    let event_type = v
        .get("event_type")
        .and_then(|t| t.as_str())
        .unwrap_or("?")
        .to_string();
    let project = v.get("project").and_then(|p| p.as_str()).map(String::from);
    let summary = summarize(&event_type, &v);
    Some(AuditRow {
        timestamp,
        event_type,
        project,
        summary,
        raw_json: line.to_string(),
    })
}

fn summarize(event_type: &str, v: &Value) -> String {
    let data = v.get("data");
    match event_type {
        "exec_attempt" => data
            .and_then(|d| d.get("binary"))
            .and_then(|b| b.as_str())
            .map(|b| b.to_string())
            .unwrap_or_default(),
        "exec_complete" => data
            .and_then(|d| d.get("exit_code"))
            .map(|e| format!("exit={e}"))
            .unwrap_or_default(),
        "approval_requested" => data
            .and_then(|d| d.get("token"))
            .and_then(|t| t.as_str())
            .map(|t| format!("token={t}"))
            .unwrap_or_default(),
        "yolo_invocation" => data
            .and_then(|d| d.get("raw"))
            .and_then(|r| r.as_str())
            .unwrap_or_default()
            .chars()
            .take(40)
            .collect(),
        "file_read" => data
            .and_then(|d| d.get("path"))
            .and_then(|p| p.as_str())
            .map(|p| format!("file_read {p}"))
            .unwrap_or_else(|| "file_read".to_string()),
        "file_write" => data
            .and_then(|d| d.get("path"))
            .and_then(|p| p.as_str())
            .map(|p| format!("file_write {p}"))
            .unwrap_or_else(|| "file_write".to_string()),
        "file_read_complete" => {
            let path = data
                .and_then(|d| d.get("path"))
                .and_then(|p| p.as_str())
                .unwrap_or("");
            let bytes = data
                .and_then(|d| d.get("bytes_returned"))
                .and_then(|b| b.as_u64())
                .unwrap_or(0);
            let truncated = data
                .and_then(|d| d.get("truncated"))
                .and_then(|t| t.as_bool())
                .unwrap_or(false);
            let fmt_bytes = format_bytes(bytes);
            if truncated {
                format!("file_read {path} ({fmt_bytes}, truncated)")
            } else {
                format!("file_read {path} ({fmt_bytes})")
            }
        }
        "file_write_complete" => {
            let path = data
                .and_then(|d| d.get("path"))
                .and_then(|p| p.as_str())
                .unwrap_or("");
            let bytes = data
                .and_then(|d| d.get("bytes_written"))
                .and_then(|b| b.as_u64())
                .unwrap_or(0);
            let truncated = data
                .and_then(|d| d.get("truncated"))
                .and_then(|t| t.as_bool())
                .unwrap_or(false);
            let fmt_bytes = format_bytes(bytes);
            if truncated {
                format!("file_write {path} ({fmt_bytes}, truncated)")
            } else {
                format!("file_write {path} ({fmt_bytes})")
            }
        }
        _ => String::new(),
    }
}

fn format_bytes(b: u64) -> String {
    if b < 1024 {
        format!("{b}B")
    } else if b < 1024 * 1024 {
        format!("{:.1}KB", b as f64 / 1024.0)
    } else {
        format!("{:.1}MB", b as f64 / 1024.0 / 1024.0)
    }
}
