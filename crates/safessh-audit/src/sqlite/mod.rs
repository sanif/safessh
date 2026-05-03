//! SQLite index over the JSONL audit log.
//!
// SAFETY-INVARIANT-4: this module is **read-side only**. It never participates
// in the audit-write path. Exec / file / tunnel paths write JSONL via
// `AuditWriter::append` and return without touching SQLite. If anything in
// here fails, the JSONL log is unaffected and callers must degrade to
// log-scan rather than blocking.

// Filled in by Task 1.
