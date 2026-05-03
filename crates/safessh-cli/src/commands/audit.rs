//! `safessh audit query` — structured query over the SQLite-backed audit log.
//!
//! Falls back to a JSONL scan if the SQLite index is unavailable.

use crate::cli::AuditFormat;
use chrono::{DateTime, Duration, Utc};
use safessh_audit::query::{query, Filters, Row};
use safessh_audit::sqlite::Index;
use safessh_core::error::{Error, Result};
use safessh_storage::paths::Paths;

#[allow(clippy::too_many_arguments)]
pub fn run(
    project: Option<String>,
    event_type: Option<String>,
    grep: Option<String>,
    since: Option<String>,
    until: Option<String>,
    limit: i64,
    decision: Option<String>,
    exit_code: Option<String>,
    target: Option<String>,
    format: AuditFormat,
) -> Result<()> {
    let since_ts = parse_when_opt(since.as_deref())?;
    let until_ts = parse_when_opt(until.as_deref())?;
    if let (Some(s), Some(u)) = (&since_ts, &until_ts) {
        if s > u {
            return Err(Error::Usage("--since must be earlier than --until".into()));
        }
    }

    let exit_range = parse_exit_code_opt(exit_code.as_deref())?;

    let filters = Filters {
        project,
        event_type,
        target,
        decision,
        exit_code: exit_range,
        since: since_ts,
        until: until_ts,
        grep,
        limit,
    };

    let paths = Paths::user().map_err(Error::Io)?;

    match try_sqlite(&paths, &filters) {
        Ok(rows) => emit(&rows, format),
        Err(_) => {
            eprintln!("safessh: warning: audit index unavailable, falling back to log scan");
            log_scan(&paths, &filters, format)
        }
    }
}

fn try_sqlite(paths: &Paths, f: &Filters) -> Result<Vec<Row>> {
    let mut idx = Index::open_or_create(paths)?;
    query(&mut idx, f)
}

fn emit(rows: &[Row], format: AuditFormat) -> Result<()> {
    match format {
        AuditFormat::Count => println!("{}", rows.len()),
        AuditFormat::Jsonl => {
            for r in rows {
                println!("{}", r.raw_json);
            }
        }
        AuditFormat::Table => print_table(rows),
    }
    Ok(())
}

fn print_table(rows: &[Row]) {
    println!(
        "{:<25} {:<22} {:<14} {:<10} {:<18} {:>9}",
        "timestamp", "event_type", "project", "target", "decision", "exit"
    );
    for r in rows {
        let exit = r.exit_code.map(|c| c.to_string()).unwrap_or_default();
        println!(
            "{:<25} {:<22} {:<14} {:<10} {:<18} {:>9}",
            short(&r.timestamp, 25),
            short(&r.event_type, 22),
            short(r.project.as_deref().unwrap_or(""), 14),
            short(r.target.as_deref().unwrap_or(""), 10),
            short(r.decision.as_deref().unwrap_or(""), 18),
            exit
        );
    }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn log_scan(paths: &Paths, f: &Filters, format: AuditFormat) -> Result<()> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let log_path = paths.audit_log();
    if !log_path.exists() {
        if matches!(format, AuditFormat::Count) {
            println!("0");
        }
        return Ok(());
    }
    let file = File::open(&log_path).map_err(Error::Io)?;
    let reader = BufReader::new(file);
    let mut matched = 0usize;
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        if !matches_filters_jsonl(&line, f) {
            continue;
        }
        if matches!(format, AuditFormat::Jsonl) {
            println!("{line}");
        }
        matched += 1;
        if f.limit > 0 && matched as i64 >= f.limit {
            break;
        }
    }
    if matches!(format, AuditFormat::Count) {
        println!("{matched}");
    }
    Ok(())
}

