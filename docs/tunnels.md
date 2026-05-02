# Tunnels

`safessh forward` opens a local port forward (`ssh -L <spec> -N`) to a project target and exits immediately, leaving a self-supervising background process in charge of the connection. The supervisor enforces the project's `output.tunnel_ttl_minutes` deadline (SIGTERM on expiry, SIGKILL after a 5-second grace — SAFETY-INVARIANT-8). Every tunnel is audit-logged with an opacity warning: safessh can record that a tunnel was open and for how long, but it cannot observe the data that flowed through it.

## Quick start

```sh
# 1. Add a project (skip if you already have one).
safessh project add prod --alias my-prod-host

# 2. Allow the network:tunnel category in the project's policy.
#    Without this, every forward requires interactive approval.
safessh project edit prod
#    Add under [policy]:  allow = ["read:safe", "file:read", "network:tunnel"]

# 3. Open a tunnel.
safessh prod forward 5432:db.internal:5432
```

The command exits 0 in under a second. The local port `5432` is now forwarded to `db.internal:5432` on the prod target for up to `tunnel_ttl_minutes` minutes (default 30).

## `safessh <project> [--on <target>] forward <spec>`

Open a port forward to a project target.

**Usage:**

```sh
safessh <project> forward <local_port>:<remote_host>:<remote_port>
safessh <project> --on <target> forward <local_port>:<remote_host>:<remote_port>
```

**Spec format:** `<local_port>:<remote_host>:<remote_port>`

- All three parts are required. Two-part specs (`local:remote`) are rejected (exit 2).
- Port numbers must be in the range 1–65535. Port 0 is rejected (exit 2).

**Example:**

```sh
safessh prod forward 5432:db.internal:5432
safessh prod --on db forward 15432:localhost:5432
```

**Output (success):**

```
tunnel open id=ab3fzc9p spec=5432:db.internal:5432 ttl=30min expires=2026-05-02T14:00:00Z
```

The tunnel ID is a short alphanumeric string you pass to `safessh tunnels close <id>`.

**Policy gate:** The forward operation is classified as `network:tunnel`. It is default-deny (SAFETY-INVARIANT-15) — the command exits 10 and prints a `BLOCKED:` token unless the project policy explicitly allows or the agent has an active approval.

**Exit codes:**

| Code | Meaning |
|---:|---|
| `0` | Tunnel is open and the supervisor is running. |
| `2` | Usage error — bad spec format or out-of-range port. |
| `10` | Approval required (`network:tunnel` not in project allow list). |
| `11` | Persistently blocked. |
| `12` | Denied for this invocation. |
| `20` | SSH failure — `ssh -L` exited immediately (e.g. auth error, host unreachable). |
| `40` | Storage error writing the tunnel record. |
| `50` | Audit-write failure — operation refused. |

## `safessh tunnels list`

Show all tunnels whose supervisor process is still alive.

**Usage:**

```sh
safessh tunnels list
```

**Output columns:**

```
ID        PROJECT  TARGET   SPEC                          OPENED               TTL
ab3fzc9p  prod     default  5432:db.internal:5432         2026-05-02T13:30:00Z 24min
r7qm4ws2  staging  db       15432:localhost:5432           2026-05-02T13:45:00Z 9min
```

| Column | Description |
|---|---|
| `ID` | 8-character tunnel identifier. Pass to `close`. |
| `PROJECT` | Project name. |
| `TARGET` | Target name within the project. |
| `SPEC` | `local_port:remote_host:remote_port` as given to `forward`. |
| `OPENED` | UTC timestamp when the tunnel was opened. |
| `TTL` | Minutes until the supervisor sends SIGTERM. |

**What counts as active:** A tunnel record in `state/tunnels/<id>.toml` whose `supervisor_pid` corresponds to a live process. Records with dead supervisor PIDs are reaped (the TOML file is removed) during this command so stale entries do not accumulate.

**Exit codes:** `0` on success (even if there are no rows). `40` on storage error.

## `safessh tunnels close <id>`

Request cooperative shutdown of a running tunnel.

**Usage:**

```sh
safessh tunnels close <id>
```

**Sequence:**

1. Looks up `state/tunnels/<id>.toml`.
2. Sends SIGTERM to the supervisor PID.
3. Polls every 500 ms for up to 5 seconds.
4. If the supervisor is still alive after 5 seconds, sends SIGKILL.
5. Removes the tunnel record and writes a `tunnel_close` audit event with `reason: user-close`.

**Exit codes:**

| Code | Meaning |
|---:|---|
| `0` | Tunnel is closed (SIGTERM or SIGKILL succeeded). |
| `1` | Tunnel ID not found (already closed or never existed). |
| `40` | Storage error. |
| `50` | Audit-write failure. |

