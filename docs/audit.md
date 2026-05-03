# Audit log reference

Every gating decision and every completed operation is appended to a single JSONL file before any user-visible output (SAFETY-INVARIANT-4). v0.5 adds a SQLite index, lazily built from the JSONL, that powers `safessh audit query` and the TUI audit screen.

JSONL is the source of truth. SQLite is a derived cache — if it disappears, the next query rebuilds it. Anything in this doc that reads "indexed column" describes a query optimisation, not a data store.

## File locations

`safessh` follows the platform's standard XDG / `directories` layout. The state directory is:

| OS | Path |
|---|---|
| Linux | `~/.local/state/safessh/` |
| macOS | `~/Library/Application Support/safessh/` |
| Windows | `%LOCALAPPDATA%\safessh\safessh\` |

Inside it:

- `audit.log` — append-only JSONL. Created on first event, mode `0600` on Unix, fsynced after every record.
- `audit.log.<UTC-timestamp>` — rotated archives. When `audit.log` reaches 100 MiB, it is renamed to `audit.log.20260503T101530` (or similar) and a new file starts.
- `audit.db` — SQLite index. Created lazily by the first read-side caller (`audit query`, the TUI). Not present until then. Safe to delete — see [Recovery](#recovery).

The whole tree relocates if `SAFESSH_HOME` is set:

```sh
SAFESSH_HOME=/tmp/safessh-sandbox safessh audit query --format count
# reads /tmp/safessh-sandbox/state/audit.log
```

## JSONL schema

Each line is one JSON object. The line ordering is wall-clock-monotonic for events from a single process, but multiple `safessh` invocations may interleave by milliseconds.

### Top-level fields (every event)

| Field | Type | Notes |
|---|---|---|
| `schema_version` | u32 | Currently `1`. Bumps on a breaking change to the line format. |
| `timestamp` | string | RFC3339 UTC, e.g. `"2026-05-03T10:15:00Z"`. Filterable via `--since` / `--until`. |
| `event_type` | string | One of the values listed below. |
| `project` | string \| null | Project name, when known. |
| `data` | object | Event-specific payload. Schema varies — see per-type sections below. |
| `error_class` | string \| null | Stable error category when an event records a failure. Examples: `"audit_write_failed"`, `"approval_required"`, `"output_capped"`, `"yolo_refused"`. Source: `safessh_core::error::Error::error_class()`. |
| `error_message` | string \| null | Human-readable error detail. Redacted with the same rules as command output. |

### Event types

There are ten event types in v0.5. The schema is additive — older lines emitted before a field was introduced still parse cleanly.

#### `exec_attempt`

Emitted before the SSH driver starts a remote command. The audit append blocks the exec path: if it fails, the command is refused with exit 50.

`data` fields:

| Field | Type | Notes |
|---|---|---|
| `raw` | string | The raw command as supplied by the caller. |
| `binary` | string | The first AST token (e.g. `"ls"`, `"systemctl"`). |
| `flags` | array of string | Parsed flag tokens, in order. |
| `args` | array of string | Parsed positional tokens, in order. |
| `decision` | string | Always lowercase canonical: `"allow"` \| `"require_approval"` \| `"deny"` \| `"block"`. Query with `--decision allow` etc. |
| `target` | string \| absent | Resolved target name (additive in v0.5). Omitted on older lines. |

#### `exec_complete`

Emitted after the SSH subprocess exits, regardless of success.

| Field | Type | Notes |
|---|---|---|
| `exit_code` | i32 | Process exit status from the remote command. |
| `stdout_bytes` | u64 | Bytes of stdout captured (post-cap). |
| `stderr_bytes` | u64 | Bytes of stderr captured. |
| `duration_ms` | u64 | Wall-clock time from spawn to exit. |
| `target` | string \| absent | Same semantics as on `exec_attempt`. Additive in v0.5. |

#### `approval_requested`

Emitted when the policy engine returns `RequireApproval` and a pending-approval token is created.

| Field | Type | Notes |
|---|---|---|
| `token` | string | The token the LLM will surface in `BLOCKED:` output. Used to grant or revoke. |
| `categories` | array of string | Matched policy categories that triggered the gate (e.g. `["destructive:filesystem"]`). |
| `raw` | string | Raw command being approved. |

#### `yolo_invocation`

Emitted before any `--yolo` exec. `data.flagged` is always `true` so this event is grep-friendly.

| Field | Type | Notes |
|---|---|---|
| `raw` | string | Raw command. |
| `flagged` | bool | Always `true`. Useful for `--grep '"flagged":true'`. |

#### `file_read`

Emitted on each `safessh <project> read <path>` attempt, after the policy decision and before the sftp transfer.

| Field | Type | Notes |
|---|---|---|
| `path` | string | Canonical remote path (after `~` and `..` expansion + remote `realpath`). |
| `decision` | string | `"allow"` \| `"require_approval"` \| `"deny"` \| `"block"`. Lowercase here (file-rule style). |

#### `file_write`

Same shape as `file_read`. Emitted before the sftp upload begins.

| Field | Type | Notes |
|---|---|---|
| `path` | string | Canonical remote path. |
| `decision` | string | `"allow"` \| `"require_approval"` \| `"deny"` \| `"block"`. |

#### `file_read_complete`

Emitted after a successful sftp read.

| Field | Type | Notes |
|---|---|---|
| `target` | string | Resolved target name. |
| `path` | string | Canonical remote path. |
| `bytes_returned` | u64 | Bytes delivered to stdout (after the output cap, if any). |
| `sha256` | string | Hex SHA-256 of the bytes returned. |
| `truncated` | bool | `true` if the output cap fired. |
| `duration_ms` | u64 | Wall-clock time for the transfer. |

#### `file_write_complete`

Emitted after a successful sftp write.

| Field | Type | Notes |
|---|---|---|
| `target` | string | Resolved target name. |
| `path` | string | Canonical remote path. |
| `bytes_written` | u64 | Bytes pushed to the remote. |
| `sha256` | string | Hex SHA-256 of the bytes written. |
| `truncated` | bool | Reserved; always `false` for v0.5 writes. |
| `duration_ms` | u64 | Wall-clock time for the transfer. |

#### `tunnel_open`

Emitted when the supervisor starts a `ssh -L` subprocess.

| Field | Type | Notes |
|---|---|---|
| `id` | string | Tunnel ID (used by `tunnel_close` and `safessh tunnels`). |
| `target` | string | Resolved target. |
| `local_port` | u16 | Local listening port. |
| `remote_host` | string | Remote-side host the tunnel reaches. |
| `remote_port` | u16 | Remote-side port. |
| `expires_at` | string | RFC3339 UTC TTL deadline. SIGTERM fires at this time. |
| `opacity_warning` | string | Always `"tunnel traffic is opaque to safessh"` — written inline so anyone tailing the log sees it. |

#### `tunnel_close`

Emitted when the supervisor reaps the `ssh -L` subprocess.

| Field | Type | Notes |
|---|---|---|
| `id` | string | Same value as the matching `tunnel_open`. |
| `reason` | string | One of `"ttl"`, `"manual"`, `"crash"`, `"shutdown"`, `"unknown"`. |
| `duration_secs` | u64 | Wall-clock seconds between the matching `tunnel_open` and this close. |

### Examples

A two-event exec sequence and a successful read:

```jsonl
{"schema_version":1,"timestamp":"2026-05-03T10:15:00Z","event_type":"exec_attempt","project":"prod","data":{"raw":"ls /var/log","binary":"ls","flags":[],"args":["/var/log"],"decision":"allow","target":"web"},"error_class":null,"error_message":null}
{"schema_version":1,"timestamp":"2026-05-03T10:15:00Z","event_type":"exec_complete","project":"prod","data":{"exit_code":0,"stdout_bytes":482,"stderr_bytes":0,"duration_ms":143,"target":"web"},"error_class":null,"error_message":null}
{"schema_version":1,"timestamp":"2026-05-03T10:20:01Z","event_type":"file_read_complete","project":"prod","data":{"target":"web","path":"/etc/nginx/nginx.conf","bytes_returned":512,"sha256":"abcd1234","truncated":false,"duration_ms":45},"error_class":null,"error_message":null}
```

## SQLite index

The index is a denormalised cache of selected columns over the JSONL log. It exists to make `audit query` filters fast; it never replaces the JSONL.

### Schema (v1)

Verbatim from `crates/safessh-audit/src/sqlite/migrations/V1__initial.sql`:

```sql
CREATE TABLE IF NOT EXISTS events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    byte_offset     INTEGER NOT NULL,
    timestamp       TEXT    NOT NULL,
    event_type      TEXT    NOT NULL,
    project         TEXT,
    target          TEXT,
    decision        TEXT,
    exit_code       INTEGER,
    raw_json        TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_timestamp           ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_project_timestamp   ON events(project, timestamp);
