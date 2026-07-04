# JII — Roadmap

Incremental delivery. Each phase produces something **compilable and runnable**.
The architecture ([ARCHITECTURE.md](ARCHITECTURE.md)) does not change between phases —
each phase fills in more of the same contracts.

Legend: 🎯 MVP · 🔭 post-MVP · 🌅 future

---

## Phase 0 — Skeleton 🎯

**Goal:** a single crate that compiles, parses `jii <name>`, and prints a stub.

- Single Cargo crate, modular layout (see ARCHITECTURE §4).
- `platform.rs`: detect Fedora, arch, PATH, TTY/graphical session.
- `config.rs`: load/merge/validate TOML with defaults.
- `error.rs`, `model.rs`: core types.
- `cli/`: clap wiring, global flags.
- `ui/`: renderer facade honoring `--json` / `--no-color`.

**Done when:** `jii fastfetch` runs, loads config, prints a placeholder plan.

---

## Phase 1 — DNF end-to-end 🎯

**Goal:** actually install a real package via one source.

- `Provider` trait finalized.
- `provider/dnf.rs`: `search` / `plan_install` / `list_installed` using **dnf5**
  machine output.
- `privilege.rs`: batched `sudo`/`pkexec`, exact-command display.
- `engine/`: `search → rank → plan → execute` (single provider, full model).
- `ui/prompt.rs`: `[Y/n]` with default, trust barrier.
- `--dry-run` shows the plan and exits.

**Done when:** `jii <dnf-package>` installs it; `--dry-run` previews the plan.

---

## Phase 2 — State, remove, why 🎯

**Goal:** JII remembers and can reverse.

- `registry.rs`: JSON state store (intentions) + verification against dnf.
- `jii remove` (registry → verify → plan → remove).
- `jii list`, `jii why`, `jii history`.
- Write registry **only on success**.

**Done when:** install → `list`/`why` reflect it → `remove` uses the right source.

---

## Phase 3 — Multiple sources & ranking 🎯 ✅

**Goal:** real choice between sources; tie-breakers matter.

