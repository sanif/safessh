# CLI reference

Everything the `safessh` binary exposes through v0.3. Run `safessh --help` for the live help text — this page exists to be readable as documentation and to list things `--help` doesn't show (exit codes, output framing).

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

## `safessh project add <name> [flags]`

Register a new project.

**Flags:**

| Flag | Description |
|---|---|
| `--alias <alias>` | Reuse an existing `~/.ssh/config` alias (lazy resolution at exec time). Mutually exclusive with `--host`/`--user`/`--import-ssh-config`. |
| `--host <host>` | Hostname to connect to. Requires `--user`. |
| `--user <user>` | Remote username. Requires `--host`. |
| `--port <port>` | SSH port. Default `22`. |
| `--import-ssh-config <name>` | Snapshot `host`/`user`/`port`/`identity_file` from the matching `Host <name>` block of `~/.ssh/config` (or `$SSH_CONFIG_PATH`) into a new `Inline` target. Conflicts with `--alias`/`--host`/`--user` (clap exit 2). ProxyJump is not imported — use `--alias` if you need it. |

Specify exactly one of: `--alias`, the `--host`/`--user` pair, or `--import-ssh-config`. The new project starts with `allow = ["read:safe", "file:read"]` and an empty deny / require-approval list.

**Examples:**

```sh
safessh project add prod --alias my-prod-host
safessh project add staging --host staging.example.com --user deploy
safessh project add prod --import-ssh-config my-prod-host
```

See [docs/projects.md](projects.md#alias-vs-import--which-to-choose) for the alias-vs-import decision matrix.

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
