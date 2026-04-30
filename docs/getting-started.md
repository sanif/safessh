# Getting started

A 10-minute walkthrough from a fresh install to running your first gated command.

## Install

Pick the path that matches how you usually install CLIs. Details (Homebrew tap, curl installer URL, cargo) are in the [README](../README.md#install).

After install, verify the binary:

```sh
safessh --version
```

## Add your first project

A *project* is a named handle for a remote target. The project name is what you'll type in front of `exec`.

### From an existing `~/.ssh/config` alias

If you already SSH into a host with `ssh my-prod-host`, reuse that alias — `safessh` will shell out to OpenSSH and pick up everything (`HostName`, `User`, `IdentityFile`, `ProxyJump`, etc.) from your config.

```sh
safessh project add prod --alias my-prod-host
```

### From explicit host + user

If the host isn't already in your SSH config, supply `--host` and `--user` directly:

```sh
safessh project add staging --host staging.example.com --user deploy --port 22
```

`--port` defaults to 22 and can be omitted.

### Inspect

```sh
safessh project list
safessh policy show prod
```

`policy show prod` prints the project's `allow` / `require_approval` / `deny` lists. By default a new project allows the `read:safe` and `file:read` categories — anything else needs explicit approval.

## Install the agent skill

The skill is a markdown file that teaches your agent (Claude Code, AGENTS.md-aware tools) how to talk to `safessh` — the framing format, the `BLOCKED:` token shape, when to call `project list`, etc.

```sh
safessh skill install --target claude-code --scope user
```

This writes `~/.claude/skills/safessh.md`. For project-scoped tools that read `AGENTS.md`:

```sh
safessh skill install --target agents-md --scope project
```

That appends a `## safessh` section to `<cwd>/AGENTS.md` (creating the file if needed). See [skill.md](skill.md) for full details.

## Run your first command

```sh
safessh prod exec "ls /var"
```

`safessh` parses the command, checks the project's policy, and (if allowed) executes it via OpenSSH. Output is framed:

```
<stdout>
backups
log
lib
...
</stdout>
<stderr>
</stderr>
<exit code="0" duration="42ms"/>
```

The exit code of `safessh` mirrors the remote command's exit code on the success path. Other exit codes (approval required, blocked, ssh failure, etc.) are listed in [cli-reference.md](cli-reference.md#exit-codes).

## Handle an approval

When a command falls into a category outside the project's allow list, `safessh` returns a structured `BLOCKED:` token to stderr and exits **10**:

```
$ safessh prod exec "rm -rf /tmp/old"
BLOCKED: destructive:filesystem on this project
Approve via: safessh approve abc123def456
Token: abc123def456
```

You then choose how to grant the request:

```sh
# One-shot — re-run the original command after approving.
safessh approve abc123def456

# Allow this exact pattern for the next 30 minutes (default).
safessh approve abc123def456 --timed

# Allow this pattern persistently on this project.
safessh approve abc123def456 --always

# Convert to a permanent block (record the rule and refuse forever).
safessh approve abc123def456 --block
```

Pending tokens auto-expire after 24 hours. The whole approval lifecycle (token → store → re-run) is audited.

## Common workflows

### Read a config file

`cat` is in the read-safe category, so it runs without prompting:

```sh
safessh prod exec "cat /etc/nginx/nginx.conf"
```

(File-read via SFTP with size caps lands in v0.3. For v0.1, `cat` is the canonical way to read remote text files.)

### Run a database query

SQL CLIs are inspected statement-by-statement. `SELECT` is `db:read` and is denied by default until you allow it for the project:

```sh
# First time: returns BLOCKED with a token.
safessh prod exec 'psql -c "SELECT count(*) FROM users"'

# Approve persistently if you want this kind of read to be allowed.
safessh approve <token> --always
```

`DROP TABLE`, `TRUNCATE`, and unguarded `DELETE` map to `destructive:db` — those should always require approval.

### Inspect the audit log

Every gating decision and exec result is appended to a JSONL log:

```sh
safessh audit query --project prod
safessh audit query --type exec_complete
safessh audit query --grep "rm -rf"
```

## Next steps

- [cli-reference.md](cli-reference.md) — every subcommand, flag, and exit code.
- [security.md](security.md) — threat model, what `--yolo` does, how to lock things down.
- [skill.md](skill.md) — agent integration details and drift detection.
