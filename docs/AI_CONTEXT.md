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

**Phase 4 complete → entering Phase 5 (user-space sources).** Phases 0–4 are done and
verified (skeleton → DNF → state/remove/why → multi-source ranking + cache + doctor →
GitHub/COPR/trust/audit/health/.zip). A full **architecture re-evaluation** (ADR-0022)
gated Phase 5: the model needs **no change** to add cargo/npm/pipx/go — they are pure
new `Provider`s. Growth is additive; the core does not branch on source.

## Last completed work

**Architecture re-evaluation before Phase 5 (docs only).** Checked the live code against
the design. Verdict: load-bearing structure is sound (`Provider` seam, plan-as-`Action`,
trust threshold, registry-as-hint); **Phase 5 needs no model change**. Recorded **ADR-0022**
with three forward rules — (1) new capabilities (version mgmt, metadata, manager bootstrap)
are **optional `Provider` methods with safe defaults**, following the `probe`/`is_installed`
precedent, never a fat trait or core branch; (2) keep the **engine UI-free** — the
`&Renderer` in `Engine::install`/`remove` is the one `ui` coupling, to be decoupled via a
progress-event trait **before** a second frontend (not now, YAGNI); (3) versions/metadata/
rollback live in the provider/registry, not the core (reaffirms ADR-0009). Also **synced
`ARCHITECTURE.md`** §5/§9/§11/§15 to the evolved execution model (`Action`+`exec.rs`,
verification on `InstalledRecord`) — a stale canonical doc was an active hazard.

**Prior — GitHub `.zip` release assets** — `exec::extract` now dispatches on the archive's
file-name extension into `read_tar_gz` / `read_zip` (both decode to the same
`ArchiveFile` list, so member selection + writing stay format-agnostic — the seam
ADR-0016 predicted). github's `classify` gained `AssetKind::Zip` (ranked below `TarGz`,
which preserves unix modes) and now rejects delta-patch assets
(`.bsdiff`/`.patch`/`.delta`/`.zsync`) that used to masquerade as raw binaries —
surfaced by `denoland/deno`, which ships a `*.bsdiff` next to its Linux `.zip`. Verified:
real-release dry-run selects `deno-…-linux-gnu.zip` → Extract; zip round-trip
(create→extract→assert bytes+mode) unit-tested; the untrusted trust barrier correctly
refused a non-interactive real install (ADR-0006). Added the `zip` crate
(`default-features=false`, `deflate`). See ADR-0016 (2026-07-04 update).

Also this session (docs only): **ADR-0020** (JII is a universal layer, not another
package manager) and **ADR-0021** (integrate external backends like UPAC only via their
stable public API, as another `Provider`; implement nothing until that API exists), plus
new ROADMAP Future ideas (more managers, bootstrapping a missing manager, provider-supplied
metadata).

Prior Phase 4 slices, all verified end-to-end: `jii doctor` health/rate-limit (ADR-0019);
`jii audit` (ADR-0018); COPR provider (ADR-0017); `Action::Extract` + `.tar.gz` (ADR-0016);
github `jii remove` (`Provider::is_installed`); GitHub Releases provider (ADR-0014); the
execution model (`Action` enum + `exec.rs`, ADR-0007).

## Current task

None in progress — pick the next recommended task below.

## Next recommended task

**Phase 5 — first user-space provider: `provider/cargo.rs`.** The re-evaluation
(ADR-0022) confirmed the model is ready; `cargo install <crate>` is the cleanest start
(no root, no network parsing beyond the crates.io API, installs into `~/.cargo/bin`).
Shape it exactly like github: `is_available` (cargo present), `search` (crates.io API),
`plan_install` (a `RunCommand` `cargo install …`, `needs_root=false`), `list_installed`
(`cargo install --list`), `plan_remove` (`cargo uninstall`), community trust. No core
change. Then npm/pipx/go the same way, then `jii update`.

Polish/hardening deferred (not blocking Phase 5; several are now **future features**, do
not implement as silent heuristics):
- **GitHub repository selection** — interactive, "never silently install the wrong repo".
- `.tar.xz` archives (needs an xz decoder dep); better COPR disambiguation; real
  GPG/sigstore verification in `exec.rs::verify_bytes` (currently fail-closed).
- **Engine UI-free seam** (ADR-0022): decouple `&Renderer` from `Engine::install/remove`
  — do this **before** any GUI/second frontend, not now.

Full list in [TASKS.md](TASKS.md) Phase 4.

## Current blockers

None.

## Build status

`cargo build` — clean, no warnings. `cargo clippy` — clean.

## Test status

`cargo test` — **76 passing, 0 failing**. Coverage: dnf/flatpak parsers, ranking,
registry, cache, privilege elevation prefixing, the executor (sha256 digest,
verification accept/reject/case-insensitive/fail-closed, place+mode+remove, tar.gz **and
zip** extract + member selection, unknown-format rejection, run_action), github
(owner/repo, release JSON, asset selection incl. `.zip`/tar.gz preference, checksums,
plan shapes), copr (search parsing, exact-name + fedora/arch chroot selection, two-step
root plan), audit (verification resolution + concern logic), and doctor health mapping
(`health_from` precedence).

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
- **GitHub archives: `.tar.gz`/`.tgz` + `.zip`** — `.tar.xz`-only releases still yield
  no candidate (ADR-0016); adding it means an xz decoder dependency. `.zip` entries
  authored on non-unix systems carry no mode, so the sole-executable fallback can't fire
  — the exact-basename match still resolves the common single-binary case.
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
- **Engine↔UI seam (ADR-0022):** `Engine::install`/`remove` take `&crate::ui::Renderer`
  so the executor can print progress — the one `ui` type reaching into the engine. Fine
  now (single CLI frontend), but it must be decoupled (a progress-event/`ProgressSink`
  trait) **before** a GUI/second frontend or a workspace split. Meanwhile: **do not add
  new `ui` types to engine signatures.**

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
