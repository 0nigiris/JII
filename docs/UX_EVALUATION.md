# Terminal 1.0 — UX Evaluation (pre-implementation)

**Status:** evaluation, not yet implemented. Written 2026-07-06 after the first real
dogfooding pass on a clean Fedora VM. Per the user directive: *read the code and every ADR
first, classify each problem as "already has architecture" vs "needs new design", and write
this evaluation before touching code.*

## Re-prioritisation

Priority is now **UX polish of the terminal experience**, not breadth. Explicitly out for now:
new providers, GUI, KDE Discover. The remaining T5 feature slice (**GitHub by-name repository
chooser**) is **deferred** — it is a new feature, not one of the reported UX problems. Its design
is already written (ADR-0030, status *Proposed/deferred*) and resumes after this UX pass.

The North Star: a first-time Linux user should feel JII is a *fast, quiet, honest assistant that
cooperates with the system's package managers* — never a loud second package manager that dictates.
Notably, `README.md` already advertises the target output (search ticks, "Recommended: DNF —
Official Fedora package", a reasons grid, alternatives). Several problems below are really
"implementation has drifted from the README's own promise".

---

## Classification

Legend: **[A]** already has the architecture — pure implementation/rendering, no ADR.
**[D]** needs new design — stop-and-design (own ADR, all in the *optional Provider capability*
shape of ADR-0022; **no core branch on the source**, ADR-0004).

| # | Problem | Verdict | Root cause / seam |
|---|---------|---------|-------------------|
| 1 | Unavailable-provider spam | **[A]** | `cli/mod.rs` prints `SearchResult.failed`; `engine::search_one` tags absent tools as `"unavailable"` alongside real errors. Split the two. |
| 2 | Search is slow | **[A]** | Search is *already* parallel (`join_all`). Cost is `is_available()` (a subprocess per provider) re-probed several times per command, uncached. |
| 3 | Detect installed *before* planning | **[A]** | `Provider::is_installed` / `Engine::resolve_installed` already exist; the install flow just never calls them first. |
| 4 | Never assume one manager; let the user choose | **[A]** | The candidate chooser (T5.1) already does this interactively; needs richer, always-honest presentation. |
| 5 | Explain *why* the recommendation | **[D]** | `recommendation_reasons` is deliberately source-agnostic (ADR-0004). "Official Fedora package / native / auto-updates" is source-specific prose → needs an **optional provider capability**. |
| 6 | Doctor should help, not just report | **[D]** | Actionable diagnostics + fixes. Partly T6 (bootstrap) and the ROADMAP `jii setup`/`recommend` onboarding surface. **Scope decision required.** |
| 7 | Errors must say what/why/how-to-fix | **[D]** | Errors are flat strings; no structured remedy. Add a remedy/hint layer. |
| 8 | Too much output; Friendly vs Advanced | **[D]** | No verbosity/mode concept. Needs a `ui.mode` + a rendering discipline (drives the wizard step 1). |
| 9 | Batch install UX | **[A]** | `preview_batch` prints "Summary" *and* the full plan (duplication); tighten. |
| 10 | `update` should update the whole system | **[D]** | Today `update` only touches jii's registry. "Update everything each manager owns" is a new **optional provider capability** (`plan_update_all`). |
| 11 | `remove` when a package exists in several sources | **[A]** | `resolve_installed` returns the *first* owner; add "find all owners" + reuse the chooser. |
| 12 | `search`/`info` should look like a real pkg manager | **[A]** | Pure rendering polish. |
| 13 | Better caching | **[A]** | Search cache exists; add availability caching (feeds #2). |
| 14 | Performance: measure, don't guess | **[A]** | Instrumentation, not architecture. |
| 15 | Don't behave like "I own the system" | **[A]** | Cross-cutting philosophy, realised by #4/#5/#10 + copy. |
| W | First-run wizard / `jii setup` | **[D]** | Needs config persistence (`Config::save`, `first_run_completed`, `ui.mode`), first-run detection, a `setup` command. Pre-thought in ROADMAP "System onboarding". |

**Net:** 10 of 16 items are pure polish over existing seams. Six need design — and *every one*
fits the established growth shape (optional `Provider` method or a UI/config layer), so **none
touches the core or the `Provider` trait's mandatory surface.** No architectural rule is challenged;
ADR-0004 (no core branch), ADR-0006 (trust barrier), ADR-0020 (universal layer, not a manager) and
ADR-0022 (grow via optional capabilities) all hold and in fact *pre-authorise* this work.

---

## Designs for the [D] items

### D5 — Source-supplied recommendation highlights (own ADR)

**Problem:** the README promises "Official Fedora package · native · automatic updates · signature",
but `recommendation_reasons` may only speak in source-agnostic model terms ("official source",
"version X") because ADR-0004 forbids the UI branching on `source_id`.

**Design:** an **optional** `Provider::highlights(&PackageCandidate) -> Vec<Highlight>` (default:
`[]`). The provider — which *is* allowed to know it is dnf — returns short, honest tags
("Official Fedora package", "Native package", "Automatic security updates", "Sandboxed"). The engine
concatenates model-derived facts (trust, signed, version, arch from `recommendation_reasons`) with
the provider's highlights and hands the merged list to the UI. The core still never inspects
`source_id`; only the provider that owns the source describes it. Pure ADR-0022 shape.

### D6 — Doctor that helps + `recommend` (own ADR; **scope decision**)

The ROADMAP already fixes the philosophy (**Analyze → Explain → Ask → Apply**, never auto-modify;
read-only surfaces only report, `--fix`/`setup` propose a previewable `InstallPlan`). Two tiers:

- **Tier 1 (fits Terminal 1.0): actionable diagnostics about *JII itself working*.** Reuses existing
  seams: is `~/.local/bin` on `PATH`? (github/cargo installs land there — high-value, currently
  silent); is a missing manager offerable? (this *is* T6 `Provider::bootstrap_plan`); is
  `GITHUB_TOKEN` set (rate-limit)? Each becomes a diagnostic with an optional, previewable fix-plan.
- **Tier 2 (ROADMAP "System onboarding", currently Phase 5+): curated recommendations** — codecs,
  GPU drivers, fonts, RPM Fusion, Steam/Wine, battery. This is a **data-driven, distro-aware,
  auditable catalog** — a real content subsystem, not polish, and explicitly deferred in ROADMAP.

**Decision (user, 2026-07-06): Tier 1 + Tier 2 catalog, both in Terminal 1.0.** So `doctor`
becomes a real system helper, and the curated **recommend-catalog** ships in 1.0. Consequences to
honour:
- The catalog is its own **data-driven, distro-aware, auditable subsystem** (own ADR) — a TOML/data
  catalog routed through the `platform` seam, **not** hardcoded `if fedora`/`if nvidia` branching.
  It stays out of the core and off the `Provider` trait's mandatory surface.
- **Analyze → Explain → Ask → Apply is non-negotiable** (ROADMAP): `doctor`/`recommend` only report;
  every fix is a previewable `InstallPlan`/`Action` (`--dry-run`-able, privileged steps batched and
  shown), applied **only** after explicit confirmation. No `curl|sh`, no silent repo edits. Enabling
  a third-party repo (RPM Fusion) crosses a trust boundary and must surface the same trust story.
- This is the **largest** track in the pass (a content subsystem, not polish); it is sequenced last
  (part of U6) so the cheap, universally-wanted polish (U1–U3) lands first.

### D7 — Actionable errors (own ADR, small)

Give `JiiError` an optional **remedy**: what happened, why, and the exact next command. E.g. GitHub
rate limit → "set `GITHUB_TOKEN` (see …)"; a source's tool missing → "install it? (offer bootstrap)".
Implemented as a pure `remedy(&JiiError) -> Option<Remedy>` mapper (unit-testable, no I/O), rendered
by the UI. Dovetails with D6 Tier 1 and the T7 "error-message quality" task.

### D8 — Output verbosity / Friendly vs Advanced (own ADR)

Add `ui.mode = friendly | advanced` (default friendly) to config, and a `Verbosity` the `Renderer`
carries. Friendly: minimal, human lines, no per-package "Searching…", no duplicated Summary+plan,
reasons summarised. Advanced: today's detail + source rationale + diagnostics. `-v` and `--json`
still override. This is the lever for #1/#8/#9/#12 and the wizard's step 1 — so it lands early.

### D10 — System-wide update (own ADR)

An **optional** `Provider::plan_update_all() -> Option<InstallPlan>` (default `None`): "upgrade
everything this manager owns" (`dnf upgrade`, `flatpak update`, `cargo install-update`…). Bare
`jii update` aggregates every provider that offers one into the usual batched, previewable,
single-confirmation, single-escalation run; named `jii update <pkg>` keeps today's registry path.
Providers that can't (github) simply return `None`. Pure ADR-0022/0025 shape — the engine aggregates,
never branches on the source. This is the largest capability item and the heart of problem #15
("the universal update command").

### DW — First-run wizard + `jii setup` (own ADR)

Needs a **config write path** (JII currently only *reads* config). Add `Config::save`, a
`meta.first_run_completed` flag, and `ui.mode`. On a bare `jii` with the flag unset and an
interactive TTY: offer the 30-second wizard (mode choice → offer `doctor` Tier 1 → done), then
persist the flag. `jii setup` re-runs it anytime. Non-interactive/`--json`/piped never triggers it.
Aligns with the ROADMAP `jii setup` onboarding entry.

---

## Proposed delivery order (UX tracks)

Ordered by impact-per-risk; each keeps build/clippy/tests green and is independently shippable.

- **U0 — Measure.** Baseline startup, search latency, per-provider `is_available`/search cost
  (instrument + one run). Informs #2/#14 so optimisation is evidence-led (ADR-0010 discipline).
- **U1 — Silence & clean output.** #1 (stop unavailable spam), #9 & #12 (de-duplicate/tidy
  previews and search/info), first cut of #8. Highest impact, zero new architecture.
- **U2 — Speed.** Availability memoisation (#2/#13) guided by U0; cancel/short-circuit where cheap.
- **U3 — Cooperate, don't clobber.** #3 (already-installed pre-check) and #11 (multi-owner remove
  via the existing chooser). Reuses existing capabilities.
- **U4 — "You decide, with reasons."** #4 chooser presentation + **D5** source highlights (ADR).
  The philosophy front (#15).
- **U5 — Friendly by default.** **D8** verbosity/mode (ADR) + **DW** first-run wizard/`setup` (ADR).
- **U6 — Helpful failure & doctor.** **D7** actionable errors (ADR) + **D6 Tier 1** doctor-that-helps
  (ADR; Tier 2 only if you choose it).
- **U7 — The universal update.** **D10** `plan_update_all` (ADR) — `jii update` updates the system.
- **U8 — First-run walkthrough polish.** Play through the whole CLI as a new user; fix every
  awkward edge (#15).

New ADRs expected: D5, D6, D7, D8, D10, DW (six) — each a small, pre-authorised optional-capability
or UI/config decision, written when its track lands (per ADR-0026's "own ADR when it lands"). The
doctor **recommend-catalog** (D6 Tier 2, now in scope) also gets its own catalog ADR.

---

## U0 measurements (release build, clean Fedora VM, 2026-07-06)

- **Startup** (`jii --help`): **~0.00 s**. Not a problem — no optimisation warranted.
- **`jii sources`** (14 sequential `is_available` probes): **0.21 s**. Acceptable; availability
  memoisation (U2) trims it and, more importantly, avoids re-probing within a single command.
- **Cold `jii search git`**: **8.05 s** — the real latency problem, and the diagnosis overturns a
  guess: search is **already parallel** (`join_all`); the wall-clock is the **8 s network timeout**
  one straggler (`copr`) burns while dnf/flatpak answered in milliseconds. `join_all` waits for the
  slowest. **Levers (U2):** lower the default `network.timeout_secs` (8 → ~4); surface partial
  results as fast providers finish (or a shorter per-source budget for network sources); availability
  caching is a minor gain here, the timeout is dominant. This is evidence-led per ADR-0010.

**Two real (non-UX) findings surfaced by the noise-removal, logged as debt:**
- `copr: timeout` on a common query — copr's search/probe is slow enough to hit the timeout; needs a
  faster probe or a tighter per-source budget (feeds U2).
- `cargo: malformed json: error decoding response body` — the cargo provider fails to decode the
  crates.io response for `git`. A likely **provider correctness bug** (API shape/rate-limit/HTML
  error body), not mere UX; investigate separately during U2/U6.

## Progress

- **U0 — measured** (above).
- **U1 — in progress:**
  - killed the unavailable-provider spam. `engine::search_one` now treats "tool not installed" as the
    normal, silent state (a stale cache entry still counts); only genuine errors/timeouts land in
    `SearchResult.failed`. `jii sources`/`doctor` still report availability. Verified: `jii search git`
    7 noise lines → 0.
  - de-duplicated the single-package install preview (#9): the grouped "Summary:" block now prints
    only for a real batch (>1 plan or >1 package); a single install goes straight to its Plan, which
    already carries source + version + reasons. Verified on `jii fastfetch --dry-run`.
  - *still shows a real `copr: timeout` on every command here (copr's API is ~9 s > the 5 s budget).
    That is a genuine, non-silent failure; suppressing/summarising non-fatal secondary-source failures
    belongs to D8 friendly mode, not to unconditional hiding.*
- **U2 — in progress:** confirmed by direct measurement that COPR's search API takes **~9 s** (http
  200) on this box, so at the old 8 s budget it always timed out yet still cost the full wait. Lowered
  the default `network.timeout_secs` **8 → 5**; cold `jii search git` dropped **8.05 s → 5.08 s** with
  no loss of candidates (only copr, which was already lost, is skipped sooner). Availability
  memoisation and background-straggler caching (to recover slow copr without the wait) remain as
  follow-ups. clippy clean, 150 tests green throughout.
- **U3 — in progress:** install now checks *before planning* whether the package is already present
  (#3). `jii git` → "✓ git is already installed via dnf (2.55.0-1.fc44). Nothing to do." When the same
  owning source offers a newer version it offers an in-place update ("… Available: X. Update now?");
  confirming is the consent, so a trusted update skips the redundant batch confirm. Cross-source
  presence reads as "already installed" (versions are opaque across sources, ADR-0009). To keep the
  hot install path fast, the check is a new **targeted** `Engine::installed_lookup` (registry hint,
  else *one* lookup in the recommended source only) — not the full multi-provider `resolve_installed`
  fan-out (which measured ~1 s extra per fresh install and stays reserved for remove/update). Still
  to do in U3: multi-owner `remove` via the chooser (#11). *Follow-up: a cheap single-package
  `Provider::installed_version` would remove even the one `list_installed` the check still costs.*

---

## Appendix — NixOS support (architectural opinion only; nothing to build)

**1. Should JII support NixOS at all?** Yes, but honestly-scoped. A `nix` provider already exists
(T4) and works on *any* host with the Nix package manager (Fedora/Ubuntu/etc. with Nix installed).
**NixOS-the-OS is a different question** from **Nix-the-package-manager**, and conflating them is the
trap.

**2. Which path is right — imperative vs declarative?** For JII's model, **imperative
`nix profile install` is the only honest fit**, and that is fine. JII's entire contract is
*"search → plan a concrete previewable set of `Action`s → apply after consent"* (ADR-0003/0007).
`nix profile install/remove/upgrade` maps cleanly onto that: it is a command, previewable, reversible,
user-scoped, no root. Declarative Nix (`configuration.nix`, flakes, Home Manager) is a **fundamentally
different paradigm**: the unit of change is *editing a source-of-truth file and rebuilding the world*,
not "install this one thing now". That collides with JII in three ways:
- It is *stateful config editing*, which the ROADMAP philosophy explicitly refuses ("no silent repo
  edits", "never becomes a configuration manager"). Rewriting a user's `configuration.nix` is exactly
  the "I own your system" behaviour problem #15 warns against.
- There is no single "the config" — it may be `/etc/nixos/`, a flake anywhere, Home Manager, or all
  three. Discovering and safely editing it is a research project, not a provider.
- The result isn't a discrete `Action`; it's "regenerate a generation", which doesn't fit
  `Download/Place/Extract/RunCommand`.

**3. Does it break the Provider architecture?** No — *if kept imperative*. The existing `nix` provider
is proof: it is a plain `Provider` behind the platform seam, self-gating on the `nix` binary, no core
branch (ADR-0004/0029). Declarative support **would** break it — it needs file-editing Actions,
whole-system rebuild semantics, and "what is my config" discovery that have no home in the trait.

**4. Can it be "just another Provider" with no `if nix` in the engine?** For imperative: **yes,
already true.** The only NixOS-*specific* nuance is that on NixOS the imperative `nix profile` path,
while functional, is culturally discouraged — but that is a *documentation/trust-note* concern, not an
engine branch. If we ever wanted to *warn* "you're on NixOS; the declarative route is more idiomatic",
that is a one-line provider-supplied note (the D5 highlights mechanism), not core logic.

**5. Is imperative-only "honest enough", or should we say Nix is unsupported?** **Imperative-only is
honest and worth shipping — provided we label it precisely.** Claim exactly what is true: *"JII installs
into your Nix user profile (`nix profile`). It does not manage `configuration.nix`, flakes, or Home
Manager — your declarative setup is never touched."* That is more honest than silence and safer than
pretending to be a Nix tool. Do **not** claim generic "NixOS support"; a NixOS purist rightly expects
declarative, and overclaiming erodes the trust JII is built on (ADR-0020).

**Half-a-year-out recommendation:** keep the imperative `nix` provider as the *supported* answer for
"install this tool on a machine that has Nix"; document the declarative boundary loudly; treat any
declarative integration as a separate, opt-in, research-grade surface under the ROADMAP "System
onboarding / cross-distro" umbrella — **never** as silent edits to a user's Nix configuration. The
architecture already accommodates the honest 90%; the remaining 10% is a paradigm JII deliberately
does not own.
