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
