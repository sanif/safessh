# Agent skills

A *skill* is a single markdown document that teaches an LLM agent how to drive `safessh` — the subcommand surface, the framing format, the `BLOCKED:` token shape, and the workflow conventions. The same canonical body is shipped to every supported agent framework, wrapped in the format that framework expects.

The skill content lives inside the `safessh` binary (`include_str!`). It never reaches out to the network. Updates ride along with the binary you install.

## Targets supported in v0.1

| Target id | Framework | Default location |
|---|---|---|
| `claude-code` | [Claude Code](https://www.anthropic.com/claude-code) | `~/.claude/skills/safessh.md` (user) or `<cwd>/.claude/skills/safessh.md` (project) |
| `agents-md` | Tools that read `AGENTS.md` (project-scoped) | `<cwd>/AGENTS.md` (`## safessh` section) |

Additional targets (Cursor, Gemini, Codex) are planned for v0.6.

## Install

Run the install command for whichever framework(s) you use:

```sh
# Claude Code, user scope (recommended for personal machines).
safessh skill install --target claude-code --scope user

# Claude Code, project scope (this checkout only).
safessh skill install --target claude-code --scope project

# AGENTS.md, project scope (the only supported scope for this target).
safessh skill install --target agents-md --scope project

# Custom path.
safessh skill install --target claude-code --scope path --path /some/dir

# Fan out across whatever is detected on this machine.
safessh skill install --target all
```

`--target all` walks the detection logic (looks for `~/.claude`, `<cwd>/.claude`, etc.) and installs only where the framework appears to be set up. If nothing is detected, it prints `No agent frameworks detected.` and exits 0.

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

## Updating after a binary upgrade

In v0.1 the answer is: **re-run install**.

```sh
safessh skill install --target all
```

`install` is idempotent: it overwrites the Claude Code skill file atomically and replaces the `## safessh` section in any AGENTS.md it manages, preserving the rest of the file.

A dedicated `safessh skill update` subcommand is planned for v0.6.

## Uninstall

```sh
safessh skill uninstall --target claude-code --scope user
safessh skill uninstall --target agents-md --scope project
```

For `claude-code` this deletes the file. For `agents-md` it strips only the `## safessh` section, leaving the rest of `AGENTS.md` intact.

## Why not auto-update?

`safessh` makes **no outbound network calls** of its own (safety invariant 10). The skill is `include_str!`'d into the binary at build time and never fetched. This means:

- You always know what the agent is reading: it matches the binary in `which safessh`.
- An upgrade flow is explicit (binary → `skill install`), so a stale binary can't quietly serve an old skill.
- No telemetry, no version-check phone-home, no remote-skill exfiltration vector.

The trade-off is that you have to run `skill install` after upgrading. `skill check` makes the drift visible.