- `provider/flatpak.rs` (installs via Flatpak's own polkit — no JII root).
- `engine/ranking.rs`: source priority + trust tie-breaker + profiles, with an
  "also available" explanation in the CLI.
- Parallel fan-out with per-source timeouts + graceful degradation.
- `cache.rs`: TTL cache, stale-on-error.
- `jii doctor` (availability, latency, health).

**COPR moved to Phase 4:** `dnf5 copr` has no search, so finding which COPR provides
a package needs the COPR web API — the same fuzzy name→project resolution problem as
GitHub Releases, plus root repo-enable and trust handling. Best done alongside GitHub.

**Reserved:** `latest`/`minimal` profiles and freshness/health ranking tie-breakers
need comparable versions / dependency-footprint data we do not collect yet.

**Done:** a package in DNF+Flatpak is ranked with a clear recommendation + alternatives.

---

## Phase 4 — GitHub Releases, COPR & trust 🎯

**Goal:** the hard, security-sensitive sources that share a name→source resolution
problem.

- `provider/github.rs`: name→repo resolution, arch/libc asset filtering,
  checksum/signature verification, `GITHUB_TOKEN` support.
- `provider/copr.rs`: COPR web-API project search, root repo-enable, trust handling.
- Trust levels enforced end-to-end; `untrusted` always confirmed even in `--auto`.
- `jii audit` (signatures, sha256, GPG, sigstore, source, trust).
- Rate-limit health in `doctor` (GitHub).

**Done when:** installing a GitHub release verifies the artifact and respects trust.

---

## Phase 5 — User-space sources & update 🔭

- `provider/cargo.rs`, `npm.rs`, `pipx.rs`, `go.rs` (no root; `~/.local/bin` PATH check).
- `jii update [<name>]` across all managers.
- `jii undo`, `jii benchmark`.

---

## Phase 6 — Declarative sources & catalog 🔭

- `provider/declarative.rs` + `data/sources/*.toml`.
- `data/catalog.toml`: name aliases (`vscode → code`, `node → nodejs`).
- Light full-text search over package metadata (Stage 3).
- Fuzzy name search (Stage 2).

---

## Phase 7 — Hardening 🔭

- Full test matrix (unit ranking/parsers on fixed samples; integration dry-run).
- Docs polish, `--json` stability, error-message quality pass.
- Distribution: COPR repo + signed GitHub binary.

---

## Future 🌅

- SQLite migration (behind the same registry API).
- Cargo workspace split.
- Semantic / AI search (Stage 4).
- Cross-distro: apt, pacman, zypper, nix, AUR, snap.
- Windows (winget), macOS (Homebrew).
- Plugin SDK.
- **GUI frontend** / universal software center — see "Future ideas" below.
- **Version management** (`jii versions`, `@version`, rollback) — post-alpha; "Future ideas".
- **GitHub repository selection** (disambiguate a bare name) — GitHub search polish; "Future ideas".
- **System onboarding** (`jii doctor --fix`) — see "Future ideas".
- **Experimental cross-distro** fallback (opt-in) — see "Future ideas".

---

## Future ideas

Captured so they are not forgotten. **Not scheduled and not started.** Before acting
on any of these, revisit the engine's public API and record decisions in
[DECISIONS.md](DECISIONS.md).

### GUI frontend — a cross-provider "Discover"

**Vision:** a Discover-like desktop application that is *not* limited to a single
ecosystem. It transparently searches, compares, and installs across every enabled
provider (DNF, Flatpak, GitHub, COPR, Cargo, npm…), showing the same recommendation
and "why" the CLI gives.

**Non-goal:** the GUI does **not** replace the CLI, and it is **not** a second
implementation. It is *another frontend over the same engine*.

```
CLI ─┐
     ├── Core Engine  (search · rank · plan · trust · execute · registry)
GUI ─┘
```

**Hard architectural rule:** the GUI is a **thin frontend**. It reuses the exact
search, ranking, planning, trust model, and execution logic of the engine and
**never duplicates business logic**. Any behavior it needs must live in the engine
and be shared with the CLI — if the GUI wants something the engine can't express, the
engine grows, not the GUI. (See [DECISIONS.md](DECISIONS.md) ADR-0015.)

**Potential features** (all backed by existing or extended engine capabilities):

- Universal Linux software catalog; search across every enabled provider.
- Rich listings: application icons, screenshots, descriptions, version info.
- Source comparison side-by-side, with the engine's **"why this source?"** rationale.
- Trust indicators (official / community / untrusted) and signature/verification status.
- Dry-run preview of the plan before anything runs.
- Update management, installed applications, history, and audit — the same commands
  the CLI exposes, rendered visually.

**Implications to weigh when it is time (not now):**

- Metadata the CLI doesn't need yet — icons, screenshots, long descriptions — must be
  produced by providers through the model, not fetched ad hoc in the GUI.
- The engine's API must be callable as a library (it already operates purely on the
  model); a GUI likely links the crate directly or talks to a thin local service.
- Long-running/streamed operations (download progress) may need the engine to surface
  progress events without the GUI reaching into execution internals.

**Framing:** think of it as a *universal Linux software center* — the one place a user
installs anything, regardless of where it comes from — rather than "a GUI for jii".
That framing only sharpens the hard rule: the more the software center appears to do,
the more disciplined we must be that every capability is the engine's, exposed once
and shared with the CLI. It **never** reimplements search, ranking, trust, or planning.

### Version management — pin, list, roll back

**Priority:** post-alpha. **Status:** idea only.

**Vision:** let users see and choose versions, not just "the latest":

- `jii versions <package>` — list the versions a source can install.
- `jii install <package>@1.2.3` — install a specific version.
- Roll back to a previously installed version.

