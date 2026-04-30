# safessh — Design Spec

| | |
|---|---|
| **Status** | Approved (brainstorming complete) |
| **Date** | 2026-04-30 |
| **Audience** | Implementer (you), reviewers, future-you |
| **Project root** | `/Volumes/External/Users/sanifss/Desktop/Workspace/Projects/Personal/safessh/` |
| **License** | `MIT OR Apache-2.0` (Rust ecosystem standard) |
| **Language** | Rust (stable) |

---

## 1. Summary

safessh is a personal CLI proxy that lets LLM agents run SSH operations against your servers **without ever seeing your credentials**, with **policy-gated commands** and **persistent audit**.

The LLM invokes `safessh <project> exec "<command>"`. safessh:

1. Looks up project credentials (host, user, auth) without exposing them to the LLM.
2. Parses the command into an AST and matches it against semantic policy categories.
3. Allows / denies / asks-for-approval based on per-project rules.
4. Executes via the system `ssh(1)` binary (inheriting `~/.ssh/config`, ssh-agent, ControlMaster, ProxyJump).
5. Streams structured, redacted output back.
6. Records everything to JSONL audit + SQLite index.

A TUI (`safessh tui`) manages projects, approvals, persistent allow/deny rules, and audit history. A universal "skill" markdown file installs into Claude Code, Cursor, Gemini CLI, Codex, or generic `AGENTS.md` so agents know how to use safessh.

---

## 2. Motivation

LLM agents that need to operate on remote servers today get one of two bad options:

- **Hand the agent the credentials** (SSH key, password). The agent now has the same blast radius as the human. Anything an attacker who compromises the agent can do, they can do with your prod credentials.
- **Use an MCP SSH server** with thin or no policy. Most existing MCP SSH tools are wrappers that take a `host: 1.2.3.4, user: x, key: ...` config and expose `ssh.run`. Better than embedding creds in prompts, but the policy story is regex-on-strings or static config; there's no approval lifecycle, no audit-quality records, no AST-level command understanding.

safessh fills the gap with:

- **CLI-first, transport-agnostic.** Works for any agent that can run a subprocess, not just MCP-aware ones. A skill markdown file shipped alongside teaches agents how to use it.
- **AST-based semantic policy.** Categories like `destructive:filesystem`, `db:write`, `network:tunnel` instead of regex on the command string.
- **Approval lifecycle modeled on Claude Code.** `once / timed / always / deny / block` with the deny-over-allow ordering rule that enterprise tools like CyberArk PSM enforce.
- **Audit you can actually query.** JSONL + SQLite index, viewable in TUI.

### 2.1 Non-goals

- **Not** an enterprise multi-user system. Single dev on a workstation. Teleport or CyberArk fill that role.
- **Not** a sandbox. AST policy reduces attack surface but is not a confinement boundary. An attacker with shell access can still find ways to express forbidden actions; safessh raises the cost, doesn't eliminate the threat.
- **Not** a credential manager. Leverages ssh-agent, on-disk keys, and OS keychain. Doesn't reinvent secret storage.
- **Not** a reimplementation of OpenSSH. Drives the system `ssh(1)` binary as a subprocess.

---

## 3. Locked decisions (Q&A summary)

| # | Decision | Choice |
|---:|---|---|
| Q1 | Transport | CLI subprocess (no MCP); ships a universal skill markdown for agent integration |
| Q2 | Approval prompt UX | Inline TTY prompt when stdin is a TTY, **plus** structured-deny + token-based approval for headless agents |
| Q3 | Language | Rust |
| Q4 | Policy model | AST parsing + named semantic categories; default-deny on parse failure |
| Q5 | Project model | Multi-target inline; TUI offers ssh-config import as a convenience |
| Q6 | Credential storage | Path references + ssh-agent default; opt-in OS keychain per project |
| Q7 | SSH operations | Exec + sftp file read/write + `ssh -L` port forwarding (gated) |
| Q8 | Approval lifecycle | Five actions: `once / timed / always / deny / block`. Timed has user-configurable TTL (default 30 min). Always-rules match by AST-derived pattern, not literal string. |
| Q9 | Audit | JSONL append-only + SQLite index (rebuildable from JSONL) |
| Q10 | Output | Streamed + framed (`<stdout>...</stdout>` etc.) + size caps + redaction default-on for catastrophic patterns |
| Q11 | TUI scope | Four screens: project list/editor, pending approvals queue, persistent allow/deny lists, audit viewer |
| — | SSH driver | OpenSSH-driver (subprocess to `ssh`/`sftp`) — leverages ControlMaster, ssh-config, ssh-agent, ProxyJump |
| — | Process model | Stateless CLI per invocation; no daemon in v1 |
| — | License | `MIT OR Apache-2.0` |
| — | Conventional Commits | Yes |
| — | Release tooling | `cargo-dist` |
| — | Branching | `main` (stable) + `develop` (testing) + `feature/*` |
| — | Skill targets v1 | claude-code + agents-md (v0.1); cursor + gemini-cli + codex + plain (v0.6) |