## TTL

Each tunnel has a hard deadline enforced by the supervisor process.

**Setting TTL per project:**

```toml
# ~/.config/safessh/projects/prod.toml
[output]
tunnel_ttl_minutes = 60    # default: 30
```

**Setting TTL globally:**

```toml
# ~/.config/safessh/config.toml
tunnel_ttl_minutes = 30
```

The project-level setting takes precedence over the global default.

**How enforcement works:** The supervisor process (a re-exec of the `safessh` binary in a detached mode) holds a monotonic deadline. At expiry it sends SIGTERM to the `ssh -L -N` child. If the child is still alive after 5 seconds, it sends SIGKILL. The supervisor then writes a `tunnel_close` audit event with `reason: ttl-expired` and exits.

The TTL is a wall-clock deadline, not a process-lifetime counter. SAFETY-INVARIANT-8 states that tunnel TTL enforcement is hard: there is no project config, approval, or flag that extends a TTL once it is set at open time.

## Opacity

Port tunnels are inherently opaque to the proxy layer.

**What the audit log captures:**

- `tunnel_open` event: project, target, forward spec, tunnel ID, supervisor PID, TTL, opened-at timestamp.
- `tunnel_close` event: tunnel ID, close reason (`ttl-expired`, `user-close`, `ssh-died`, `parent-shutdown`, `failed-to-start`), closed-at timestamp.
- The `tunnel_open` event carries an `opacity_warning` field (value `"data-in-transit-not-logged"`) so that audit consumers are aware the data itself was not captured.

**What the audit log does NOT capture:**

- Any bytes that flowed through the tunnel.
- Connection counts or client identities on the local port.
- Remote-side activity triggered via the forwarded connection.

The TUI Audit screen renders `tunnel_open` rows with an `[opaque]` tag to make this limitation visible in the UI.

## Policy

Port forwards are classified as `network:tunnel`. This category is **default-deny** (SAFETY-INVARIANT-15): it does not appear in the default `allow` list and cannot be unlocked by a permissive wildcard rule.

**Allow all forwards for a project:**

```toml
[policy]
allow = ["read:safe", "file:read", "network:tunnel"]
```

**Require approval for each forward:**

```toml
[policy]
allow            = ["read:safe", "file:read"]
require_approval = ["file:write", "network:tunnel"]
deny             = ["destructive:disk", "destructive:db"]
```

When `require_approval` contains `network:tunnel`, each `forward` invocation exits 10 with a `BLOCKED:` token. Approve it with:

```sh
safessh approve <token>            # once only
safessh approve <token> --timed    # allow for the next 30 min
safessh approve <token> --always   # persist in the project's allow list
safessh approve <token> --block    # convert to a permanent block
```

**Deny all forwards:**

```toml
[policy]
deny = ["network:tunnel"]
```

Exits 12 immediately; no pending approval is created.

The TUI Approvals screen shows tunnel approval requests as a distinct variant with the same five actions (Once / Timed / Always / Deny / Block). Selecting Always or Timed writes a category-tagged rule so future forwards are resolved without another approval.

See [docs/policy.md](policy.md) for the full decision-precedence chain and [docs/approvals.md](approvals.md) for the approval lifecycle.

## Troubleshooting

### Port already in use

```
safessh: ssh: bind: Address already in use
```

The local port you requested is occupied by another process. Either close the occupying process or use a different local port:

```sh
safessh prod forward 15432:db.internal:5432   # use 15432 instead
```

`safessh tunnels list` shows any existing safessh tunnels on that port. A process-level check (`lsof -i :5432`) shows non-safessh processes.

### Tunnel closes immediately (exit 20)

The most common cause is an SSH authentication failure or the remote host being unreachable. The supervisor records `reason: failed-to-start` in the audit log. Check:

- `ssh -L 5432:db.internal:5432 -N <alias> -v` directly to see the SSH error.
- That the project target alias resolves correctly (`safessh project target list <project>`).
- That the remote `db.internal` host is reachable from the SSH server (not just from your machine).

### Supervisor unresponsive after `tunnels close`

If `safessh tunnels close <id>` returns exit 0 but the local port is still bound, the supervisor may have been replaced by another process with the same PID (PID recycling). In this case:

1. Use `lsof -i :<local_port>` to find the actual PID binding the port.
2. Send SIGKILL directly: `kill -9 <pid>`.
3. Remove the stale record manually: `rm ~/.local/state/safessh/tunnels/<id>.toml` (or equivalent on macOS).

This situation is rare. If you encounter it consistently, file an issue.