**Hard architectural rule:** versions are a **provider capability surfaced through the
model**, and the engine stays version-agnostic. A provider that can enumerate/pin
versions reports them on the candidate; the core never learns "how github tags releases"
or "how dnf lists versions" — it selects among `PkgVersion`s the provider offered. Not
every source can do this (a raw github binary may expose only `latest`); the model must
let a provider say "versions unknown" without the engine special-casing it.

**Implications to weigh when it is time:**

- `PkgVersion(String)` is deliberately not semver (ADR-0009); comparing/ordering
  versions for "roll back" or "is X newer" needs a source-provided ordering, not a
  jii-invented one.
- Rollback needs the registry to retain enough history to reinstall a prior version —
  today it records the current install, not an installable coordinate for old ones.

### GitHub repository selection — never silently install the wrong repo

**Priority:** Phase 5+ / GitHub search polish. **Status:** idea only.

**Problem:** a bare name (`jii install bat`) can match many GitHub repositories; picking
one silently risks installing the wrong — or a malicious look-alike — project. Today the
github provider only accepts explicit `owner/repo`.

**Vision:** when a name is ambiguous, present the best few candidate repositories and let
the user choose:

- Show ~5 best repos with **stars**, **owner**, an **official/verified** indicator where
  known, and a short **description**.
- Actions: **select**, **next page**, **cancel**, or **refine** the query.
- **Never silently install the wrong repository** — ambiguity is resolved by the user,
  visibly, or not at all.

**Hard architectural rule:** the *ranking/heuristics* (what "best" means, star weighting,
official detection) live in the engine, reusing the trust model; the CLI/GUI only render
the choices and collect the selection. This mirrors how COPR disambiguation is handled
(exact-name + visible `owner/project` + confirmation, ADR-0017) — extend that pattern,
don't fork it.

### System onboarding — `jii doctor --fix`

**Priority:** Future / Phase 5+. **Status:** idea only.

**Vision:** help a fresh Fedora install become "ready" — enable Flathub and RPM Fusion,
offer common codecs, GPU drivers, Steam, and everyday utilities — as an *opt-in*
extension of `jii doctor`.

**Philosophy (non-negotiable):** **Analyze → Explain → Ask → Apply.** `jii` **never
modifies the system automatically.** `doctor` (no flag) only *reports*; `--fix` proposes
concrete, previewable steps (the same `InstallPlan`/`Action` model, `--dry-run`-able,
privileged steps batched and shown) and applies them **only after explicit confirmation**.
Each fix is a plan, not a side effect — reusing execution and privilege exactly as
installs do (ADR-0003/0005/0007). No hidden `curl | sh`, no silent repo edits.

**Implications to weigh when it is time:**

- "What a healthy system looks like" is a policy/catalog that must be data-driven and
  auditable, not hardcoded branching — and Fedora-specific until the platform layer
  generalizes it.
- Enabling third-party repos (RPM Fusion) crosses a trust boundary; it must surface the
  same trust/confirmation story as installing from them.

### Experimental cross-distribution compatibility

**Priority:** very long-term (aligns with the "Cross-distro" Future bullet). **Status:**
idea only — explicitly experimental.

**Vision:** where a distro has **no native method** for something, *optionally* offer a
best-effort cross-distro path. This is a fallback, never a default.

**Guardrails (all required):**

- **Only when no native method exists** — native/first-party always wins.
- **Disabled by default** and clearly marked **Experimental** wherever it appears.
- **Never automatic** — requires **explicit user confirmation** every time.
- **Integrate with an existing, dedicated project** (e.g. an established compatibility
  layer) rather than reimplementing one inside jii.

**Hard architectural rule:** this is *another `Provider`* (or a wrapper over an external
tool), behind the same trait, trust model, and plan/confirmation flow — the core still
never branches on the source. An experimental source simply carries a lower trust and an
"experimental" marker in the model; it does not earn special cases in the engine.