---

## 4. Architecture

```
LLM / human
    │
    ▼
safessh <project> exec "command"          ──┐
safessh <project> read /path                │
safessh <project> write /path < file        │   safessh-cli
safessh <project> forward L:R:P             │   (entrypoint)
safessh tui                                ──┘
    │
    ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│  policy      │───▶│  approvals   │───▶│  ssh driver  │
│  (AST + cat) │    │  (5 actions) │    │  (subprocess)│
└──────────────┘    └──────────────┘    └──────────────┘
       │                   │                   │
       └───────────┬───────┴───────────────────┘
                   ▼
           ┌──────────────┐
           │  audit       │  ──▶  audit.log (JSONL)
           │              │  ──▶  audit.db  (SQLite index)
           └──────────────┘
                   ▲
           ┌──────────────┐
           │  storage     │  ──▶  ~/.config/safessh/projects/*.toml
           │              │  ──▶  ~/.local/state/safessh/approvals/*
           └──────────────┘
```

### 4.1 Top-level invariants (held across all code paths)

1. **Policy runs before any subprocess.** SSH never opens until policy decision is `Allow`.
2. **Secrets never flow into argv or logs.** Passwords/passphrases come from the OS keychain at point of use.
3. **Audit writes happen before user-visible output.** If the JSONL append fails, the operation aborts.

### 4.2 Process model

One process per CLI invocation. Stateless. Connection reuse comes from OpenSSH's ControlMaster (sockets in `~/.cache/safessh/control-sockets/`). First call to a target ~300ms (handshake); subsequent calls within the multiplex window <20ms.

The TUI is a separate `safessh tui` invocation that reads/writes the same files the CLI does, watched via the `notify` crate for live updates. No daemon, no IPC socket, no state divergence.

### 4.3 Data layout

```
~/.config/safessh/
  config.toml                # global: default TTLs, redaction patterns, log paths
  projects/
    <name>.toml              # targets, ssh-config alias (optional), policy refs
  policies/
    presets.toml             # built-in categories (shipped, read-only)
    custom.toml              # user-added rules

~/.local/state/safessh/
  audit.log                  # JSONL, source of truth, append-only
  audit.db                   # SQLite index (rebuildable from audit.log)
  approvals/
    pending/<token>.toml     # open requests waiting for human
    timed/<project>.toml     # active timed allows + expiry
    always/<project>.toml    # persistent allow-rules
    blocked/<project>.toml   # persistent block-rules

~/.cache/safessh/
  ssh-config-snapshot.toml   # cached parse of ~/.ssh/config (mtime-invalidated)
  control-sockets/           # OpenSSH ControlMaster socket directory
```

All TOML, human-readable, backup-friendly. SQLite is derivative — delete and the indexer rebuilds.

### 4.4 Project TOML schema (illustrative)

```toml
# ~/.config/safessh/projects/cureocity.toml
name = "cureocity"
default_target = "web"

# Multi-target list. Each target either inlines host/user/port/auth,
# or references an existing ~/.ssh/config alias.

[[targets]]
name = "web"
ssh_config_alias = "cureocity-web"   # all connection details from ~/.ssh/config

[[targets]]
name = "db"
host = "10.0.5.4"                     # inline form
port = 22
user = "deploy"
identity_file = "~/.ssh/id_ed25519_cureocity"
proxy_jump = "bastion.cureocity.io"   # optional, also overridable via ssh-config

[[targets]]
name = "cache"
ssh_config_alias = "cureocity-cache"
keychain_secret = "cureocity-cache-passphrase"  # opt-in OS keychain

[policy]
# References category names; presets shipped, custom defined in policies/custom.toml.
allow = ["read:safe", "file:read"]
require_approval = ["destructive:filesystem", "db:write", "file:write"]
deny = ["destructive:disk", "system:control"]
# network:tunnel is default-deny via preset; not listed here means inherit default-deny.

[approvals]
timed_default_minutes = 30
yolo = false              # per-project yolo flag; global disable_yolo overrides

[output]
stdout_cap_bytes = 1048576       # 1 MB
stderr_cap_bytes = 262144        # 256 KB
file_read_cap_bytes = 5242880    # 5 MB
tunnel_ttl_minutes = 30
```

Nothing in this file is secret. Secrets are in the OS keychain referenced by `keychain_secret` (opt-in) or in ssh-agent / on-disk key files referenced by `identity_file`.

### 4.5 Policy presets (shipped categories)

The `policies/presets.toml` file is read-only and shipped with the binary. Categories are AST-level matchers:

