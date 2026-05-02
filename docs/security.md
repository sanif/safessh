# Security model

`safessh` is a personal-use proxy designed for one threat: the gap between "an LLM agent is helpful at SSH chores" and "the LLM has full SSH access to my servers." It narrows that gap. It is not a sandbox, an IDS, or a hardened multi-tenant gateway.

## Threat model

### What we protect against

- **The agent seeing credentials.** Private keys, passphrases, and password material live on your machine (ssh-agent, keychain, `~/.ssh/config`). The agent only sees a project name. There is no `user:password@host` URL anywhere in the call graph and no place where credentials become argv (which would leak via `ps`).
- **Accidentally destructive commands without a human in the loop.** Every command is AST-parsed; matched categories are checked against the project's policy; anything outside `allow` either prompts or exits with `BLOCKED:` and a token. The agent cannot self-approve — that requires a separate `safessh approve <token>` invocation, which is normally a human at the keyboard.
- **Loss of audit trail.** Every gating decision and every exec result is appended to `audit.log` (JSONL) before any user-visible output. If the audit write fails, the command is refused (exit 50). The trail is yours to grep.

### What we don't protect against

- **A determined attacker with shell access on your machine.** They can edit project TOML, drop allow rules, or just run `ssh` directly. `safessh` is a fence on a desktop, not a vault.
- **Adversarial commands designed to fool the AST policy.** The policy uses category matchers over a parsed shell AST; it is not a sandbox or a syscall filter. `bash -c '...'` and friends are categorised as `exec:opaque` and require approval, but a human approving a hostile `exec:opaque` defeats the system. Treat policy as a speed-bump, not a wall.
- **Leakage via redaction misses.** The redactor is a best-effort regex pass over stdout/stderr. It catches obvious patterns (private keys, common token shapes) but can be defeated by encoding, partial matches, or formats it has never seen.
- **`--yolo`.** When you bypass the policy engine, the policy engine is bypassed. You still get audit logging (yolo events log *more*) and the redactor still applies, but allow/deny rules are gone for that invocation. See the warnings below.
- **Network-side attacks against your remote hosts.** `safessh` shells out to OpenSSH; the security of the connection is whatever OpenSSH and your config provide.

## Safety invariants

These are the load-bearing rules. Each is enforced in code with a `// SAFETY-INVARIANT-N:` comment marker and asserted in tests.

1. **Default-deny on parse failure.** When the AST parser cannot resolve a command, the decision is `RequireApproval` — never `Allow`.
2. **Deny > Allow ordering.** Block-list and persistent-deny rules are evaluated before any allow rules.
3. **Secrets never in argv.** Passwords and passphrases come from the OS keychain at exec time, fed via stdin or environment. Never visible in `ps`.
4. **Audit-write before user-visible output.** A failed JSONL append exits 50 and aborts. The SQLite indexer can fail silently (JSONL is authoritative); the JSONL append is non-negotiable.
5. **Atomic file writes.** All config and state writes go through `tempfile::NamedTempFile` + `persist()`. No half-written files observable.
6. **Redaction last.** Output passes through `core::Redactor` after framing, immediately before stdout.
7. **TTLs are wall-clock, not process lifetime.** Timed allows persist across invocations; expiry is `expires_at` comparison.
8. **Tunnel TTL is hard.** Tunnel processes are SIGTERM'd at TTL, SIGKILL after a 5-second grace. (Tunneling itself lands in v0.4; the invariant is fixed now.)
9. **`--yolo` only bypasses the policy engine.** It does not bypass: audit logging, the `disable_yolo` setting, output caps, or the redactor.
10. **No outbound network calls from `safessh` itself.** No telemetry, no auto-update, no version-check phone-home.
11. **Skill content is binary-embedded, never fetched.** Updates ride with binary updates.
12. **Concurrent invocations are race-safe.** Rule files are accessed under advisory file locks; CLI and (future) TUI cannot corrupt each other's writes.
13. **Partial uploads are never visible.** sftp writes go to a temp file and are renamed into place atomically; a failed write leaves no half-written file at the destination path.
14. **Preset deny-list cannot be overridden.** The compiled-in preset for sensitive paths (`/etc/shadow`, `~/.ssh/id_*`, etc.) is checked before any project rule; a project `allow` entry for a preset-blocked path has no effect.
15. **`network:tunnel` is default-deny.** New tunnels require approval unless project policy or a persisted rule explicitly allows the `network:tunnel` category. The category cannot be unlocked implicitly (e.g. via a permissive wildcard rule).

## `--yolo` warnings

`--yolo` is a deliberate, audited escape hatch. It exists because sometimes you need to run a one-off script and the policy engine is in the way. It is **not** a shortcut for poorly-configured projects.

- **Audited.** Every yolo invocation logs `yolo_invocation` with the raw command before the SSH driver starts. You cannot use yolo to run something the trail doesn't show.
- **Output caps and redaction still apply.** Your stdout is still capped; the redactor still runs.
- **`disable_yolo` is a hard kill switch.** Setting `disable_yolo = true` in `~/.config/safessh/config.toml` (or wherever `Paths::user()` resolves on your platform) makes every `--yolo` invocation exit 13 — refused before any project lookup, before any network I/O.
- **Granted to the binary, not the agent.** Don't install a skill that tells the agent to use `--yolo`. The skill that ships with `safessh` does not.

If you're tempted to give an agent a yolo workflow: instead, run the command yourself, capture what's needed, and feed the relevant output back to the agent.

### Locking yolo down

Add this to your global config:

```toml
# ~/.config/safessh/config.toml  (Linux; macOS uses ~/Library/Application Support/...)
disable_yolo = true
```

Other tunables in the same file:

```toml
default_timed_minutes = 30
tunnel_ttl_minutes    = 30
disable_yolo          = true

[[redaction_patterns]]
name  = "internal-token"
regex = "INT-[A-Z0-9]{16}"
```

## Where things live

`safessh` follows the platform's standard XDG / `directories` conventions. On Linux:

- **Config** — `~/.config/safessh/`
  - `config.toml` — global settings (`disable_yolo`, redaction patterns).
  - `projects/<name>.toml` — one file per project.
  - `policies/` — reserved for cross-project policy rules (v0.2+).
- **State** — `~/.local/state/safessh/` (`%LOCALAPPDATA%` / `Application Support` on macOS/Windows)
  - `audit.log` — JSONL audit trail (append-only).
  - `approvals/pending/<token>.toml`
  - `approvals/timed/<project>.toml`
  - `approvals/always/<project>.toml`
  - `approvals/blocked/<project>.toml`
- **Cache** — `~/.cache/safessh/`
  - `control-sockets/` — OpenSSH `ControlMaster` sockets for connection reuse.

You can override the whole tree for testing or sandboxing:

```sh
SAFESSH_HOME=/tmp/safessh-sandbox safessh project list
```

## Reporting issues

Security issues should be reported privately. See the repository's security policy for the current contact (`SECURITY.md` is added with the v0.7 hardening pass). Until then, open a GitHub issue marked `[security]` only for non-sensitive concerns; for anything that could impact users, email the maintainer directly via the address listed on the GitHub profile.