CREATE INDEX IF NOT EXISTS idx_events_event_type_ts       ON events(event_type, timestamp);
CREATE INDEX IF NOT EXISTS idx_events_decision_timestamp  ON events(decision, timestamp);

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

### Indexed columns vs. `raw_json` lookup

Filter speed comes from the dedicated columns. Anything else falls back to `LIKE '%substring%'` on `raw_json`.

| Column | Source field in JSONL | Filter flag |
|---|---|---|
| `timestamp` | top-level `timestamp` | `--since`, `--until` |
| `event_type` | top-level `event_type` | `--type` |
| `project` | top-level `project` | `--project` |
| `target` | `data.target` | `--target` |
| `decision` | `data.decision` | `--decision` |
| `exit_code` | `data.exit_code` | `--exit-code` |
| `raw_json` | the full line | `--grep` (substring) |

`byte_offset` records where the line begins in `audit.log`; the catch-up loop uses it as a watermark. It's not exposed via `audit query`.

### `meta` table keys

| Key | Value |
|---|---|
| `schema_version` | `"1"` — bumps when the SQLite schema breaks compatibility. Older binaries refuse newer DBs (`audit_index_newer`); newer binaries upgrade older DBs by replaying migrations. |
| `last_indexed_offset` | Decimal byte offset in `audit.log` where the next catch-up should resume. |
| `source_file` | Absolute path of the indexed log. If `Paths::user()` resolves elsewhere on the next run (e.g. `SAFESSH_HOME` flipped), the index is wiped and rebuilt. |
| `log_fingerprint` | Hex of the first 256 bytes of `audit.log`. Catch-up compares this to the live log to detect rotation or truncation; on mismatch the index resets to offset 0. |

