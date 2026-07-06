# JII — Road to the first public Beta (+ frozen post-Beta backlog)

**Decision (2026-07-06, owner-set).** The terminal CLI is functionally complete
(14 providers, 180 tests, T1–T5 + the whole UX-polish pass U0–U8 done). We are
**freezing new large features** and driving to the **first public Beta**. The
goal now is *real feedback from real users*, not more breadth. What to build
*after* Beta will be decided by that feedback — not guessed at now.

This document is the single source of truth for that decision. `docs/TASKS.md`
tracks the active Beta work; the frozen backlog below is parked here so it is
**not forgotten and not silently resurrected**.

> **Freeze rule.** Until the Beta is cut, do not start any item in the "Frozen"
> section. Bug fixes, hardening, tests, docs, and packaging are always in scope;
> new user-facing capabilities are not. If a frozen item turns out to be a *true
> blocker* for Beta, say so explicitly and get the owner's go-ahead first.

---

## Active — the road to Beta (priority order, owner-set)

Legend: 🤖 an agent can do it end-to-end · 🧑 needs the owner (hardware, design,
accounts, or a human judgement call) · 🤝 agent prepares, owner finishes.

### 1. CI 🤖
GitHub Actions on push/PR: `cargo build`, `cargo clippy -D warnings`, `cargo test`,
`cargo fmt --check`. Mechanises the CLAUDE.md "clippy clean, tests green at every
commit" rule so regressions can't land silently. (fmt is not on the dev host, so
CI is also where formatting is actually enforced.)

### 2. Integration tests 🤖
CLI-level end-to-end tests (`assert_cmd` + `predicates`) over an isolated
`XDG_CONFIG_HOME`/`XDG_STATE_HOME`: `search`/`info`/`list`/`history`/`sources`/
`--json`/`--dry-run`/`setup`, exit codes, and the not-installed / empty-ledger
edges. Plus a **registry-partial-failure** test (a batch that fails midway leaves
the registry consistent). Today all 180 tests are unit tests — there is no
end-to-end coverage, which is the biggest gap for a tool that escalates to root.

### 3. Clean-VM verification on Arch / Ubuntu / Debian / openSUSE 🧑 (🤝 agent scripts it)
The whole cross-distro layer (apt, pacman, zypper, nix, snap, plus the non-Fedora
`plan_update_all` impls and the recommend-catalog's distro filter) is written but
**never run on a live non-Fedora host**. Beta cannot ship without this. The agent
can prepare a **repeatable smoke-test script** (install/remove/update/search/info/
doctor/list on each distro) and a results checklist; the owner runs it on real VMs
and reports back. This is the one Beta blocker an agent cannot close alone.

### 4. Public documentation & assets 🤝
- **README polish** 🤖 — sharpen the current README for a first-time visitor;
  honest "Status / Limitations" section; quick-start.
- **CONTRIBUTING.md + SECURITY.md** 🤖 — required for a public repo; SECURITY
  matters doubly for an installer with a trust model (how to report, what the
  trust levels mean, the "never fully root" guarantee).
- **asciinema script** 🤝 — agent writes the exact command sequence/narration;
  owner records it on a real terminal.
- **Logo + screenshots** 🧑 — design/visual work; owner (or a designer) produces
  them. Agent can suggest a concept and wire them into the README once they exist.

### 5. Public release 🤝
Tag `v0.1.0-beta`, GitHub Release with notes, a signed binary and/or a COPR
package (dogfood: install `jii` via `jii`). Agent can draft the release notes,
the `.spec`/packaging, and shell completions + a man page (`clap_complete` /
`clap_mangen`) to bundle; owner owns the actual publish, signing keys, and any
account/registry steps.

**Beta is cut when:** CI is green, integration tests exist and pass, the CLI is
verified on ≥1 real host per target distro, the repo reads as a finished public
project, and a tagged Beta release is published.

---

## Frozen until after Beta

Parked deliberately. **Do not start these without the owner's explicit go-ahead
after Beta feedback lands.** Each already has a design/ADR sketch (see refs) so
picking one up later is cheap. Priority *within* this list is intentionally left
open — Beta feedback decides it.

### Features (were P2)
- **`jii doctor --fix`** — apply what `doctor` reports (PATH tail in shell rc,
  offer `GITHUB_TOKEN`) as a previewable plan. ROADMAP "System onboarding";
  Analyze→Explain→Ask→Apply; natural continuation of U6.
- **Catalog aliases** (`data/catalog.toml`: `vscode→code`, `node→nodejs`, …) —
  pure data + a thin lookup; big search hit-rate win. ROADMAP Phase 6.
- **Version chooser** (`name@version`) — `@ref` is already parsed and *rejected*;
  implement via an optional `Provider::available_versions`, provider-ordered
  (ADR-0009: versions are opaque; provider supplies the ordering). T5 tail; own ADR.
- **GitHub repo chooser** — on an ambiguous bare name, show the top repos
  (stars/owner/verified) and let the user pick; never silently install a
  look-alike. T5 tail; ROADMAP "GitHub repository selection".
- **T6 — Bootstrap a missing manager** — "this program is only on Homebrew;
  install Homebrew first?" as an explicit, `--dry-run`-able plan step; strongest
  consent, never auto, never launders trust. Own ADR.
- **`jii undo`** — reverse the last operation from history. ROADMAP Phase 5.
- **Streaming / progressive search** — show fast sources immediately, let slow
  ones stream in; the real fix for perceived speed, lets the search timeout be
  raised again. UX_EVALUATION §A; own ADR.

### Tech debt (were P3 — do opportunistically, not a Beta gate)
- **Split `cli/mod.rs`** (~1820 lines) into `cli/commands/*`.
- **Flag-shed (ADR-0031):** `--auto` → hidden alias of `-y`; `--profile` →
  config/wizard only; `--no-color` → also honour the `NO_COLOR` env var.
- **Update-staleness:** a bulk-updated tracked package shows a stale version in
  `jii list` — refresh the registry record after a system update.
- **`#![allow(dead_code)]` on `model.rs`** — narrow or remove it as later phases
  consume the API (CLAUDE.md wants module-wide silencers gone).
- **Declarative TOML providers** (`data/sources/*.toml` + `provider/declarative.rs`)
  — so trivial sources don't need Rust. ROADMAP Phase 6.

### Long-range (were P4 — post-Beta, likely well after)
SQLite migration behind the same registry API · semantic/fuzzy search (Phase 6
Stage 2–4) · AUR · winget/macOS · Cargo workspace split · UPAC backend (ADR-0021)
· the **GUI / universal software center** (explicitly out of terminal scope;
design frozen in ROADMAP "Future ideas").

---

*When Beta feedback arrives, revisit this list, let the feedback reorder it, and
promote the chosen items into `docs/TASKS.md` with a fresh ADR where the design
sketch says one is needed.*
