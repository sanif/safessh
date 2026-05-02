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

### From an ssh-config alias

Use this when you've already configured the host in `~/.ssh/config` and want safessh to delegate connection details to ssh at exec time. ProxyJump and other advanced ssh-config features pass through verbatim.

```sh
safessh project add prod --alias prod-host
```

The resulting target is `Target::SshConfigAlias { ssh_config_alias = "prod-host" }`. ssh resolves `prod-host` from your ssh-config every invocation.

### Inline (host + user + port)

Use this when you want safessh to own the connection details — useful for ephemeral environments where ssh-config would clutter, or when the target lives in a CI variable.

```sh
safessh project add stage \
  --host stage.example.com \
  --user deploy \
  --port 2222
```

### Importing from ssh-config (snapshot)

Different from `--alias`: this **snapshots** the host details into the project TOML. `host`, `user`, `port`, `identity_file` are read from the matching `Host` block in `~/.ssh/config` and pinned in the project. Subsequent ssh-config edits won't drift the project.

```sh
safessh project add prod --import-ssh-config prod-host
```

**ProxyJump is not imported** — `ssh2-config` 0.3 does not expose it. If your alias relies on ProxyJump, use `--alias` instead so ssh handles the chain at exec time.

When `--import-ssh-config` is given, `--alias` / `--host` / `--user` are clap-rejected (exit 2). Unknown alias names exit 1 with `safessh: config: ... no ssh-config alias: <name>`.

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
safessh project edit prod                # opens $EDITOR on prod.toml
safessh project remove prod              # deletes the file (atomic)
```

`project edit` honors `$EDITOR` (defaults to `vi`). The file is overwritten atomically when the editor exits.

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
