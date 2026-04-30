# Release checklist for v0.1.0

safessh uses cargo-dist for releases. This is the manual sequence to actually
ship v0.1.0 to users.

## Prerequisites

The Homebrew tap repo (`sanif/homebrew-tap`) and the safessh repo (`sanif/safessh`) already exist. The `dist-workspace.toml` `tap` field and `release.yml` repository reference both point at `sanif/homebrew-tap`.

## Step 1: Pre-release verification

On `develop`:
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo test --workspace --features integration` (requires Docker)
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `dist plan` shows 4 targets + 2 installers

## Step 2: Merge develop → main

```sh
git checkout main
git merge --no-ff develop
git push origin main
```

## Step 3: Tag and push (rc first)

```sh
git tag -a v0.1.0-rc.1 -m "Release candidate 1 for v0.1.0"
git push origin v0.1.0-rc.1
```

GitHub Actions runs the release workflow. Verify:
- GitHub Release created with binaries for macOS arm64/x64 and Linux x64/arm64.
- Checksum file present.
- `installer.sh` URL works.
- Homebrew formula updated in tap repo.

## Step 4: Clean-machine verification

On a clean macOS arm64 (or Docker linux container):
- `brew install sanif/tap/safessh`
- `safessh --version` should print `safessh 0.1.0-rc.1`.
- `safessh project add demo --alias localhost && safessh demo exec "echo hi"`

On a clean Linux x86_64:
- `curl -fsSL <installer-url> | sh`
- Same verification commands.

## Step 5: Promote to v0.1.0

If rc.1 is clean:
```sh
git tag -a v0.1.0 -m "v0.1.0 release"
git push origin v0.1.0
```

## Step 6: Update CHANGELOG

- Move the date in `CHANGELOG.md` to the actual release date if it changed.
- Add an empty `[Unreleased]` section above v0.1.0.
- Commit: `chore: release v0.1.0`.
