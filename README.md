<p align="center">
  <img src="assets/logo.svg" alt="safessh" width="720">
</p>

<p align="center">
  <strong>SSH gated by policy. Audited by default. Built for LLM agents.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="License">
  <img src="https://img.shields.io/badge/rust-1.75%2B-orange.svg" alt="Rust">
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey.svg" alt="Platform">
  <img src="https://img.shields.io/github/v/tag/sanif/safessh?label=release&color=informational" alt="Release">
</p>

<p align="center">
  <img src="screenshots/approval-flow.svg" alt="safessh approval flow" width="900">
</p>

---

## What makes safessh different

### Your keys never leave your machine

The agent runs a fixed CLI surface — `safessh <project> exec "..."`, `read`, `write`, `forward`. Credentials stay in macOS Keychain or your existing `~/.ssh/config`. The LLM never sees a password, a private key, or a hostname unless you've made it part of the project name.

### Default-deny on parse failure

Every command is parsed into an AST. If parsing fails — for any reason, ever — the decision is **`RequireApproval`**, never `Allow`. Block-list and persistent-deny rules evaluate **before** any allow rule, so a typo in your allow list can't accidentally green-light a destructive command.

### Auditable by default

Every gating event lands in an append-only JSONL log **before** any user-visible output. A lazy SQLite index makes it queryable in milliseconds; if the index breaks, queries fall back to a raw log scan and the JSONL is unaffected.

<p align="center">
  <img src="screenshots/audit-query.svg" alt="audit query examples" width="900">
</p>

### A real TUI for review

`safessh tui` opens a four-screen review tool — projects, approvals, rules, audit — with live filesystem watching. Edit a `.toml` policy in your editor and the TUI reflects it on the next paint. Filter the audit screen by project, target, decision, exit code, time window. Cap-free — backed by the SQLite index.

<p align="center">
  <img src="screenshots/tui.svg" alt="safessh tui — audit screen" width="900">
</p>

---

## Quick start

```sh
# Install (macOS / Linux)
brew install sanif/tap/safessh
# or:
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/sanif/safessh/releases/latest/download/safessh-cli-installer.sh | sh

# 1. Add a project — interactive prompts walk you through name, target,
#    SSH alias / inline host, and key picker.
safessh project add

# 2. Install the skill so your LLM agent knows the workflow.
safessh skill install --target claude-code --scope user
# Or fan out to every detected agent at once:
safessh skill install --target all --scope user

# 3. Run a command. Read-only stuff is allowed by default.
safessh prod exec "systemctl status nginx"

# 4. Review what happened.
safessh audit query --since 24h --decision deny --format table
safessh tui
```

Prefer to script it? Skip the prompts:

```sh
safessh project add prod --alias my-prod-host
safessh project add staging --host stg.internal --user deploy --port 2222
safessh project add prod-imported --import-ssh-config my-prod-host
```

---

## Features

| | Feature | Description |
|---|---|---|
| **Exec** | Gated `<project> exec "<cmd>"` | AST-parsed, policy-gated SSH commands |
| **Exec** | `<project> read <path>` / `write <path>` | File operations with path-glob rules + size caps |
| **Exec** | `<project> forward L:H:R` | Port forwarding with TTL + opaque-tunnel audit |
| **Exec** | `--yolo` bypass | Audited as `yolo_invocation`; killable via `disable_yolo` global flag |
| **Project** | Multi-target | One project, many hosts; pick per-call with `--on <target>` |
| **Project** | ssh-config import | `--import-ssh-config <alias>` materializes user/host/port/key |
| **Project** | Inline editor | `safessh project add` / `project edit` interactive prompts |
| **Policy** | Categories + AST | `read:safe`, `read:state`, `filesystem:write`, `network:tunnel`, … |
| **Policy** | Per-call rules | One-time approve, timed (N min), always (pattern), block (deny+log) |
| **Policy** | File rules | Path-glob allow/approve/deny/block per category |
| **Audit** | JSONL append-only | Authoritative log; rotates at 100 MiB; redaction always on |
| **Audit** | Lazy SQLite index | Built on demand from JSONL; rebuildable with `rm audit.db` |
| **Audit** | Structured query | `--since/--until/--decision/--exit-code/--target/--limit/--format` |
| **TUI** | Projects screen | `a` add · `e` edit · `i` import · live watcher |
| **TUI** | Approvals screen | Approve / deny / always / timed / block — keyboard-driven |
| **TUI** | Rules screen | Browse persistent rules; revoke individually |
| **TUI** | Audit screen | Filter, paginate, no event cap (SQLite-backed) |
| **Skill** | 6 adapters | claude-code · agents-md · cursor · gemini-cli · codex · plain |
| **Skill** | `skill install --target all` | Walks the supported (target, scope) matrix |
| **Skill** | `skill update [--dry-run]` | Re-renders the embedded body and rewrites every installed copy |
| **Skill** | `skill detect` | Per-target install status (`current`, `drift`, `not detected`, …) |
| **Ops** | Atomic file writes | Every config / state mutation goes through `tempfile + persist` |
| **Ops** | No outbound network | Skill content embedded; no telemetry; no auto-update |

