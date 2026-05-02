# Changelog

All notable changes documented here. Format: [Keep a Changelog](https://keepachangelog.com/),
versioning: [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.4.1] - 2026-05-03

### Fixed
- `safessh tui` quit (`q` / `Esc` / `Ctrl-C`) restored the terminal
  but the process kept running until the user sent another signal.
  Cause: the keyboard event reader runs on tokio's blocking pool —
  unlike regular tokio tasks, blocking-pool tasks block runtime
  shutdown until they return. The reader sat in
  `crossterm::event::poll(100ms)` forever, so the runtime never
  shut down after the main loop broke. The reader now checks
  whether its sender has been dropped on every poll iteration, so
  it returns within 100 ms of the TUI loop exiting.
- `crates/safessh-tui/tests/watcher.rs` was intermittently failing on
  macos-14 CI because macOS FSEvents emits backlog events for the
  directories `ensure_dirs()` had just created, and they landed inside
  the watcher's first 200 ms debounce window alongside the test's own
  write — making the first event off the channel non-deterministic.
  Tests now drain the channel after a 300 ms warmup before asserting,
  so the channel sees only the test's own write. No runtime behavior
  changes; this is a CI-only fix.

## [0.4.0] - 2026-05-02

### Added
- `safessh <project> [--on <target>] forward <local>:<remote_host>:<remote_port>` —
  open a port forward (`ssh -L <spec> -N`) under a detached supervisor that
  enforces the project's `output.tunnel_ttl_minutes` (default 30 min) via
  SIGTERM → 5s grace → SIGKILL (SAFETY-INVARIANT-8).
- `safessh tunnels list` — show active tunnels, their forward specs, and
  remaining TTL minutes; reaps records whose supervisor PID is dead.
- `safessh tunnels close <id>` — cooperative close (SIGTERM → 5s poll →
  SIGKILL fallback).
- `network:tunnel` policy category, default-deny per SAFETY-INVARIANT-15.
  Can be approved Once / Timed / Always from CLI prompts and the TUI
  Approvals screen.
- New `state/tunnels/<id>.toml` store, atomically written
  (SAFETY-INVARIANT-5).
- Two new audit event types: `tunnel_open` (with `opacity_warning` field)
  and `tunnel_close` (with kebab-case `reason`: `ttl-expired`,
  `user-close`, `ssh-died`, `parent-shutdown`, `failed-to-start`).
- `SshDriver::open_tunnel(target, spec) -> Box<dyn TunnelHandle>` and a
  mock `MockTunnelHandle` for unit tests.
- TUI Approvals screen: tunnel approval variant — Always/Timed/Block actions
  on `network:tunnel` pendings write category-tagged rules.
- TUI Audit screen: `tunnel_open` rows carry an `[opaque]` tag.
- New docs: [`docs/tunnels.md`](docs/tunnels.md). Updated:
  [`docs/security.md`](docs/security.md), [`docs/policy.md`](docs/policy.md),
  [`docs/cli-reference.md`](docs/cli-reference.md).

## [0.3.0] - 2026-05-02

### Added
- `safessh <project> [--on <target>] read <path>` — fetch a remote file over
  sftp, framed as `<stdout>…</stdout>` identical to `exec` output. Capped by
  the project's `output_cap_bytes` (default 1 MiB).
- `safessh <project> [--on <target>] write <path>` — upload stdin to a remote
  path via sftp. Writes atomically (temp file + rename on the remote side);
  SAFETY-INVARIANT-13 preserves this — a partial upload is never visible at
  the destination path.
- `[[policy.file_rules]]` TOML array in project files: path-glob–based allow /
  require-approval / deny rules for file ops. Schema is additive — v0.2 project
  files with no `file_rules` key continue to work unchanged (backward-compat).
- Preset deny-list for sensitive remote paths (`/etc/shadow`, `~/.ssh/id_*`,
  `~/.aws/credentials`, and others). The preset is evaluated before any
  project-level `file_rules` so a project cannot accidentally allow a path
  that the preset blocks (SAFETY-INVARIANT-14).
- `SshDriver` trait extended with `read_file(&self, path) -> Result<Bytes>` and
  `write_file(&self, path, data) -> Result<()>`. The mock driver (`MockSshDriver`)
  implements both, keeping unit tests free of real SSH.
- Two new audit event-type pairs:
  - `file_read` (attempt) / `file_read_complete` (outcome with byte count).
  - `file_write` (attempt) / `file_write_complete` (outcome with byte count).
  Audit-write still happens before user-visible output (SAFETY-INVARIANT-4).
- TUI Rules screen: new **File** tab alongside Timed / Always / Blocked,
  listing `[[policy.file_rules]]` entries from the active project.
- TUI Approvals screen: file-rule action variant — approve / deny a pending
  file-op request from the same 5-action UI as `exec` approvals.
- TUI Audit screen: file event one-liners (`file_read` / `file_write` events
  display remote path, byte count, and outcome alongside existing exec events).
- New docs: [`docs/policy.md`](docs/policy.md) (categories, AST matching,
  `[[policy.file_rules]]` schema) and [`docs/files.md`](docs/files.md) (file
  read / write subcommands, path-glob rules, safety invariants 13–14).
- Skill markdown (`crates/safessh-skill/src/content/safessh.md`) updated: added
  `read` / `write` usage, exit-code entries for file ops, and guidance on
  `[[policy.file_rules]]` configuration.

## [0.2.0] - 2026-05-02

### Added
- Multi-target project support. Each project can hold multiple targets
  (`SshConfigAlias` or `Inline`); a `--on <name>` flag on `<project> exec`
  picks one for that invocation. Without the flag the project's
  `default_target` is used (back-compat with v0.1).
- `safessh project target add/list/remove` subcommands for managing the
  targets array of an existing project without hand-editing TOML.
- `safessh project add --import-ssh-config <alias>` snapshots a Host
  block from `~/.ssh/config` (or `$SSH_CONFIG_PATH`) into a new project
  as an `Inline` target. Conflicts with `--alias` / `--host` / `--user`
  at the clap level. ProxyJump intentionally not imported (use `--alias`
  instead when ProxyJump is required).
- `safessh-storage::ssh_config` module: parses `~/.ssh/config` host
  aliases (excluding wildcards) and caches the parse to
  `~/.cache/safessh/ssh-config-snapshot.toml`, mtime-invalidated.
- New `safessh-tui` crate (ratatui 0.28 + crossterm 0.28). Four
  screens — Projects, Approvals, Rules, Audit — sharing a single
  `notify-debouncer-mini` watcher so changes from the CLI / hand-edits /
  another safessh process arrive within ~250 ms.
- TUI Approvals screen exposes the same 5 actions as `safessh approve`
  (Once / Timed / Always / Deny / Block), writing through the same
  storage API. CLI/TUI parity guaranteed by SAFETY-INVARIANT-12.
- TUI Rules screen with three tabs (Timed / Always / Blocked) and `d`
  to delete the selected rule. Adds `TimedStore::remove` to mirror
  `AlwaysStore::remove` and `BlockedStore::remove`.
- TUI Audit screen with offset-tracked tail (only new bytes are read on
  `FsEvent::AuditAppended`) and project / event-type / grep filters.
  `g` / `G` jump to top / bottom; `G` resumes auto-scroll.
- TUI ssh-config import dialog on the Projects screen (`i` key).
  Multi-select: Space toggles, Enter creates one project per checked
  alias (skipping name collisions), Esc cancels.
- External-edit toast: when the watcher fires `ProjectsChanged` or
  `ApprovalsChanged`, a 3 s banner appears reading
  `config changed externally — reloaded`. Replaces any earlier toast
  (no stacking).
- Tab / Shift-Tab cycle screens; `?` toggles a help overlay reachable
  from any screen. `safessh_tui::help_text()` is `pub` so docs/tui.md
  reproduces the keymap from the binary verbatim.
- `safessh tui` CLI subcommand — replaces the v0.1 placeholder. Refuses
  non-TTY environments (exit 1).
- Three new user docs: `docs/projects.md`, `docs/tui.md`,
  `docs/approvals.md`. README features table flipped for v0.2 status.

### Changed
- `safessh <project> exec` argv parser is unified into a single
  `parse_extras` helper that strips both `--yolo` and `--on <name>`
  (clap's `external_subcommand` doesn't see them). Both placements
  (anywhere in argv) work identically.
- `safessh_storage::paths::Paths` now derives `Clone` so screens can
  store their own handle without re-walking env vars on every reload.

### Fixed
- macOS fsevents reports paths under `/private/var/folders/...` while
  watch dirs were `/var/folders/...`; the watcher now canonicalizes
  watch directories so `starts_with` comparisons line up.
- `Tokio::sync::mpsc::Sender::blocking_send` from notify's worker thread
  was dropping events; switched to `try_send` (the channel is sized at
  64 and dropping a wakeup is acceptable when full).

## [0.1.0] - 2026-04-30

### Added
- CLI for running policy-gated SSH commands on user-configured projects.
- AST-based policy engine with semantic categories (read:safe, destructive:filesystem,
  destructive:disk, destructive:db, db:read, db:write, privilege:escalation,
  system:control, network:listen, exec:opaque).
- Approval lifecycle: once / timed / always / deny / block, via TTY prompt or
  headless structured-deny tokens.
- JSONL audit log with redaction (AWS keys, JWTs, bearer tokens, private-key
  blocks, password params).
- Project management subcommands (add/list/edit/remove).
- Skill install for Claude Code (`~/.claude/skills/safessh.md`) and `AGENTS.md`.
- `--yolo` flag for trusted operations (audited; refusable via global
  `disable_yolo`).
- Homebrew + curl + cargo install paths via `cargo-dist` release pipeline.
- Conventional Commits + GitHub Actions CI (fmt, clippy, test, integration).
- End-to-end integration tests against `linuxserver/openssh-server` containers.
