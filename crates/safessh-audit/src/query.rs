//! Structured query API over the SQLite audit index.
//!
// SAFETY-INVARIANT-4: this module is read-side only. Failures here must be
// recoverable by callers (CLI degrades to log-scan; TUI degrades to JSONL
// tail). Never invoked from any write path.

// Filled in by Task 3.