| Category | Matches |
|---|---|
| `read:safe` | `ls`, `cat`, `head`, `tail`, `grep`, `find` (without `-delete` / `-exec`), `stat`, `file`, `wc`, `sort`, `uniq`, `which`, `whereis`, `pwd`, `id`, `whoami`, `uname`, `date`, `uptime`, `df`, `du`, `free`, `ps`, `top` (read-only invocations only) |
| `file:read` | sftp `read` operation (first-class — not exec-shaped) |
| `file:write` | sftp `write` operation (first-class) |
| `destructive:filesystem` | `rm`, `rmdir`, `unlink`, `shred`, `find … -delete`, `find … -exec rm`, recursive `mv` to `/dev/null` |
| `destructive:disk` | `dd`, `mkfs.*`, `fdisk`, `parted`, `wipefs`, raw redirects to `/dev/sd*` / `/dev/nvme*` / `/dev/disk*` |
| `destructive:db` | SQL `DROP`, `TRUNCATE`, `DELETE` without `WHERE`, `ALTER … DROP`, recognized inside `psql -c` / `mysql -e` / `sqlite3` argv |
| `db:read` | SQL `SELECT`, `EXPLAIN`, `SHOW`, `DESCRIBE` inside `psql` / `mysql` / `sqlite3` |
| `db:write` | SQL `INSERT`, `UPDATE`, scoped `DELETE`, `CREATE`, `ALTER` (non-DROP) |
| `privilege:escalation` | `sudo`, `su`, `doas`, `pkexec`, setuid manipulations |
| `system:control` | `shutdown`, `reboot`, `halt`, `poweroff`, `systemctl stop`/`disable`/`mask` on critical units |
| `network:listen` | opens listening sockets: `nc -l`, `socat … LISTEN`, `python -m http.server` |
| `network:tunnel` | safessh `forward` operation (first-class) |
| `exec:opaque` | `eval`, `sh -c`, `bash -c`, `zsh -c`, `python -c`, `perl -e`, base64-pipe-to-shell, `curl … | sh` patterns. **Excludes** known semantic interpreters that the policy engine understands: `psql -c`, `mysql -e`, `sqlite3 <sql>` are matched as `db:*` categories instead. |

Default project policy when none specified: allow `read:safe` and `file:read`; require approval for everything else; default-deny on `network:tunnel`, `destructive:disk`, `system:control`, `exec:opaque`.

Users can extend with custom rules in `policies/custom.toml` (path globs, additional binary matches, project-scoped overrides).

---

## 5. Components

Workspace of seven crates, dependencies flow downward only:

```
safessh-cli ─┬─▶ safessh-tui ──┐
             ├─▶ safessh-ssh ──┤
             ├─▶ safessh-policy┤
             ├─▶ safessh-audit ┼─▶ safessh-storage ──▶ safessh-core
             └─▶ safessh-skill ┘
```

### 5.1 `safessh-core`

Shared types, error types, redaction. No I/O, no network.

- Types: `ProjectId`, `Target`, `ParsedCommand`, `PolicyDecision`, `ApprovalToken`, `AuditEvent`.
- `Redactor` — pluggable regex set with default-on patterns (AWS keys, JWT, private-key blocks, bearer tokens).
- `Error` enum via `thiserror`.

### 5.2 `safessh-storage`

Only crate that touches the filesystem for config/state.

- Loads `~/.config/safessh/{config.toml, projects/*.toml, policies/*.toml}`.
- Manages `~/.local/state/safessh/approvals/{pending,timed,always,blocked}/*.toml`.
- Atomic writes via `tempfile::NamedTempFile` → `persist()` (rename).
- `notify`-crate watcher for TUI live updates.
- Provides `keyring` crate access for opt-in per-project secrets.
- Advisory locks (`fs2::FileExt::lock_exclusive`) on rule-file writes for race safety.

### 5.3 `safessh-policy`

Pure functions, no I/O.

- Parses commands with `conch-parser` (or fork). Returns either typed AST or `ParseError::Opaque(reason)`.
- Walks AST, emits matched categories.
- Resolves `(project, parsed-command)` against allow/timed/always/block stores → `PolicyDecision`.
- **Default-deny when parsing fails.**
- **Block-list checked before allow-list** (deny > allow ordering).

### 5.4 `safessh-ssh`

Only crate that knows about `ssh(1)`.

- `exec(target, command) -> impl Stream<StdoutChunk | StderrChunk | Exit>`.
- `read(target, path) -> impl AsyncRead`.
- `write(target, path) -> impl AsyncWrite`.
- `forward(target, spec) -> Tunnel` (Drop closes cleanly + audits).
- Uses ssh-config aliases when projects reference them. ControlMaster sockets in `~/.cache/safessh/control-sockets/`.
- Mocked via `SshDriver` trait for unit tests; integration tests use `testcontainers-rs` with `linuxserver/openssh-server`.

### 5.5 `safessh-audit`

- JSONL writer to `~/.local/state/safessh/audit.log`. Append-only, fsync per line. Rotates at 100 MB.
- SQLite index at `audit.db`. Schema migrations via `refinery` or `sqlx::migrate!`. Indexer runs as a small background task per CLI invocation; if it fails, JSONL is still authoritative.
- `safessh audit rebuild-index` command for emergencies.
- Redacts every event through `core::Redactor` before write.

### 5.6 `safessh-tui`

