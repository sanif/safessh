//! Structured query API over the SQLite audit index.
//!
// SAFETY-INVARIANT-4: this module is read-side only. Failures here must be
// recoverable by callers (CLI degrades to log-scan; TUI degrades to JSONL
// tail). Never invoked from any write path.

use crate::sqlite::Index;
use rusqlite::types::ToSql;
use safessh_core::error::{Error, Result};

/// One row returned by a query — mirrors the indexed columns plus `raw_json`.
#[derive(Clone, Debug)]
pub struct Row {
    pub timestamp: String,
    pub event_type: String,
    pub project: Option<String>,
    pub target: Option<String>,
    pub decision: Option<String>,
    pub exit_code: Option<i64>,
    pub raw_json: String,
}

/// AND-combined filters.
#[derive(Clone, Debug, Default)]
pub struct Filters {
    pub project: Option<String>,
    pub event_type: Option<String>,
    pub target: Option<String>,
    pub decision: Option<String>,
    /// Inclusive (low, high). Pass `(N, N)` for an exact match.
    pub exit_code: Option<(i64, i64)>,
    /// RFC3339 lower bound (inclusive).
    pub since: Option<String>,
    /// RFC3339 upper bound (inclusive).
    pub until: Option<String>,
    /// Substring match against `raw_json` (no regex).
    pub grep: Option<String>,
    /// 0 means unlimited; default 100.
    pub limit: i64,
}

impl Filters {
    /// `Filters::default()` returns `limit: 0` (unlimited). Use this when you
    /// want the CLI's default page size.
    pub fn with_default_limit() -> Self {
        Self {
            limit: 100,
            ..Self::default()
        }
    }
}

pub fn query(idx: &mut Index, f: &Filters) -> Result<Vec<Row>> {
    idx.catch_up()?;

    let mut sql = String::from(
        "SELECT timestamp, event_type, project, target, decision, exit_code, raw_json \
         FROM events",
    );
    let mut conds: Vec<String> = vec![];
    let mut params: Vec<Box<dyn ToSql>> = vec![];

    if let Some(p) = &f.project {
        conds.push("project = ?".into());
        params.push(Box::new(p.clone()));
    }
    if let Some(t) = &f.event_type {
        conds.push("event_type = ?".into());
        params.push(Box::new(t.clone()));
    }
    if let Some(t) = &f.target {
        conds.push("target = ?".into());
        params.push(Box::new(t.clone()));
    }
    if let Some(d) = &f.decision {
        conds.push("decision = ?".into());
        params.push(Box::new(d.clone()));
    }
    if let Some((lo, hi)) = f.exit_code {
        conds.push("exit_code BETWEEN ? AND ?".into());
        params.push(Box::new(lo));
        params.push(Box::new(hi));
    }
    if let Some(s) = &f.since {
        conds.push("timestamp >= ?".into());
        params.push(Box::new(s.clone()));
    }
    if let Some(u) = &f.until {
        conds.push("timestamp <= ?".into());
        params.push(Box::new(u.clone()));
    }
    if let Some(g) = &f.grep {
        conds.push("raw_json LIKE ?".into());
        params.push(Box::new(format!("%{g}%")));
    }

    if !conds.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
    }
    sql.push_str(" ORDER BY timestamp DESC, id DESC");
    if f.limit > 0 {
        sql.push_str(&format!(" LIMIT {}", f.limit));
    }

    let conn = idx.conn();
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| Error::AuditIndexFailed(format!("prepare: {e}")))?;
    let param_refs: Vec<&dyn ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(param_refs.iter()), |r| {
            Ok(Row {
                timestamp: r.get(0)?,
                event_type: r.get(1)?,
                project: r.get(2)?,
                target: r.get(3)?,
                decision: r.get(4)?,
                exit_code: r.get(5)?,
                raw_json: r.get(6)?,
            })
        })
        .map_err(|e| Error::AuditIndexFailed(format!("query: {e}")))?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| Error::AuditIndexFailed(format!("row: {e}")))?);
    }
    Ok(out)
}
