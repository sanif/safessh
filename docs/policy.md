# Policy

safessh gates every operation through a policy engine before opening an SSH connection. This doc is the user-facing reference for v0.3. The source-of-truth specification lives in [spec §4.5](superpowers/specs/2026-04-30-safessh-design.md#45-policy-presets-shipped-categories) (categories) and the v0.3 file-ops design in [spec §4 (file_rules)](superpowers/specs/2026-05-02-safessh-v0.3-file-ops-design.md).

## Overview

When you run `safessh prod exec "ls /var/log"`, the policy engine:

1. Parses the command into an AST.
2. Matches the AST against named semantic **categories** (`read:safe`, `destructive:filesystem`, etc.).
3. Evaluates a 5-tier precedence chain — block-list first, then file rules, then category-level rules, then defaults.
4. Returns one of four decisions: `Allow`, `RequireApproval`, `Deny`, or `Block`.

Policy runs before any subprocess. If parsing fails, the decision is always `RequireApproval` — never `Allow`. This is SAFETY-INVARIANT-1.

## Categories

Categories are AST-level matchers, not regex matches on the raw command string. The full list with match criteria is in [spec §4.5](superpowers/specs/2026-04-30-safessh-design.md#45-policy-presets-shipped-categories).

| Category | What it covers |
|---|---|
| `read:safe` | `ls`, `cat`, `head`, `tail`, `grep`, `find` (no `-delete`/`-exec`), `stat`, `df`, `ps`, and other read-only inspection commands |
| `file:read` | sftp `read` operation (first-class, not an exec command) |
| `file:write` | sftp `write` operation (first-class) |
| `destructive:filesystem` | `rm`, `rmdir`, `shred`, `find -delete`, `find -exec rm`, recursive `mv` to `/dev/null` |
| `destructive:disk` | `dd`, `mkfs.*`, `fdisk`, `parted`, `wipefs`, raw redirects to `/dev/sd*` / `/dev/nvme*` |
| `destructive:db` | SQL `DROP`, `TRUNCATE`, `DELETE` without `WHERE`, `ALTER … DROP` inside `psql`/`mysql`/`sqlite3` |
| `db:read` | SQL `SELECT`, `EXPLAIN`, `SHOW`, `DESCRIBE` inside `psql`/`mysql`/`sqlite3` |
| `db:write` | SQL `INSERT`, `UPDATE`, scoped `DELETE`, `CREATE`, `ALTER` (non-DROP) |
| `privilege:escalation` | `sudo`, `su`, `doas`, `pkexec`, setuid manipulations |
| `system:control` | `shutdown`, `reboot`, `halt`, `poweroff`, `systemctl stop/disable/mask` on critical units |
| `network:listen` | `nc -l`, `socat … LISTEN`, `python -m http.server`, and similar listening-socket openers |
| `network:tunnel` | safessh `forward` operation (first-class) |
| `exec:opaque` | `eval`, `sh -c`, `bash -c`, `python -c`, `perl -e`, base64-pipe-to-shell, `curl … | sh` — i.e. code whose semantics can't be parsed further. Note: `psql -c` and `mysql -e` are not `exec:opaque`; the engine descends into them as `db:*` categories. |

The default policy when a project has no `[policy]` section:

- **Allow:** `read:safe`, `file:read`
- **RequireApproval:** everything else that does not appear in the deny set below
- **Deny (hard):** `network:tunnel`, `destructive:disk`, `system:control`, `exec:opaque`

## Project policy

The `[policy]` section in a project TOML declares which categories each project allows, gates on approval, or denies outright.

```toml
[policy]
allow            = ["read:safe", "file:read"]
require_approval = ["destructive:filesystem", "db:write", "file:write"]
deny             = ["destructive:disk", "system:control"]
```

- **`allow`** — operations in these categories proceed without prompting.
- **`require_approval`** — the agent is blocked (exit 10) until you grant an approval via `safessh approve <token>` or the TUI. See [`docs/approvals.md`](approvals.md) for the full lifecycle.
- **`deny`** — operations are rejected immediately (exit 12) without creating a pending approval. The LLM should surface this to the user.

Unlisted categories fall through to the default `RequireApproval`.

### `network:tunnel` category

Port forwards (`safessh <project> forward <spec>`) are classified as `network:tunnel`. This category is default-deny (SAFETY-INVARIANT-15) — it does not appear in any project's implicit allow list and cannot be unlocked by a wildcard allow rule. You must list it explicitly:

```toml
[policy]
allow            = ["read:safe", "file:read", "network:tunnel"]
require_approval = ["file:write"]
deny             = ["destructive:disk", "destructive:db"]
```

Or gate each forward on an approval:

```toml
[policy]
allow            = ["read:safe", "file:read"]
require_approval = ["file:write", "network:tunnel"]
deny             = ["destructive:disk", "destructive:db"]
```

See [`docs/tunnels.md`](tunnels.md) for the full forward workflow, TTL semantics, and approval flow.

Categories absent from `allow`, `require_approval`, and `deny` all inherit the default described above. You do not need to list every category — only override what you need.

## File rules

`[[policy.file_rules]]` adds path-level granularity on top of the `file:read` and `file:write` categories. Without file rules, `file:read` in `allow` permits reads anywhere; file rules let you narrow or expand that.

### Schema

```toml
[[policy.file_rules]]
category = "file:read"                          # "file:read" | "file:write"
paths    = ["/etc/nginx/*", "/var/log/nginx/*"] # glob patterns (see below)
decision = "allow"                              # "allow" | "approve" | "deny" | "block"

[[policy.file_rules]]
category = "file:write"
paths    = ["/tmp/safessh-staging/*"]
decision = "allow"

[[policy.file_rules]]
category = "file:read"
paths    = ["/var/log/app/error.log"]
decision = "deny"
```

Rules are evaluated in order. The first matching rule wins.

### Decision values

| Value | Effect |
|---|---|
| `"allow"` | Operation proceeds without prompting. |
| `"approve"` | Generates a pending approval (exit 10). |
| `"deny"` | Rejects immediately (exit 12). No pending created. |
| `"block"` | Persistently blocks (exit 11). Same weight as a manually created block rule. |

### Glob syntax

Paths are matched against the **canonical** path returned by the remote `realpath`, after client-side `~` and `..` expansion. The glob engine is [`globset`](https://docs.rs/globset) with `literal_separator(true)`, which means:

- `*` matches any sequence of characters **within a single path segment** — it does not cross `/`.
- `**` matches any sequence of characters including `/`, i.e. any number of path segments (recursive).
- `?` matches exactly one character, excluding `/`.
- `[abc]` and `[a-z]` match character classes (standard bracket expressions).
- No shell brace-expansion (`{a,b}` is not supported).

Examples:

| Pattern | Matches | Does not match |
|---|---|---|
| `/etc/nginx/*` | `/etc/nginx/nginx.conf` | `/etc/nginx/conf.d/site.conf` |
| `/etc/nginx/**` | `/etc/nginx/conf.d/site.conf` | `/etc/app/nginx.conf` |
| `/var/log/*.log` | `/var/log/app.log` | `/var/log/app/error.log` |
| `**/.env*` | `/home/user/project/.env.local` | — |
| `/home/*/.ssh/**` | `/home/deploy/.ssh/authorized_keys` | `/root/.ssh/authorized_keys` |

## Decision precedence

For any single operation safessh walks five tiers in order and stops at the first match:

```
1. Preset block-list (SAFETY-INVARIANT-2, -14)
   └─ preset file_rules with decision="deny" evaluated here
2. Project file_rules (in declaration order, first match wins)
   └─ block → Block (exit 11)
   └─ deny  → Deny  (exit 12)
   └─ approve → RequireApproval (exit 10)
   └─ allow → Allow
3. Project category-level deny list (policy.deny)
   └─ matched category → Deny (exit 12)
4. Persistent approval rules (timed + always)
   └─ unexpired timed rule → Allow
   └─ always rule         → Allow
5. Category-level allow / require_approval / default
   └─ all matched categories in policy.allow → Allow
   └─ any category not in allow → RequireApproval (exit 10)
   └─ parse failure (no categories) → RequireApproval (invariant 1)
```

Block-list and project deny rules are always evaluated before any allow rules. This is SAFETY-INVARIANT-2 — see [spec §7.2](superpowers/specs/2026-04-30-safessh-design.md#72-twelve-safety-invariants).

### Worked example

`safessh prod read /etc/shadow`

1. **Preset block-list:** `/etc/shadow` matches the preset `file:read` deny-list → **Deny** (exit 12). Evaluation stops.

`safessh prod read /etc/nginx/nginx.conf` with:
```toml
[policy]
allow = ["read:safe", "file:read"]

[[policy.file_rules]]
category = "file:read"
paths    = ["/etc/nginx/*"]
decision = "allow"
```

1. Preset block-list: `/etc/nginx/nginx.conf` does not match any preset path.
2. Project file_rules: matches `/etc/nginx/*` → **Allow**. Evaluation stops.

`safessh prod read /etc/passwd` with the same project:

1. Preset block-list: `/etc/passwd` does not match.
2. Project file_rules: no rule matches.
3. Project deny list: `file:read` not in `deny`.
4. Persistent rules: no timed/always rule.
5. Category-level: `file:read` is in `allow` → **Allow**.

## Preset deny-list

The preset deny-list is compiled into the binary (`crates/safessh-storage/src/policies/presets.toml`) and cannot be overridden by any project rule. It is evaluated at tier 1, before any project-level `file_rules` or `allow` entries.

**file:read — denied regardless of project policy:**

```
/etc/shadow
/etc/sudoers
/etc/sudoers.d/**
/root/.ssh/**
/home/*/.ssh/**
**/.env*
**/id_rsa*
**/id_ed25519*
**/id_ecdsa*
```

**file:write — denied regardless of project policy:**

```
/etc/shadow
/etc/sudoers
/etc/sudoers.d/**
/root/.ssh/**
/home/*/.ssh/**
```

Adding an `allow` rule for any of these paths in a project's `[[policy.file_rules]]` has no effect — the preset is checked first (SAFETY-INVARIANT-14, [spec §7.2](superpowers/specs/2026-04-30-safessh-design.md#72-twelve-safety-invariants)).

If you believe a path should be removed from the preset, file an issue. The preset list is intentionally conservative.

## Custom rules

User-defined rules that apply across all projects (rather than per-project) land in:

```
~/.config/safessh/policies/custom.toml
```

The `custom.toml` schema is the same `[[file_rules]]` format as the preset. Custom rules are merged in after the preset and before per-project rules. The `safessh policy show <project>` command prints the full resolved rule chain for a project, including preset and custom entries.

Custom cross-project rules are planned for a later milestone. Cross-links: [spec §4.5](superpowers/specs/2026-04-30-safessh-design.md#45-policy-presets-shipped-categories) and [spec §4.3](superpowers/specs/2026-04-30-safessh-design.md#43-data-layout).

## See also

- [`docs/approvals.md`](approvals.md) — approval lifecycle, the five actions, persistent rule stores.
- [`docs/projects.md`](projects.md) — project TOML schema, adding and editing projects.
- [`docs/cli-reference.md`](cli-reference.md) — `safessh policy show`, exit codes.
- [Spec §4.5](superpowers/specs/2026-04-30-safessh-design.md#45-policy-presets-shipped-categories) — category table with full match criteria.
- [Spec §7.2](superpowers/specs/2026-04-30-safessh-design.md#72-twelve-safety-invariants) — the twelve safety invariants, especially invariants 1, 2, and 14.