ratatui screens, consumes only public APIs of other crates.

- Four screens (Section 6 of TUI scope): project list/editor, pending approvals queue, persistent allow/deny per project, audit viewer.
- `notify`-driven refresh on filesystem changes.
- Tested via ratatui's `TestBackend` + `insta` snapshots.

### 5.7 `safessh-skill`

Universal skill markdown embedded as `include_str!("safessh.md")`. Single source of truth for the agent-facing instructions.

**Subcommands:**
- `safessh skill install` — interactive multi-select. Detects which agent frameworks are installed, asks user/project/custom path per target, confirms before writing.
- `safessh skill install --target <name> --scope user|project|path <dir>` — non-interactive.
- `safessh skill install --target all` — every detected target with defaults.
- `safessh skill uninstall [--target <name>]`.
- `safessh skill show [--target <name>]` — print formatted output.
- `safessh skill check` — list installed locations, version drift detection.
- `safessh skill update` — re-install to every currently-installed location with the binary's embedded version.

**Targets and default locations:**

| Target | Format | User-level | Project-level |
|---|---|---|---|
| `claude-code` | YAML frontmatter + body | `~/.claude/skills/safessh.md` | `.claude/skills/safessh.md` |
| `cursor` | MDC frontmatter + body | `~/.cursor/rules/safessh.mdc` | `.cursor/rules/safessh.mdc` |
| `gemini-cli` | Plain markdown section | `~/.gemini/GEMINI.md` | `./GEMINI.md` |
| `agents-md` | Plain markdown section | — | `./AGENTS.md` |
| `codex` | Plain markdown | `~/.codex/instructions.md` | `./.codex/instructions.md` |
| `plain` | Raw markdown body | user-specified path | user-specified path |

**Skill content covers:**
- When to invoke safessh (frontmatter `description` triggers on SSH/server tasks).
- Subcommand reference.
- How to handle structured-deny output (recognize `BLOCKED:` token, surface verbatim, do not retry the same command).
- Output framing format.
- When to use `--yolo` (essentially never; only when human explicitly asks).
- How to discover available projects: `safessh project list`.

**v0.1 ships** `claude-code` + `agents-md` adapters. Other targets land in v0.6.

### 5.8 `safessh-cli`

Thin entrypoint. Parses argv with `clap`, dispatches, formats output (with framing for `exec`), handles `--yolo`, prints structured-deny tokens, owns TTY detection for inline prompts.

**Subcommands:**
- `<project> exec "<command>"`
- `<project> read <remote-path>`
- `<project> write <remote-path>` (reads stdin)
- `<project> forward <local>:<remote-host>:<remote-port>`
- `<project> [--on <target>]` — multi-target dispatch (v0.2)
- `project list / add / edit / remove`
- `policy show <category>`, `policy show <project>`
- `approve <token>` — grant approval for a pending request
- `tui` — launch the TUI
- `skill install / uninstall / show / check / update`
- `audit query [--project ...] [--since ...] [--type ...]`
- `audit rebuild-index`
- `tunnels list / close <id>` (v0.4)
- `--yolo` flag, `--version`, `--help`

---

## 6. Data flow

### 6.1 Allowed exec

```
LLM: safessh prod exec "ls /var/log"
  │
  ├─[cli]──▶ load config + project "prod"
  ├─[policy]─▶ parse "ls /var/log" → AST{bin:"ls", flags:[], args:["/var/log"]}
  ├─[policy]─▶ match categories → {read:safe}
  ├─[policy]─▶ check rules: prod allows read:safe → Allow
  ├─[audit]──▶ write event{type:"exec", project:"prod", parsed:..., decision:"allow"}
  ├─[ssh]────▶ spawn `ssh prod -- ls /var/log` (ControlMaster reuses if warm)
  ├─[cli]────▶ stream stdout/stderr through framing layer:
  │             <stdout>...lines...</stdout>
  │             <stderr></stderr>
  │             <exit code="0" duration="34ms"/>
  └─[audit]──▶ write event{type:"exec_complete", exit:0, stdout_bytes:412, ...}
```

### 6.2 Gated path — first-time destructive command

```
LLM: safessh prod exec "rm -rf /var/log/old"
  │
  ├─[policy]─▶ AST → {bin:"rm", flags:["-r","-f"], args:["/var/log/old"]}
  ├─[policy]─▶ categories → {destructive:filesystem}
  ├─[policy]─▶ rules: prod denies destructive:* unless approved → RequireApproval
  ├─[approvals]▶ generate token "ab12cd"
  │              write ~/.local/state/safessh/approvals/pending/ab12cd.toml
  ├─[audit]──▶ event{type:"approval_requested", token:"ab12cd", ...}
  └─[cli]────▶ TTY available?
                ├─ YES: prompt inline → user picks once/timed/always/deny/block
                └─ NO:  write to stderr:
                        BLOCKED: destructive:filesystem on project=prod
                        Command: rm -rf /var/log/old
                        Approve via: safessh approve ab12cd  OR  safessh tui
                        Token: ab12cd
                        exit code 10 (= "approval required")
```

