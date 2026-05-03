# Agent skills

A *skill* is a single markdown document that teaches an LLM agent how to drive `safessh` — the subcommand surface, the framing format, the `BLOCKED:` token shape, and the workflow conventions. The same canonical body is shipped to every supported agent framework, wrapped in the format that framework expects.

The skill content lives inside the `safessh` binary (`include_str!`). It never reaches out to the network. Updates ride along with the binary you install.

## Supported targets

`safessh` ships six adapters. Each maps a target id to the file (or section of a file) that the matching agent framework reads, and an explicit set of scopes (`user`, `project`, or `path`) that target supports.

| Target id | Framework | User scope | Project scope | File shape |
|---|---|---|---|---|
| `claude-code` | [Claude Code](https://www.anthropic.com/claude-code) | `~/.claude/skills/safessh.md` | `<cwd>/.claude/skills/safessh.md` | Standalone file with YAML frontmatter |
| `agents-md` | Tools that read `AGENTS.md` | — | `<cwd>/AGENTS.md` | `## safessh` section |
| `cursor` | [Cursor](https://cursor.com/) rules | — | `<cwd>/.cursor/rules/safessh.md` | Standalone file |
| `gemini-cli` | [Gemini CLI](https://github.com/google-gemini/gemini-cli) | `~/.gemini/GEMINI.md` | `<cwd>/GEMINI.md` | `## safessh` section |
| `codex` | [Codex](https://openai.com/codex) `AGENTS.md` | `~/.codex/AGENTS.md` | — | `## safessh` section |
| `plain` | Anything else | — | — (requires `--path`) | Standalone file |

**Detection rules** (used by `skill detect` and the section-present heuristic in `skill check`):

| Target | User scope detected when… | Project scope detected when… |
|---|---|---|
| `claude-code` | `~/.claude/` exists | `<cwd>/.claude/` exists |
| `agents-md` | not applicable | always — `<cwd>/AGENTS.md` will be created if needed |
| `cursor` | not applicable | `<cwd>/.cursor/` exists |
| `gemini-cli` | `~/.gemini/` exists | `<cwd>/GEMINI.md` already exists |
| `codex` | `~/.codex/` exists | not applicable |
| `plain` | never auto-detected — must be installed via `--scope path` | not applicable |

## Install

Run the install command for whichever framework(s) you use:

```sh
# Claude Code, user scope (recommended for personal machines).
safessh skill install --target claude-code --scope user

# Claude Code, project scope (this checkout only).
safessh skill install --target claude-code --scope project

# AGENTS.md, project scope (the only supported scope for this target).
safessh skill install --target agents-md --scope project

# Cursor rules, project scope.
safessh skill install --target cursor --scope project

# Gemini CLI, user scope.
safessh skill install --target gemini-cli --scope user

# Codex, user scope.
safessh skill install --target codex --scope user

# Plain file (anything else). Requires --scope path --path.
safessh skill install --target plain --scope path --path ./docs/safessh.md

# Custom path for any target.
safessh skill install --target claude-code --scope path --path /some/dir

# Fan out across all targets supported at the requested scope.
safessh skill install --target all --scope user
safessh skill install --target all --scope project
```

`--target all` walks the supported (target, scope) matrix and installs every pair that has a default path at the supplied `--scope`. It is **not** detection-based — it always installs every supported pair, creating parent directories as needed. Pairs that have no install path for the requested scope (e.g., `agents-md` at `--scope user`) are skipped with a stderr note. `--scope path` is rejected with exit 2.

## What gets written

### Claude Code (`claude-code`)

A standalone markdown file with YAML frontmatter, written atomically:

```markdown
---
name: safessh
description: SSH proxy for running gated commands on user-configured servers without seeing credentials. Use when the user asks to run commands on a remote server they've configured in safessh.
---

# safessh

`safessh` is an SSH proxy that runs gated commands on user-configured remote
servers without exposing credentials to the agent...
```

The `description` field drives Claude Code's automatic skill activation, so the agent only loads the skill when the user is asking about remote-server operations.

### AGENTS.md (`agents-md`)

The skill is installed as an `## safessh` section appended to an `AGENTS.md` file. If the file already exists, any prior `## safessh` section is stripped and replaced — other sections are preserved verbatim.

```markdown
## safessh

# safessh

`safessh` is an SSH proxy that runs gated commands on user-configured remote
servers without exposing credentials to the agent...
```

## Inspect what would be installed

```sh
safessh skill show                       # claude-code by default
safessh skill show --target agents-md
```

Prints the formatted body (with the right wrapper) to stdout. Useful for piping to a custom location, diffing against an existing install, or seeing exactly what an agent will be reading.

## Drift detection

```sh
safessh skill check
```

For every detected framework / scope, this prints one of:

- `[claude-code user] not installed: <path>`
- `[claude-code user] installed (current): <path>`
- `[claude-code user] installed (DRIFT — re-run install): <path>`

It also prints the embedded skill body's hash so you can confirm two machines are running the same skill content.

A drift report typically means you upgraded the `safessh` binary but didn't refresh the on-disk skill copy.

## Detect what's installed

```sh
safessh skill detect              # fixed-width table
safessh skill detect --format json
```

`skill detect` walks every supported (target, scope) combination and reports, per pair, the resolved path and one of:

- `not detected` — the framework's parent directory does not exist.
- `detected, not installed` — parent exists but no skill file/section yet.
- `installed (current)` / `installed (section present)` — file or section matches the embedded body.
- `installed (drift)` — body diverges; run `skill update` to refresh.
- `requires --path` — emitted for `plain`, which has no default location.

Use `skill check` for the same information in a more conversational format (one line per scope plus the embedded-content hash). Use `skill detect --format json` from tooling or LLM agents.

## Updating after a binary upgrade

`skill update` re-renders the embedded skill body and rewrites every currently-installed copy. It does **not** create new files — pairs that aren't already installed are skipped. This is the recommended path after upgrading the `safessh` binary.

```sh
safessh skill update                         # update everything installed (both scopes)
safessh skill update --dry-run               # preview unified diff per file, change nothing
safessh skill update --target claude-code    # restrict to one target (repeatable)
safessh skill update --scope user            # restrict to one scope (default: both)
```

For section-style targets (`agents-md`, `gemini-cli`, `codex`) the `## safessh` section is replaced cleanly and the rest of the file is preserved. For file-style targets the file is rewritten atomically.

`skill install` remains idempotent if you'd rather force-create the file at the same time — `update` is the lighter-weight option once a copy already exists.

## Uninstall

```sh
safessh skill uninstall --target claude-code --scope user
safessh skill uninstall --target agents-md --scope project
safessh skill uninstall --target cursor --scope project
safessh skill uninstall --target gemini-cli --scope user
safessh skill uninstall --target codex --scope user
```

For file-style targets (`claude-code`, `cursor`, `plain`) this deletes the file. For section-style targets (`agents-md`, `gemini-cli`, `codex`) it strips only the `## safessh` section, leaving the rest of the host file intact.

## Why not auto-update?

`safessh` makes **no outbound network calls** of its own (safety invariant 10). The skill is `include_str!`'d into the binary at build time and never fetched. This means:

- You always know what the agent is reading: it matches the binary in `which safessh`.
- An upgrade flow is explicit (binary → `skill install`), so a stale binary can't quietly serve an old skill.
- No telemetry, no version-check phone-home, no remote-skill exfiltration vector.

The trade-off is that you have to run `skill install` after upgrading. `skill check` makes the drift visible.