### Catch-up behaviour

Each call into the read path (`audit query`, TUI open, TUI live-tail) runs `Index::catch_up`, which:

1. Stats `audit.log`. If the file is missing, returns 0 inserted.
2. Reads the current 256-byte fingerprint.
3. Compares to `last_indexed_offset` and `log_fingerprint`. If the offset is past EOF, or the fingerprint changed, deletes all rows and resets the offset to 0 (rotation / truncation handling).
4. Streams from the offset to EOF, parsing each line. Unparseable lines are skipped silently — but the offset still advances so they aren't retried forever.
5. Commits one transaction with all inserts plus the new offset and fingerprint.

Rotated files (`audit.log.<ts>`) are not indexed. They are frozen history; query them with `--grep` over the live log only, or use `jq` directly against the rotated file.

## `safessh audit query`

Read-side CLI over the index. Defaults to the JSONL line format so the LLM (and `jq`) can parse output. Falls back to a raw log scan if the index can't be opened, with a one-line stderr warning.

```
safessh audit query [OPTIONS]
```

| Flag | Type | Default | Effect |
|---|---|---|---|
| `--project <name>` | string | — | Match `project = ?` exactly. |
| `--type <event_type>` | string | — | Match `event_type = ?` exactly (e.g. `exec_attempt`, `tunnel_open`). |
| `--target <name>` | string | — | Match `data.target = ?` exactly. |
| `--decision <value>` | string | — | Match `data.decision = ?` exactly. Always lowercase canonical: `allow`, `require_approval`, `deny`, `block` (uniform across exec and file events). |
| `--exit-code <N \| N..M>` | range | — | `42` matches exit 42; `1..255` matches a range. |
| `--since <when>` | RFC3339 or duration | — | Lower bound (inclusive). `2026-05-01T00:00:00Z` or `7d`, `24h`, `30m`. |
| `--until <when>` | RFC3339 or duration | — | Upper bound (inclusive). Same accepted forms as `--since`. |
| `--grep <pattern>` | string | — | Substring match against `raw_json`. No regex. |
| `--limit <N>` | i64 | `100` | Max rows returned. `0` means unlimited. |
| `--format <fmt>` | enum | `jsonl` | `jsonl` (one event per line, full row), `table` (human-readable columns), `count` (total only). |

Filters AND together. Results are sorted newest-first.

### Examples

Last 100 events (default behaviour, what the LLM gets):

```sh
safessh audit query
```

All events for the `prod` project in the past 24 hours, as a table:

```sh
safessh audit query --project prod --since 24h --format table
```

Every blocked or denied operation across all projects in the past week:

