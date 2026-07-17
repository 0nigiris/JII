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

## Terminal 1.0 — the CLI completion plan 🎯 (ADR-0026)

**Goal:** finish the *whole* terminal version — "CLI 1.0" — before cutting the first public
Beta. This **promotes** the already-designed "Future ideas" below into an ordered delivery
plan; it is not a redesign (the hard rules already written for each idea stand). GUI/daemon/
Discover/plugins stay out (kept ready, not built). Ordered to minimise architectural risk:
cheap read-only honesty first, the biggest cross-distro push only after additive breadth has
exercised the model.

- **T1 — Read-only honesty layer:** `jii search`, `jii info`, `jii sources`. Pure rendering
  over the existing engine (`search`/`rank`) — zero new architecture. Closes the gap where the
  CLI advertised `search`/`info`/`config` that were stubs/absent.
- **T2 — Batch symmetry:** `jii update a b c`, `jii remove a b c` via the ADR-0025 machinery
  (optional `plan_update_many`/`plan_remove_many` + engine `update_batch`/`remove_batch`).
- **T3 — Provider breadth (proven shape):** Homebrew → Snap → AppImage (additive `Provider`s).
  Empirical check at Homebrew: does a shared `RegistryProvider` scaffold finally pay off?
- **T4 — Cross-distro system providers:** Apt, Pacman, Zypper, Nix behind the platform seam
  (`is_supported` becomes "≥1 native system provider available"; distro-aware `is_available`).
  Never breaks Fedora behaviour.
- **T5 — Interactive choosers:** GitHub **repository chooser** (paged select; engine ranks,
  CLI renders) and **version chooser** (provider surfaces + orders versions; engine stays
  version-agnostic).
- **T6 — Bootstrap a missing manager:** offer-then-install as a previewable plan step
  (`Provider::bootstrap_plan`), strongest consent, never auto, never launders trust.
- **T7 — Hardening:** CLI-level integration tests, registry-partial-failure test, error-message
  quality, clean-VM runs on Fedora/Arch/Ubuntu/Debian/openSUSE.
- **T8 — Public polish:** professional README, logo, screenshots/asciinema, architecture
  diagram, CONTRIBUTING/SECURITY, examples, limitations — then cut the first Beta.

**Done when:** `jii` is a *complete* universal Linux terminal installer — not "will become
one" — verified on five clean distros and presented as a polished public repo.

---

## Future 🌅

> Cross-distro managers, the repository/version choosers, and bootstrapping are **now scheduled
> under Terminal 1.0 (T3–T6)** above; the design notes for each live in "Future ideas" below.
> The bullets here are what remains genuinely post-1.0.

- SQLite migration (behind the same registry API).
- Cargo workspace split.
- Semantic / AI search (Stage 4).
- Cross-distro: apt, pacman, zypper, nix, AUR, snap.
- Windows (winget), macOS (Homebrew) — planned in three waves, **macOS first** (ADR-0068);
  gated on the external-tester round finding no criticals.
- **Landing page + demo** (a one-page site with an asciinema/GIF of `jii htop`) and
  **launch content** (a launch post for r/linux / Hacker News / Habr) — the pre-mortem
  said the biggest risk is "никто не узнал"; distribution work is scheduled here, after
  the tester round, not before.
- Plugin SDK.
- **GUI frontend** / universal software center — see "Future ideas" below.
- **Version management** (`jii versions`, `@version`, rollback) — post-alpha; "Future ideas".
- **GitHub repository selection** (disambiguate a bare name) — GitHub search polish; "Future ideas".
- **System onboarding** (`jii doctor --fix`) — see "Future ideas".
- **Experimental cross-distro** fallback (opt-in) — see "Future ideas".
- **More managers** (AppImage, Snap, Homebrew, Nix, Pacman/Apt/Zypper) — breadth via
  `Provider`s; see "Future ideas".
- **Bootstrapping a missing manager** (offer, never auto) — see "Future ideas".
- **Provider-supplied metadata** (icons/screenshots/…) for the GUI — see "Future ideas".
- **UPAC / external-library backends** via stable public API only — ADR-0021.

---

## Future ideas

Captured so they are not forgotten. **Not scheduled and not started.** Before acting
on any of these, revisit the engine's public API and record decisions in
[DECISIONS.md](DECISIONS.md).

> **The one principle behind all of these (ADR-0020):** JII is a *universal layer*
> over the sources that already exist — not another package manager, not a new package
> format, not a competitor to DNF/Flatpak/Homebrew. It unifies them under one honest
> interface (search · choose · trust · install · manage) and never asks users to change
> their habits. The test for every idea below is the same: **does it make the user's
> life easier without making the architecture heavier?** If not, it doesn't ship.

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

