# safessh — Brand positioning brief

**Status:** approved 2026-05-03.
**Scope:** README, logo tagline, screenshot lineup, GitHub repo description, voice for the README body. Implementation is a documentation/asset update, not a code change.

This brief is the source of truth for how `safessh` is positioned to outside readers. When updating marketing surfaces (README, repo description, future docs/* hero text, social posts) they should defer to this document.

---

## 1. Audience

Two equally-weighted layers. The brand speaks to both at once.

**Layer 1 — the universal pain (broad, evergreen).** Anyone who SSHs into more than two servers and has felt the friction: forgetting which key goes with which IP, scattered bastion configs, no real audit of "what did I run on that box last Tuesday?". This is the larger audience and the more stable pain — independent of any AI cycle.

**Layer 2 — the kicker (timely, high leverage).** The LLM-curious operator who wants Claude Code / Cursor / Codex / Gemini-CLI / Codex-CLI to do real ops work without ever holding the keys. This audience is smaller today but growing; the value proposition compounds with the breadth of agent tooling on the market.

The same features serve both layers. The brand never picks a side.

## 2. Tone

**Hybrid (option C from the brainstorm).** Crisp, marketing-tight wordmark and tagline at the top of the README; conversational-but-professional body copy. Concrete examples in every section — code blocks, real command shapes, real file paths. No marketing puffery; no false modesty. The voice is "experienced operator describing a tool they built, with examples to back the claim."

Reference: matches the [`cli-pm/kata`](https://github.com/sanif/cli-pm) README pattern at the macro structure level (centered logo + badges + headline + per-feature sections + table + docs index), with safessh's content and voice.

## 3. Lead message

**Primary tagline (under the wordmark):**

> **Safe remote-server access for humans and LLMs.**

Four words of value prop. "Safe" earns the brand name (`safe` + `ssh`). "Remote-server access" frames the product at the conceptual level the audience cares about, not the protocol level. "For humans and LLMs" makes the dual-pitch explicit.

**Two-line subtitle (between badges and the hero screenshot):**

> Stop juggling keys, IPs, and bastions across a dozen servers. Save each as a project, run commands by name, and get every action in a queryable audit log. Hand a project to an LLM agent and the credentials never leave your machine.

**GitHub repo description (one-liner shown in repo cards, search results, social previews):**

> Run remote commands by project name. Audited. Safe to hand to an LLM.

## 4. Hero visual

A new **split-panel SVG screenshot** lives directly above the lead message. It shows two concurrent terminals:

- **Left panel:** human typing `safessh prod exec "systemctl status nginx"` and receiving framed `<stdout>…<exit code=0…/>` output.
- **Right panel:** Claude Code (or generic agent) calling the *same* `safessh prod exec "..."` and receiving the *same* framed output.
- **Below both:** a single audit-log strip showing the two events in chronological order — same project, same target, same audit row shape, different `actor` / source.

The visual proves the tagline ("same projects, same audit, same guard rails") without requiring the reader to read the body. File: `screenshots/hero-split.svg`. Width 1200px to fit the README content frame on GitHub.

The existing screenshots stay and are reused further down the page:

- `screenshots/approval-flow.svg` — under the "Approve before it runs" / "Safe to hand to an LLM" sections.
- `screenshots/audit-query.svg` — under "Audit by default".
- `screenshots/tui.svg` — under "Review with the TUI".

A new small terminal SVG, `screenshots/projects.svg`, supports the "Servers as named projects" section. It shows `safessh project add` interactive flow (truncated) followed by `safessh project list` output.

## 5. Body structure

The README has five "what makes it different" sections (each ~80–150 words plus a screenshot or code block), in this order:

1. **Servers as named projects** — needs `screenshots/projects.svg`. The abstraction. Why this exists.
2. **Audit by default** — `screenshots/audit-query.svg`. The receipts. Half of the dual-pitch.
3. **Approve before it runs** — `screenshots/approval-flow.svg`. The guard rails. The other half.
4. **Review with the TUI** — `screenshots/tui.svg`. Human-friendly review surface.
5. **Plug into any agent** — short code block listing the six adapters and the `--target all` form. No screenshot.

After the five sections, a single condensed feature table grouped by area (Projects · Audit · Policy · Skill · TUI · Ops). One row per feature, no "Available in vX" status badges (the README ships with the latest tag and the changelog is the right place for version history).

After the feature table: a "How it fits in your agent loop" numbered walk-through (already present today, light touch-up only), the command index, the agent-integration matrix, the docs table, requirements, license.

## 6. Voice samples (approved)

Three representative passages that anchor the tone for the rest of the body. Future README edits should match this register — declarative lead-in, concrete code-block examples, no hedging or marketing puff.

### Servers as named projects

> Tracking eleven server identities across SSH config, password managers, and scattered notes is unmanageable in practice. `safessh` collapses each connection into a single named project — `prod`, `staging`, `db-replica` — sourced from an existing `~/.ssh/config` Host block or from a one-time interactive setup. Subsequent commands address the project by name:
>
> ```sh
> safessh prod exec "systemctl status nginx"
> safessh app --on web exec "tail -n 100 /var/log/access.log"
> ```
>
> Multi-target projects allow several hosts under one logical name; the active target is selected per-call with `--on <name>`.

### Approve before it runs

> Read-only commands are allowed by default. Anything that mutates — `rsync`, `apt-get install`, redirected output — pauses for explicit approval. The proxy emits a structured `BLOCKED:` block to stdout containing the parsed command, matched policy categories, and a one-time approval token:
>
> ```
> BLOCKED: approval required
>   category   filesystem:write
>   reason     rsync writes outside the allow list
>   token      ABC-7K2
> ```
>
> A separate invocation releases the command:
>
> ```sh
> safessh approve ABC-7K2              # one-shot
> safessh approve ABC-7K2 --timed 30   # next 30 minutes
> safessh approve ABC-7K2 --always     # persistent rule
> safessh approve ABC-7K2 --block      # permanent deny
> ```

### Safe to hand to an LLM

> Granting an LLM agent unrestricted SSH carries significant operational risk: arbitrary command execution against production hosts with no review surface. `safessh` reduces that surface to a fixed CLI vocabulary — `safessh <project> exec`, `read`, `write`, `forward` — without exposing the underlying credentials, hostnames, or ports. Every invocation passes through the same policy gate and the same audit trail as a human-issued command. A representative agent loop:
>
> ```
> claude  ›  deploy the latest build to prod
>         ↓
>         safessh prod exec "rsync -av build/ /srv/app/"
>         ↓
>         BLOCKED: filesystem:write — token ABC-7K2
>         ↓
>         (operator) safessh approve ABC-7K2 --once
>         ↓
>         <stdout>... 2 files synced ...</stdout>
>         <exit code="0" duration="312ms"/>
> ```
>
> The agent receives a predictable, parsable interface. The operator keeps the keys, the policy, and the audit.

## 7. What to deprioritize

These are kept out of the README to maintain focus, and remain available in their own dedicated docs:

- **The 12 safety invariants.** Detail belongs in `docs/security.md` (full breakdown there). The README mentions "policy + audit + atomic writes" as a single phrase and links to the security doc.
- **File ops + port forwarding internals.** The README references both as one-row entries in the feature table. Full coverage stays in `docs/files.md` and `docs/tunnels.md`.
- **cargo-dist mechanics, Conventional Commits, Rust crate layout, the workspace dep graph.** Of interest to contributors; lives in `docs/development.md`.
- **Per-version status badges ("Available in v0.3", "Planned for v0.7", etc.).** Removed from the README. The CHANGELOG is the right place for version history.
- **The internal `// SAFETY-INVARIANT-N` markers.** A code-comment convention, not a user-facing concept.

## 8. What to *not* lead with

When tempted to add new copy, the brief explicitly excludes:

- **"AI safety" framing.** Sounds either trivial (yet another safety wrapper) or ominous (existential AI risk). The actual story is "this lets the LLM be useful without the operator giving up control." That's a tooling story, not a safety-discourse story.
- **"Zero-trust" language.** Overused, doesn't accurately describe the model (we trust local users, we don't trust commands; that's not the same as zero-trust as a marketing term).
- **"Enterprise-grade".** This is a personal tool. False claim and wrong audience anyway.
- **"AI-first".** Layer 2 is a kicker, not a primary. Half the audience never runs an LLM and still wins.
- **Self-deprecating openings** ("I built this for myself, take what's useful"). Hybrid tone allows honest paragraphs in the body, but the headline carries weight.

## 9. Implementation checklist

The README rewrite + asset additions:

- [ ] Update README headline + tagline + subtitle to match §3.
- [ ] Update GitHub repo description (via `gh repo edit --description`) to match §3.
- [ ] Create `screenshots/hero-split.svg` per §4.
- [ ] Create `screenshots/projects.svg` per §4.
- [ ] Reorder README body to the five sections in §5, with the screenshot per section.
- [ ] Replace the existing flat feature table with a category-grouped table.
- [ ] Drop the per-version status badges from the feature table.
- [ ] Sanity-pass the body voice against the §6 samples.

These are all documentation / asset changes — no Rust code touched. They land on `develop` as a single commit (or two, if the screenshot creation deserves its own commit), no version bump required.

---

## Spec self-review

**Placeholders:** none — every section has a concrete decision. The implementation checklist is concrete actions, not "TBD".

**Internal consistency:** §3 (lead message), §4 (hero visual), §5 (body structure), §6 (voice) all reinforce the same dual-audience claim. §7 and §8 are explicitly negative-space about what the brand is *not*.

**Scope:** focused on README + assets + repo description. The implementation is a single PR/commit. Doesn't bleed into code changes or feature work.

**Ambiguity:** "Safe" in the tagline is intentionally elastic — covers credentials hidden, policy gating, audit trail, and approval flow. The body sections concretize what "safe" means without the headline having to.
