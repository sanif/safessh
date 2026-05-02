# Approvals

When an LLM agent runs `safessh prod exec "rm -rf /tmp/foo"` and the policy returns `RequireApproval`, the operator (you) must explicitly grant or deny the request before any SSH dispatch happens. This doc describes that lifecycle, the five actions available, and where the persistent state lives.

This is the v0.2 reference. Cross-links into the spec point at the source of truth ([§7.2](superpowers/specs/2026-04-30-safessh-design.md#72-twelve-safety-invariants) for invariants 7 & 12, [§5.6](superpowers/specs/2026-04-30-safessh-design.md#56-approvals) for the data model).

## Lifecycle

```
agent runs            policy decides         operator chooses
─────────────         ──────────────         ───────────────
                                             ┌── Once     → drop pending, no rule
safessh prod exec     RequireApproval        ├── Timed N  → rule expires in N min
   "rm -rf /tmp"  ─→  + write pending  ─→ ─→ ├── Always   → persist allow rule
                      + emit BLOCKED:        ├── Deny     → drop pending, no rule
                        token via stderr     └── Block    → persist deny rule
                                                ↓
                                             pending file removed
                                             agent retries → Allow / Block / Deny
```

The agent process exits with code 10 and a structured `BLOCKED:` block on stderr (see "Headless format" below). It cannot proceed until the pending request is taken (regardless of which action).

## The five actions

Both the CLI's `safessh approve <token>` subcommand and the TUI's Approvals-screen modal expose the same five actions (CLI/TUI parity, SAFETY-INVARIANT-12).

| Action | What persists | Effect on subsequent matches |
|---|---|---|
| **Once** | Nothing | None — the next exec re-trips the policy and writes a new pending. |
| **Timed N** | `state/approvals/timed/<project>.toml` | Allowed until `expires_at` (wall clock). |
| **Always** | `state/approvals/always/<project>.toml` | Allowed forever or until you `d` it on the Rules screen. |
| **Deny** | Nothing | None — the request is dropped; agent must retry. |
| **Block** | `state/approvals/blocked/<project>.toml` | Future matching exec attempts return `Blocked` (exit 11) without prompting. |

Pattern matching uses `binary` + `flags` + `categories`. `args` are not part of the pattern (the pattern would otherwise be too narrow to be useful — `rm -rf /var/log` and `rm -rf /tmp/x` should match the same rule).

The CLI:

```sh
safessh approve TOK001                  # Once (the default)
safessh approve TOK001 --timed --minutes 60
safessh approve TOK001 --always
safessh approve TOK001 --block
# (no flag for Deny — equivalent to PendingStore::take without writing a rule;
#  same effect as letting the pending file expire via cleanup_expired)
```

The TUI: highlight the request, press `Enter`, pick from the modal. Same writes happen.

## Persistent rule stores

```
~/.local/state/safessh/approvals/
├── pending/<token>.toml       # written by exec, consumed by approve/take
├── timed/<project>.toml       # RuleList<TimedRule>, mtime-based expiry
├── always/<project>.toml      # RuleList<PatternRule>
└── blocked/<project>.toml     # RuleList<PatternRule>
```

All four files are written through `safessh_storage::atomic::write_string` under an exclusive `LockedFile` lock. The TUI uses the same API the CLI does — neither bypasses it (SAFETY-INVARIANT-12, [§7.2](superpowers/specs/2026-04-30-safessh-design.md#72-twelve-safety-invariants)).

Timed rules are wall-clock-based (SAFETY-INVARIANT-7). `purge_expired` rewrites the file with expired entries dropped.

## Headless format (the `BLOCKED:` block)

When the calling process is non-TTY (LLM agents, CI), the policy decision lands as a structured stderr block:

```
BLOCKED: destructive:filesystem on this project
Approve via: safessh approve TOK001
Token: TOK001
```

with exit code 10. This is the contract the agents parse. The format is verbatim — three lines, the second one is the literal `safessh approve <token>` invocation, the third is `Token: <token>` so a regex-based agent can extract it without parsing the first line.

If your agent picks this up, the recovery path is:
1. Show the user the `BLOCKED:` block.
2. Run the suggested `safessh approve` (with whatever flag the user picks, defaulting to `Once`).
3. Re-run the original `safessh prod exec ...` invocation; the policy returns `Allow` (or `Blocked` if Block was chosen).

The skill markdown at `crates/safessh-skill/src/content/safessh.md` documents this contract for the LLM and is installed by `safessh skill install`.

## CLI vs TUI parity

The TUI is a layer on top of the same store API; it cannot do anything the CLI can't do, and vice versa. Specifically:

- A pending request created by `safessh prod exec` is visible in the TUI within ~250 ms of the file write (the watcher's debounce + tick).
- Writing through the TUI fires a `notify` event, so a CLI `safessh approve` running in another shell sees the pending file disappear and exits silently if you've already taken it from the TUI.
- `safessh approve` and the TUI write the same `PatternRule` shape into the same files. There's no way to produce a "TUI-only" rule the CLI can't see.

This is enforced by SAFETY-INVARIANT-12: **the TUI never bypasses the storage API**. When you change a rule store's `add`/`remove`/`take` semantics, the TUI's screens automatically inherit them.

## Cleanup

Stale pending requests (e.g. abandoned by an agent crash) are dropped by `PendingStore::cleanup_expired(max_age_hours)`. This isn't wired into the v0.2 binary yet — it'll be a `safessh approvals gc` subcommand in a later milestone. For now, manually remove old `<token>.toml` files if they accumulate.

`Timed::purge_expired` runs eagerly on every policy evaluation in the exec path (see `decide_and_record` in `crates/safessh-cli/src/commands/exec.rs`), so timed rule files self-clean. Manual purge is not necessary.

## See also

- [`docs/cli-reference.md`](cli-reference.md) — `safessh approve` flags + exit codes.
- [`docs/tui.md`](tui.md) — Approvals screen layout + keymap.
- [`docs/policy.md`](policy.md) — what triggers `RequireApproval` in the first place.
- Spec [§5.6](superpowers/specs/2026-04-30-safessh-design.md#56-approvals) — full data model.
- Spec [§7.2](superpowers/specs/2026-04-30-safessh-design.md#72-twelve-safety-invariants), invariants 7 & 12 — the load-bearing rules this doc relies on.
