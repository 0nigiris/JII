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
  fan-out (which measured ~1 s extra per fresh install and stays reserved for remove/update).
  *Follow-up: a cheap single-package `Provider::installed_version` would remove even the one
  `list_installed` the check still costs.*
  - **done (#11):** `remove` now finds a package across **all** sources (`Engine::resolve_all_installed`,
    a deliberate full fan-out — remove is not the hot path). When several sources own it, an
    interactive session picks which — or "all of them" — via the existing chooser; `--source` narrows
    to one; non-interactive takes every owner (the removal preview + confirm still gate it). Since the
    resolver sees the whole system now (not just jii's registry), the miss message is the accurate
    "Not installed:" (was "Not installed via jii"). **U3 complete.**
- **U4 — in progress (ADR-0031 + #4 + D5):**
  - **`PackageSpec::parse`** (pure, isolated in `model.rs`, 11 tests): `name[:source][@ref]`, split on
    the last non-leading `@` (npm scope-safe) and the last `:`.
  - **install wiring:** `:source` pins the provider per-package and suppresses the chooser (one clause
    on `offer_choice`); `@ref` is parsed but explicitly rejected ("coming with the version chooser");
    an unknown pinned source fails fast with the known list (did-you-mean); an explicit source with no
    match errors honestly (no silent substitution). clap untouched; backwards compatible.
  - **D5 source highlights** (ADR-0022 optional `Provider::highlights`, default empty): dnf/copr/
    flatpak/github/cargo return short honest source-specific reasons; `Engine::candidate_highlights`
    exposes them; `jii info` now reads like the README ("✓ Official Fedora package · Native · …"),
    the UI still never branching on the source id.
  - **chooser presentation (#4):** clearer header + a "⭐ recommended" tag on the top option.
  - **spec universal across all verbs (ADR-0031 tail done):** `remove`/`update`/`info` now parse the
    spec too (via the same `parse_specs`). `jii remove firefox:flatpak` pins the copy and *is* the
    non-interactive answer to the multi-owner chooser; `update node:brew` selects the copy to update;
    `info firefox:flatpak` narrows the report (`ranked_for` gained an explicit `source` override).
    `@ref` rejected everywhere; `search` stays free-text (a query, not a spec). Backwards compatible.
  - 162 tests, clippy clean; verified on Fedora (info highlights + narrowing, pty chooser, remove/
    update/info spec paths, @ref rejection, did-you-mean). **U4 complete.**
- **U5 — done (D8 + DW):**
  - **Friendly/Advanced modes (D8):** `config::OutputMode { Friendly (default), Advanced }` in
    `[ui] mode`; `Renderer::is_friendly()`; `-v`/`--verbose` forces Advanced for one run without
    editing the config. Friendly **hides secondary-source failure noise** (`report_source_failures`
    is a no-op in Friendly — no `copr: timeout` on a normal search) and **collapses the install
    preview** to one line per package (`Install <name> (<ver>) via <source> — <why>  [needs sudo]`);
    `--dry-run` and Advanced keep the full Plan block.
  - **First-run wizard + `jii setup` (DW):** `Config::save()` + `MetaConfig::first_run_completed` +
    `is_first_run()`; a bare `jii` in an interactive first-run session offers a 30-second setup
    (welcome → mode chooser → optional doctor → save), declining still marks it done; `jii setup`
    re-runs it on demand; non-interactive/`--json`/piped never triggers it.
  - **clap parse fix (found while testing):** a global flag *before* a subcommand (`jii -v search git`,
    `jii --json search git`) used to misparse as `install` — `args_conflicts_with_subcommands` removed,
    full parse matrix re-verified.
  - 165 tests, clippy clean; wizard + Friendly paths pty-verified in an isolated `XDG_CONFIG_HOME`.
    **U5 complete.**

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

---

# Second UX pass — architectural notes (evaluation only, 2026-07-06)

Four follow-up questions from the user after reviewing U0–U3. Architectural opinion only; nothing
implemented here.

## A. Progressive results (show fast sources first, stream in the slow ones)

**The user is right that U2 treated the symptom.** The real cause is the barrier at
`engine::search`: `futures::future::join_all(...)` waits for *every* provider before returning one
`SearchResult`; the CLI then ranks and renders once. So the slowest source gates the fast ones even
though search is already parallel. Lowering the timeout only shrinks the worst case.

**Does progressive fit, or is it a redesign? It fits — as an additive change to two seams, with the
core model untouched.** What changes:
- `engine::search` swaps `join_all` for a **`FuturesUnordered`** (or an mpsc channel / a stream
  return), emitting `(provider_id, result)` as each completes instead of collecting.
- the text renderer prints **append-only** in arrival order — exactly the user's mock (a first block
  for the fast sources + a live "Recommended", then an "Additional sources" block as stragglers land).
  Append-only means no ANSI cursor rewriting, so it still behaves under a pipe.

The `Provider` trait, `PackageCandidate`, `ranking`, and the cache are **all unchanged**. This is a
refinement of *how results are consumed*, squarely within ADR-0010 (parallel search already
decided); it warrants its own small ADR for the streaming API + the rule below, not a redesign.

**Why it's safe here specifically — priority correlates with speed.** The recommendation is the
highest-*priority* source that matched (dnf > … > github). On this system the high-priority sources
are **local and fast** (dnf, flatpak) and the slow ones are **network and lower-priority** (copr,
github, cargo). So the early recommendation from the fast set is almost always the final one; a
straggler arrives *below* it and only fills the "Additional sources" list. The one pathological case
— a slow source ranked *above* the current best (e.g. dnf disabled, slow copr outranks fast flatpak)
— is handled by a cheap, well-defined rule: **hold the final "Recommended:" line until every source
ranked above the current best has reported** (those are few, and usually the fast ones anyway).

**Bonus: streaming subsumes U2's trade-off.** Because slow sources no longer gate the felt
experience, we can *raise the timeout back up* (let copr take its ~9 s in the background and fill in
late) without the search ever *feeling* slow — best of both. So progressive is the better home for
"fast search", and I'd re-open the U2 timeout as part of it.

**Costs to name honestly:** `--json` must stay one coherent document, so JSON **buffers** (collects)
while text streams — trivial to gate. The install path is more than rendering: to let the user act on
the fast result immediately you either (a) still collect the full set before the chooser/plan (so
only the *perceived* wait improves — safe, easy), or (b) go fully progressive on install
(act-then-background) — more complex, more edge cases. I'd ship (a) first: stream the "✓ DNF /
Recommended" feedback, still gather all candidates before the chooser/confirm.

**Verdict: worth doing, as its own track (fold into an expanded U2). Additive, ADR-0010-aligned, no
core redesign.**

## B. Single-dash long flags (`-source` instead of `--source`)

**clap (v4, derive) facts:** clap models exactly two flag shapes — `short` (`-s`, a single char) and
`long` (`--source`, always double-dash). There is **no** per-arg "single-dash long" (Go/`flag`-style
`-source`). Single-dash multi-char also collides conceptually with clap's short **bundling** (`-yn`
= `-y -n`), which is why clap doesn't offer it.

**Could we support it anyway?** Only by **normalising argv before clap sees it** — a shim that
rewrites a leading-single-dash token matching a known long name (`-source` → `--source`) while
leaving real shorts (`-y`, `-n`, `-v`, `-`, bundles) and value-like `-…` alone. It's doable but:
- clap's **help, error messages, and shell completions stay `--source`** (clap only knows the long
  form), so the single-dash form would be an undocumented, second-class alias — inconsistent, and a
  new maintenance/edge-case surface (values that look like flags, bundles, `--` passthrough).
