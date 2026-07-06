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

**Recommendation:** ship Tier 1 in Terminal 1.0 (turns `doctor` from a status printer into a
helper using machinery we already have); keep Tier 2 as the ROADMAP `jii recommend` surface *unless
you want the catalog inside 1.0* — that materially enlarges 1.0 and reopens the "no new features"
line. **This is the one genuine fork that needs your call.**

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
or UI/config decision, written when its track lands (per ADR-0026's "own ADR when it lands").

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