fn matches_filters_jsonl(line: &str, f: &Filters) -> bool {
    use serde_json::Value;
    if let Some(g) = &f.grep {
        if !line.contains(g) {
            return false;
        }
    }
    let v: Option<Value> = serde_json::from_str(line).ok();
    let Some(v) = v else {
        return f.project.is_none()
            && f.event_type.is_none()
            && f.target.is_none()
            && f.decision.is_none()
            && f.exit_code.is_none()
            && f.since.is_none()
            && f.until.is_none();
    };
    let str_field = |path: &[&str]| {
        let mut cur = &v;
        for k in path {
            cur = cur.get(*k)?;
        }
        cur.as_str().map(String::from)
    };
    let int_field = |path: &[&str]| {
        let mut cur = &v;
        for k in path {
            cur = cur.get(*k)?;
        }
        cur.as_i64()
    };
    if let Some(p) = &f.project {
        if str_field(&["project"]).as_deref() != Some(p.as_str()) {
            return false;
        }
    }
    if let Some(t) = &f.event_type {
        if str_field(&["event_type"]).as_deref() != Some(t.as_str()) {
            return false;
        }
    }
    if let Some(t) = &f.target {
        if str_field(&["data", "target"]).as_deref() != Some(t.as_str()) {
            return false;
        }
    }
    if let Some(d) = &f.decision {
        if str_field(&["data", "decision"]).as_deref() != Some(d.as_str()) {
            return false;
        }
    }
    if let Some((lo, hi)) = f.exit_code {
        let Some(c) = int_field(&["data", "exit_code"]) else {
            return false;
        };
        if c < lo || c > hi {
            return false;
        }
    }
    if let Some(s) = &f.since {
        if let Some(ts) = str_field(&["timestamp"]) {
            if ts.as_str() < s.as_str() {
                return false;
            }
        }
    }
    if let Some(u) = &f.until {
        if let Some(ts) = str_field(&["timestamp"]) {
            if ts.as_str() > u.as_str() {
                return false;
            }
        }
    }
    true
}

fn parse_when_opt(s: Option<&str>) -> Result<Option<String>> {
    let Some(s) = s else { return Ok(None) };
    if let Ok(d) = humantime::parse_duration(s) {
        let then = Utc::now() - Duration::from_std(d).unwrap_or_else(|_| Duration::zero());
        return Ok(Some(
            then.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ));
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(Some(
            dt.with_timezone(&Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ));
    }
    Err(Error::Usage(format!(
        "--since/--until: expected RFC3339 timestamp or duration like 7d/24h/30m, got {s}"
    )))
}

fn parse_exit_code_opt(s: Option<&str>) -> Result<Option<(i64, i64)>> {
    let Some(s) = s else { return Ok(None) };
    if let Some((lo, hi)) = s.split_once("..") {
        let lo = lo
            .parse::<i64>()
            .map_err(|_| Error::Usage(format!("--exit-code: expected N or N..M, got {s}")))?;
        let hi = hi
            .parse::<i64>()
            .map_err(|_| Error::Usage(format!("--exit-code: expected N or N..M, got {s}")))?;
        if lo > hi {
            return Err(Error::Usage(format!(
                "--exit-code: low ({lo}) > high ({hi})"
            )));
        }
        return Ok(Some((lo, hi)));
    }
    let n = s
        .parse::<i64>()
        .map_err(|_| Error::Usage(format!("--exit-code: expected N or N..M, got {s}")))?;
    Ok(Some((n, n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exit_code_single() {
        assert_eq!(parse_exit_code_opt(Some("0")).unwrap(), Some((0, 0)));
        assert_eq!(parse_exit_code_opt(Some("50")).unwrap(), Some((50, 50)));
    }

    #[test]
    fn parse_exit_code_range() {
        assert_eq!(parse_exit_code_opt(Some("1..255")).unwrap(), Some((1, 255)));
    }

    #[test]
    fn parse_exit_code_bad() {
        assert!(parse_exit_code_opt(Some("abc")).is_err());
        assert!(parse_exit_code_opt(Some("9..1")).is_err());
    }

    #[test]
    fn parse_when_duration_then_rfc3339_idempotent() {
        let p = parse_when_opt(Some("1h")).unwrap().unwrap();
        let p2 = parse_when_opt(Some(&p)).unwrap().unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn parse_when_bad() {
        assert!(parse_when_opt(Some("not-a-time")).is_err());
    }
}
