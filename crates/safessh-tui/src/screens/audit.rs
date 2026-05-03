//! Audit screen — windowed view backed by the SQLite index, with a
//! JSONL-tail fallback when SQLite is unavailable.
//!
//! On open and on `FsEvent::AuditAppended` the screen runs
//! `Index::catch_up + query::query` to populate `rows`. When the index
//! cannot be opened or queried, the screen flips into `fallback_mode`
//! and tails the JSONL log directly (the original behavior).
//!
// SAFETY-INVARIANT-4: the SQLite index is read-side only — failures here
// degrade to JSONL tail. The TUI never blocks the audit-write path.

use crate::theme;
use chrono::{DateTime, Utc};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Paragraph, Row, Table, TableState},
    Frame,
};
use safessh_audit::{
    query::{self, Filters as QueryFilters},
    sqlite::Index,
};
use safessh_core::error::{Error, Result};
use safessh_storage::paths::Paths;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

/// Cap the displayed window at 200 events; older history can be queried
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
    pub since: Option<String>,
    pub until: Option<String>,
    pub decision: Option<String>,
    /// Raw text the user typed; parsed at query time as `N` or `N..M`.
    pub exit_code: Option<String>,
    pub target: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EditField {
    Project,
    Type,
    Grep,
    Since,
    Until,
    Decision,
    ExitCode,
    Target,
}

pub struct AuditScreen {
    paths: Paths,
    rows: Vec<AuditRow>,
    /// Total event count (across the entire indexed log) for the title.
    total_count: usize,
    /// JSONL-fallback offset; only used when `fallback_mode` is true.
    offset_bytes: u64,
    /// `true` when SQLite is unusable; we tail JSONL directly instead.
    fallback_mode: bool,
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
            total_count: 0,
            offset_bytes: 0,
            fallback_mode: false,
            selected: 0,
            auto_scroll: true,
            filters: Filters::default(),
            editing: None,
            edit_buf: String::new(),
        };
        // Best-effort refresh on open. If SQLite errors, refresh()
        // flips to fallback_mode and runs the JSONL-tail path.
        let _ = s.refresh();
        Ok(s)
    }

    pub fn empty(paths: &Paths) -> Self {
        Self {
            paths: paths.clone(),
            rows: vec![],
            total_count: 0,
            offset_bytes: 0,
            fallback_mode: false,
            selected: 0,
            auto_scroll: true,
            filters: Filters::default(),
            editing: None,
            edit_buf: String::new(),
        }
    }

    /// Re-run the catch-up + query against SQLite and replace `rows`.
    /// On failure, flips to fallback_mode and reads JSONL directly.
    fn refresh(&mut self) -> Result<()> {
        if self.fallback_mode {
            return self.full_reload();
        }
        match self.refresh_sqlite() {
            Ok(()) => Ok(()),
            Err(_) => {
                self.fallback_mode = true;
                self.full_reload()
            }
        }
    }

    fn refresh_sqlite(&mut self) -> Result<()> {
        let mut idx = Index::open_or_create(&self.paths)?;
        idx.catch_up()?;

        // Query with current filters AND a TAIL_LIMIT cap so the screen
        // shows at most the most recent N events.
        let qf = self.build_query_filters();
        let rows = query::query(&mut idx, &qf)?;

        // For the title we want the unfiltered total, capped to a count.
        // Reuse the same Index by running a count(*) directly.
        let total: i64 = idx
            .conn()
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap_or(0);
        self.total_count = total.max(0) as usize;

        // SQLite returns DESC by timestamp; UI displays oldest→newest so
        // selection-at-bottom = most recent.
        let mut mapped: Vec<AuditRow> = rows.into_iter().map(row_from_query).collect();
        mapped.reverse();
        self.rows = mapped;
        if self.auto_scroll {
            self.selected = self.filtered_indices().last().copied().unwrap_or(0);
        }
        Ok(())
    }

    fn build_query_filters(&self) -> QueryFilters {
        QueryFilters {
            project: self.filters.project.clone(),
            event_type: self.filters.event_type.clone(),
            target: self.filters.target.clone(),
            decision: self.filters.decision.clone(),
            exit_code: parse_exit_code(self.filters.exit_code.as_deref()),
            since: self.filters.since.clone(),
            until: self.filters.until.clone(),
            grep: self.filters.grep.clone(),
            limit: TAIL_LIMIT as i64,
        }
    }

    /// Read the entire audit log from disk and keep only the trailing
    /// `TAIL_LIMIT` events. Sets `offset_bytes` to the file's current
    /// length. Used as the JSONL-tail fallback.
    pub fn full_reload(&mut self) -> Result<()> {
        let path = self.paths.audit_log();
        if !path.exists() {
            self.rows.clear();
            self.offset_bytes = 0;
            self.total_count = 0;
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
        self.total_count = all.len();
        let start = all.len().saturating_sub(TAIL_LIMIT);
        self.rows = all.split_off(start);
        self.offset_bytes = total_bytes;
        if self.auto_scroll {
            self.selected = self.filtered_indices().last().copied().unwrap_or(0);
        }
        Ok(())
    }

    /// Refresh the screen after the audit log has been appended-to.
    /// In SQLite mode this re-runs catch_up + query. In fallback mode,
    /// it reads only the new tail bytes (or full-reloads on rotation).
    pub fn append_tail(&mut self) -> Result<()> {
        if !self.fallback_mode {
            // SQLite mode: re-run the query. catch_up handles
            // rotation/truncation by detecting fingerprint changes.
            return match self.refresh_sqlite() {
                Ok(()) => Ok(()),
                Err(_) => {
                    self.fallback_mode = true;
                    self.full_reload()
                }
            };
        }

        let path = self.paths.audit_log();
        if !path.exists() {
            self.rows.clear();
            self.offset_bytes = 0;
            self.total_count = 0;
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
                self.total_count += 1;
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
                EditField::Since => self.filters.since = value,
                EditField::Until => self.filters.until = value,
                EditField::Decision => self.filters.decision = value,
                EditField::ExitCode => self.filters.exit_code = value,
                EditField::Target => self.filters.target = value,
            }
        }
        self.edit_buf.clear();
        // Filter changed → re-run query so SQLite mode reflects it.
        // In fallback mode the in-memory filtering in filtered_indices
        // does the work; refresh is a no-op for that path beyond
        // re-reading the log, which is harmless.
        let _ = self.refresh();
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

    pub fn fallback_mode(&self) -> bool {
        self.fallback_mode
    }

    fn filtered_indices(&self) -> Vec<usize> {
        // In SQLite mode, query() already applied filters server-side.
        // In fallback mode, do the original in-memory filtering so the
        // existing filter tests continue to pass.
        if !self.fallback_mode {
            return (0..self.rows.len()).collect();
        }
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
                Some(EditField::Since) => format!("    editing /s: {}_", self.edit_buf),
                Some(EditField::Until) => format!("    editing /u: {}_", self.edit_buf),
                Some(EditField::Decision) => format!("    editing /d: {}_", self.edit_buf),
                Some(EditField::ExitCode) => format!("    editing /e: {}_", self.edit_buf),
                Some(EditField::Target) => format!("    editing /T: {}_", self.edit_buf),
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
        let title = if self.total_count > visible.len() {
            format!(
                "Audit  ({} shown / {} total{})",
                visible.len(),
                self.total_count,
                if self.auto_scroll { ", live" } else { "" }
            )
        } else {
            format!(
                "Audit  ({} events{})",
                visible.len(),
                if self.auto_scroll { ", live" } else { "" }
            )
        };
        let table = Table::new(rows, widths)
            .header(Row::new(vec!["TIME", "TYPE", "PROJECT", "SUMMARY"]).style(theme::title()))
            .block(Block::default().title(title).borders(Borders::ALL))
            .highlight_style(theme::title())
            .highlight_symbol("> ");
        frame.render_stateful_widget(table, layout[1], &mut state);
    }
}

