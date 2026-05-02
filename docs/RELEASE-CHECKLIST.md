# Release checklist for v0.2.0

safessh uses cargo-dist for releases. This is the manual sequence to actually
ship v0.2.0 to users. The same template was used for v0.1.0 — bump the version
strings throughout for future releases.

## Prerequisites

The Homebrew tap repo (`sanif/homebrew-tap`) and the safessh repo (`sanif/safessh`)
already exist from v0.1.0. The `dist-workspace.toml` `tap` field and
`release.yml` repository reference both point at `sanif/homebrew-tap`.

## Step 1: Pre-release verification

Merge the v0.2.0 feature branch into `develop` (if it isn't already), then
on `develop`:

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo test --workspace --features integration` (requires Docker)
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `dist plan` shows 4 targets + 2 installers
- Manual sanity: `cargo run --bin safessh -- tui` opens the TUI on a real
  terminal; `q` quits cleanly without leaving the terminal in raw mode.
- Manual sanity: piped invocation (`safessh tui </dev/null`) exits 1 with
  `safessh: error: tui requires a TTY`.

## Step 2: Merge develop → main

```sh
git checkout main
git merge --no-ff develop
git push origin main
```

## Step 3: Tag and push (rc first)

```sh
git tag -a v0.2.0-rc.1 -m "Release candidate 1 for v0.2.0"
git push origin v0.2.0-rc.1
```

GitHub Actions runs the release workflow. Verify:

- GitHub Release created with 4 binaries (macOS arm64/x64, Linux x64/arm64).
- Checksum file present.
- `installer.sh` URL resolves.
- Homebrew formula updated in `sanif/homebrew-tap`.

## Step 4: Clean-machine verification

On a clean macOS arm64 (or new Mac VM):

- `brew install sanif/tap/safessh`
- `safessh --version` prints `safessh 0.2.0-rc.1`.
- `safessh project add demo --alias localhost`
- `safessh demo exec "echo hi"` round-trips through the policy engine.
- `safessh tui` opens the TUI; cycle Tab through the four screens; `?`
  shows the help overlay; `q` quits cleanly.

On a clean Linux x86_64 (a fresh Docker container is fine — the container
just needs a TTY allocated, e.g. `docker run -it ubuntu`):

- `curl -fsSL <installer-url> | sh`
- Same verification commands as above.

## Step 5: Promote to v0.2.0

If rc.1 is clean:

```sh
git tag -a v0.2.0 -m "v0.2.0 release"
git push origin v0.2.0
```

If rc.1 has issues, fix them on `develop`, fast-forward `main`, and tag
`v0.2.0-rc.2`. Repeat until a clean rc.

## Step 6: Update CHANGELOG

- Move the date in `CHANGELOG.md` to the actual release date if it changed
  from the placeholder.
- Add an empty `[Unreleased]` section above the `[0.2.0]` block.
- Commit: `chore: release v0.2.0`.

## Step 7: Back-merge into develop

```sh
git checkout develop
git merge --no-ff main
git push origin develop
```

This keeps `develop` ahead of (or equal to) `main` for the next milestone.
