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

**Phase 5 — user-space sources & update (in progress).** Phases 0–4 done and verified.
The pre-Phase-5 re-evaluation (ADR-0022) confirmed the model needs **no change** for
these providers. **`cargo`, `npm`, `pipx` are done** — pure `Provider`s, no core change.
Next: `go` (and at `go`, evaluate extracting a shared `command_plan` helper), then
`jii update`.

## Last completed work

**`provider/pipx.rs` (PyPI, via pipx).** Third Phase 5 provider, mirrors cargo:
`pipx install/uninstall`, first-class `pipx upgrade`, `pipx list --json`, installs to
`~/.local/bin` (no root), community trust. **Key decision — ADR-0023:** PyPI's API exposes
no reliable program-vs-library signal (the `Environment :: Console` classifier is ~40%
unreliable — measured on 10 popular CLIs), so pipx does **not** pre-filter (unlike cargo's
`bin_names` / npm's `bin`); it offers the package and lets `pipx install` reject non-apps.
Principle: a visible false positive beats silently hiding a real app. No core change, no
engine special-case. Verified: real PyPI search through JII (black + requests both offered),
dry-run (single unprivileged command), via a stubbed `pipx` on PATH (pipx not installed
here). Before writing pipx: assessed duplication — nothing hit the 3× threshold beyond the
already-extracted `http_client`, so no pre-pipx refactor (the `command_plan` extraction is
scheduled for `go`, the 4th user-space provider).

**Prior — `provider/npm.rs` (npm registry)** + a shared-`http_client()` refactor. npm mirrors
cargo: `search` hits the npm registry `/<pkg>/latest` and **only offers packages that
install a CLI** (non-empty `bin`), so a library like `lodash` yields no candidate.
Installs are unprivileged and forced into `$HOME/.local` via `--prefix` (binaries →
`~/.local/bin`, never root, regardless of npm's host prefix). `list_installed` reads
`npm ls -g --json` tolerantly. Community trust; no core change, no engine special-case.
Verified: real registry search through JII (prettier→v3.9.4 offered, lodash rejected),
dry-run (single unprivileged command), multi-source ranking. Also **extracted
`provider::http_client()`** (the reqwest builder + User-Agent was copied 3× in
copr/github/cargo; npm would have been the 4th) — pure refactor, `jii doctor` verified.

**Prior — `provider/cargo.rs` (crates.io).** First Phase 5 provider. `cargo install <crate>`
builds executables into `~/.cargo/bin` — user-space, no root. `search` hits the
crates.io `crates/{name}` API and **only offers crates that ship a binary** (checks
`bin_names` on the newest version), so a library-only crate (`serde`) yields no
candidate — JII installs *programs*, not libraries. Community trust (crates.io registry;
cargo verifies checksums itself, so the plan is one unprivileged `RunCommand`, no
separate Download/verify). `list_installed` parses `cargo install --list`. Registered in
`provider/mod.rs` like the others — **no engine special-case, no model change** (ADR-0022
holds). Verified: real crates.io search through JII (ripgrep→v15.1.0 offered, serde
rejected), dry-run (single unprivileged command), multi-source ranking (dnf recommended,
cargo listed as alternative), 5 unit tests. From-source compile not run (COPR precedent).

**Prior — architecture re-evaluation before Phase 5 (docs only).** Checked the live code against
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

**Phase 5 — next provider: `provider/go.rs`.** Same shape as cargo/pipx: `is_available`
(`go`), `search` — resolve a module path (query is likely `host/user/repo`, e.g.
`github.com/junegunn/fzf`); the Go module proxy (`https://proxy.golang.org/<mod>/@latest`)
gives the version. Only `main` packages are installable and the proxy can't cheaply tell,
so **no app-filter** (ADR-0023) — offer it, let `go install` be the authority.
`plan_install` = `go install <mod>@latest` (no root; installs to `~/go/bin` or `$GOBIN` —
PATH-warn), `list_installed` (Go has no clean global list — likely scan `~/go/bin`, or
accept an empty list like copr and rely on the registry hint), `plan_remove` (remove the
binary from the bin dir). Community trust. **At go, run the helper evaluation** (extract
`command_plan` if it shrinks code — see TASKS). Then `jii update` (wire `plan_update`).

Polish/hardening deferred (not blocking Phase 5; several are now **future features**, do
not implement as silent heuristics):
- **GitHub repository selection** — interactive, "never silently install the wrong repo".
- `.tar.xz` archives (needs an xz decoder dep); better COPR disambiguation; real
  GPG/sigstore verification in `exec.rs::verify_bytes` (currently fail-closed).
- **Engine UI-free seam** (ADR-0022): decouple `&Renderer` from `Engine::install/remove`
  — do this **before** any GUI/second frontend, not now.

Full list in [TASKS.md](TASKS.md) Phase 5.

## Current blockers

None.

## Build status

`cargo build` — clean, no warnings. `cargo clippy` — clean.

## Test status

`cargo test` — **91 passing, 0 failing**. Coverage: dnf/flatpak parsers, ranking,
registry, cache, privilege elevation prefixing, the executor (sha256 digest,
verification accept/reject/case-insensitive/fail-closed, place+mode+remove, tar.gz **and
zip** extract + member selection, unknown-format rejection, run_action), github
(owner/repo, release JSON, asset selection incl. `.zip`/tar.gz preference, checksums,
plan shapes), copr (search parsing, exact-name + fedora/arch chroot selection, two-step
root plan), cargo (binary-crate vs library-only candidate filtering, unprivileged plan
shape, `cargo install --list` parsing), npm (CLI vs library-only filter incl. bin-as-
string, user-prefixed plan shape, `npm ls -g --json` parsing), pipx (candidate shape,
install/upgrade plans, `pipx list --json` parsing), audit (verification resolution +
concern logic), and doctor health mapping (`health_from` precedence).

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
- **pipx/go offer libraries (ADR-0023, by design):** PyPI/Go expose no reliable
  program-vs-library signal, so `pipx`/`go` don't pre-filter (cargo/npm do). They offer
  the package; the tool rejects a non-app at install. Accepted — a visible false positive
  beats silently hiding a real app. Add a filter only if reliable metadata appears.
- **Engine↔UI seam (ADR-0022):** `Engine::install`/`remove` take `&crate::ui::Renderer`
  so the executor can print progress — the one `ui` type reaching into the engine. Fine
  now (single CLI frontend), but it must be decoupled (a progress-event/`ProgressSink`
  trait) **before** a GUI/second frontend or a workspace split. Meanwhile: **do not add
  new `ui` types to engine signatures.**

## Where things live

```
src/
  model.rs       core types (Action, InstallPlan, PackageCandidate, TrustLevel…)
  provider/      Provider trait + http_client + dnf, copr, flatpak, github, cargo, npm, pipx
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
