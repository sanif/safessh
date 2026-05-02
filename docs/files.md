# Reading and writing files

For configuration files, logs, or scripts, use `safessh <project> read` and `safessh <project> write` instead of `exec "cat"` or `exec "tee"`. These are first-class operations with stricter path-level policy controls, atomicity guarantees, and structured audit logging.

This doc covers the syntax, when to use each, and the approval workflow. The full spec is in [§6 of the file-ops design](superpowers/specs/2026-05-02-safessh-v0.3-file-ops-design.md).

---

## Reading files

```sh
safessh <project> [--on <target>] read <remote-path>
```

**What it does:**
- Fetches the remote file over SFTP.
- Streams bytes through the redactor (removes AWS keys, private keys, etc.).
- Outputs framed result to stdout (see [Output framing](cli-reference.md#output-framing)).

**Caps and limits:**
- Default size cap: 5 MB (`output.file_read_cap_bytes`). If exceeded, the first 5 MB is returned and `truncated: true` is recorded in the audit log.
- Exit code 30 on exceed.

**Errors:**
- File not found → exit 1.
- Permission denied or other SFTP error → exit 20.

**Sensitive paths:**
A binary-embedded **deny-list** blocks reads of sensitive files regardless of project policy:
- `/etc/shadow`, `/etc/sudoers`, `/etc/sudoers.d/**`
- `/root/.ssh/**`, `/home/*/.ssh/**`
- `**/.env*`, `**/id_rsa*`, `**/id_ed25519*`, `**/id_ecdsa*`

These are enforced at the preset level (SAFETY-INVARIANT-14) — a per-project allow rule cannot override them.

**Example: read a config file**

```sh
$ safessh prod read /etc/nginx/nginx.conf
<stdout>
worker_processes auto;
events { worker_connections 1024; }
http {
  server { listen 80; ...
</stdout>
<exit code="0" duration="0.142s"/>
```

**Example: read from a non-default target**

```sh
$ safessh prod --on staging read /var/log/app.log
<stdout>
2026-05-02T14:23:45Z [INFO] startup complete
2026-05-02T14:24:12Z [WARN] connection pool at capacity
</stdout>
<exit code="0" duration="0.089s"/>
```

---

## Writing files

```sh
echo "content" | safessh <project> [--on <target>] write <remote-path>
```

**What it does:**
- Reads stdin until EOF.
- Uploads to a remote sibling tempfile (`.safessh.<random>.tmp`).
- Atomically renames over the target (SAFETY-INVARIANT-13).
- Returns exit 0 on success.

**Atomicity guarantee:**
If the rename fails or the connection drops mid-upload, the target file is untouched. You never see a half-written remote file.

**Caps and limits:**
- Default size cap: 5 MB (`output.file_write_cap_bytes`). If stdin exceeds this, the upload is aborted and no write occurs. Exit code 30.
- Exit code 1 if the parent directory doesn't exist (won't auto-`mkdir`).
- Exit code 1 if the target exists and is a directory.

**No redaction on write:**
Bytes originating from stdin are written as-is. There is no redaction on the write path — what you send is what lands. (Redaction prevents server secrets leaking *to* the agent, not the reverse.)

**Example: write a config file from stdin**

```sh
$ cat <<EOF | safessh prod write /etc/app/config.toml
[server]
port = 8080
log_level = "info"
EOF
<stdout/>
<exit code="0" duration="0.156s"/>
```

**Example: update a file with a heredoc**

```sh
$ safessh prod write /tmp/deploy.sh <<'SCRIPT'
#!/bin/bash
set -e
cd /opt/app
git pull origin main
systemctl restart app
SCRIPT
<stdout/>
<exit code="0" duration="0.203s"/>
```

**Example: write to a staging target**

```sh
$ echo "feature flag: enabled" | safessh prod --on staging write /var/lib/app/flags.conf
<stdout/>
<exit code="0" duration="0.087s"/>
```

---

## `read` vs `exec "cat"`, and `write` vs `exec "tee"`

Use the native `read` and `write` subcommands instead of exec equivalents when:

| Reason | Why |
|---|---|
| **Path-level policy** | Project policy can allow/deny reads under `/var/log/app/*` without permitting `/var/log/system/*`. See [`docs/policy.md`](policy.md#file-rules) for the `[[policy.file_rules]]` schema. `exec "cat /var/log/app/x.log"` only gates on the `read:safe` category. |
| **Atomicity on write** | `write` guarantees the target is either untouched or fully written (temp + rename). `exec "tee"` creates the file immediately; a connection loss leaves partial content. |
| **Structured audit** | Every read/write is logged with the exact path, size, and redaction count. Audit searches can answer "show me all reads of `/etc/*`". `exec "cat"` logs the command text but not the path-specific result. |
| **Redaction direction** | `read` redacts server secrets from output. `exec "cat | grep password"` returns the unredacted grep output. |

---

## What's intentionally absent

### No `--from-file` flag

You cannot write a remote file from a local file path:

```sh
# This does NOT exist:
safessh prod write /tmp/config.toml --from-file ./local-config.toml  # ✗
```

**Why:** A `--from-file` option would let an agent silently exfiltrate by pointing at the wrong local path:
```
safessh prod write /etc/app/config.toml --from-file /etc/passwd
```

The agent sends `/etc/passwd` to the remote without the human realizing. Piping through stdin is explicit and auditable — the content in the approval log shows what was transferred.

**Workaround:** Use your shell's redirection:
```sh
safessh prod write /tmp/config.toml < ./local-config.toml
cat ./local-config.toml | safessh prod write /tmp/config.toml
```

### No directory listing or recursive copy

Directory recursion and listing are deferred. Future milestones will add `ls`, `find`, and `cp -r` as first-class operations with the same policy and audit structure.

---

## Approval flow

If the project policy doesn't allow the path, the command returns exit 10 and prints a `BLOCKED:` block on stderr (same format as `exec`):

```
BLOCKED: file:read on this project
Approve via: safessh approve TOK001
Token: TOK001
```

### CLI approval

Run the suggested command, then retry the original read/write:

```sh
$ safessh prod read /var/log/app/error.log
BLOCKED: file:read on this project
Approve via: safessh approve TOK001
Token: TOK001

$ safessh approve TOK001 --always
Added allow rule: file:read /var/log/app/error.log

$ safessh prod read /var/log/app/error.log
<stdout>
[ERROR] database connection timeout
</stdout>
<exit code="0" duration="0.142s"/>
```

The `--always` flag persists an allow rule for that exact path. See [`docs/approvals.md`](approvals.md) for all five actions: `Once`, `Timed N`, `Always`, `Deny`, `Block`.

### TUI approval

1. Run the blocked command (exit 10, stderr shows `BLOCKED:` block).
2. Open the TUI: `safessh tui`.
3. Navigate to **Approvals** (keyboard shortcut or menu).
4. Highlight the pending request and press `Enter`.
5. Choose an action from the modal (Once / Timed / Always / Deny / Block).
6. Return to the CLI and retry the original command.

---

## Exit codes

| Code | Meaning | Example |
|---|---|---|
| 0 | Success. File read or written. | — |
| 1 | Not found: file missing, parent dir missing, or target is a directory. | `read /nonexistent` → exit 1 |
| 2 | Usage error. | `read` with no path argument |
| 10 | Approval required. Stderr shows `BLOCKED:` token. | Path in `require_approval` category |
| 11 | Blocked by rule. | Hit a persistent `decision = "block"` rule |
| 12 | Denied. | Hit a `policy.deny` or preset deny-list |
| 20 | SFTP error: permission denied, connection lost, protocol error. | No permission to read the file |
| 30 | Size cap exceeded. Output truncated (read) or not written (write). | Read > 5 MB or write > 5 MB stdin |

See [`docs/cli-reference.md`](cli-reference.md#exit-codes) for the full exit code table and what they mean for agents.

---

## See also

- [`docs/policy.md`](policy.md) — the `[[policy.file_rules]]` schema and decision precedence.
- [`docs/approvals.md`](approvals.md) — the five approval actions and persistent rule stores.
- [`docs/cli-reference.md`](cli-reference.md) — all subcommands, flags, and exit codes.
- [File-ops design spec §6](superpowers/specs/2026-05-02-safessh-v0.3-file-ops-design.md#6-data-flow) — data flow and safety invariants.
