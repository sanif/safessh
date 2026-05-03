# safessh

A personal CLI proxy for LLM-driven SSH operations. Credentials stay yours; commands get gated.

**Status:** v0.6.0

`safessh` is a single Rust binary that sits between an LLM agent (Claude Code, AGENTS.md-aware tools) and your servers. The agent gets a fixed CLI surface; you keep the keys, the policy, and the audit trail.

## Install

### Homebrew

```sh
brew install sanif/tap/safessh
```

(Formula published to `sanif/homebrew-tap` on the first tagged release.)

### curl

```sh
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/sanif/safessh/releases/latest/download/safessh-cli-installer.sh | sh
```

### cargo

From a checkout, while the crate is unpublished:

```sh
cargo install --path crates/safessh-cli
```

## Quick start

```sh
# 1. Add a project — interactive prompts walk you through name, target source
#    (ssh-config alias or inline host/user/key), and let you pick a private
#    key from ~/.ssh/ via fuzzy-search.
safessh project add

# 2. Install the skill so Claude Code knows the safessh workflow.
safessh skill install --target claude-code --scope user

# 3. Run a command. Read-only stuff is allowed by default.
safessh prod exec "ls /var"
```

Prefer to script it? Pass flags and the interactive flow is bypassed:

```sh
safessh project add prod --alias my-prod-host
safessh project add staging --host stg.internal --user deploy --port 2222
safessh project add prod-imported --import-ssh-config my-prod-host
```

Output is framed for agents to parse:

```
<stdout>
... captured stdout ...
</stdout>
<stderr>
... captured stderr ...
</stderr>
<exit code="0" duration="34ms"/>
```

## Agent integration

| Target | Install command | Default location |
|---|---|---|
| Claude Code | `safessh skill install --target claude-code --scope user` | `~/.claude/skills/safessh.md` |
| AGENTS.md   | `safessh skill install --target agents-md --scope project` | `<cwd>/AGENTS.md` (`## safessh` section) |

Run `safessh skill check` to verify what's installed and whether it matches the binary you have on disk.

## Features

| Feature | Status |
|---|---|
| SSH command execution (gated) | Available in v0.1 |
| Multi-target projects (`--on <target>`) | Available in v0.2 |
| ssh-config import (CLI + TUI) | Available in v0.2 |
| TUI: projects / approvals / rules / audit screens | Available in v0.2 |
| Live filesystem watcher (TUI auto-reloads on external edits) | Available in v0.2 |
| File operations (read / write with path-globs) | Available in v0.3 |
| Port forwarding | Available in v0.4 |
| Interactive `project add` / `project edit` (ssh-config import, fuzzy key picker) | Available in v0.4.2 |
| SQLite audit index | Available in v0.5 |
| Multi-agent skill targets (Cursor, Gemini, Codex) | Available in v0.6 |

## Documentation

- [docs/getting-started.md](docs/getting-started.md) — first-run walkthrough.
- [docs/cli-reference.md](docs/cli-reference.md) — every subcommand and flag, with exit codes.
- [docs/projects.md](docs/projects.md) — projects, multi-target, ssh-config import.
- [docs/tui.md](docs/tui.md) — TUI screens, keymap, live updates.
- [docs/approvals.md](docs/approvals.md) — approval lifecycle and persistent rule stores.
- [docs/skill.md](docs/skill.md) — installing the skill into your agent framework.
- [docs/security.md](docs/security.md) — threat model and the twelve safety invariants.
- [docs/policy.md](docs/policy.md) — policy categories, AST matching, `[[policy.file_rules]]`.
- [docs/files.md](docs/files.md) — file read / write subcommands, path-glob rules, safety invariants 13–14.
- [docs/tunnels.md](docs/tunnels.md) — port forwarding, TTL, opacity, `network:tunnel` policy.
- [docs/development.md](docs/development.md) — workspace layout, build, contributing.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
