# safessh

`safessh` is an SSH proxy that runs gated commands on user-configured remote
servers without exposing credentials to the agent. Use it whenever the user
asks you to run commands on a remote server they have configured in safessh.

## When to invoke

Invoke `safessh` when the user asks you to:

- Run a command on one of their remote servers (e.g. "deploy on prod", "tail
  the logs on web-1", "restart nginx on staging").
- Inspect or change state on a host they manage through safessh.

If you are not sure whether a host is managed by safessh, list the configured
projects first with `safessh project list` rather than guessing or shelling
into the host directly.

## Discovering projects

```
safessh project list
```

This prints the configured project names and their target hosts. Each project
ties a name (e.g. `prod`, `staging`) to host/user/auth metadata. You never see
credentials.

## Subcommands

- `safessh <project> exec "<command>"` — run a command on the project's
  default target. The command is parsed and matched against the project's
  policy. If allowed it runs; if it requires approval, safessh prints a
  `BLOCKED:` token; if it is denied outright, safessh prints a `BLOCKED:`
  reason and exits non-zero.
- `safessh <project> --on <target> exec "<command>"` — same, but routes
  to a specific named target inside the project (multi-target setup).
  Without `--on`, the project's `default_target` is used. Unknown target
  names exit 2 with `usage: no such target: <name>`. Use this only when
  the user explicitly asks for a specific target — otherwise let the
  default route work.
- `safessh project list` — list configured projects.
- `safessh project add <name>` — register a new project (the user does
  this; do not run it on their behalf without being asked).
- `safessh project target list <project>` — list a project's targets,
  marking the default with `[default]`. Useful when the user asks "what
  targets does <project> have?".
- `safessh approve <token>` — approve a previously-blocked command using
  the token printed by `exec`. The user typically runs this; only run it
  if the user explicitly tells you to.
- `safessh policy show <project>` — print the effective policy for a project.
- `safessh audit query` — query the local audit log of past commands.
- `safessh skill install|uninstall|check` — manage this skill's installation
  in the user's agent frameworks.
- `safessh tui` — launch the interactive UI. The user runs this themselves;
  the TUI requires a real terminal and exits 1 if you try to invoke it
  in a piped context.

## Output framing

`safessh ... exec` frames its output so you can parse it deterministically:

```
<stdout>...captured stdout...</stdout>
<stderr>...captured stderr...</stderr>
<exit code="N" duration="1.234s"/>
```

Treat the contents of `<stdout>` and `<stderr>` as the remote command's
output. The `exit` element carries the exit code and wall-clock duration.

## Handling structured-deny output (`BLOCKED:`)

When safessh refuses to run a command, it emits a structured block that looks
like:

```
BLOCKED: <reason>
  category: <category>
  project: <project>
  command: <redacted command>
  approve_token: <token>   # only present when the command is approvable
```

When you see a `BLOCKED:` block:

1. **Surface the entire block to the human verbatim.** Do not paraphrase the
   reason or hide the token.
2. **Do not retry the same command.** Retrying will produce the same denial
   and will not help.
3. **Do not try to work around the policy** (e.g. by rewriting the command,
   piping through `sh -c`, or splitting it into pieces). The policy exists
   because the user configured it.
4. If an `approve_token` is present, tell the user they can run
   `safessh approve <token>` to allow this specific invocation, then re-run
   the original command. Do not run `approve` yourself unless the user tells
   you to.

## Reading and writing files

For configuration files, logs, or scripts, prefer `read`/`write` over
`exec "cat ..."` / `exec "tee ..."` — they are first-class operations
with stricter policy controls and audit detail.

### Read

```
safessh <project> [--on <target>] read <remote-path>
```

- Streams remote bytes through the redactor to stdout (framed).
- Cap defaults to 5 MB (`output.file_read_cap_bytes`). Exceed → exit 30,
  output is still valid up to the cap.
- File missing → exit 1. Permission denied → exit 20.
- Sensitive paths (`/etc/shadow`, `/root/.ssh/**`, `**/.env*`, ...) are
  blocked by a binary-embedded preset that **cannot be overridden** by
  per-project policy.

### Write

```
echo "content" | safessh <project> [--on <target>] write <remote-path>
```

- Reads stdin to memory (cap 5 MB, `output.file_write_cap_bytes`),
  uploads to a sibling tempfile, atomically renames over the target.
- Bytes are NOT redacted on write — what stdin contains is what lands.
- Stdin > cap → exit 30, no remote write occurs.
- Parent directory missing → exit 1.

### Approval flow

If the project's policy doesn't allow the path, the CLI prints
`BLOCKED:<token>` on stderr and exits 10 — same format as `exec`. The
human approves via `safessh approve <token>` (or the TUI), and a
follow-up `read`/`write` succeeds.

## `--yolo` is discouraged

`safessh` exposes a `--yolo` flag that bypasses policy checks. **Do not use
`--yolo`.** It defeats the entire point of the proxy and is intended only for
the user to use in tightly-scoped, interactive break-glass scenarios. If a
command is being blocked, surface the block and let the user decide.