LLM relays to human. Human approves via TUI (or `safessh approve ab12cd` from another shell).

| Action | Effect |
|---|---|
| `once` | Pending → executed. CLI rerun completes. |
| `timed` | Write `approvals/timed/<project>.toml` with `expires_at` and AST pattern. |
| `always` | Write `approvals/always/<project>.toml` with AST pattern. Persists. |
| `deny` | Pending → denied. Next try re-blocks (same flow). |
| `block` | Write `approvals/blocked/<project>.toml`. Persistent block. Removable from TUI only. |

LLM retries the command; policy re-checks. **Block-list checked first**; matching block → exit 11. Matching always/unexpired-timed → Allow with `decision_source: timed_rule | always_rule` audit annotation.

### 6.3 Tunnel lifecycle

```
LLM: safessh prod forward 5432:db.internal:5432
  │
  ├─[policy]─▶ category {network:tunnel} → default-deny → RequireApproval
  ├─[approvals]▶ pending token, same flow as 6.2
  └─ on approval (timed/always):
       ├─[ssh]──▶ spawn `ssh prod -L 5432:db.internal:5432 -N`
       ├─[audit]▶ event{type:"tunnel_open", local:5432, remote:"db.internal:5432", pid:...}
       │          + warning: "tunnel traffic is opaque to safessh"
       └─[cli]──▶ print "tunnel open on localhost:5432 (max 30 min)"
       │
       ├─ on TTL: SIGTERM subprocess (5s grace, then SIGKILL)
       ├─[audit]▶ event{type:"tunnel_close", reason:"ttl_expired", duration_s:1800}
       └─[storage]▶ remove from active-tunnels list
```

`safessh tunnels list / close <id>` for management. TUI audit screen shows tunnel events with the opacity warning.

### 6.4 File read with redaction

```
LLM: safessh prod read /etc/nginx/nginx.conf
  │
  ├─[policy]─▶ category {file:read}, path "/etc/nginx/*"
  ├─[policy]─▶ rules: prod allows file:read on /etc/nginx/* → Allow
  ├─[audit]──▶ event{type:"file_read", path:..., decision:"allow"}
  ├─[ssh]────▶ sftp via ControlMaster
  ├─[core]───▶ Redactor scans bytes, replaces matches with <REDACTED:type>
  ├─[cli]────▶ write to stdout (framed), size cap 5MB default, truncate if exceeded
  └─[audit]──▶ event{type:"file_read_complete", bytes_returned:..., sha256:...,
                       redactions:[{type:"aws_access_key", count:0}], truncated:false}
```

Audit logs the file's sha256 and **counts** of redactions per type — never the redacted content itself.

### 6.5 `--yolo` bypass

```
LLM: safessh prod exec --yolo "anything"
  │
  ├─[cli]────▶ check yolo allowed?
  │             - per-invocation flag: yes
  │             - per-project config yolo:true: also yes
  │             - global config disable_yolo:true: refuse → exit 13
  ├─[audit]──▶ event{type:"yolo_invocation", parsed:..., flagged:true}
  ├─[policy]─▶ SKIPPED
  └─[ssh]────▶ exec
```

Yolo invocations are tagged in JSONL and counted in the SQLite index. TUI's audit screen has a default filter showing yolo-tagged events with a visible counter.

### 6.6 TUI live updates

The TUI doesn't poll. `notify` watches:

- `~/.local/state/safessh/approvals/pending/` — refreshes the approvals queue.
- `~/.local/state/safessh/audit.log` — tails when audit screen is open.
- `~/.config/safessh/projects/` — refreshes the project list when files change externally.

TUI writes go through the same `safessh-storage` API the CLI uses — atomic temp-write + rename. Neither side observes a half-written file.

---

## 7. Error model and safety invariants

### 7.1 Exit codes

| Code | Meaning | LLM should… |
|---:|---|---|
| `0` | success | proceed |
| `1` | generic error (project not found, missing config) | report to human |
| `2` | usage error | report and stop |
| `10` | approval required | report token and prompt human, **do not retry** |
| `11` | persistently blocked (matched block-rule) | report, **do not retry** |
| `12` | denied for this invocation | report, may re-prompt with explicit human ask |
| `13` | yolo refused (disabled globally) | report and stop |
| `20` | ssh failure (subprocess error) | report, may retry |
| `21` | connection refused / timeout | report, may retry |
| `30` | output exceeded size cap (truncated) | output is still valid up to cap |
| `40` | storage error (config corrupt, lock held) | report and stop |
| `50` | audit-write failure (operation refused) | report and stop — **do not yolo around it** |

### 7.2 Twelve safety invariants

Each is enforced in code with a `// SAFETY-INVARIANT-N:` comment marker, and asserted in tests.