/// Parse the exit-code text buffer as either `N` or `N..M` into the
/// (lo, hi) tuple expected by `query::Filters`. Returns `None` on
/// empty/invalid input.
fn parse_exit_code(s: Option<&str>) -> Option<(i64, i64)> {
    let s = s?.trim();
    if s.is_empty() {
        return None;
    }
    if let Some((lo, hi)) = s.split_once("..") {
        Some((lo.parse().ok()?, hi.parse().ok()?))
    } else {
        let n: i64 = s.parse().ok()?;
        Some((n, n))
    }
}

fn row_from_query(qr: query::Row) -> AuditRow {
    let v: Value = serde_json::from_str(&qr.raw_json).unwrap_or(Value::Null);
    let timestamp = DateTime::parse_from_rfc3339(&qr.timestamp)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let summary = summarize(&qr.event_type, &v);
    AuditRow {
        timestamp,
        event_type: qr.event_type,
        project: qr.project,
        summary,
        raw_json: qr.raw_json,
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
        "tunnel_open" => {
            let lp = data
                .and_then(|d| d.get("local_port"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let rh = data
                .and_then(|d| d.get("remote_host"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let rp = data
                .and_then(|d| d.get("remote_port"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            format!("tunnel localhost:{lp} → {rh}:{rp}  [opaque]")
        }
        "tunnel_close" => {
            let reason = data
                .and_then(|d| d.get("reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let dur = data
                .and_then(|d| d.get("duration_secs"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let dur_label = if dur >= 60 {
                format!("{} min", dur / 60)
            } else {
                format!("{dur}s")
            };
            format!("tunnel close {reason} ({dur_label})")
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

#[doc(hidden)]
pub fn summarize_for_test(event_type: &str, v: &serde_json::Value) -> String {
    summarize(event_type, v)
}