- it trains users a habit that breaks the moment they use `--help`, tab-completion, or any other
  Linux tool.

**Recommendation: keep `--source` canonical; do *not* adopt single-dash longs.** Better ways to kill
the friction the user actually feels:
1. **Short aliases** for the common flags — `-s`/`--source`, `-p`/`--profile` — trivial, idiomatic,
   clap-native, and shown in help.
2. **Make the flag rarely needed at all.** The interactive chooser (U4/T5.1) already means you *pick*
   the source from a menu instead of typing `--source`. That is the cooperative-assistant answer
   (§C): the fix is *needing the flag less*, not making the flag shorter. Typing `--source flatpak`
   should be the rare power-user path, not the everyday one.

If you still want the single-dash form after this, the argv-shim is the only route and I'd scope it
tightly (whitelist of known longs) and accept the help/completion inconsistency — but I recommend
against it.

## C. The cooperation lens (JII must never feel like "its own package manager")

Agreed, and it is already the load-bearing principle (**ADR-0020: universal layer, not a manager**).
Proposal: make it a **standing review question** applied to every command — *"does this cooperate
with the system, or behave like it owns it?"* Current **ownership smells** and how the pass detoxes
each:
- "Not installed **via jii**" wording — implies jii only knows its own world. *Fixed in U3* (whole-
  system resolver → "Not installed:").