1. **Default-deny on parse failure.** AST parser returns `ParseError::Opaque` → `RequireApproval`, never `Allow`.
2. **Deny > allow ordering.** Block-list and persistent-deny rules are checked before any allow rules.
3. **Secrets never in argv.** Passwords and passphrases are pulled from keychain at exec time, fed via stdin or environment — never visible in `ps`.
4. **Audit-write before user-visible output.** JSONL append failure → exit 50, abort. SQLite indexer can fail silently (JSONL is authoritative), but JSONL itself is non-negotiable.
5. **Atomic file writes.** All config and state via tempfile + rename. No half-written files observable.
6. **Redaction last.** Output goes through `core::Redactor` after framing, before stdout.
7. **TTLs are wall-clock, not process lifetime.** Timed allows persist across invocations, expire by `expires_at` comparison.
8. **Tunnel TTL is hard.** Tunnel processes SIGTERM'd at TTL, SIGKILL after 5s grace.
9. **`--yolo` only bypasses the policy engine.** Does not bypass: audit logging (logs MORE), `disable_yolo` setting, output cap, redactor.
10. **No outbound network calls from safessh itself.** No telemetry, no auto-update, no version check.
11. **Skill content is binary-embedded, never fetched.** Updates come with binary updates.
12. **Concurrent invocations race-safe.** Rule files accessed under advisory file locks. TUI and CLI cannot corrupt each other's writes.

### 7.3 Failure-mode matrix

| Failure | Behavior |
|---|---|
| `~/.config/safessh/projects/<name>.toml` malformed | exit 40 with line/col, suggest `safessh config validate <name>` |
| ControlMaster socket exists but stale | auto-clean (`ssh -O exit`), retry once, log to audit |
| ssh-agent unavailable + keychain not configured + no TTY | exit 20, suggest `safessh project edit <name>` |
| Network drops mid-exec | partial stdout preserved through framing, exit reflects ssh's exit code |
| SQLite index corrupt | warn on TUI open, continue from JSONL, suggest `safessh audit rebuild-index` |
| Pending approval older than 24h | auto-expired on next CLI invocation, audit logs `approval_expired` |
| Two CLIs racing on the same allow-rule write | second waits for advisory lock, both writes succeed in order |
| Disk full during JSONL append | exit 50, command refused (invariant 4) |
| User edits TOML while TUI open | `notify` watcher reloads, TUI shows "config changed externally" toast |

### 7.4 Error output format

- **Human-readable to stderr.** Single line, prefixed `safessh: <category>: <message>`. For `BLOCKED` cases, multi-line per Section 6.2.
- **Structured to audit.** Every error event has `event_type`, `error_code`, `error_class`, `error_message`. SQLite index lets TUI show "errors in last 24h grouped by class."
- **Stdout reserved for command output and framing.** LLM's stdout parser doesn't have to filter error noise.

---

## 8. Testing strategy

Goal: every behavior testable, deterministic, fast. Workspace test suite under 60s; integration suite under 3 min including container startup.

### 8.1 Per-crate test type

| Crate | Primary test type | Tools |
|---|---|---|
| `safessh-core` | Unit | `cargo test` |
| `safessh-policy` | Table-driven unit + property tests | `cargo test`, `proptest`, `insta` |
| `safessh-storage` | Unit with `tempfile::tempdir` | `cargo test`, `tempfile` |
| `safessh-audit` | Unit + golden JSONL snapshots | `cargo test`, `insta` |
| `safessh-ssh` | Trait-mocked unit + container integration | `mockall`, `testcontainers-rs` |
| `safessh-tui` | TestBackend + snapshot tests | `ratatui::TestBackend`, `insta` |
| `safessh-skill` | Adapter unit + golden file snapshots per target | `cargo test`, `insta` |
| `safessh-cli` | E2E against compiled binary | `assert_cmd`, `predicates` |

### 8.2 Five tiers

1. **Unit** — fast, hermetic, no I/O beyond `tempfile`. Target <10s for all units.
2. **Property** (`proptest`) — three high-value targets:
   - Policy AST parser: arbitrary inputs, assert no panics + only valid AST or `ParseError::Opaque`.
   - Redactor: text with embedded secret-shaped tokens, assert original token never appears in output.
   - Allow/deny evaluation: random rule-set + command pairs, assert deny > allow holds.
3. **TUI snapshot** — `insta` on rendered buffers, driven by synthesized key events.
4. **CLI E2E** — `assert_cmd` against compiled binary in temp `HOME`. Every exit code in §7.1 has a producing test. Every `BLOCKED:` format string is byte-exact-asserted.
5. **Real SSH integration** — `testcontainers-rs` + `linuxserver/openssh-server`. Gated by `--features integration`. Asserts on: happy-path exec, ControlMaster reuse (second command faster), sftp read/write, port forward open/close + TTL SIGTERM, mid-stream connection failure.

### 8.3 CI

