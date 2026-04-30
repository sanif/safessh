# Development

This guide covers what you need to hack on `safessh` itself: workspace layout, build / test / lint, branching conventions, how to add a new policy category, and how releases happen.

For higher-level architecture and design rationale, see the [design spec](superpowers/specs/2026-04-30-safessh-design.md).

## Workspace layout

`safessh` is a Cargo workspace of seven crates. Dependencies flow downward only (no cycles).

| Crate | Responsibility |
|---|---|
| `safessh-core` | Shared types, errors, redactor. No I/O. |
| `safessh-storage` | All filesystem access for config and state (atomic writes, advisory locks, paths, keychain). |
| `safessh-policy` | AST parsing, semantic categories, decision engine. Pure (no I/O). |
| `safessh-ssh` | Subprocess driver for `ssh`/`sftp`/`ssh -L`. The only crate that talks to OpenSSH. |
| `safessh-audit` | JSONL writer (and, in v0.5+, the SQLite index). |
| `safessh-skill` | Embedded skill markdown plus per-target format adapters and install logic. |
| `safessh-cli` | Thin entrypoint, arg parsing, dispatch, output framing. Glues the rest together. |

A crate must not reach sideways. If it feels like `safessh-ssh` needs to know about a policy decision, the call is being routed wrong — it should come back through `safessh-cli`.

## Build, test, lint

```sh
cargo build --workspace
cargo test --workspace                            # unit + property tests
cargo test --workspace --features integration     # adds container-based SSH integration tests (Linux + Docker)
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo insta review                                # update TUI / golden snapshots interactively (when present)
```

CI runs the same on every PR. Container integration tests run on Linux only. Anything that can be tested in a temp `HOME` (`SAFESSH_HOME=$(mktemp -d)`) is faster and parallelizable — prefer that over containers for new tests.

## Branching

- `main` — protected, stable, tagged releases only.
- `develop` — integration branch.
- `feature/<topic>` → PR into `develop` → manual testing → fast-forward into `main` with a version bump.
- `hotfix/<topic>` → PR into `main`, back-merge into `develop`.

Never amend, never `--no-verify`, never force-push to `main` or `develop`. If a hook fails the commit didn't happen — fix and re-stage as a new commit.

## Commit conventions

Conventional Commits are required. Auto-changelog tooling depends on the format.

```
feat: …
fix: …
chore: …
docs: …
refactor: …
test: …
perf: …
build: …
ci: …
```

Commits should describe *why*, not just *what*. The diff already says what changed.

## Adding a new policy category

Two flavours of category live under `safessh-policy::categories`:

- **`shell`** — looks at the parsed binary, flags, and args alone. Add a new matcher here when the category is decidable from argv shape.
- **`sql`** — recognises a small set of database CLIs (`psql`, `mysql`, `sqlite3`) and parses their SQL payload with `sqlparser-rs`. Add here when the category depends on the SQL statement type.

### Steps for a shell category

1. **Add a matcher.** Edit `crates/safessh-policy/src/categories/shell.rs`. Add a `pub fn is_<name>(cmd: &ParsedCommand) -> bool` and call it from `match_shell_categories`.
2. **Cover the obvious cases with unit tests** in the same file (or `tests/`), including read-vs-write distinctions, flag-bundling edge cases, and the default-deny path on `find` / `bash -c`-shaped invocations.
3. **Surface it in `policy show`.** Edit `crates/safessh-cli/src/commands/policy.rs` — add the new category name to the lookup tables so `safessh policy show <name>` lists matching binaries / patterns.
4. **Document it.** Add the new category to [docs/cli-reference.md](cli-reference.md#safessh-policy-show-categoryproject). The skill content (`crates/safessh-skill/src/content/safessh.md`) only mentions categories generically — update it only if the new category changes how an agent should reason about commands.
5. **Update the project default if appropriate.** A new category that's safe by default goes into the `Project::policy.allow` list initialised in `crates/safessh-cli/src/commands/project.rs::ProjectCmd::Add`. Most new categories should start in `require_approval`, not `allow`.

### Steps for a SQL category

Similar shape, but in `crates/safessh-policy/src/categories/sql.rs`. The `classify_statement` match is the right hook. Note that SQL parse failure conservatively returns `db:write` (SAFETY-INVARIANT-1) — preserve that.

## How to release

Releases are driven by `cargo-dist`. The pipeline is configured in `dist-workspace.toml`.

1. Make sure `develop` is green and feature-complete for the version.
2. Open a release PR from `develop` to `main`. Bump `[workspace.package].version` in the root `Cargo.toml`, update `CHANGELOG.md`, and merge.
3. Tag the merge commit on `main`:

   ```sh
   git checkout main && git pull
   git tag -s v0.1.0 -m "v0.1.0"
   git push origin v0.1.0
   ```

4. `cargo-dist`'s GitHub Actions workflow takes over: cross-compiles for the four targets in `dist-workspace.toml`, builds the curl installer, opens the GitHub Release, and publishes the Homebrew formula to the configured tap.
5. Verify after release:

   ```sh
   curl --proto '=https' --tlsv1.2 -fsSL \
     https://github.com/sanif/safessh/releases/latest/download/safessh-installer.sh | sh
   safessh --version
   ```

For pre-releases use `vX.Y.Z-rc.N` tags; `cargo-dist` will publish them as draft / prerelease automatically.

## Style notes

- **Comments only when the WHY is non-obvious.** Well-named identifiers do the work. Exceptions: `// SAFETY-INVARIANT-N` markers (load-bearing) and `// SAFETY:` comments on `unsafe` blocks (CI fails without).
- **No abstractions for hypothetical futures.** Three similar lines beats a premature trait. The exception: the `SshDriver` trait, which exists explicitly so unit tests can mock SSH.
- **Errors via `thiserror`.** `anyhow` only at the CLI boundary.
- **Async via `tokio::main`.** Don't mix runtimes.