- `remove`/`update`/`list` historically saw only jii's registry. *`remove` now sees all sources
  (U3 #11); `update` for the whole system is U7 (#10 `plan_update_all`); `list` should grow toward
  "what's on this system", not just "what jii installed".*
- Re-installing something already present — *fixed in U3* (already-installed pre-check).
- The recommendation stated as a verdict without alternatives/reasons — *U4 chooser + D5 highlights*
  turn it into "here's what I'd pick and why; you decide".
- `doctor` reporting jii's own source health rather than helping the system — *U6 Tier 1/Tier 2*.

I'll carry this lens explicitly through U4–U8 and note, per command, which side of the line it lands
on.

## D. Friction inventory (a first-time-user walkthrough)

Pretending never to have seen JII, going command by command over the *current* code. Each item is a
small friction + the proposed simplification (no code yet); tags map to the tracks above.

- **Every source-touching command prints `Searching for 'x'...`** — fine for one slow search, noise
  when repeated in a batch or when results are instant. → progressive/append-only feedback (A); drop
  the line entirely once the fast block prints. *(A, D8)*
- **`⚠ ✗ copr: timeout` on essentially every command** — two problems: the marker is a doubled
  `⚠ ✗` (`warn()` prepends `⚠` to a string already starting with `✗`), and a transient *secondary*
  source failure is shown even when it didn't affect the result. → single marker; in friendly mode
  **summarise or suppress** non-fatal secondary-source failures ("1 source timed out"), show detail
  only in advanced/`-v` or `doctor`. *(U1 polish + D8)*
- **`jii sources` prints "More sources arrive in upcoming releases — see docs/ROADMAP.md." every
  time** — advertising as recurring noise. → drop it (or show only in the wizard/`--help`). *(U1)*
- **`search`/`info` rows** (`source  vX  trust — summary`, a leading `→`) — functional but hard to
  scan. → align columns, a `⭐`/`recommended` tag on the top row, group by trust, humanise. *(U4/#12)*
- **`info` "Recommended" reasons are thin** ("official source"). → D5 source highlights ("Official
  Fedora package · native · auto-updates"). *(U4/D5)*
- **Single-candidate trusted install still asks `Install? [Y/n]`** — for a "just install it" tool a
  one-keystroke default-yes is acceptable, but consider a tighter one-line confirm (name + source +
  version on one line) rather than the multi-line Plan for the friendly path. *(D8)*
- **The full `Plan:` block** (`privileges: root required`, `actions: # dnf5 install -y x`) shows on
  every install/dry-run — advanced-grade detail. → friendly shows a one-line "will install X (vY)
  via dnf [needs sudo]"; advanced/`--dry-run` shows the full plan. *(D8)*
- **`update` version transitions** printed as separate lines then the plan — could be one compact
  "X: a → b" block. *(D8/#9)*
- **`list`/`history`/`audit` columns** use ad-hoc spacing (`{}  {}  {}`) — → aligned, headers,
  humanised dates. *(U1/#12)*
- **Bare `jii`** → `Usage: jii <package…> (try \`jii --help\`)` — a cold landing. → the first-run
  wizard (DW) makes the very first bare `jii` a warm welcome; afterwards a compact hint is fine.
- **`Aborted.` / `Nothing to do.`** — terse but clear; keep, maybe warm the wording in friendly mode.
- **Errors are flat strings with no next step** — → D7 remedies (what/why/how-to-fix).

Most of these converge on **D8 (friendly vs advanced verbosity)** as the single biggest lever, plus
the progressive-search feel (A). Neither is a redesign; both are contained UI/config work over the
existing seams.

## E. CLI syntax from first principles (evaluation only)

The user asked to question Unix conventions rather than copy them: which flags deserve short
aliases, which could be positional, which should be prompts, which can vanish because JII infers
intent — and to propose a genuinely cleaner package/source syntax (they floated `firefox @flatpak`
/ `firefox:flatpak`, without insisting on any).

**Guiding principle (first-principles, JII-specific).** JII's dominant interaction, by a wide
margin, is *"install this software"* — `jii <name>`. So the syntax should make the 90 % case need
**zero ceremony**, and treat flags as **overrides and scripting knobs**, not the everyday path. The
cooperation lens (§C) sharpens this: the everyday user should *pick* and *be asked*, not *flag*.
Crucially, **"question convention" cuts two ways** — a *flag's spelling* (`-y`, `--dry-run`) is where
convention IS usability (muscle memory, `--help`, shell completion), so reinventing it costs more
than it saves; but *what is a flag at all* (source, version, profile) is fair game, and that's where
the real win is.

### Q1 — Which flags are used often enough to deserve a short alias?

| Flag | Real frequency | Verdict |
|------|----------------|---------|
| `-y/--yes` | high (scripts, "just do it") | already short — keep |
| `-n/--no` | low-med (symmetry) | already short — keep |
| `-v/--verbose` | med | already short — keep |
| `--auto` | med | **redundant with `-y`** (see Q4) — collapse, don't alias |
| `--source` | occasional | don't alias — **promote to inline spec** (below); keep `--source` for scripts |
| `--profile` | rare (a *preference*, not a per-run choice) | **move to config/wizard** (Q3) — no alias |
| `--dry-run` | occasional, deliberate | keep long; it's typed slowly and self-documents, a short buys little |
| `--json` | machine only | keep long — clarity beats brevity for a machine flag |
| `--no-color` | rare | keep long; also honour `NO_COLOR` env so the flag is rarely needed |

**Net:** almost every flag frequent enough to matter is *already* short. The only everyday friction is
`--source`, and the right fix isn't a shorter flag — it's making source selection **not a flag**.

### Q2 — Which commands could become positional instead of flags?

Commands are already positional **verbs** (`jii remove firefox`), and install is the default
(`jii firefox`) — the biggest ergonomic win, already in place. The remaining flag-shaped thing that is
really a *positional qualifier of the package* is **source** (and, later, **version**): "which
firefox" is part of naming the package, not a separate global switch. That's the strongest candidate
to move out of flag-land.

### Q3 — Which options should become interactive prompts?

- **`--source` → the chooser.** Already true (U4/T5.1): interactively you *pick* the source from a
  menu; you should almost never type a source at all. The flag/inline-spec is the power/scripting path.
- **`--profile` → config + the first-run wizard.** Ranking profile is a standing *preference*, not a
  per-invocation decision; it belongs in `config.toml`/`jii setup`, not on every command line.
- **confirmation → the prompt it already is**, with `-y` as the scripting override. Correct as-is.

### Q4 — Which flags can disappear because JII infers intent?

- **`--auto` folds into `-y`.** In the code both merely skip the confirm for trusted candidates and
  suppress the chooser; the trust barrier (ADR-0006) already forces an explicit answer for untrusted
  even with `-y`. So `-y` *is* "yes within trust". Two names for ~one behaviour is surface to shed —
  keep `-y/--yes`, drop `--auto` (or make it a hidden alias) and the `install.auto` config knob stays.
- **`--profile`** leaves the hot path (Q3) — inferred from config.
- **`--no-color`** inferred from `NO_COLOR` + tty; flag kept only as an explicit override.
- **source** is *inferred* by ranking in the common case (you don't specify it); the spec below is the
  *override*, needed only when you disagree with the recommendation.

### Proposal — an inline **package spec**, flags kept for scripting

Adopt a small, familiar package-spec grammar as the natural way to qualify a package, and demote
`--source` to its scriptable equivalent:

```
name[:source]          # firefox            → install, JII picks the source
                       # firefox:flatpak    → install firefox from flatpak
                       # (reserve  name@version  for when the version chooser lands, e.g. firefox@120)
```

- **`:source`** is per-package and unambiguous (`jii firefox:flatpak cava:dnf` — different sources in
  one command), mirrors `docker image:tag`, is **shell-safe unquoted** (`:` and `@` are not special in
  bash/zsh), and is shorter *and* more readable than `firefox --source flatpak`.
- Reserve **`@version`** for the (deferred) version chooser — that matches the near-universal
  `npm/pip/go pkg@version` muscle memory, so `@`=version and `:`=source never fight each other:
  `firefox:flatpak@120`.
- I recommend **`:` over `@` for source** precisely because `@` is so strongly "version" everywhere
  else; using `@` for source would mis-train users the moment they meet another tool.

**Why this fits the architecture (not a clap fight).** The spec is just a **positional value**; clap
is untouched (no single-dash-long, no custom parser settings). JII parses `name:source` itself in a
tiny pure, unit-tested `PackageSpec::parse` (exactly the ADR-0012 "isolate + test parsers" pattern),
validates the source against `KNOWN_SOURCES` with a did-you-mean suggestion, and a package name that
literally contains a colon (vanishingly rare) has the `--source` escape hatch. `--source` stays as a
discoverable, scriptable synonym (and applies to the whole command when given).

### Recommendation

**Keep conventional flags for things that are genuinely flags** (`-y`, `-v`, `--dry-run`, `--json`,
`--no-color`) — there, convention *is* usability. **But do question "what deserves to be a flag":**
1. `jii <name>` as default install — **done**.
2. Promote **source** (and later version) from `--source` to an inline **`name:source`** spec; keep
   `--source` as the scripting equivalent and the **chooser** as the interactive default. *(the one
   real new ergonomic win)*
3. **Collapse `--auto` into `-y`**; move **`--profile`** to config/`jii setup`; infer `--no-color`
   from `NO_COLOR`. *(shed surface)*

This reduces typing and reads naturally **without** inventing an alien flag grammar or breaking
completion/help. It is **additive and non-breaking** (flags still work), but it *defines* the 1.0
surface, so it deserves its own **ADR** and should land before we lock Terminal 1.0 — most naturally
alongside U4 (chooser) since spec + chooser are the two faces of "choose a source". If, after all
this, one preferred pure flags, that's defensible — but the inline `name:source` spec is a low-risk,
familiar, genuinely cleaner win, so I recommend adopting it.

**Honest counter-arguments weighed:** discoverability (a newcomer won't guess `:flatpak`) — mitigated
by keeping `--source` in `--help`, by the chooser teaching that sources exist, and by an error hint
("multiple sources offer firefox — try `jii firefox:flatpak`"). Colon collisions with package names —
real but negligible, with the `--source` escape hatch. Net: the spec earns its place; the flag
*syntax* does not need reinventing.

### E.1 — Locking the grammar: "should this even be a flag?" (critical pass)

The user pushed the philosophy further: anything that *belongs to the package* (source, version,
channel) should live in the **package spec**, not be a flag; the remaining flags should be *truly
global*; and an **explicit spec must suppress the matching question** (a pinned source skips the
chooser). Critical evaluation, still no code.

**Refined grammar — `name[:source][@ref]`, where `@ref` is source-interpreted.** The user's own
examples show `@` meaning different things per source (`node:brew@22` = a version; `firefox:flatpak@stable`
= a flatpak *branch/channel*; snap has channels stable/candidate/beta/edge). So `@ref` is **one
"which version/channel/branch" slot that the owning provider resolves** — the core never interprets
it (ADR-0004 holds; ADR-0009's "versions are opaque to the core" extends naturally to refs). This
folds *channel* into `@ref` too, so we don't need a third separator. `:` = source, `@` = source-ref,
and — as agreed — `@` is left to mean version/ref because every other ecosystem trained that.

**Bigger realization: the spec is universal across *all* commands, not just install.** `name:source`
is the one "which one" disambiguator everywhere a package is named:
- `jii firefox:flatpak` — install from flatpak (skip the source chooser);
- `jii remove firefox:flatpak` — **this is the non-interactive answer to the multi-owner remove
  chooser (#11)**: pick the copy directly instead of being asked;
- `jii info node:brew`, `jii update node:brew@22` — same grammar.
So the spec *unifies* source disambiguation that today is scattered across `--source` and two
different interactive choosers. That consistency is the real prize — one grammar to learn, it works
identically in every verb.

**Explicit intent suppresses the question (the requested rule), with nuance:**
- `:source` present → **skip the source chooser** (install) and the owning-source chooser (remove).
  Code-wise this is one added clause on the existing `offer_choice` gate (`spec.source.is_none()`),
  which confirms it fits — no new machinery.
- `@ref` present → **skip any version prompt** and pin the ref.
- **Partial spec `firefox@120` (ref, no source):** a ref is inherently source-specific, so this means
  "version 120 from the *recommended* source" — resolve there, don't pop a generic source chooser
  after the user has already narrowed intent. Only if the recommended source lacks that ref do we
  fall back to "sources that have 120". So a ref *also* damps the source question.
- **Explicit source with no match** (`firefox:flatpak` but flatpak has no firefox) → an honest error
  ("firefox is not available from flatpak"), **never a silent substitution** to another source. That
  is the cooperation lens (§C): respect intent, don't override it. (We may still print a one-line
  "also available via dnf, cargo" — inform, don't nag.)

**Resulting flag taxonomy (the "truly global" set shrinks hard):**

| Destination | Flags / concepts |
|-------------|------------------|
| **Truly global flags** (kept, conventional — convention *is* usability here) | `-y/--yes`, `-n/--no`, `--dry-run`, `-v/--verbose`, `--json` |
| **Into the package spec** | source (`:source`), version/channel (`@ref`) |
| **Into the chooser** (interactive) | source selection when unspecified |
| **Into config / `jii setup`** | `--profile` (a standing preference, not a per-run choice) |
| **Eliminated / inferred** | `--auto` → folds into `-y`; `--no-color` → inferred from `NO_COLOR`+tty (kept only as an override) |
| **Demoted but kept** | `--source` as the *whole-command* sweep (`jii a b c --source flatpak`, where repeating `:flatpak` per package would be tedious) and the scriptable/discoverable synonym |

So the everyday surface a user must remember becomes: **`jii name[:source][@ref]`** plus a handful of
global switches (`-y`, `--dry-run`, `-v`, `--json`). That is dramatically easier to hold in the head
than a dozen flags.

**Critical edge cases (why the pure, tested parser matters — ADR-0012):**
- **npm scoped names start with `@`** (`@angular/cli`, `@vue/cli`) — a *leading* `@` is part of the
  name, not a ref. Rule: split a ref only on a **non-leading** `@`, and only the **last** one, so
  `@angular/cli@18` → name `@angular/cli`, ref `18`. This must be an explicit, unit-tested parser
  rule, not an afterthought.
- **`:` inside a name** is vanishingly rare on Linux; the `--source` flag is the escape hatch.
- **github `owner/repo`** uses `/` (untouched by the spec); `owner/repo:github` would be redundant but
  harmless.
- **Parse the full grammar *now*, but reject an unimplemented `@ref` clearly.** The version chooser is
  deferred, yet we are locking the 1.0 *surface*. Silently ignoring a version pin is dangerous (the
  user asks for 120, gets latest). So parse `name[:source][@ref]`, and if `@ref` is used before
  version selection exists, **error explicitly** ("pinning a version/channel is coming in a later
  release") rather than dropping it. This locks a forward-compatible grammar without half-building it.

**Recommendation — lock this as the Terminal 1.0 grammar.** It reads like a package specification
designed *for* JII, it is additive/non-breaking (every flag still works), it *unifies* source
selection across all verbs, and it fits the architecture with zero core changes (a pure `PackageSpec`
parser; clap untouched; provider resolves `@ref`). It deserves its **own ADR** and should land with
U4 (spec + chooser are the two faces of choosing a source; the "skip chooser when `:source` given"
rule is literally one clause). After a genuinely critical pass I do **not** find a reason to prefer
flags for package-belonging attributes — the spec wins. The only things that stay flags are the
truly global ones, exactly as the user framed it.
