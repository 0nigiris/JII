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

**Execution model evolution** (Phase 4 prerequisite) — landed and stable:

- `model.rs`: `Action` enum (`RunCommand` / `Download` / `Place` / `RemoveFile`)
  replaced the old argv-only `Step`. `InstallPlan.needs_root()` is derived.
- `exec.rs` (new): `run_plan` dispatches each action to a focused handler; downloads
  are HTTPS-only with **enforced** verification (sha256; gpg/sigstore fail closed).
- `privilege.rs`: reduced to `prime()` (once) + `run()` (one command); no more
  `execute_plan`.
- DNF and Flatpak providers emit `RunCommand` — behavior unchanged.
- UI `describe_action` / `action_to_json` render every action (preview == execution).

See [DECISIONS.md](DECISIONS.md) ADR-0007 for the rationale.

## Current task

Not started yet — the executor is done; the next provider is the immediate task.

## Next recommended task

**`provider/github.rs`** (Phase 4, step 2), building on the new execution model:

1. Name → repo resolution (start from an explicit `owner/repo`; broader resolution
   later).
2. Fetch latest release; filter assets by arch/libc.
3. Emit `Download` (with `Verification::Sha256` when a checksum asset exists, else
   `Verification::None` + `untrusted` trust) → `Place` into `~/.local/bin` (mode
   0o755). No root.
4. `GITHUB_TOKEN` support to lift rate limits.
5. Per-candidate trust: default `untrusted` unless verified.

Then: **trust enforcement** (`untrusted` always confirmed, even `--auto`), then
`provider/copr.rs`, then `jii audit` and rate-limit health in `doctor`. Full list in
[TASKS.md](TASKS.md) Phase 4.

## Current blockers

None.

## Build status

`cargo build` — clean, no warnings. `cargo clippy` — clean.

## Test status

`cargo test` — **38 passing, 0 failing**. Coverage: dnf/flatpak parsers, ranking,
registry, cache, privilege elevation prefixing, and the new executor (sha256 digest,
verification accept/reject/case-insensitive/fail-closed, place+mode+remove,
run_action success/failure).

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
  provider/      Provider trait + dnf.rs, flatpak.rs (add github.rs here)
  engine/        orchestration (search→rank→plan→execute) + ranking.rs
  exec.rs        plan executor (the one place that runs a plan's actions)
  privilege.rs   sudo/pkexec elevation (prime + run)
  cache.rs       on-disk TTL search cache (stale-on-error)
  registry.rs    JSON install registry
  cli/, ui/, config.rs, platform.rs, error.rs
docs/            ARCHITECTURE (canonical) · ROADMAP · TASKS · DECISIONS · this file
```

To add a source: implement `Provider` (or a declarative TOML later) — never edit the
core. Use the `/new-provider` skill.
