# TUI

`safessh tui` opens an interactive terminal interface backed by ratatui + crossterm. Four screens — Projects, Approvals, Rules, Audit — share a single notify-watcher so changes made elsewhere (CLI, hand-edits, another safessh process) are picked up without manual refresh.

## Launching

```sh
safessh tui
```

Requirements:
- A real TTY on both stdin and stdout. Piped IO exits 1 with `safessh: error: tui requires a TTY`.
- `notify` works on macOS (fsevents), Linux (inotify), and Windows (ReadDirectoryChangesW). Filesystem watching is best-effort on network mounts.

## Layout

```
safessh — Projects
┌Projects──────────────┐┌Detail─────────────────────────────────┐
│> prod                ││name: prod                             │
│  staging             ││default target: default                │
│                      ││                                       │
│                      ││targets                                │
│                      ││  default [default]  alias=prod-host   │
│                      ││                                       │
│                      ││policy                                 │
│                      ││  allow:            read:safe          │
│                      ││  require_approval: destructive:fs     │
└──────────────────────┘└───────────────────────────────────────┘
q quit  Tab next  ↑↓/jk move
```

The header reads `safessh — <Screen>`. The footer is a one-line keymap reminder. A transient toast may appear on row 0 when external edits arrive (see "Live updates" below).

## Keymap

The full keymap is what `?` shows in the TUI. It's also exported as `safessh_tui::help_text()` so this section stays in sync with the binary — they're the same string.

```
Global
  q / Esc      quit
  Ctrl-C       quit
  Tab          next screen
  Shift-Tab    previous screen
  ?            toggle this help

Projects
  Up / k       move selection up
  Down / j     move selection down
  a            add a new project (suspends TUI, runs interactive flow)
  e            edit the selected project (suspends TUI, runs interactive flow)
  i            import multiple targets from ~/.ssh/config

Approvals
  Up/Down/k/j  move selection
  Enter        open action picker (Once / Timed / Always / Deny / Block)
  Esc          close picker

Rules
  < / Left     previous project
  > / Right    next project
  1 / 2 / 3    Timed / Always / Blocked tab
  Up/Down/k/j  move selection
  d            delete selected rule

Audit
  Up/Down/k/j  move selection
  g / G        jump to top / bottom (G resumes auto-scroll)
  /p           filter by project
  /t           filter by event type
  /            grep substring filter
  Esc          cancel current filter edit
```

If you change the keymap, edit the string in `crates/safessh-tui/src/help.rs::help_text()` and re-run `cargo test --package safessh-tui --test routing` to confirm `help_text_is_public_and_non_empty` still passes. Then update this section. Do not duplicate the keymap text — copy it from the help_text output.

## Screens

### Projects

Left list of project names (sorted). Right pane shows the selected project's `name`, `default_target`, every `Target` (`alias=X` for SshConfigAlias, `user@host:port` for Inline), and the `policy` `allow`/`require_approval`/`deny` arrays.

Empty state: `No projects. Press \`a\` (or run \`safessh project add\`) to create one.`

**Add (`a`).** Suspends the TUI (leaves alt-screen + raw mode), shells out to the same `safessh project add` interactive flow described in [docs/projects.md](projects.md#interactive-default--recommended), then re-enters the TUI and reloads the project list. Picking `Discard` or hitting Ctrl-C inside the prompts is fine — the TUI just resumes with no new project.

**Edit (`e`).** Same shell-out pattern, but with the highlighted project name pre-filled so the CLI flow skips its picker step. The action menu (add target / remove target / change default / edit policy / save / discard) is the same as if you ran `safessh project edit <name>` directly.

**Import (`i`).** Opens the in-TUI multi-select dialog for batch-creating projects from `~/.ssh/config` aliases — see [docs/projects.md](projects.md#tui-import-multiple-aliases-at-once). Use this when you want to mass-onboard several hosts at once; for one-off setup, `a` gives you per-prompt control (target source, key location, etc.) that the import dialog skips.

### Approvals

Pending approvals queue. Each row: `TOKEN  PROJECT  BIN  CATEGORIES  AGE`. Sorted by `created_at` ascending so the oldest pending bubbles to the top.

`Enter` opens a centered modal picker:
- **Once** — allow this single retry, drop the pending request.
- **Timed N** — allow for N minutes (default from project's `approvals.timed_default_minutes`); persists in `state/approvals/timed/`.
- **Always** — persist a PatternRule in `state/approvals/always/`.
- **Deny** — drop the pending request without rule.
- **Block** — persist a PatternRule in `state/approvals/blocked/` so future matching exec attempts are blocked.

The picker selection writes through the same `safessh_storage::approvals` API the CLI's `approve` subcommand uses (atomic, locked, SAFETY-INVARIANT-12).

### Rules

Three-tab view of persistent rules per project: Timed / Always / Blocked.

- Top bar shows the active project (`<` and `>` cycle).
- Tabs: `1` Timed, `2` Always, `3` Blocked.
- Each row: `RULE  BIN  FLAGS  CATEGORIES  [EXPIRES Xm]`. The expiry column only shows on Timed.
- `d` deletes the selected rule via the matching store's `remove()`.

Empty list per tab reads `No <kind> rules for <project>.` When no projects exist, the body suggests `safessh project add`.

### Audit

Live tail of `state/audit.log` (last 200 events). Each row: `TIME  TYPE  PROJECT  SUMMARY` where SUMMARY is event-type-specific (binary for exec_attempt, exit code for exec_complete, token for approval_requested, first 40 chars for yolo_invocation).

Filters:
- `/p` then text + Enter — filter by project.
- `/t` — filter by event type.
- `/`  — grep substring against the raw JSON line.

Empty filter input clears the filter. `g`/`G` jump to top/bottom; `G` resumes auto-scroll. New events arriving via `FsEvent::AuditAppended` are appended in-place — only the new bytes are read (offset-tracked), so a multi-megabyte log doesn't re-parse on every append.

## Live updates

A `notify-debouncer-mini` watcher (200 ms debounce) runs alongside the App and emits `FsEvent::ApprovalsChanged`, `ProjectsChanged`, or `AuditAppended` when the corresponding directory or file is touched.

- ProjectsChanged → Projects screen reloads + a 3 s toast appears: `config changed externally — reloaded`.
- ApprovalsChanged → Approvals screen reloads + same toast.
- AuditAppended → audit log tail re-reads silently (no toast — appends are expected).

The toast replaces any earlier toast (no stacking) and clears itself on the next tick after 3 s.

## Contributor: snapshot tests

Screens are tested with `insta` snapshots over `ratatui::TestBackend`. To update after intentional rendering changes:

```sh
INSTA_UPDATE=auto cargo test --package safessh-tui
# Review the .new files, then accept:
for f in crates/safessh-tui/tests/snapshots/*.new; do mv "$f" "${f%.new}"; done
git add crates/safessh-tui/tests/snapshots/
```

Or use `cargo insta review` if you prefer the interactive flow.

Snapshots that capture time-relative data (the Approvals `AGE` column, the Audit `TIME` column) pin their inputs:
- Approvals snapshots set `created_at = Utc::now()` so AGE always reads `now`.
- Audit snapshots hand-roll `audit.log` lines with literal RFC3339 timestamps.

When in doubt, look at how the existing tests pin their inputs before adding a new one.

## Platform notes

- macOS: fsevents reports paths under `/private/...` for `/var/...` symlinks. The watcher canonicalizes its watch directories so `starts_with` comparisons line up.
- Linux: inotify is exact — no canonicalization quirk.
- Windows: ReadDirectoryChangesW. Untested in CI; report any regressions.
