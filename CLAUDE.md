# CLAUDE.md — safessh

A CLI proxy that lets LLM agents run SSH operations against your servers without seeing credentials, with policy-gated commands and persistent audit. Single Rust binary, OpenSSH-driver, file-based state, ratatui TUI, multi-agent skill.

**Read the full design before changing anything load-bearing:** [`docs/superpowers/specs/2026-04-30-safessh-design.md`](docs/superpowers/specs/2026-04-30-safessh-design.md). It is the source of truth for architecture, components, data flow, error model, and milestones.

---

## Load-bearing rules — never violate

These are the ones that, if broken, silently invalidate the security model. The full list of 12 invariants lives in the [spec §7.2](docs/superpowers/specs/2026-04-30-safessh-design.md#72-twelve-safety-invariants); the essentials:

1. **Default-deny on parse failure.** If `safessh-policy` can't AST-parse a command, the decision is `RequireApproval` — never `Allow`. Search for `// SAFETY-INVARIANT-1` in code.
2. **Deny > Allow ordering.** Block-list and persistent-deny rules are evaluated **before** any allow rules. Don't reorder. Search `// SAFETY-INVARIANT-2`.
3. **Secrets never in argv.** Passwords/passphrases come from the OS keychain at exec time, fed via stdin or env. Never construct `ssh user:password@host` URLs. Don't pass secrets as flags. They show up in `ps`.
4. **Audit-write before user-visible output.** If JSONL append fails, `safessh-cli` returns exit 50 and aborts. The SQLite indexer can fail silently (JSONL is authoritative), but the JSONL append is non-negotiable.
5. **Atomic file writes only.** Every write to `~/.config/safessh/**` and `~/.local/state/safessh/**` goes through `tempfile::NamedTempFile` + `persist()`. No half-written files observable.
6. **No outbound network calls from `safessh` itself.** No telemetry, no auto-update, no version-check phone-home. The skill content is `include_str!`'d, never fetched.
7. **`--yolo` only bypasses the policy engine.** It does not bypass: audit logging (yolo events log MORE), the `disable_yolo` global setting, the output cap, or the redactor.

When you add a code path that touches policy, audit, or storage, add a `// SAFETY-INVARIANT-N: <one-line reason>` comment naming the invariant it preserves, and a test that asserts it.

---

## Architecture at a glance

Workspace of seven crates. Dependencies flow downward only — no cycles.

```
safessh-cli ─┬─▶ safessh-tui ──┐
             ├─▶ safessh-ssh ──┤
             ├─▶ safessh-policy┤
             ├─▶ safessh-audit ┼─▶ safessh-storage ──▶ safessh-core
             └─▶ safessh-skill ┘
```

| Crate | Responsibility | I/O? |
|---|---|---|
| `safessh-core` | Shared types, errors, redactor | None |
| `safessh-storage` | All filesystem access for config + state | Filesystem |
| `safessh-policy` | AST parsing, semantic categories, decisions | None (pure) |
| `safessh-ssh` | Subprocess driver for `ssh`/`sftp`/`ssh -L` | Subprocess + network |
| `safessh-audit` | JSONL writer + SQLite index | Filesystem |
| `safessh-tui` | ratatui screens | Terminal + filesystem (via storage) |
| `safessh-skill` | Embedded skill markdown + per-target adapters | Filesystem (writes only) |
| `safessh-cli` | Thin entrypoint, arg parsing, dispatch, framing | All of the above |

A crate must not reach sideways. If you find yourself wanting `safessh-ssh` to know about a policy decision, you're routing the call wrong — it should come back through `safessh-cli`.

Details: [spec §4–§5](docs/superpowers/specs/2026-04-30-safessh-design.md#4-architecture).

---

## Build, test, lint

```sh
cargo build --workspace
cargo test --workspace                    # unit + property + tier 1–4 (fast)
cargo test --workspace --features integration  # adds container-based SSH integration tests
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo insta review                        # update TUI / golden snapshots interactively
```

CI runs the same on every PR. Container integration tests run on Linux only (Docker required). Anything you can test in a temp `HOME` with `tempfile::tempdir()`, do — it's faster and parallelizable.

---

## Branching and commits

- `main` — protected, stable, tagged releases only.
- `develop` — integration branch for testing.
- `feature/<topic>` → PR into `develop` → manual testing → `develop` → `main` with version bump.
- `hotfix/<topic>` → PR into `main`, back-merge to `develop`.

**Conventional Commits required:** `feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, `test:`, `perf:`, `build:`, `ci:`. Auto-changelog tooling depends on this format.

**Never amend, never `--no-verify`, never force-push to `main`/`develop`.** Hotfix the issue in a new commit. If a hook fails, the commit didn't happen — fix and re-stage.

---

## Releases

Configured via `[workspace.metadata.dist]` in `Cargo.toml`. Tag `vX.Y.Z` on `main` → `cargo-dist` cross-compiles, publishes a GitHub Release, updates the Homebrew tap, refreshes the curl `installer.sh`. Don't hand-roll release workflows; modify the `cargo-dist` config.

---

## Output and logging conventions

- **stdout** — command output and structured framing (`<stdout>...</stdout>`, `<stderr>...</stderr>`, `<exit code="N" duration="..."/>`). The LLM parses this; do not pollute it.
- **stderr** — human-readable errors, single line, prefix `safessh: <category>: <message>`. The exception is the `BLOCKED:` multi-line format for approval-required cases (LLM needs the token verbatim).
- **Exit codes** — see [spec §7.1](docs/superpowers/specs/2026-04-30-safessh-design.md#71-exit-codes). They are the LLM's primary signal. Adding a new exit code requires updating both the table and the skill markdown — keep them in sync.

---

## When you change…

- **Policy categories or rule semantics** — also update `safessh-skill/src/safessh.md` (the embedded skill content). Skill is the LLM's user-manual; out-of-date skill = LLM does the wrong thing.
- **The exit-code table** — update `docs/cli-reference.md` and the skill markdown.
- **The audit JSONL schema** — bump the schema version in `audit.log` events; add a `refinery` migration for the SQLite index.
- **Project TOML schema** — update [spec §4.4](docs/superpowers/specs/2026-04-30-safessh-design.md#44-project-toml-schema-illustrative) and add a migration if existing configs need rewriting.

---

## Style

- **No comments unless the WHY is non-obvious.** Well-named identifiers do the work. The exceptions: `// SAFETY-INVARIANT-N` markers, `// SAFETY:` on `unsafe` blocks (CI fails without).
- **No abstractions for hypothetical futures.** Three similar lines beats a premature trait. The exception: the `SshDriver` trait, which exists explicitly so unit tests can mock SSH.
- **Errors via `thiserror`.** `anyhow` only at the CLI boundary.
- **Async via `tokio`.** Don't mix runtimes.

---

## Where the docs live

| File | What it has |
|---|---|
| `docs/superpowers/specs/2026-04-30-safessh-design.md` | Full design spec — read first |
| `README.md` | User-facing intro, install, feature table (updates per release) |
| `docs/getting-started.md` | New-user walkthrough (lands v0.1) |
| `docs/cli-reference.md` | Every flag, every subcommand (lands v0.1) |
| `docs/skill.md` | Multi-agent skill installation (lands v0.1) |
| `docs/security.md` | Threat model, what we protect against, what we don't (lands v0.1, expanded v0.7) |
| `docs/development.md` | Workspace layout, build flow, how to add a policy category (lands v0.1) |
| `docs/projects.md` | Project model, ssh-config import, multi-target (lands v0.2) |
| `docs/policy.md` | Categories, AST matching, custom rules (lands v0.2) |
| `docs/approvals.md` | Lifecycle, TUI flow (lands v0.2) |
| `docs/tui.md` | Keymap, screens (lands v0.2) |
| `docs/audit.md` | JSONL schema + SQLite query recipes (lands v0.5) |
| `docs/performance.md` | Benchmarks (lands v0.9) |

When you add a feature, update the relevant doc in the same PR. Docs not in the same PR rot fast.

---

## Current milestone

See [spec §9](docs/superpowers/specs/2026-04-30-safessh-design.md#9-milestones). Active milestone is tracked in `README.md`'s features table. The implementation plan for the active milestone lives next to this CLAUDE.md once the writing-plans skill produces it.
