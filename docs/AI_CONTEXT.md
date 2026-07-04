# JII — AI Context (Current State)

> **Purpose:** the single-page current state of the project, so any agent (AI or
> human) can pick up development in under five minutes. This file describes **only
> the present** — no history. History lives in git; decisions in
> [DECISIONS.md](DECISIONS.md); the plan in [TASKS.md](TASKS.md).
>
> **Keep this file current.** Updating it at the end of every session is mandatory
> (see the AI Handoff Policy in [CLAUDE.md](../CLAUDE.md)).

_Last updated: 2026-07-04_

---

## What JII is

A smart universal package **installer** (not a manager) for Linux, in Rust,
Fedora-first. It searches multiple sources (DNF, Flatpak, and — soon — GitHub
Releases, COPR…), ranks them, installs the best, and explains why. Read
[CLAUDE.md](../CLAUDE.md) for binding constraints and
[ARCHITECTURE.md](ARCHITECTURE.md) for the canonical design.

## Current phase

**Phase 4 — GitHub Releases, COPR & trust** (in progress). Phases 0–3 are complete
and verified (skeleton → DNF end-to-end → state/remove/why → multi-source ranking +
cache + doctor).

## Last completed work

**`jii remove` for GitHub (file-based) installs** — added `Provider::is_installed(record)`
(default = list lookup; github overrides to check `~/.local/bin/<name>` exists), so
`resolve_installed` confirms file-based installs without a manifest or a new record
field, and with no source branching in the core. Verified with a real jq install→remove
cycle (file removed, registry + history updated).

Prior slice — **GitHub Releases provider** (`provider/github.rs`), raw-binary,
verified end-to-end:

- Query `owner/repo` → latest release; selects a raw executable asset for the host
  arch (Linux, musl preferred over gnu); rejects other-OS/packages/archives.
- **All network in `search`** (release + checksums), so `plan_install` is pure and
  unit-tested. sha256 is resolved from a checksums asset and **enforced** by the
  executor; `⚠ unverified` shown when none is published.
- Plans `Download`→`Place` into `~/.local/bin` (mode 0o755, **no root**).
- Trust `untrusted` → always confirmed, even under `--auto` (verified: `--auto`
  aborts non-interactively). `GITHUB_TOKEN` supported via `network.github_token_env`.
- Verified with a real `jqlang/jq` install in an isolated `$HOME`: checksum matched,
  binary runs (`jq-1.8.2`), registry recorded. See [DECISIONS.md](DECISIONS.md) ADR-0014.

Prior slice: the execution model (`Action` enum + `exec.rs`), ADR-0007.

## Current task

None in progress — pick the next recommended task below.

## Next recommended task

Continue Phase 4. In rough priority order:

1. **`Extract` action + archive assets** — most releases ship `.tar.gz`/`.zip`; add a
   focused `Extract` action to the execution model and let github select archives.
2. **`provider/copr.rs`** — COPR web-API project search, root repo-enable, trust.
3. **`jii audit`** and **rate-limit health in `doctor`** (GitHub).
4. Broad name→repo resolution and release pagination for github.

Full list in [TASKS.md](TASKS.md) Phase 4.

## Current blockers

None.

## Build status

`cargo build` — clean, no warnings. `cargo clippy` — clean.

## Test status

`cargo test` — **50 passing, 0 failing**. Coverage: dnf/flatpak parsers, ranking,
registry, cache, privilege elevation prefixing, the executor (sha256 digest,
verification accept/reject/case-insensitive/fail-closed, place+mode+remove,
run_action success/failure), and github (owner/repo parsing, release JSON, asset
selection incl. musl preference + archive rejection, checksums parsing, plan shape).

## Environment & commands

- **Target/dev OS:** Fedora (dnf5). Rust edition 2024.
- **Build:** `cargo build` (must be warning-clean).
- **Lint:** `cargo clippy` (installed; must be clean).
- **Test:** `cargo test`.
- **Preview a plan:** `cargo run -- install <pkg> --dry-run` (no side effects).
- **`cargo fmt` / `rustfmt` are NOT installed** on this dev host — match the
  surrounding code style by hand; do not rely on `cargo fmt`.
- External tools invoked at runtime: `dnf5`, `flatpak`, `sudo`/`pkexec`. GitHub
  provider (next) will use HTTPS via `reqwest` and optionally `GITHUB_TOKEN`.
- **CI** (`.github/workflows/ci.yml`) runs clippy (`-D warnings`) + tests on every
  push/PR — the automated Definition of Done (ADR-0013). The runner has no
  dnf5/flatpak, so end-to-end `--dry-run` checks stay manual on Fedora.

## Important architectural decisions (quick reference)

Full rationale in [DECISIONS.md](DECISIONS.md). The load-bearing ones:

- **Core never branches on source** — everything behind the `Provider` trait (ADR-0004).
- **`Plan` is first-class** — declarative `Action`s, always previewable via `--dry-run`
  (ADR-0003), executed by `exec.rs` (ADR-0007).
- **JII never fully root** — only concrete steps escalate, via `privilege.rs` (ADR-0005).
- **Trust threshold, not global yes** — `untrusted` always confirmed (ADR-0006).
- **Single crate**, **JSON registry** (not SQLite), **`PkgVersion(String)`** not semver
  (ADR-0001/0002/0009).

## Known technical debt

- **GitHub archives unsupported** — no `Extract` action yet, so `.tar.gz`/`.zip`-only
  releases yield no candidate (ADR-0014). Next task #1.
- **Flatpak identified by appid** (`org.gimp.GIMP`): `jii remove gimp` may not resolve
  a Flatpak by friendly name. Revisit with a name/id split if it becomes painful.
- **`latest`/`minimal` profiles + freshness/health ranking tie-breakers** are reserved
  — they need comparable versions / dependency-footprint data not yet collected.
- **GPG / sigstore verification** are stubbed to fail closed in `exec.rs::verify_bytes`
  — implement when a source needs them (GitHub).
- **`cli/mod.rs`** (~410 lines) holds command handlers inline; split into
  `cli/commands/*` if it grows unwieldy.

## Where things live

```
src/
  model.rs       core types (Action, InstallPlan, PackageCandidate, TrustLevel…)
  provider/      Provider trait + dnf.rs, flatpak.rs, github.rs
  engine/        orchestration (search→rank→plan→execute) + ranking.rs
  exec.rs        plan executor (the one place that runs a plan's actions)
  privilege.rs   sudo/pkexec elevation (prime + run)
  cache.rs       on-disk TTL search cache (stale-on-error)
  registry.rs    JSON install registry
  cli/, ui/, config.rs, platform.rs, error.rs
docs/            ARCHITECTURE (canonical) · ROADMAP · TASKS · DECISIONS · this file
AGENTS.md        tool-neutral onboarding entry (read first); CLAUDE.md = Claude's copy
LICENSE          MIT
```

To add a source: implement `Provider` (or a declarative TOML later) — never edit the
core. Use the `/new-provider` skill.
