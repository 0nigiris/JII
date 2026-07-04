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

**`jii doctor` health checks** — `Provider::probe()` reports raw facts
(`reachable`, `rate_limited`, human `detail`; default = local binary availability);
github overrides it to hit `/rate_limit` (surfacing `remaining/limit` as detail, and
flagging `RateLimited` at 0), copr pings `project/search` for reachability. The engine
maps facts + latency to `Health` via a pure, unit-tested `health_from()` (precedence
Offline → RateLimited → Slow → Healthy) — the decision logic stays in the engine
(ADR-0015/0019). `SourceHealth` gained `detail`, shown in the human table and `--json`.
Verified live: github `healthy (58/60 req left)`, copr reachable-but-slow.

Prior Phase 4 slices, all verified end-to-end: `jii audit` (ADR-0018); COPR provider
(ADR-0017); `Action::Extract` + `.tar.gz` (ADR-0016); github `jii remove`
(`Provider::is_installed`); GitHub Releases provider (raw-binary, ADR-0014); the
execution model (`Action` enum + `exec.rs`, ADR-0007).

## Current task

None in progress — pick the next recommended task below.

## Next recommended task

Phase 4's core is complete. What remains is polish/hardening (revisit before starting
Phase 5):

1. Broad name→repo resolution and release pagination for github.
2. More archive formats (`.zip`, `.tar.xz`) if real tools need them.
3. Better COPR project disambiguation if the chroot-count heuristic proves weak.
4. Real GPG/sigstore verification in `exec.rs::verify_bytes` (currently fail-closed).

Full list in [TASKS.md](TASKS.md) Phase 4.

## Current blockers

None.

## Build status

`cargo build` — clean, no warnings. `cargo clippy` — clean.

## Test status

`cargo test` — **71 passing, 0 failing**. Coverage: dnf/flatpak parsers, ranking,
registry, cache, privilege elevation prefixing, the executor (sha256 digest,
verification accept/reject/case-insensitive/fail-closed, place+mode+remove, tar.gz
extract + member selection, run_action), github (owner/repo, release JSON, asset
selection, checksums, plan shapes), copr (search parsing, exact-name + fedora/arch
chroot selection, two-step root plan), audit (verification resolution + concern
logic), and doctor health mapping (`health_from` precedence).

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

- **COPR project ambiguity** — several projects can share a package name; we pick the
  exact-name match building for the most Fedora chroots, but that is a weak signal (a
  fork may build widely). The visible `owner/project` in the plan + confirmation is the
  safety net. A real popularity/quality metric isn't in the search API (ADR-0017).
- **GitHub archives: only `.tar.gz`/`.tgz`** — `.zip` / `.tar.xz`-only releases still
  yield no candidate (ADR-0016). Add formats when a real tool needs them.
- **GitHub binary named after the repo** — the placed file is `~/.local/bin/<repo>`;
  when the archive's binary basename differs (e.g. ripgrep's `rg`), it's still
  installed as `<repo>`. Fine for now (repo==binary in the common case).
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
