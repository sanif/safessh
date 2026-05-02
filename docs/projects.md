# Projects

A safessh **project** binds one or more SSH targets to a policy. The CLI's external subcommand (`safessh <project> exec "<cmd>"`) routes through the project's `targets` list to pick which host to dispatch to.

The project model is defined in [spec §4.4](superpowers/specs/2026-04-30-safessh-design.md#44-project-toml-schema-illustrative); this doc is the user-facing reference for v0.2.

## Project file layout

Projects live as one TOML file per project under `$XDG_CONFIG_HOME/safessh/projects/`:

```
~/.config/safessh/
└── projects/
    ├── prod.toml
    └── staging.toml
```

A minimal project:

```toml
name = "prod"
default_target = "default"

[[targets]]
name = "default"
ssh_config_alias = "prod-host"

[policy]
allow = ["read:safe"]
require_approval = ["destructive:filesystem"]
```

## Adding projects

### Interactive (default — recommended)

```sh
safessh project add
```

Bare `project add` enters an interactive flow that walks you through:

1. **Project name** — validated against existing projects and constrained to `[A-Za-z0-9_-]`.
2. **Target source** — pick between an ssh-config alias and an inline host/user/key target.
3. **Alias mode** (if you picked alias) — *reference* the alias at exec time (lets `~/.ssh/config` evolve), or *snapshot* its values into safessh now.
4. **Inline fields** (if you picked inline) — host, user (defaults to `$USER`), port (defaults to 22).
5. **Private key** (optional, inline only) — fuzzy-pick a key from `~/.ssh/` or paste a path manually.
6. **ProxyJump** (optional, inline only) — `user@bastion[:port]`.
7. **Preview + confirm** — the resulting TOML is printed before it's written. Decline and nothing lands on disk.

The interactive flow requires a TTY. In non-TTY contexts (CI, agents, piped scripts) the same command refuses with exit 2 — pass flags instead (see below) so the path stays scriptable.

### Scripted (flags-only, bypasses interactive)

If any of `--alias`, `--host`, `--user`, or `--import-ssh-config` is set, the interactive flow is skipped and the legacy positional form applies:

```sh
# Reference an ssh-config alias at exec time:
safessh project add prod --alias prod-host

# Inline:
safessh project add stage \
  --host stage.example.com \
  --user deploy \
  --port 2222

# Snapshot from ssh-config alias (decoupled from later ssh-config edits):
safessh project add prod --import-ssh-config prod-host
```

`--import-ssh-config` is mutually exclusive with `--alias` / `--host` / `--user` at the clap level (exit 2). Unknown alias names exit 1 with `safessh: config: ... no ssh-config alias: <name>`. **ProxyJump is not imported** — `ssh2-config` 0.3 does not expose it; use `--alias` instead so ssh handles the chain at exec time.

### Alias vs import — which to choose

| You want… | Use |
|---|---|
| ssh-config drift to follow automatically | `--alias` |
| Pinned, version-controlled connection details | `--import-ssh-config` |
| ProxyJump or any advanced ssh-config feature | `--alias` |
| To stop relying on `~/.ssh/config` for this project | `--import-ssh-config` |

## Listing and editing

```sh
safessh project list                     # one project per line
safessh project edit                     # interactive: pick a project, then edit
safessh project edit prod                # interactive: skip the picker
safessh project remove prod              # deletes the file (atomic)
```

The interactive `project edit` loop lets you:
- Add or remove a target.
- Change the default target.
- Toggle policy categories (`allow`, `require_approval`, `deny`) via a multi-select over the shipped category list.
- Save & exit, or discard & exit.

The TOML preview is printed before the loop starts so you can spot what's currently configured.

If you want raw-TOML editing through `$EDITOR` (defaults to `vi`), set `SAFESSH_EDIT_RAW=1`; the file is overwritten atomically when the editor exits. Use this for bulk edits where the prompts would be tedious.

```sh
SAFESSH_EDIT_RAW=1 safessh project edit prod
```

## Multi-target projects

Each project's `targets` array can hold multiple hosts. The default routing target is `default_target`; override it per-invocation with `--on <name>`.

### Adding additional targets

```sh
# Snapshot from ssh-config alias:
safessh project target add prod \
  --name db \
  --alias prod-db

# Or inline:
safessh project target add prod \
  --name web \
  --host web.prod.internal \
  --user www \
  --port 22 \
  [--identity ~/.ssh/web_id] \
  [--proxy-jump bastion.example.com]
```

`--alias` and the `--host`/`--user` pair are mutually exclusive (one or the other). Either form requires `--name` to disambiguate within the project.

### Listing targets

```sh
safessh project target list prod
# default [default]  alias=prod-host
# db                 alias=prod-db
# web                www@web.prod.internal:22
```

The `[default]` marker shows the project's `default_target`.

### Removing targets

```sh
safessh project target remove prod --name db
```

Two refusals to know about:

- Removing the project's `default_target` exits 1 with `safessh: config: cannot remove default target`. Re-point `default_target` first via `project edit`, then remove.
- Removing a name that doesn't exist exits 1 with `no such target: <name>`.

### Choosing a target at exec time

The `--on <target>` flag selects which target a single `exec` invocation uses. Without it, the project's `default_target` is used (back-compat with v0.1).

```sh
# Routes to the target named "db" in project "prod".
safessh prod --on db exec "psql -c 'select 1'"

# Equivalent placements (--on can appear anywhere in argv):
safessh prod exec --on db "psql -c 'select 1'"
safessh prod exec "psql -c 'select 1'" --on db
safessh prod exec --on=db "psql -c 'select 1'"
```

Unknown target names exit 2 (`safessh: usage: usage: no such target: <name>`).

## TUI: import multiple aliases at once

The TUI's Projects screen has an `i` key that opens an ssh-config import dialog:

```
┌Import from ~/.ssh/config  (Space toggle, Enter create, Esc cancel)─┐
│> [x] alpha    -> deploy@alpha.example                              │
│  [ ] beta     -> deploy@beta.example                               │
│  [x] gamma    -> deploy@gamma.example                              │
└────────────────────────────────────────────────────────────────────┘
```

- `Space` toggles the checkmark on the current row.
- `Enter` creates one project per checked alias (using the alias name as the project name).
- Aliases whose name collides with an existing project are silently skipped — the operation is idempotent.
- `Esc` cancels without committing.

Each created project gets a single Inline target with snapshot-time `host`/`user`/`port`/`identity_file`. Same caveat as `--import-ssh-config`: ProxyJump is not imported.

## ssh-config snapshot caching

The first `--import-ssh-config` (or first TUI `i`) read parses `~/.ssh/config` and writes a TOML snapshot to:

```
~/.cache/safessh/ssh-config-snapshot.toml
```

Subsequent reads return the cached snapshot if the source `mtime` hasn't changed — so Tab-spamming the import dialog doesn't repeatedly invoke the parser.

Override the source path with `SSH_CONFIG_PATH=/path/to/config` (used by tests; also useful for projects with non-standard config layouts).

## Where to look next

- [`docs/cli-reference.md`](cli-reference.md) — every flag, every exit code.
- [`docs/tui.md`](tui.md) — TUI screens, keybindings, live-update behavior.
- [`docs/approvals.md`](approvals.md) — approval lifecycle, persistent rule stores.
- [Spec §4.4](superpowers/specs/2026-04-30-safessh-design.md#44-project-toml-schema-illustrative) — full TOML schema with all fields.