---

## How it fits in your agent loop

1. The agent runs `safessh prod exec "rsync -av build/ /srv/app/"`.
2. `safessh-policy` parses the command into an AST and checks it against the project's allow list, persistent rules, and category catalogue.
3. **Allow** → `safessh-ssh` shells out via `ssh` (or `sftp` / `ssh -L` for read/write/forward) using a `ControlMaster` socket so subsequent calls reuse the connection.
4. **RequireApproval** → a `BLOCKED:` token block is printed to stdout for the agent to parse, and a `PendingRequest` is persisted. You approve in another shell with `safessh approve <token>` (once / timed / always / block) or in the TUI.
5. **Deny / Block** → exit non-zero immediately with a clear category and reason.
6. **Every step** writes a JSONL audit row before any user-visible output.

The agent never sees the SSH layer. You see everything via `audit query` or the TUI.

---

## Commands

| Command | Description |
|---|---|
| `safessh <project> exec "<cmd>"` | Run a gated command |
| `safessh <project> read <path>` | Read a remote file (size-capped, audited) |
| `safessh <project> write <path>` | Write a remote file (categorized, audited) |
| `safessh <project> forward L:H:R` | Open a port forward (TTL'd, opaque) |
| `safessh project {add,edit,list,remove}` | Manage projects (interactive when omitted args) |
| `safessh policy show <project>` | Print resolved policy for a project |
| `safessh approve <token> [--timed N \| --always \| --block]` | Resolve a pending approval |
| `safessh audit query [--since ...] [--decision ...] [--format ...]` | Structured audit query |
| `safessh tui` | Launch the review TUI |
| `safessh skill {install,uninstall,update,detect,show,check}` | Manage agent skill files |
| `safessh tunnels {list,close}` | Manage open port forwards |

See [`docs/cli-reference.md`](docs/cli-reference.md) for every flag and exit code.

---

## Agent integration

| Target | Install paths | Format |
|---|---|---|
| `claude-code` | `~/.claude/skills/safessh.md` (user) · `<cwd>/.claude/skills/safessh.md` (project) | YAML frontmatter |
| `agents-md` | `<cwd>/AGENTS.md` (project) | `## safessh` section |
| `cursor` | `<cwd>/.cursor/rules/safessh.md` (project) | Cursor frontmatter |
| `gemini-cli` | `~/.gemini/GEMINI.md` (user) · `<cwd>/GEMINI.md` (project) | `## safessh` section |
| `codex` | `~/.codex/AGENTS.md` (user) | `## safessh` section |
| `plain` | caller-supplied via `--path` | verbatim body |

```sh
safessh skill install --target all --scope user      # claude-code, gemini-cli, codex
safessh skill install --target all --scope project   # claude-code, agents-md, cursor, gemini-cli
safessh skill update --dry-run                       # show what `update` would change
safessh skill detect --format json                   # script-friendly install report
```

---

## Documentation

| Document | Description |
|---|---|
| **[Getting started](docs/getting-started.md)** | First-run walkthrough |
| **[CLI reference](docs/cli-reference.md)** | Every subcommand, flag, exit code |
| **[Projects](docs/projects.md)** | Project model, multi-target, ssh-config import |
| **[Policy](docs/policy.md)** | Categories, AST matching, file rules |
| **[Approvals](docs/approvals.md)** | Approval lifecycle, persistent rule stores |
| **[Audit](docs/audit.md)** | JSONL schema, SQLite index, query recipes, recovery |
| **[TUI](docs/tui.md)** | Screens, keymap, live filesystem watcher |
| **[Files](docs/files.md)** | File read/write, path-globs, safety invariants 13–14 |
| **[Tunnels](docs/tunnels.md)** | Port forwarding, TTL, opacity, `network:tunnel` policy |
| **[Skill](docs/skill.md)** | Multi-agent skill installation, update flow, detection |
| **[Security](docs/security.md)** | Threat model and the twelve safety invariants |
| **[Development](docs/development.md)** | Workspace layout, build, contributing |

---

## Requirements

- **macOS** or **Linux** · **Rust 1.75+** to build from source · **OpenSSH** (`ssh`, `sftp`, `ssh-agent`) on the host
- The `rusqlite` `bundled` feature compiles SQLite from source — needs a C compiler, which all major dev environments and CI containers ship with.

---

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