`.github/workflows/ci.yml`:
- Lint: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`.
- Build matrix: macos-14 (arm64), macos-13 (x64), ubuntu-22.04 (x64).
- Unit + integration (no container): `cargo test --workspace`.
- Container integration: Linux only with Docker available, `cargo test --features integration`.
- Fuzz quick-run on `develop` and `main` only: `cargo test --features proptest-extended`.
- `cargo audit` for known CVEs (warn on PRs, error on `main`).

`.github/workflows/release.yml` — generated by `cargo-dist`. Triggered on `v*` tags. Cross-compile, GitHub Release, Homebrew tap update, install script publish.

### 8.4 Coverage philosophy

- `cargo llvm-cov` reports stored as artifact.
- Target: 80%+ per crate except `safessh-cli` (thin glue).
- Coverage is a signal, not a hard gate.
- CI fails on: `unsafe` blocks lacking `// SAFETY:` comments, `#[ignore]`'d tests on `main`.

### 8.5 Explicitly not tested

- Real SSH against actual user-owned servers (container is the substitute).
- Linux Secret Service operations in CI (mocked at trait boundary; macOS keychain tested only locally during release verification).
- Kernel-level packet drops (rely on SSH's own error reporting).

---

## 9. Milestones

Each milestone is a shippable, usable subset. README's features table updates per release.

### v0.1.0 — Foundation + end-to-end exec (2–3 weeks)

Install, add a project, run gated commands via LLM with full approval lifecycle. Audit captures. Skill installs.

- Crates: `safessh-core`, `safessh-storage`, `safessh-policy`, `safessh-audit` (JSONL only), `safessh-ssh` (exec only), `safessh-cli`, `safessh-skill` (claude-code + agents-md adapters).
- Subcommands: `<project> exec`, `project add/list/edit/remove`, `policy show`, `approve <token>`, `skill install/uninstall/show/check`, `audit query` (grep over JSONL), `--yolo`, `--version`, `--help`.
- Approval lifecycle: full five actions via CLI prompts (TTY) and `safessh approve <token>` (headless).
- Project model: schema supports multi-target list; `--on` flag and target switching land in v0.2.
- Release pipeline: `cargo-dist`, GitHub Releases, Homebrew tap, curl install. Conventional Commits. CI lint+unit+container-integration.
- Docs: `README.md`, `docs/getting-started.md`, `docs/cli-reference.md`, `docs/skill.md`, `docs/security.md`, `docs/development.md`.

### v0.2.0 — TUI + multi-target (1–2 weeks)

- `safessh-tui` four core screens.
- ssh-config import in project editor.
- Multi-target via `--on <target>`.
- `notify`-based live updates.
- Docs: `docs/projects.md`, `docs/tui.md`, `docs/approvals.md`.

### v0.3.0 — File operations (3–5 days)

- `safessh <project> read / write`.
- `file:read` / `file:write` categories with path-glob rules.
- Size caps + framing for `read`. Hash-of-content in audit.

### v0.4.0 — Port forwarding (~1 week)

- `safessh <project> forward`.
- `network:tunnel` category, default-deny.
- `safessh tunnels list/close`.
- Tunnel lifecycle audit + opacity warning.

### v0.5.0 — Audit power (~1 week)

- SQLite indexer.
- TUI audit screen filters.
- `safessh audit query` with structured filters.
- `docs/audit.md` with JSONL schema + SQLite recipes.

### v0.6.0 — Multi-agent skill targets (3–5 days)

- Adapters for `cursor`, `gemini-cli`, `codex`, `plain`.
- `safessh skill install --target all`.
- `safessh skill update`.

### v0.7.0 — Hardening + polish (1–2 weeks)

- Redactor: default + custom patterns audited and tested.
- Property tests: extended runs in nightly.
- Performance: cold-start budget regression test.
- Threat-model doc expansion.
- Documentation pass.

### v0.9.0-rc — Release candidate

- Bug-fix only.
- Install rehearsal on clean machines per platform.
- Performance benchmarks in `docs/performance.md`.

### v1.0.0 — Stable

- CLI surface, config schema, audit JSONL schema committed.
- Submit to homebrew-core if usage justifies.

**Total realistic timeline:** 4–6 months evenings/weekends.

### 9.1 Explicitly deferred to post-v1

- First-class DB queries (P3 from Q7).
- Daemon-shape architecture.
- Approval channel plugins (Slack/Telegram/Discord push).
- Context-predicate rules ("only between 9–5").
- Aider, continue.dev, windsurf as skill targets.

---

## 10. Operational and release plan

### 10.1 Branching

- `main` — stable, tagged releases only, protected branch.
- `develop` — integration branch for testing.
- `feature/<topic>` → PR into `develop` → tested → `develop` → `main` with version bump.
- `hotfix/<topic>` → PR into `main`, back-merged to `develop`.

### 10.2 Versioning

SemVer. v0.x.y while pre-stable. v1.0.0 commits CLI surface + config schema + audit JSONL schema as stable.

### 10.3 Conventional Commits

`feat:` / `fix:` / `chore:` / `docs:` / `refactor:` / `test:` / `perf:` / `build:` / `ci:`. Auto-changelog generation tied to `cargo-dist` or a follow-on tool.

### 10.4 Release tooling: `cargo-dist`

Configured via `[workspace.metadata.dist]` in `Cargo.toml`. On `v*` tag push:

- Cross-compile for: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. (Optionally Windows.)
- Generate checksums.
- Create GitHub Release with binaries + checksums.
- Update Homebrew tap formula (separate `homebrew-safessh` repo).
- Generate `installer.sh` for `curl ... | sh` flow.

### 10.5 Install methods

1. **Homebrew:** `brew install <tap>/safessh/safessh`.
2. **curl:** `curl -fsSL https://safessh.dev/install.sh | sh` (or GitHub URL until domain owned). Detects OS+arch, downloads from GH Releases, verifies checksum, installs to `~/.local/bin` (or `/usr/local/bin` if writable). Honors `INSTALL_DIR` env var. Documents the "download → inspect → run" alternative.
3. **`cargo install safessh`** for Rust users.
4. **Pre-built binaries** from GitHub Releases.

The curl installer accepts an optional `--install-skill auto` flag that, after binary install, runs `safessh skill install --target all`. Off by default — opt-in only.

### 10.6 README.md structure

- Tagline.
- Install (Homebrew → curl → cargo).
- Quick start (3 commands: add a project, run a command, open the TUI).
- Agent integration (skill install table per target).
- Features table (status: shipped / in-progress / planned).
- Links to `docs/`.
- License + contributing.

The features table is updated per release; in-progress entries carry a `v0.X` tag.

### 10.7 Docs folder

Plain markdown. mdBook only if a hosted docs site is added later.

- `docs/installation.md`
- `docs/getting-started.md`
- `docs/projects.md`
- `docs/policy.md`
- `docs/approvals.md`
- `docs/audit.md`
- `docs/tui.md`
- `docs/cli-reference.md`
- `docs/skill.md`
- `docs/security.md`
- `docs/development.md`
- `docs/performance.md` (added v0.9)

---

## 11. Open questions / future work

These are intentionally **not** blockers for v1; flagged so they don't get lost.

- **First-class DB queries (P3).** A `safessh <project> query --db postgres "SELECT ..."` operation that holds DB connections through the SSH transport and returns structured rows with proper SQL-AST policy. Right shape for an LLM-DB workflow once safessh is stable.
- **Daemon mode.** Long-running `safessh-daemon` for real-time approval push and in-process state. Add only if file-based queue + `notify` proves insufficient.
- **Approval channel plugins.** Slack / Telegram / Discord push approvals via the agent-channels infra. Wait for the API surface to settle before exposing.
- **Context-predicate rules.** Rules conditional on time-of-day, source IP, etc. (Cmd Control's pattern.)
- **Webhook audit shipping.** JSONL → SIEM. Probably trivial as a `safessh-audit-shipper` companion crate.
- **Encrypted config bundle export.** For users who want to sync safessh config across machines. Single-passphrase age-encrypted bundle.

---

## 12. Existing-tools context (for future readers)

safessh occupies a niche that is adjacent to but not filled by:

- **MCP SSH servers** ([tufantunc/ssh-mcp](https://github.com/tufantunc/ssh-mcp), [mixelpixx/SSH-MCP](https://github.com/mixelpixx/SSH-MCP), [bvisible/mcp-ssh-manager](https://github.com/bvisible/mcp-ssh-manager), and others) — thin wrappers; no AST policy, no approval lifecycle, no TUI, no JSONL+SQLite audit.
- **AI-agent sandbox tools** — [Guardian Shell](https://guardianshell.com/), [Agent Safehouse](https://agent-safehouse.dev/), [agentsh](https://www.agentsh.org/) — local-only sandboxes (kernel/macOS-sandbox/HTTP-proxy). Different transport; agentsh is the closest in spirit ("agent doesn't see credentials") but for HTTP, not SSH.
- **Enterprise SSH command policy** — [CyberArk PSM](https://docs.cyberark.com/pam-self-hosted/latest/en/content/pasimp/configuring-ssh-commands-access-control-in-psmp.htm), [Cmd Control](https://help.cmd.com/en/articles/3396505-can-cmd-allowlist-denylist-commands), [Teleport](https://goteleport.com/features/access-requests/) — multi-user, heavyweight, not personal-dev-tool shaped. Source of the deny-over-allow ordering rule.

Patterns explicitly borrowed:
- **Deny > allow ordering** — CyberArk; cf. [Claude Code issue #12690](https://github.com/anthropics/claude-code/issues/12690) for the bug case.
- **Approval action vocabulary** — Claude Code's `once / session / always` (renamed `session` → `timed`).
- **Credential substitution** — agentsh's "agent doesn't see the keys" model, applied to SSH instead of HTTP.

---

## 13. Implementation handoff

Next step: invoke the `superpowers-extended-cc:writing-plans` skill to produce a detailed task breakdown for **v0.1.0**. v0.2 and beyond get their own plans at the start of each milestone.