### System onboarding — `jii doctor --fix` / `jii setup` / `jii recommend`

**Priority:** Future / Phase 5+. **Status:** idea only.

**Vision:** help a fresh install become "ready" — enable Flathub and RPM Fusion, offer
multimedia codecs, GPU drivers, Steam, Wine, container tooling, and everyday utilities —
as an *opt-in* surface. This is JII widening from "a layer over software sources" toward
**a thin layer between the user and Linux**: it *analyses the system and recommends*, it
does not become a configuration manager. Likely command surfaces (same engine, different
entry): `jii doctor --fix` (fix what `doctor` reported), `jii setup` (first-run
onboarding), `jii recommend` (suggest, don't apply). Recommendations are **distro-aware**
(what's right for Fedora ≠ for another distro), routed through the `platform` abstraction.

**Philosophy (non-negotiable):** **Analyze → Explain → Ask → Apply.** `jii` **never
modifies the system automatically.** `doctor`/`recommend` (read-only surfaces) only
*report*; `--fix`/`setup` propose concrete, previewable steps (the same
`InstallPlan`/`Action` model, `--dry-run`-able, privileged steps batched and shown) and
apply them **only after explicit confirmation**.
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

### Additional package managers — breadth, not a bigger core

**Priority:** Phase 5 (user-space: Cargo, npm, pipx, Go) → Future (the rest). **Status:**
partially planned.

**Vision:** cover the sources users actually reach for, one `Provider` at a time:
Cargo, npm, pipx, Go (Phase 5, no root, install into `~/.local/bin`), then AppImage,
Snap, Homebrew (Linuxbrew), Nix, and — behind the platform abstraction — Pacman, Apt,
Zypper for other distros.

**Hard architectural rule:** **breadth is additive.** Each manager is a `Provider`
behind the one trait (native Rust, or later a declarative `data/sources/*.toml`), and
the core never branches on it (ADR-0004/0020). We add sources **because users need
them, not to inflate a count** — the guiding test still applies: does it help users
without making the architecture heavier? A manager that can't be expressed through the
existing trait + model is a signal to grow the *model* deliberately (with an ADR), not
to special-case the core.

### Bootstrapping a missing manager — offer, never auto-install

**Priority:** Future (pairs with the manager breadth above). **Status:** idea only.

**Problem:** an application may exist *only* through a manager the user doesn't have
installed (e.g. a formula available only via Homebrew).

**Vision:** detect this and *offer* to bootstrap the manager, then the app — explicitly:

> "This program is available only through Homebrew.
>  Install Homebrew and then install the application?"

**Philosophy (non-negotiable):** **never automatically.** Installing a whole package
manager is a bigger commitment than installing one app, so it demands the strongest
consent: the bootstrap is its own previewable `InstallPlan` step (`--dry-run`-able,
shown in full), gated on explicit confirmation, and the manager's own official install
method is used — no hidden `curl | sh` improvised by JII. If declined, JII simply
reports that the app is only reachable that way.

**Hard architectural rule:** "manager present?" is provider availability
(`is_available`); the bootstrap is *actions in a plan*, not engine special-casing.
Trust of a just-bootstrapped manager is the trust of that source — bootstrapping does
not launder an untrusted source into a trusted one.

### Provider-supplied metadata — for the GUI, fetched by providers only

**Priority:** Future (prerequisite for the GUI/software-center). **Status:** idea only.

**Vision:** richer listings — **icons, descriptions, screenshots, homepage, changelog**
— so a future GUI can render real application cards.

**Hard architectural rule:** **all metadata flows through the `Provider`/model**, the
same path candidates already take. A frontend (GUI included) **never fetches anything
itself** — it renders what the engine, via providers, produced. This keeps one code
path, one trust boundary, and one cache; it also means the CLI can expose the same data
(e.g. `--json`) for free. Metadata is *additive* on the model (optional fields a
provider may populate), not a new subsystem; providers that have no such data simply
omit it, with no core branching.

### UPAC / external-library backends — cooperate via a stable public API

**Priority:** Future, gated on an external milestone. **Status:** design-only (see
[DECISIONS.md](DECISIONS.md) ADR-0021).

**Context:** the author of **UPAC** has agreed to collaborate. The intent is *not* for
either project to absorb the other — each evolves independently. If `libupac` becomes a
public, stable library that solves some task better than JII's own code, JII can use it
as a **backend/provider** *there*.

**Hard architectural rule (ADR-0021):** integrate **only through UPAC's stable public
API**, never its internals; model it as just another `Provider`; and **implement
nothing until that API exists** — for now this is architecture on paper only. JII must
not depend on UPAC's internal types or unreleased behavior.