```sh
safessh audit query --since 7d --grep '"decision":"deny"'
```

How many `yolo_invocation` events have I emitted since April?

```sh
safessh audit query --type yolo_invocation --since 2026-04-01T00:00:00Z --format count
```

Failed exec results (any non-zero exit) on the `web` target since yesterday:

```sh
safessh audit query --type exec_complete --target web --exit-code 1..255 --since 24h
```

Pending approvals issued for the `staging` project, newest 20:

```sh
safessh audit query --project staging --type approval_requested --limit 20
```

Did anyone read `/etc/nginx/nginx.conf` in the past hour?

```sh
safessh audit query --type file_read_complete --grep nginx.conf --since 1h --format table
```

All tunnel opens against `db` between two specific timestamps:

```sh
safessh audit query \
  --type tunnel_open --target db \
  --since 2026-05-01T00:00:00Z \
  --until 2026-05-03T00:00:00Z \
  --format jsonl
```

How many file writes were `denied` last week?

```sh
safessh audit query \
  --type file_write --decision deny \
  --since 7d --format count
```

Unbounded export of every event for `prod` (use `--limit 0` carefully on large logs):

```sh
safessh audit query --project prod --limit 0 > prod-audit.jsonl
```

### Fallback behaviour

If `audit.db` can't be opened (e.g. permission error, disk full during catch-up), the CLI prints a single line to stderr:

```
safessh: warning: audit index unavailable, falling back to log scan
```

…and re-runs the same filter set as a JSONL scan. Results are the same; performance is O(log size). Exit status remains `0` (no rows is not an error).

## Recovery

The SQLite index is a derived cache. Anything wrong with it can be fixed by deleting it.

```sh
# Linux
rm ~/.local/state/safessh/audit.db
# macOS
rm "$HOME/Library/Application Support/safessh/audit.db"

# Sandbox / SAFESSH_HOME override
rm "$SAFESSH_HOME/state/audit.db"

# The next read rebuilds it from JSONL:
safessh audit query --format count
```

`audit.log` is unaffected. No events are lost; they are re-indexed in a single transaction the next time `audit query` or the TUI opens.

When to reach for this:

- `safessh: warning: audit index unavailable…` keeps appearing.
- `audit query` returns wildly fewer rows than `wc -l ~/.local/state/safessh/audit.log`.
- You upgraded `safessh` and see an `audit_index_newer` error after switching back to an older binary. The older binary refuses a newer DB by design — delete `audit.db` and let the older binary rebuild it.

If JSONL itself looks corrupted (mid-line truncation from a system crash), the catch-up loop skips the bad line and continues. The line still occupies bytes in `audit.log`; you can verify with `jq -c . < audit.log >/dev/null` — `jq` will report the bad offset.

## Power-user: direct SQLite access

The schema is documented above and stable for v0.5. Future minor versions may add columns or migrations; treat the contract as `audit query`, not the SQL.

```sh
sqlite3 ~/.local/state/safessh/audit.db \
  "SELECT decision, COUNT(*) FROM events GROUP BY decision ORDER BY 2 DESC"
```

```sh
# Per-project event volume
sqlite3 ~/.local/state/safessh/audit.db \
  "SELECT project, COUNT(*) FROM events
   WHERE timestamp >= '2026-05-01T00:00:00Z'
   GROUP BY project"
```

```sh
# Pull the raw_json of the last 5 yolo invocations
sqlite3 ~/.local/state/safessh/audit.db \
  "SELECT raw_json FROM events
   WHERE event_type = 'yolo_invocation'
   ORDER BY timestamp DESC LIMIT 5" | jq .
```

Open the DB read-only if you want hard guarantees that nothing in your shell session can disturb the index:

```sh
sqlite3 -readonly ~/.local/state/safessh/audit.db \
  "SELECT timestamp, event_type, project, raw_json FROM events ORDER BY timestamp DESC LIMIT 10"
```

## See also

- [`docs/cli-reference.md`](cli-reference.md) — full flag listing for `safessh audit query`.
- [`docs/security.md`](security.md) — SAFETY-INVARIANT-4 (audit-write before user-visible output) and threat model context.
- [`docs/tui.md`](tui.md) — the audit screen, which shares the filter set documented here.
- [Spec §3](superpowers/specs/2026-05-03-safessh-v0.5-v0.6-design.md#3-v050--audit-power) — design of the SQLite index, catch-up loop, and rotation handling.
