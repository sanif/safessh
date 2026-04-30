# Changelog

All notable changes documented here. Format: [Keep a Changelog](https://keepachangelog.com/),
versioning: [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
