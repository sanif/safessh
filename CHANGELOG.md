# Changelog

All notable changes documented here. Format: [Keep a Changelog](https://keepachangelog.com/),
versioning: [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
