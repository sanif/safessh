# CLI reference

Everything the `safessh` binary exposes in v0.1. Run `safessh --help` for the live help text — this page exists to be readable as documentation and to list things `--help` doesn't show (exit codes, output framing).

## Global flags

| Flag | Effect |
|---|---|
| `--yolo` | Bypass the policy engine for this invocation. Refused with exit 13 if `disable_yolo = true` in the global config. Output cap, redactor, and audit logging still apply (yolo is logged as `yolo_invocation`). Works at top level (`safessh --yolo prod exec …`) or after the project (`safessh prod exec --yolo …`). |
| `--version` | Print the safessh version. |
| `--help` | Print help. Available on every subcommand. |

## `safessh <project> exec "<command>"`

Run a command on the project's default target. The project name is captured as an external subcommand, so the literal command string is passed through verbatim.

**Usage:**

```sh
safessh <project> exec "<command>"
safessh <project> exec --yolo "<command>"
```

**Example:**

```sh
safessh prod exec "ls -la /var/log"
safessh staging exec 'systemctl status nginx'
```

The command runs through the policy engine. Possible outcomes:

- **Allow** — runs immediately, output is framed (see [Output framing](#output-framing)).
- **RequireApproval** — exits 10 with a `BLOCKED:` token (headless) or shows a 5-action prompt (TTY).
- **Block** — exits 11 with a rule reference.
- **Deny** — exits 12 with a reason.

## `safessh project add <name> [flags]`

Register a new project.

**Flags:**

| Flag | Description |
|---|---|
| `--alias <alias>` | Reuse an existing `~/.ssh/config` alias. Mutually exclusive with `--host`/`--user`. |
| `--host <host>` | Hostname to connect to. Requires `--user`. |
| `--user <user>` | Remote username. Requires `--host`. |
| `--port <port>` | SSH port. Default `22`. |

Specify either `--alias` **or** the `--host`/`--user` pair. The new project starts with `allow = ["read:safe", "file:read"]` and an empty deny / require-approval list.

**Examples:**

```sh
safessh project add prod --alias my-prod-host
safessh project add staging --host staging.example.com --user deploy
```

## `safessh project list`

Print the names of all configured projects, one per line.

## `safessh project edit <name>`

Open the project's TOML file in `$EDITOR` (falling back to `vi`) for manual edits. The file is loaded once before launching the editor; if the project doesn't exist, exit 1.

## `safessh project remove <name>`

Delete the project's TOML file. Approval rules tied to the project name are not touched — remove them manually if you're recycling a name.

## `safessh policy show <category|project>`

Inspect either a built-in category or a saved project's policy.

**Category names** — pass any of:

- Shell categories: `read:safe`, `destructive:filesystem`, `destructive:disk`, `privilege:escalation`, `system:control`, `network:listen`, `exec:opaque`.
- SQL categories: `db:read`, `db:write`, `destructive:db`.

**Examples:**

```sh
safessh policy show destructive:filesystem
safessh policy show prod
```

When passed a project name, prints the project's `allow` / `require_approval` / `deny` lists.

## `safessh approve <token> [flags]`

Apply an approval to a pending request.

**Flags:**

| Flag | Effect |
|---|---|
| (none) | Once-only. The pending entry is consumed; re-run the original command. |
| `--timed` | Allow this command pattern for `--minutes` (default 30). |
| `--minutes <N>` | Override the timed-allow duration. Implies `--timed` semantics. |
| `--always` | Persist the pattern in the project's allow list. |
| `--block` | Convert to a persistent block. Future invocations of this pattern exit 11. |

Flags are evaluated in priority order: `--block` > `--always` > `--timed` > once. Passing more than one is unusual but won't error.

**Examples:**

```sh
safessh approve abc123def456
safessh approve abc123def456 --timed --minutes 60
safessh approve abc123def456 --always
safessh approve abc123def456 --block
```

## `safessh audit query [flags]`

Read the JSONL audit log line-by-line, filter, and print matching lines verbatim.

**Flags:**

| Flag | Effect |
|---|---|
| `--project <name>` | Match only events whose `project` field equals `<name>`. |
| `--type <event_type>` | Match only events whose `event_type` field equals `<event_type>` (e.g. `exec_attempt`, `exec_complete`, `approval_requested`, `yolo_invocation`). |
| `--grep <pattern>` | Substring match against the raw JSONL line. |

A missing audit log is treated as empty output (not an error).

**Examples:**

```sh
safessh audit query --project prod
safessh audit query --type approval_requested
safessh audit query --grep "destructive:db"
```

## `safessh skill install --target <target> [flags]`

Install the embedded skill for the specified agent target.

**Flags:**

| Flag | Effect |
|---|---|
| `--target <claude-code\|agents-md\|all>` | Required. `all` fans out across detected frameworks. |
| `--scope <user\|project\|path>` | Where to install. Default `user`. |
| `--path <dir>` | Required when `--scope path`. |

**Examples:**

```sh
safessh skill install --target claude-code --scope user
safessh skill install --target agents-md --scope project
safessh skill install --target all
```

## `safessh skill uninstall --target <target> [flags]`

Reverse of `install`. For Claude Code this deletes the file; for AGENTS.md it strips the `## safessh` section in place.

## `safessh skill show [--target <target>]`

Print the formatted skill content for the given target (default `claude-code`) to stdout. Useful for piping into your own automation.

## `safessh skill check`

Detect installed agent frameworks and report, per scope:

- Not installed.
- Installed and current.
- Installed but drifted (re-run `install` to refresh).

Also prints the embedded-content hash so you can compare across machines.

## Output framing

`safessh <project> exec` writes a byte-stable wrapper to **stdout**:

```
<stdout>
... captured remote stdout ...
</stdout>
<stderr>
... captured remote stderr ...
</stderr>
<exit code="N" duration="34ms"/>
```

When output is truncated by the project's caps, the closing tag adds an attribute: `<exit code="N" duration="34ms" truncated="true"/>`.

`safessh`'s own diagnostics go to **stderr** with the prefix `safessh: <category>: <message>`, except for the multi-line `BLOCKED:` block (which the agent parses verbatim).

## Exit codes

| Code | Meaning | Agent should… |
|---:|---|---|
| `0` | Success. | Proceed. |
| `1` | Generic error (project not found, missing config). | Report to human. |
| `2` | Usage error. | Report and stop. |
| `10` | Approval required. | Surface the token and prompt the human. **Do not retry.** |
| `11` | Persistently blocked (matched a block-rule). | Report. **Do not retry.** |
| `12` | Denied for this invocation. | Report. May re-prompt with explicit human ask. |
| `13` | Yolo refused (disabled globally). | Report and stop. |
| `20` | SSH failure (subprocess error). | Report; may retry. |
| `21` | Connection refused / timeout. | Report; may retry. |
| `30` | Output exceeded cap (truncated). | Output up to the cap is still valid. |
| `40` | Storage error (config corrupt, lock held). | Report and stop. |
| `50` | Audit-write failure (operation refused). | Report and stop. **Do not yolo around it.** |
