//! `safessh audit query` — structured query over the SQLite-backed audit log.
//!
//! Falls back to a JSONL scan if the SQLite index is unavailable. (Body
//! filled in by Task 7; this task wires flags + parsing only.)

use crate::cli::AuditFormat;
use chrono::{DateTime, Duration, Utc};
use safessh_audit::query::Filters;
use safessh_core::error::{Error, Result};

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

    // Task 7 fills this in.
    let _ = filters;
    let _ = format;
    Ok(())
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
