# CLI reference

Everything the `safessh` binary exposes through v0.4. Run `safessh --help` for the live help text — this page exists to be readable as documentation and to list things `--help` doesn't show (exit codes, output framing).

## Global flags

| Flag | Effect |
|---|---|
| `--yolo` | Bypass the policy engine for this invocation. Refused with exit 13 if `disable_yolo = true` in the global config. Output cap, redactor, and audit logging still apply (yolo is logged as `yolo_invocation`). Works at top level (`safessh --yolo prod exec …`) or after the project (`safessh prod exec --yolo …`). |
| `--version` | Print the safessh version. |
| `--help` | Print help. Available on every subcommand. |

## `safessh <project> exec "<command>"`

Run a command on a project target (the project's `default_target` unless `--on` is given). The project name is captured as an external subcommand, so the literal command string is passed through verbatim.

**Usage:**

```sh
safessh <project> exec "<command>"
safessh <project> exec --yolo "<command>"
safessh <project> --on <target> exec "<command>"
safessh <project> exec --on <target> "<command>"      # equivalent
safessh <project> exec --on=<target> "<command>"      # equivalent
```

**Example:**

```sh
safessh prod exec "ls -la /var/log"
safessh prod --on db exec "psql -c 'select 1'"
safessh staging exec 'systemctl status nginx'
```

The command runs through the policy engine. Possible outcomes:

- **Allow** — runs immediately, output is framed (see [Output framing](#output-framing)).
- **RequireApproval** — exits 10 with a `BLOCKED:` token (headless) or shows a 5-action prompt (TTY).
- **Block** — exits 11 with a rule reference.
- **Deny** — exits 12 with a reason.

`--on` resolution: the value must match a `[[targets]] name` in the project TOML. Unknown target names exit 2 with `safessh: usage: usage: no such target: <name>`. Without `--on` the project's `default_target` is used (back-compat with v0.1).

See [docs/projects.md](projects.md) for the multi-target model.

## `safessh <project> [--on <target>] read <remote-path>`

Fetch a remote file over sftp and write its contents to **stdout** using the same framing as `exec`.

**Usage:**

```sh
safessh <project> read <remote-path>
safessh <project> --on <target> read <remote-path>
```

**Example:**

```sh
safessh prod read /etc/hostname
```

The path is checked against the preset deny-list (sensitive paths such as `/etc/shadow`, `~/.ssh/id_*`) and then against the project's `[[policy.file_rules]]` in order. If no rule matches, the decision is `RequireApproval`. See [docs/files.md](files.md) for the full matching precedence and [docs/policy.md](policy.md) for the `[[policy.file_rules]]` schema.

Output is capped at the project's `output_cap_bytes` (default 1 MiB). A truncated read exits with code 30.

**Exit codes:** same table as `exec` — see [Exit codes](#exit-codes). File-operation-specific outcomes use the same codes (10 = approval required, 11 = blocked, 12 = denied, 30 = truncated).

## `safessh <project> [--on <target>] write <remote-path>`

Upload stdin to a remote path over sftp. The upload is atomic: the driver writes to a temp file and renames it into place on the remote side (SAFETY-INVARIANT-13 — a partial upload is never visible at the destination path).

**Usage:**

```sh
safessh <project> write <remote-path>
safessh <project> --on <target> write <remote-path>
```

**Example:**

```sh
echo "hello" | safessh prod write /tmp/hello.txt
```

Path matching follows the same precedence as `read`: preset deny-list → `[[policy.file_rules]]` → `RequireApproval`. The preset deny-list blocks writes to sensitive paths regardless of project config (SAFETY-INVARIANT-14).

**Exit codes:** same table as `exec` — see [Exit codes](#exit-codes).

## `safessh <project> [--on <target>] forward <spec>`

Open a local port forward (`ssh -L <spec> -N`) under a detached supervisor. The CLI exits immediately; the supervisor enforces the project's TTL and writes a `tunnel_close` audit event when it terminates.

**Usage:**

```sh
safessh <project> forward <local_port>:<remote_host>:<remote_port>
safessh <project> --on <target> forward <local_port>:<remote_host>:<remote_port>
```

**Example:**

```sh
safessh prod forward 5432:db.internal:5432
safessh prod --on db forward 15432:localhost:5432
```

**Output (success):**

```
tunnel open id=ab3fzc9p spec=5432:db.internal:5432 ttl=30min expires=2026-05-02T14:00:00Z
```

Policy gate: the operation is classified as `network:tunnel`, which is default-deny (SAFETY-INVARIANT-15). See [docs/tunnels.md](tunnels.md) for TTL settings, opacity notes, and troubleshooting.

**Exit codes:**

| Code | Meaning |
|---:|---|
| `0` | Tunnel open, supervisor running. |
| `2` | Bad spec format or out-of-range port. |
| `10` | Approval required. |
| `11` | Persistently blocked. |
| `12` | Denied. |
| `20` | SSH failure (auth error, host unreachable). |
| `40` | Storage error. |
| `50` | Audit-write failure. |

## `safessh project add [name] [flags]`

Register a new project. The default form is **interactive**:

```sh
safessh project add
```

Walks you through name validation, target source (ssh-config alias vs inline), alias mode (reference vs snapshot), inline host/user/port/identity-key (fuzzy-pick from `~/.ssh/` or paste a path), and an optional ProxyJump. Prints the resulting TOML for review before saving. Requires a TTY; in non-TTY contexts (CI, agents, piped scripts) the command refuses with exit 2.

For scripted use, pass any of the flags below — the interactive flow is bypassed and a positional `<name>` is required:

| Flag | Description |
|---|---|
| `--alias <alias>` | Reuse an existing `~/.ssh/config` alias (lazy resolution at exec time). Mutually exclusive with `--host`/`--user`/`--import-ssh-config`. |
| `--host <host>` | Hostname to connect to. Requires `--user`. |
| `--user <user>` | Remote username. Requires `--host`. |
| `--port <port>` | SSH port. Default `22`. |
| `--import-ssh-config <name>` | Snapshot `host`/`user`/`port`/`identity_file` from the matching `Host <name>` block of `~/.ssh/config` (or `$SSH_CONFIG_PATH`) into a new `Inline` target. Conflicts with `--alias`/`--host`/`--user` (clap exit 2). ProxyJump is not imported — use `--alias` if you need it. |

The new project starts with `allow = ["read:safe", "file:read"]` and an empty deny / require-approval list (whether created interactively or via flags).

**Examples (scripted):**

```sh
safessh project add prod --alias my-prod-host
safessh project add staging --host staging.example.com --user deploy
safessh project add prod-imported --import-ssh-config my-prod-host
```

See [docs/projects.md](projects.md#adding-projects) for the alias-vs-import decision matrix.

## `safessh project target add <project> --name <name> [flags]`

Append a new target to an existing project's `targets` array.

| Flag | Description |
|---|---|
| `--name <name>` | Target name (required, unique within the project). |
| `--alias <alias>` | SshConfigAlias target. Mutually exclusive with `--host`/`--user`. |
| `--host <host>` | Inline target host. Requires `--user`. |
| `--user <user>` | Inline target user. Requires `--host`. |
| `--port <port>` | Inline target port. Default `22`. |
| `--identity <path>` | Path to an identity file. Inline targets only. |
| `--proxy-jump <host>` | ProxyJump host. Inline targets only. |

Duplicate `--name` within the same project exits 2.

**Examples:**

```sh
safessh project target add prod --name db --alias prod-db
safessh project target add prod --name web --host web.prod.internal --user www
```

## `safessh project target list <project>`

Print one line per target in `<project>`. The project's `default_target` is marked `[default]`.

```
default [default]  alias=prod-host
db                 alias=prod-db
web                www@web.prod.internal:22
```

## `safessh project target remove <project> --name <name>`

Remove the named target. Refuses with exit 1 if `<name>` is the project's `default_target` (re-point it via `project edit` first). Unknown names exit 1 with `no such target: <name>`.

## `safessh project list`

Print the names of all configured projects, one per line.

## `safessh project edit [name]`

Interactively edit a project. Without a name argument, you're presented with a fuzzy-search picker over your existing projects; with a name, that step is skipped. Inside the loop you can:

- Add a target (alias or inline; reuses the same prompts as `project add`).
- Remove a target (refuses to remove the project's `default_target`).
- Change the default target (fuzzy-pick from existing target names).
- Toggle policy categories (`allow` / `require_approval` / `deny`) via a multi-select over the shipped category list.
- Save & exit, or discard & exit.

The current TOML is printed before the loop starts. Requires a TTY.

**Raw-TOML editing.** Set `SAFESSH_EDIT_RAW=1` to fall back to the legacy flow that opens the project file in `$EDITOR` (defaults to `vi`); the file is overwritten atomically when the editor exits. Useful for bulk edits where the prompts would be tedious. In raw mode, a positional `<name>` is required and `<name>` must already exist (otherwise exit 1).

```sh
SAFESSH_EDIT_RAW=1 safessh project edit prod
```

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

Query the audit log. As of v0.5 the command is backed by the SQLite index for fast filtered reads; if the index is missing or unreadable it falls back to a line-by-line scan of the JSONL log and prints `safessh: warning: audit index unavailable, falling back to log scan` to stderr.

**Flags:**

| Flag | Effect |
|---|---|
| `--project <name>` | Match only events whose `project` field equals `<name>`. |
| `--type <event_type>` | Match only events whose `event_type` field equals `<event_type>` (e.g. `exec_attempt`, `exec_complete`, `approval_requested`, `yolo_invocation`, `tunnel_close`). |
| `--grep <pattern>` | Substring match against the raw JSONL line. |
| `--since <when>` | Lower bound on `timestamp`. Accepts an RFC3339 timestamp (`2026-05-01T00:00:00Z`) or a humantime duration relative to now (`7d`, `24h`, `30m`). |
| `--until <when>` | Upper bound on `timestamp`. Same accepted shapes as `--since`. Must be later than `--since` (otherwise exit 2). |
| `--limit <N>` | Cap the number of matches returned. Default `100`. `0` is treated as no limit. |
| `--decision <kind>` | Match `data.decision`. One of `allow`, `require_approval`, `deny`, `block`. |
| `--exit-code <N\|N..M>` | Match `data.exit_code` exactly (`0`) or within an inclusive range (`1..255`). |
| `--target <name>` | Match the per-event `data.target` field (the project target the event was scoped to). Recorded for `exec_*` events from v0.5 onward; older events have no target and won't match. |
| `--format <jsonl\|table\|count>` | Output format. Default `jsonl` (raw lines, one per match). `table` prints a fixed-width view (`timestamp event_type project target decision exit`). `count` prints a single integer. |

A missing audit log is not an error: `--format count` prints `0`, other formats emit nothing and exit 0.

**Exit codes:** `0` on success (including empty result); `2` on invalid `--since`/`--until`/`--exit-code` syntax; `40` on storage errors that the log-scan fallback can't recover from.

**Examples:**

```sh
safessh audit query --project prod
safessh audit query --type approval_requested --since 7d
safessh audit query --decision deny --since 24h --format table
safessh audit query --exit-code 1..255 --format count
safessh audit query --target db --type exec_complete --limit 20
safessh audit query --grep "destructive:db" --format jsonl
```

## `safessh tunnels list`

List all tunnels whose supervisor process is still alive, and reap stale records for supervisors that have already exited.

**Usage:**

```sh
safessh tunnels list
```

**Output:**

```
ID        PROJECT  TARGET   SPEC                          OPENED               TTL
ab3fzc9p  prod     default  5432:db.internal:5432         2026-05-02T13:30:00Z 24min
r7qm4ws2  staging  db       15432:localhost:5432           2026-05-02T13:45:00Z 9min
```

Prints nothing (exit 0) if no active tunnels exist. `40` on storage error.

## `safessh tunnels close <id>`

Cooperatively shut down a running tunnel: SIGTERM → 5-second poll → SIGKILL fallback. Writes a `tunnel_close` audit event with `reason: user-close`.

**Usage:**

```sh
safessh tunnels close <id>
```

**Exit codes:**

| Code | Meaning |
|---:|---|
| `0` | Tunnel closed. |
| `1` | Tunnel ID not found. |
| `40` | Storage error. |
| `50` | Audit-write failure. |

See [docs/tunnels.md](tunnels.md) for the full tunnel lifecycle.

## `safessh skill install --target <target> [flags]`

Install the embedded skill for the specified agent target.

**Flags:**

| Flag | Effect |
|---|---|
| `--target <name>` | Required. One of `claude-code`, `agents-md`, `cursor`, `gemini-cli`, `codex`, `plain`, or `all`. See [docs/skill.md](skill.md) for per-target install paths. |
| `--scope <user\|project\|path>` | Where to install. Default `user`. |
| `--path <dir>` | Required when `--scope path`, and required (with `--scope path`) when `--target plain`. |

**`--target all` matrix.** `all` walks the supported (target, scope) combinations and installs only those that match the supplied `--scope`. It is **not** detection-based — it always installs every supported pair, creating parent directories as needed. Pairs that have no install path for the requested scope are skipped with a stderr note.

| `--scope` | Targets installed |
|---|---|
| `user` | `claude-code` (user), `gemini-cli` (user), `codex` (user). Skips `agents-md` and `cursor` (project-only). |
| `project` | `claude-code` (project), `agents-md` (project), `cursor` (project), `gemini-cli` (project). Skips `codex` (user-only). |
| `path` | Refused with exit 2 — `--target all` is incompatible with `--scope path`. |

`plain` is never included in `all` and must be installed explicitly with `--scope path --path <file>`.

**Examples:**

```sh
safessh skill install --target claude-code --scope user
safessh skill install --target agents-md --scope project
safessh skill install --target gemini-cli --scope user
safessh skill install --target plain --scope path --path ./skill.md
safessh skill install --target all --scope user
safessh skill install --target all --scope project
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

## `safessh skill update [flags]`

Re-render the embedded skill body and rewrite every currently-installed copy in place. Pairs that aren't installed are silently skipped — unlike `skill install`, this does not create new files.

**Flags:**

| Flag | Effect |
|---|---|
| `--target <name>` | Restrict to one or more targets. Repeat the flag to pass multiple (`--target claude-code --target cursor`). Without it, every supported target in scope is considered. |
| `--scope <user\|project\|both>` | Which scope(s) to walk. Default `both`. |
| `--dry-run` | Print a unified diff per file that would change (and `(unchanged)` per file that would not), without writing anything. |

For section-style targets (`agents-md`, `gemini-cli`, `codex`) the `## safessh` section is replaced cleanly; the rest of the file is preserved. For file-style targets the file is rewritten atomically. If nothing is installed, exits 0 and prints `safessh: skill: no installed copies found` to stderr. If everything is already current, prints `All installed copies are up to date.` and exits 0.

**Examples:**

```sh
safessh skill update --dry-run
safessh skill update --target claude-code --scope user
safessh skill update --target cursor --target gemini-cli --scope project
safessh skill update --scope both
```

## `safessh skill detect [flags]`

Walk the supported (target, scope) matrix and report, for each pair, whether the framework directory exists, whether the skill is installed there, and whether the installed body is current. Unlike `skill check` (human-readable per-scope lines), `skill detect` produces a stable structured listing intended for tooling and LLM agents.

**Flags:**

| Flag | Effect |
|---|---|
| `--format <table\|json>` | Output format. Default `table` (fixed-width columns: `target scope path status`). `json` emits a pretty-printed JSON array of `{target, scope, path, status}` records. |

`status` is one of:

- `not detected` — the framework's parent directory does not exist (e.g., no `~/.claude` for Claude Code user scope).
- `detected, not installed` — parent exists but the skill file/section isn't there.
- `installed (current)` — file matches the embedded body byte-for-byte (file-style targets only).
- `installed (section present)` — `## safessh` section matches the embedded body (section-style targets).
- `installed (drift)` — file or section exists but doesn't match. Run `skill update` to refresh.
- `requires --path` — emitted for `plain`, which has no default location.

**Examples:**

```sh
safessh skill detect
safessh skill detect --format json
```

## `safessh tui`

Launch the interactive terminal UI. Four screens (Projects / Approvals / Rules / Audit) share a `notify` watcher so external edits land within ~250 ms.

Refuses non-TTY environments with exit 1:

```
$ safessh tui </dev/null
safessh: error: tui requires a TTY
```

See [docs/tui.md](tui.md) for screen layouts, the full keymap, and snapshot-test workflow.

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
