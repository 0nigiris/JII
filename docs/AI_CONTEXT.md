# JII — AI Context (Current State)

> **Purpose:** the single-page current state of the project, so any agent (AI or
> human) can pick up development in under five minutes. This file describes **only
> the present** — no history. History lives in git; decisions in
> [DECISIONS.md](DECISIONS.md); the plan in [TASKS.md](TASKS.md).
>
> **Keep this file current.** Updating it at the end of every session is mandatory
> (see the AI Handoff Policy in [CLAUDE.md](../CLAUDE.md)).

_Last updated: 2026-07-09_

---

## What JII is

A smart universal package **installer** (not a manager) for Linux, in Rust,
Fedora-first. It searches multiple sources (DNF, Flatpak, and — soon — GitHub
Releases, COPR…), ranks them, installs the best, and explains why. Read
[CLAUDE.md](../CLAUDE.md) for binding constraints and
[ARCHITECTURE.md](ARCHITECTURE.md) for the canonical design.

## Current phase

**Terminal 1.0 (ADR-0026) — T1–T4 done; T5 candidate chooser landed; now pivoted to a UX-polish
pass.** After the first real dogfooding on a clean Fedora VM the user re-prioritised (2026-07-06):
**no new features/providers/GUI — polish the terminal experience.** T5's remaining **GitHub by-name
repo chooser is deferred** (ADR-0030 Proposed/deferred — a new feature, not a reported UX problem);
the version chooser is likewise paused. The current deliverable is **[docs/UX_EVALUATION.md](UX_EVALUATION.md)**:
16 UX problems classified (10 pure polish over existing seams, 6 need small optional-capability/UI
designs), a delivery order (U0–U8), and a NixOS architectural opinion. **Awaiting the user's one open
decision: doctor scope for 1.0 (actionable JII-diagnostics only vs the full codec/driver/font
recommend-catalog).** Cross-distro is real: JII runs on Debian/Ubuntu (apt), Arch (pacman),
openSUSE (zypper) and Nix, not just Fedora. Below is the pre-T4 Phase-5 context (still accurate).

**Phase 5 — user-space sources & update (done).** Phases 0–4 done and verified.
The pre-Phase-5 re-evaluation (ADR-0022) confirmed the model needs **no change** for
these providers. **`cargo`, `npm`, `pipx`, `go` are done** (pure `Provider`s, sharing
`get_json_opt`/`command_plan`); **`jii update` is done** (no per-source branching); the
post-8-provider **architecture review** is done (ADR-0024: architecture healthy, no code
change); and **batch install is done** (ADR-0025: `jii install a b c`, same-source merge
via optional `plan_install_many`, no model change). Next: **Homebrew** provider (ADR-0024).

## Last completed work

**U8 — first-run walkthrough polish (2026-07-06). The UX-polish pass (U0–U8) is now complete.**
Played the whole CLI as a new user and fixed the awkward edges (#15); no architectural change, no ADR.
Two small commits: (1) **aligned ledger tables** — `list`/`history`/`audit` printed ad-hoc
`{}  {}  {}` with no header/alignment (and `history` leaked the `Action` enum via `{:?}`); all three
now render through one `table_lines` helper (header row + data-driven column widths, so a long name
like `visual-studio-code` no longer breaks alignment the way audit's fixed `{:20}` did), plus
`Action::label()` for human past-tense history verbs (installed/removed/updated). (2) **update
message fix** — `jii update <not-installed>` printed the correct `✗ Not installed: X` then a
misleading `Nothing installed via jii yet.`; since bare `update` routes to the system update, an
empty named-path record set always means "the named ones aren't installed" (already stated), so the
follow-up is dropped (mirrors `remove`). Fedora-verified (list/history/audit short+long names; the
first-run wizard replayed via pty reads clean; friendly vs `-v` install preview). **180 tests green,
clippy clean.** Noted follow-up (not done, low value): in friendly single-install the "Also
available" block prints just before the recommendation line — mildly backwards, but reordering it
would restructure the preview flow, so left as-is.

**U7 — system-wide update (2026-07-06, D10, ADR-0034).** Bare `jii update` now updates the **whole
system**, not just JII's registry slice (#15, "the universal update command"). New **optional**
`Provider::plan_update_all() -> Result<Option<InstallPlan>>` (default `None`): "upgrade everything
this source owns". `Engine::plan_update_all` aggregates every available provider's `Some(plan)` into
a `SystemUpdate { plans, sources }`; `Engine::run_system_update` primes privilege **once** across the
mixed root/user plans and runs them — the engine never branches on the source id. **Non-regression:**
sources with no bulk path (github/cargo/go → `None`) still get their JII-installed packages updated
per-record, via a fallback batch appended to the same run (the version-refresh loop is extracted to
`refresh_for_update`, shared with the named path). Named `jii update <pkg>` is unchanged (registry
path; `:source` still pins). Implemented for **all** bulk managers: dnf `upgrade`, flatpak `update`,
apt `upgrade`, pacman `-Syu`, zypper `update`, snap `refresh`, nix `profile upgrade --all`, brew
`upgrade`, pipx `upgrade-all`, npm `update -g`. Bulk plans upgrade beyond JII's ledger so they are
**not** recorded (only the per-record fallbacks refresh the registry) → `jii list` may show a stale
version for a bulk-updated tracked package (documented debt). **177 tests green, clippy clean**;
verified on Fedora (bare `update --dry-run` = dnf + flatpak, friendly one-line preview, `-n` abort,
named path intact). Non-Fedora bulk impls unverified on a live host (T7 debt).

**U6 — helpful failure & doctor (2026-07-06).** All small commits, no architectural change to the
core; two ADRs (0032, 0033):
- **Actionable errors (D7, ADR-0032).** A pure, unit-tested `JiiError::remedy() -> Option<String>`
  maps a *typed* failure to a next step, rendered under the error (`  → …`) by `main.rs::report`
  (so a bad-config failure, before any `Renderer` exists, still gets its remedy). `UnknownSource`
  lists `KNOWN_SOURCES` + points at the config/`jii setup`; `Config`/`Io` (by `ErrorKind`) get
  specific advice; `Other(anyhow)` returns `None` on purpose — no string-sniffing opaque text into
  a misleading remedy.
- **doctor Tier 1 (D6).** `jii doctor` now prints, under the per-source health table, a "System
  checks:" section about JII itself working — is `~/.local/bin` on `PATH` (where cargo/npm/pipx/go/
  GitHub installs land), is `GITHUB_TOKEN` set. Read-only (reports + advises, no auto-apply). Pure
  `system_checks` decides; JSON stays the stable per-source array. Consumed the previously
  dead-coded `Platform::is_on_path`/`path_dirs`.
- **recommend-catalog Tier 2 (D6, ADR-0033).** A **data subsystem**, not code, not a `Provider`:
  `data/recommend/catalog.toml` embedded via `include_str!`, typed + loaded in `src/recommend.rs`,
  filtered by host distro via the new `Distro::id()` (the first real consumer of distro-awareness
  ADR-0029 deferred — entries *declare* their distros, no `if fedora` branch). `jii recommend`
  lists curated Fedora suggestions (RPM Fusion, codecs, VLC, fonts, Steam, Wine, tuned-ppd) grouped
  by category — each with why + the exact way to get it. `jii recommend <id>` **applies** one by
  routing its `packages` through the normal install path (preview → confirm → execute, so the U3
  pre-check + U5 preview come free); a `manual` repo-enable (RPM Fusion) is **shown, never run**
  (the trust boundary is called out). Analyze → Explain → Ask → Apply throughout. **175 tests
  green, clippy clean**; verified on Fedora (remedy, doctor checks, recommend list/apply/manual/
  unknown-id). **Debt:** Fedora catalog entries are hand-curated, unverified on a clean VM (T7).

**U5 — the Friendly/Advanced UX pass (2026-07-06).** A big verbosity + onboarding pass, all
landed as small commits, no architectural change:
- **Friendly/Advanced output modes (D8).** `config::OutputMode { Friendly (default), Advanced }`
  (serde lowercase, in `[ui] mode`). `Renderer` carries the mode; `is_friendly()` is `!json &&
  Friendly`. `-v`/`--verbose` forces Advanced for one run without touching the config. Friendly
  **hides secondary-source failure noise** (`report_source_failures` returns early — no more
  `⚠ copr: timeout` spam on a normal search) and **collapses the install preview** to one short
  scannable line per package (`Install <name> (<ver>) via <source> — <why>  [needs sudo]`);
  `--dry-run` and Advanced still print the full Plan block (the point of a dry-run is the detail).
- **First-run wizard + `jii setup` (DW).** `config::MetaConfig { first_run_completed }` +
  `Config::save()` (toml::to_string_pretty, `create_dir_all`) + `is_first_run()`. A bare `jii` in
  an interactive first-run session offers a 30-second setup (welcome → mode chooser → optional
  doctor → save); declining still marks it done so it never nags again. `jii setup` re-runs it on
  demand. Non-interactive/`--json`/piped sessions never trigger it.
- **A clap parse fix** discovered while testing: a global flag *before* a subcommand
  (`jii -v search git`, `jii --json search git`) used to misparse as `install ["search","git"]`
  because of `args_conflicts_with_subcommands = true` — removed; the full parse matrix re-verified.
- Neutral chooser prompt wording ("Your choice [N] (or 'n' to cancel):") so it reads the same for
  install/remove/setup. **165 tests green, clippy clean**, wizard + Friendly paths pty-verified in
  an isolated `XDG_CONFIG_HOME`.

**T5 (slice 1) — the interactive candidate chooser (`ui/prompt::choose`).** A single
interactive install that resolves to **more than one** candidate now shows a numbered source
menu — the recommendation pre-selected as the default (Enter installs it), each other source
selectable by number, `n` to cancel — instead of silently taking the top rank. The chooser
addresses the "never silently install the wrong thing" requirement. **Honest architectural
finding: no ADR and no engine/model change were needed.** The pre-declared "chooser/selection
model" growth turned out to already exist: `Provider::search` has returned `Vec<PackageCandidate>`
and the engine has ranked the whole set together since Phase 3, so the chooser is **pure
`cli`/`ui`** over the ranked list the install path already had. Design points: (1) picking a
source is itself the consent, so a **trusted** interactive pick skips the otherwise-redundant
`[Y/n]` (tracked by `chose_interactively`), while an **untrusted** pick still hits the trust
barrier (ADR-0006 preserved — `skip_confirm` is gated on `least_trusted <=
default_yes_max_trust`); (2) the chooser only fires for a **single**-package install with
`ranked.len() > 1` in an **interactive** session (`!--source && !effective_auto && !--yes &&
!--no && tty && !json`) — batch installs stay auto-picked to avoid a prompt storm, and every
non-interactive/intent-expressing path is unchanged. The pure `parse_choice` (empty→default,
`n`/`q`/`cancel`→cancel, in-range number→pick, else→re-ask) is unit-tested; the three live
paths (Enter→dnf, `2`→cargo, `n`→abort) plus the `--auto` bypass and the piped non-TTY
fallback were verified on a real pseudo-terminal. **150 tests.**

**T4 — cross-distro system providers + the platform-seam relaxation (ADR-0029, Accepted).**
The whole codebase coupled to the distro in exactly one place (`Platform::is_supported` →
`matches!(distro, Fedora)`); a full audit (real code) showed every provider already self-gates
on its **binary** via `which`, never the distro. So T4 was not an engine refactor — it removed
one artificial wall and de-privileged the `Distro` enum. Enacted: **removed**
`Platform::is_supported`/`require_supported` and `JiiError::UnsupportedPlatform`; `Platform` is
now a **pure host-facts value object** (`distro` kept as a fact, no reader until T6 config-seed/
bootstrap). "Supported" is redefined as **"≥1 usable install source"** (`Engine::any_source_available`,
the same `is_available` fan-out `source_catalog` uses), guarded at the 5 CLI entry points by a
shared `ensure_usable_source` (distinguishes "none enabled" from "none available" — clearer than
the distro wall even on Fedora). Then four providers, each a pure additive `Provider` that
self-gates on its binary, with `_many` batching:
- **apt** (Debian/Ubuntu): `apt-cache show` deb822 (pure `parse_show`, first stanza),
  `apt-get install/remove/install --only-upgrade` (root), `dpkg-query` list. Official.
- **pacman** (Arch): `pacman -Si` (pure `parse_si`), `pacman -S`/`-Rs` (root), `pacman -Q` list.
  Official; official repos only (AUR is a separate future source).
- **zypper** (openSUSE): `zypper --xmlout search` (dependency-free `<solvable>` attr parse),
  `zypper --non-interactive install/remove/update` (root), `rpm -qa` list. Official.
- **nix** (any distro): modern flakes CLI (`--extra-experimental-features`), `nix search --json`
  (exact `pname` decided in code), `nix profile install/remove/upgrade` — **user-space, no root**;
  empty list + `~/.nix-profile/bin/<name>` `is_installed` (go precedent). Community.
Shared **`provider::run_capture_lax`** (stdout even on non-zero exit) added beside `run_capture`:
apt-cache exits 100, `pacman -Si` 1, `zypper` 104 for "unknown package" = "no candidate", not a
source failure. No core branch on the source. Fedora behaviour verified unchanged (`jii sources`,
dnf dry-run). **150 tests.** **Debt:** nix `profile` CLI is version-fragile and was **not** verified
on a live Nix host (none here) — flagged for the T7 clean-VM pass; the `id`/`id_like` distro
predicate stays deferred to its first consumer (T6), per ADR-0029.

**Prior — Batch install — `jii install a b c …`.** Install many packages as one operation with no
change to `InstallPlan` or the Executor (ADR-0025). Each package runs the normal
search→rank→pick; the engine groups the chosen candidates by source and **merges
same-source installs into one command** where the source can (`dnf/cargo/npm/go install a b
c`) via a new **optional** `Provider::plan_install_many` (default `None` → per-candidate
fallback — the ADR-0022 growth pattern; the engine never branches on the source). One
grouped "Summary" preview + action preview, one confirmation governed by the **least-trusted
candidate** (`prompt::confirm_install_batch`; untrusted still always explicit, ADR-0006),
one root escalation (`exec::prime_for` once across all plans), one run
(`exec::run_actions`), and records written **as each plan succeeds** so a mid-batch failure
leaves the registry accurate. A not-found package is reported and does **not** cancel the
rest (offer to continue). A group of one keeps the richer single-package plan, so
`jii install <pkg>` output is byte-identical to before. Single install is now a batch of one
— the old `Engine::install` and `plan_install` wrapper were removed (one install
write-path, no duplicated recording to drift). Bootstrap-a-missing-manager is **deferred,
not faked** (needs the manager-install feature; the per-source grouping is its future
hook). Verified: dnf/cargo merges, mixed-source grouping, not-found continue, single-package
UX unchanged. **99 tests.**

**Prior — `jii update [<pkg>]`.** Wires the existing per-provider `plan_update` into a command,
with no per-source branching (ADR-0004 holds). For one named package (must be installed)
or every registry record, it re-searches the **owning** source via the normal search→rank
path (filtered by `source_id`) to get the latest version, **skips provably-current
packages** (exact version-string equality → an up-to-date system is a clean no-op, not a
reinstall), then runs each `plan_update` through the same preview → confirm (a single batch
prompt) → execute pipeline as install/remove. Engine gained `plan_update`/`update`; the
registry gained `record_update` (logs a history `Update`, refreshes the stored version),
sharing an `upsert` helper with `record_install` so the "replace + log + push" invariant
lives in one place. Version handling is honest: it records the just-installed latest from
the re-search, falling back to the prior version only when the source no longer reports one.
Verified end-to-end via `--dry-run` (a simulated go install showing `v0.60.0 → v0.73.1` +
the `go install …@latest` plan), the no-op path, and the missing-package error. 96 tests.

**Prior — `provider/go.rs` (Go modules, via `go install`)** + the pre-`go` helper refactor
(commit `f2e8377`). go is the 4th user-space provider, mirroring cargo/pipx: `search`
resolves a module path via the Go module proxy (`{proxy}/<mod>/@latest`, uppercase → `!x`
escaping), `plan_install`/`plan_update` = one unprivileged `go install <mod>@latest` into
`$GOBIN`/`$GOPATH/bin`/`~/go/bin` (PATH-warn), `plan_remove` deletes the installed binary
(Go has no uninstall — an `Action::RemoveFile`, like github), `list_installed` is empty
(no cheap global module→binary list; the registry + a file-existence `is_installed` track
it). **No app-filter (ADR-0023):** the proxy can't cheaply say which modules are `main`
(installable), so — like pipx — go offers the module and lets `go install` be the
authority. Community trust (go verifies checksums via `go.sum`/sum.golang.org).
`is_available` overrides the shared `which` because go uses `go version`, not `--version`
(the latter exits non-zero). Verified: real proxy search through JII (fzf→v0.73.1 offered,
BurntSushi/toml resolves with `!burnt!sushi` escaping), dry-run (single unprivileged
command). **Pre-`go` refactor:** the search 404-dance and single-command `InstallPlan`
construction had each reached 3× across cargo/npm/pipx (→ 4× with go), so extracted
`provider::get_json_opt` (GET → `Ok(None)` on 404, else typed JSON) and
`provider::command_plan` (one-`RunCommand` plan). Deliberately did **not** extract
`PackageCandidate` construction (per-provider, would leak trust/arch_ok) or the tolerant
stdout read (only 2×) — reducing maintenance cost, not line count.

**Prior — `provider/pipx.rs` (PyPI, via pipx).** Third Phase 5 provider, mirrors cargo:
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

**UX-WAVE 2 — real-use feedback from a clean Fedora VM (2026-07-06, owner-set).** The owner ran the
pushed build on a VM and filed 15 UX points; priority is now **product/UX polish, not architecture**.
Agreed decisions: **command order 1→2→3→4** = ① arrow-key TUI choosers → ② doctor becomes a real
*system helper* (PATH, ~/.cargo/bin, internet, missing managers, flathub, permissions, broken repos,
updates — Analyze→Explain→Ask→Apply, previewable fix plans) → ③ providers/marketplace (manage the
ecosystems themselves: install/remove/update npm/cargo/brew/snap/nix + bootstrap a missing manager)
→ ④ `info` becomes an app *card* (description/GitHub/site/license/author). Also decided: **recommend
folds into the new doctor and the standalone `jii recommend` command is removed** (owner disliked it);
**`list` and `audit` merge** into one (`jii list`, security via `jii list --audit`).
**Done this session (pushed):** #3 setup stops advertising next commands; #10 crisp "already
installed" (no "Nothing to do"); #12 `jii why`→`jii how` (`why` hidden alias); #13 crisp
Installed/Removed/Updated confirmed; **#1 arrow-key TUI choosers via `dialoguer` Select** (↑↓/Enter/
Esc, upgrades setup + source chooser + multi-owner remove at once; pty-verified); #6 `-d` alias for
`--dry-run`. **Diagnosed #9** (npm `lodash` finds nothing) = **by design**, not a bug (npm/cargo only
offer packages with a CLI `bin`; libraries aren't "programs"); a helpful "it's a library" message
needs a small provider signal — noted follow-up. **#11 already done in U7** (bare `jii update` =
whole system).

**② doctor-as-system-helper — slice 1 (read-only diagnostics) landed.** `jii doctor` now probes the
host environment beyond the two Tier-1 checks: **internet reachability** (a fast HTTPS HEAD; a
failure reads red/critical), **git** and **curl** presence (advice points at `jii git`/`jii curl`),
**~/.cargo/bin on PATH** (only when cargo is present or the dir exists), and **Flathub remote**
configured (only when Flatpak is installed). Facts are gathered concurrently (`tokio::join!`) in
`gather_system_facts`; the verdict/wording logic stays a pure, unit-tested `system_checks(&SystemFacts)`.
A closing summary line reports how many things need attention. 180 tests, clippy clean; verified live
(caught `~/.cargo/bin` missing from PATH on the dev host).
**② doctor-as-system-helper — slice 2 (`--fix`) landed.** `jii doctor --fix` offers the fixable
checks: git/curl route through the normal install path (which previews + confirms itself); the
Flathub remote is a plain command shown before it runs (`run_plain_command`; Flatpak elevates via
its own polkit, so JII wraps no sudo/pkexec). Each `Fix` is data on the `SystemCheck`
(`Fix::Install(pkg)` / `Fix::Command{argv,show}`), kept pure and unit-tested. `--dry-run` previews
every fix without asking or changing anything; a plain `jii doctor` nudges "run --fix" only when
something is fixable. PATH/token/internet stay manual-only (JII won't edit your shell rc or invent a
token). 183 tests, clippy clean; live-verified (nothing-fixable path on the dev host).
**② doctor-as-system-helper — slice 3 (fold recommend) landed (ADR-0035).** The recommend catalog
now surfaces at `doctor`'s tail as a compact "Suggestions for your system" section (title — why · the
exact command to run; `note:` caveats shown). The **standalone `jii recommend` command and its
apply-by-id path are removed** — applying is now just running the shown command (`jii vlc` / the
`manual` command), more transparent than `recommend <id>`. `Recommendation.id` is no longer read at
runtime (uniqueness invariant moved to `title`; slug kept in the TOML as an authoring anchor).
Catalog data subsystem (ADR-0033) untouched; only its presentation moved. 183 tests, clippy clean,
live-verified. **② doctor is now complete.**

**③ providers/marketplace landed (ADR-0036).** New read-only **`jii providers`** lists the installable
*ecosystem* managers (npm, cargo, brew, Flatpak, snap, pipx, go, nix) with their presence on this host
(installed vs available); base repos (dnf/apt) and non-managers (github) are absent — you don't install
those. Ecosystem-ness is **provider metadata**: an optional `Provider::ecosystem() -> Option<Ecosystem>`
(default `None`, ADR-0022 growth) declaring a `label`, `binary`, and a `Bootstrap`. **`jii providers add
<name>`** bootstraps a missing manager: `Bootstrap::Packages(&[…])` is an **ordered cross-distro
candidate list** (`nodejs-npm`→`npm`; `golang`→`go`→`golang-go`) resolved by `Engine::first_available_package`
(first that searches non-empty wins — JII's own search does the per-distro work, no source branch) then
handed to the **normal install path** (preview→confirm→execute→record, the `doctor --fix` reuse pattern);
`Bootstrap::Script(cmd)` (brew, nix) is **shown, never run** (trust boundary, ADR-0005/0006). Already-
installed / unknown-ecosystem answer clearly. 184 tests, clippy clean; live-verified on Fedora (providers
list + JSON; add already-installed/unknown/script/packages-dry-run → pipx resolved to dnf `pipx` with full
preview). **Debt:** the `Packages` candidate lists are hand-curated, unverified on clean non-Fedora VMs (T7).

**④ info app-card landed (ADR-0037).** `jii info` is now an app **card**: name → description → an aligned
metadata block (Source, Version, License, Homepage, Repository, Author — present fields only) → the source
list + recommendation. Rich metadata is an optional **`async Provider::describe(&candidate) -> Option<PackageInfo>`**
(default `None`, ADR-0022 growth) called only for the recommended candidate on `info` (never on the search
path). **dnf implements it fully** (one `dnf5 info` call, pure tested `parse_info`: Description/URL/License/
Vendor, first stanza wins, folds continuation lines); **github gives a cheap card** (repo URL + owner as
author from the `owner/repo` already in `raw`, no extra call); every other source inherits `None` and shows
the basic card (name/summary/version/trust/source degrade gracefully). `--json` now returns
`{candidates, recommended, info}` (was a bare array). 185 tests, clippy clean; live-verified (firefox full
dnf card, jqlang/jq github card, ripgrep:cargo sparse card, JSON). **Debt:** dnf License/Vendor shown
verbatim (RPM's SPDX-ish strings); cargo/npm/flatpak richer cards + the GitHub repo-metadata fetch are
follow-ups.
**list+audit merged (ADR-0038).** `jii list` gained a `--audit` flag: bare = the plain NAME/SOURCE/VERSION
table; `--audit` = the security view (trust/verification/concerns + "N need attention"). The **standalone
`jii audit` command is removed** (rendering moved to a private `audit_view` helper; the engine `audit()`
computation + `AuditEntry` model untouched). Same fold-a-command-into-a-flag pattern as ADR-0035. 185 tests,
clippy clean; live-verified (`list`, `list --audit`, and that `jii audit` now falls through to install).

**✅ UX-WAVE 2 COMPLETE** — all agreed items landed and pushed: ① arrow-key TUI choosers, ② doctor-as-
system-helper (+`--fix`, +folded recommend, ADR-0035), ③ providers/marketplace (ADR-0036), ④ info app-card
(ADR-0037), and the list+audit merge (ADR-0038), plus the earlier small fixes (#3/#6/#10/#12/#13). **Next:
Beta prep resumes** (see BETA_ROADMAP.md): integration tests → clean-VM verification on Arch/Ubuntu/Debian/
openSUSE (the one blocker needing the owner's real hosts) → README/logo/screenshots/asciinema → cut Beta by
pushing a `v*` tag. The `cli/mod.rs` split into `cli/commands/*` (now ~1900 lines) is the queued structural
cleanup, best done before more feature work.

**Superseded — BETA-READINESS FEATURE FREEZE (2026-07-06).** Was: freeze features, drive to Beta
(CI ✓ already present; release workflow + install docs landed — see [BETA_ROADMAP.md](BETA_ROADMAP.md)).
The VM run reprioritised to UX-wave 2 *before* cutting Beta; the Beta plan still stands and resumes
after this polish wave. Release infra is ready (owner cuts it by pushing a `v*` tag). The UX-polish pass (U0–U8) is complete
and the CLI is functionally done. The owner has **frozen new large features** and set the drive to the
**first public Beta**, priority order: **(1) CI → (2) integration tests → (3) clean-VM verification on
Arch/Ubuntu/Debian/openSUSE → (4) README/logo/screenshots/asciinema/docs → (5) public release.** The
plan and the parked backlog (undo, bootstrap, version chooser, doctor --fix, declarative providers,
etc.) live in **[docs/BETA_ROADMAP.md](BETA_ROADMAP.md)** — its "Frozen" section must NOT be started
without an explicit post-Beta go-ahead. Bug fixes / hardening / tests / docs / packaging stay in
scope; new user-facing capabilities do not. **#3 (clean-VM) is the one Beta blocker an agent can't
close alone** — it needs the owner's real non-Fedora hosts (agent can script the smoke test). Next
recommended action: **#1 CI** (GitHub Actions: build + clippy -D warnings + test + fmt --check).

**Prior phase — Terminal 1.0 (ADR-0026), UX-polish pass (2026-07-06, DONE).** After dogfooding on a
clean Fedora VM the owner re-prioritised to **UX polish, no new features**; the remaining T5 feature
slices (GitHub by-name repo chooser, version chooser) are **deferred** (now parked in BETA_ROADMAP).
Plan + classification live in **[docs/UX_EVALUATION.md](UX_EVALUATION.md)** (U0–U8, "Progress" is the
live status).
Doctor scope decided: **Tier 1 + the recommend-catalog, both in 1.0** (own catalog ADR; ROADMAP
"Analyze→Explain→Ask→Apply" holds). **Landed so far (all [A], no ADR):** U0 measured (startup ~0ms
fine; cold search was 8s because one straggler — copr, ~9s API — burned the timeout, not a
parallelism problem); U1 killed unavailable-provider spam + de-duped the single-package preview; U2
lowered the search timeout 8→5s (search 8.05→5.08s); U3 added an already-installed pre-check
(targeted `installed_lookup`, in-place update offer) and multi-owner `remove` (`resolve_all_installed`
+ chooser with "all"). U4 landed the `PackageSpec` grammar (ADR-0031) across install/remove/update/
info; **U5** added Friendly/Advanced modes + the first-run wizard/`jii setup`; **U6** added
actionable errors (ADR-0032), doctor Tier 1 system checks, and the recommend-catalog (ADR-0033:
`jii recommend` list + apply); **U7** made bare `jii update` a system-wide upgrade (ADR-0034:
`plan_update_all` across all bulk managers + per-record fallback); **U8** was the final walkthrough
polish — aligned, headered tables for `list`/`history`/`audit` (one data-driven `table_lines` helper,
`Action::label()` for human history verbs) and a fix for `jii update <not-installed>` no longer
claiming the ledger is empty. **The UX-polish pass (U0–U8) is complete.** 180 tests green throughout.

**CLI grammar LOCKED — ADR-0031.** After a first-principles pass (UX_EVALUATION §E/§E.1) the package
spec **`name[:source][@ref]`** is now the *language of JII*: source/version/channel belong to the
**spec**, not flags; `@ref` is **source-interpreted** (core never parses it, ADR-0004/0009); the spec
is universal across install/remove/update/info and an explicit `:source` suppresses the chooser.
Durable binding principle: *"does this belong to the package or the command?"* — package attributes
extend `PackageSpec`, never a new flag. `--auto`→`-y`, `--profile`→config/wizard, `--source` demoted
to whole-command synonym. **Syntax is settled — do not re-open it.**

**U4 landed (ADR-0031 + #4 + D5).** `PackageSpec::parse` (pure, `model.rs`, 11 tests) for
`name[:source][@ref]`; wired into **install** — `:source` pins the provider and suppresses the chooser,
`@ref` parsed but explicitly rejected until the version chooser lands, unknown source → did-you-mean,
explicit source with no match → honest miss (no silent substitution); clap untouched, backwards
compatible. **D5**: optional `Provider::highlights` (dnf/copr/flatpak/github/cargo) → `jii info` reads
like the README; UI still never branches on source id. **Chooser (#4):** clearer header + "⭐
recommended" tag. **162 tests green, clippy clean**, verified on Fedora (pty chooser, info, spec paths).

**ADR-0031 tail done:** the spec is now universal — `remove`/`update`/`info` parse it too (same
`parse_specs`). `jii remove firefox:flatpak` pins the copy (the non-interactive answer to the
multi-owner chooser); `update node:brew` picks the copy to update; `info firefox:flatpak` narrows
(`ranked_for` gained a `source` override). `@ref` rejected everywhere; `search` stays free-text.
**U4 complete** — 162 tests green, clippy clean.

**U5 landed (D8 + DW).** Friendly/Advanced output modes + first-run wizard/`jii setup` + a clap fix.
**U6 landed (D7 + D6, ADR-0032/0033).** Actionable errors (`JiiError::remedy`), doctor Tier 1
system checks, recommend-catalog. **U7 landed (D10, ADR-0034).** System-wide `jii update`
(`plan_update_all` across all bulk managers + per-record fallback). Both detailed under "Last
completed work". **177 tests green.**

**Next: U8** — first-run walkthrough polish (the last UX track). Then the UX pass is complete.
Streaming/progressive search (UX_EVALUATION §A, own ADR) is the real speed fix and is on the list.
`--auto`→`-y`, `--profile`→config, `--no-color`→NO_COLOR are the flag-shed follow-ups from ADR-0031.
Structural cleanup queued: **split `cli/mod.rs`** (~1700 lines) into `cli/commands/*`. Recommend
follow-ups: interactive multi-pick, skip already-satisfied entries, a real repo-enable capability (so
RPM Fusion becomes a previewable plan, not a shown command). Update debt: a bulk-updated tracked
package can show a stale version in `jii list`.

<details><summary>Earlier T1–T3 detail (all landed)</summary>

**Terminal 1.0 (ADR-0026) — T1 & T2 done; T3 next.** Priority changed (ADR-0026): finish the
*whole* terminal version ("CLI 1.0") before the first public Beta, instead of going straight to
Homebrew. The full ordered plan is T1–T8 in [ROADMAP.md](ROADMAP.md) / [TASKS.md](TASKS.md); the
scope + the three pre-declared architecture growths (platform-seam relax, provider-ordered
versions, bootstrap-as-plan) are in **ADR-0026**.

**T1 (read-only honesty layer) landed:** `jii search` (ranked candidates, top `→`), `jii info`
(sources + recommendation with a **source-agnostic** rationale — no branching on the source id),
`jii sources` (active vs enabled-but-unavailable). Pure rendering over `search`/`rank`; engine
gained `source_catalog()`. Old `search`/`info` stubs + `not_yet` gone; README de-lied.

**T2 (batch update/remove) landed:** `jii update a b c` / `jii remove a b c` (and `jii update` =
all). Exactly the ADR-0025 machinery — **no new architecture**. Optional
`plan_remove_many`/`plan_update_many` (dnf/copr/flatpak/cargo/npm + go-update; the rest inherit
`None` → per-record fallback). Engine gained generic `group_by_source`, `RecordOp`,
`plan_record_batch` (→ `RecordBatch { plans, unplannable }`: an un-updatable package like a
github install is reported, never fatal), and `remove_batch`/`update_batch` mirroring
`install_batch`. Single = batch of one; the old single `Engine::remove`/`update`/`plan_remove`/
`plan_update` and `exec::run_plan` were removed (one write-path). Update carries the post-update
record (version = refreshed target); engine stamps installed_at/verification. Verified via
isolated `XDG_STATE_HOME` dry-runs (merged `dnf5 remove/upgrade`, mixed dnf+cargo grouping,
version transitions, single-package richer plan).

**T3 (provider breadth) landed — Homebrew, Snap, AppImage:**

**Homebrew (`brew`):** `provider/homebrew.rs`, same proven shape as cargo/npm/pipx/go —
formula API (`formulae.brew.sh/api/formula/<name>.json`) via `get_json_opt`, unprivileged `brew
install/uninstall/upgrade` (+ `_many`), `brew list --versions`, community trust, no library filter
(ADR-0023). Registered in config (`KNOWN_SOURCES` + default priority; `is_available` gates it off
where `brew` is absent). **Empirical scaffold verdict — ADR-0027: NO shared `RegistryProvider`.**
After 5 providers the only identical code is ~8 lines of boilerplate; `search`/plans/`list_installed`
are irreducibly per-provider; the genuine sharing already lives in the free-function helpers
(`get_json_opt`/`command_plan`/`run_capture`/`which`/…). Verified: real formula API shape matches
the structs (curl), 404→empty, `jii sources` lists brew.

**Snap (`snap`):** `provider/snap.rs` — first **system** provider in the breadth track (root;
`sudo snap install`). Store info API (`api.snapcraft.io/v2/snaps/info/<name>?fields=…`, needs the
`Snap-Device-Series` header → `http_client` directly, like github/copr). `snap install/remove/
refresh` (+ `_many`). **Classic confinement** handled: verifying the live API showed `confinement`
is only returned as an explicit `fields` item (and `fields` restricts the response), so the query
lists `version,confinement,summary,title`; classic snaps get `--classic`, and a classic snap in a
batch declines the merge (`--classic` can't apply selectively) → per-snap fallback. `snap list`
parsed. Community trust; registered near flatpak in priority.

**AppImage (ADR-0028): not a standalone provider.** It has no manager/API and its catalog
(`appimage.github.io/feed.json`) has no download URLs — it is a *delivery format over GitHub
releases*. So `github::classify` now accepts `.AppImage` assets as raw binaries **without** the
`linux` token (AppImages are Linux-only; arch still required; `.AppImage.zsync` rejected).
`jii owner/repo` installs an AppImage today; by-name discovery folds into T5 (repo chooser). The
reserved `"appimage"` id was removed from `KNOWN_SOURCES`.

<details><summary>Homebrew reference (from ADR-0024, the original T3 pick — now landed)</summary>

Same shape as cargo/npm/pipx/go: `is_available` (`brew`), `search` via the formula API
(`https://formulae.brew.sh/api/formula/<name>.json`) with `get_json_opt`, `plan_install`/
`update`/`remove` = single unprivileged `brew install`/`upgrade`/`uninstall` via
`command_plan` (no root; brew is user-owned), `list_installed` (`brew list --versions` or
`--json`), community trust. Handle formula-vs-cask (casks are GUI apps; on Linux brew is
formula-only, so start formula-only). **Empirical check while doing it:** this is the 5th
registry-user-space provider — evaluate (do not assume) whether a thin shared
`RegistryProvider` scaffold now pays off (resolved: ADR-0027, no scaffold).

</details>

Recorded non-blocking debts to respect (ADR-0024): version comparison (add a
provider-computed normalized key beside `PkgVersion`'s raw string only when version-aware
work is next needed), and splitting `cli/mod.rs` (~615 lines) into `cli/commands/*` when it
next grows.

Polish/hardening deferred (not blocking Phase 5; several are now **future features**, do
not implement as silent heuristics):
- **GitHub repository selection** — interactive, "never silently install the wrong repo".
- `.tar.xz` archives (needs an xz decoder dep); better COPR disambiguation; real
  GPG/sigstore verification in `exec.rs::verify_bytes` (currently fail-closed).
- **Engine UI-free seam** (ADR-0022): decouple `&Renderer` from `Engine::install/remove`
  — do this **before** any GUI/second frontend, not now.

Full list in [TASKS.md](TASKS.md) Phase 5.

</details>

## Next recommended task

**T5 (remaining) — the GitHub by-name repo chooser, then the version chooser.** The generic
candidate chooser is done (pure `cli`/`ui`, no ADR); what's left needs real new **provider
capabilities** and so each gets its own ADR:
- **GitHub by-name repo discovery** — github currently answers only explicit `owner/repo`; add a
  bare-name path (github `/search/repositories`) that returns the top few repos (with an
  installable Linux release) as candidates, which then flow into the existing chooser so the user
  disambiguates ("never silently install the wrong repo"). ADR: name→repo policy — ranking
  (stars? exact-name?), filtering to repos that actually publish a usable release, and how many to
  surface. This is the noisier/riskier piece; keep it conservative.
- **Version chooser** — `--version <v>` + an optional `Provider::available_versions` (provider-
  ordered, ADR-0022 growth pattern) so a source can offer real version choices. ADR for the
  version growth (pre-declared in ADR-0026); note the per-source pinning-syntax divergence
  (dnf `pkg-1.2.3`, cargo `--version`, github a release tag).

After T5: T6 (bootstrap a missing manager — where the `id`/`id_like` distro predicate finally
gets built, ADR-0029), T7 (hardening + **clean-VM testing on Fedora/Arch/Ubuntu/Debian/openSUSE**,
incl. verifying the nix provider **and the chooser interactively** on a live host), T8 (public
polish). Then the first Beta.

## Current blockers

None.

## Build status

`cargo build` — clean, no warnings. `cargo clippy` — clean.

## Test status

`cargo test` — **185 passing, 0 failing**. ④ coverage: `dnf::parse_info_takes_first_stanza_and_folds_continuations`
(folded description, URL/Vendor, first stanza wins over a later one). ③ coverage: `provider::ecosystems_declare_bootstrap_and_base_repos_do_not`
(every ecosystem manager declares a non-empty binary + a usable `Bootstrap`; dnf/github declare none). U7 coverage: dnf/flatpak `plan_update_all` (whole-system
upgrade, root vs user). U6 coverage: `error::remedy` (unknown-source lists the
known ones, Io branches on `ErrorKind`, opaque errors invent nothing), `cli::system_checks` (PATH +
token pass/fail + advice + env-name), `recommend` (embedded catalog parses, ids unique, empty-distros
applies everywhere, distro filter selects matching). U5 coverage: `config` mode/first-run
(`mode_defaults_to_friendly_and_first_run_is_true`, TOML round-trip, partial-TOML mode parse). U4
coverage: `PackageSpec::parse` (11 cases — plain/source/ref combos, npm scope safety, last-colon/
last-at split, trimming, structural errors). T5 coverage: `prompt::parse_choice` (empty→default,
`n`/`no`/`q`/`quit`/`cancel`→cancel, in-range number→zero-based pick, out-of-range/garbage→invalid).
T4 coverage: apt (`parse_show` first-stanza deb822,
Description-md5/folded-body excluded, batch install/remove/update-only-upgrade), pacman (`parse_si`
first stanza with URL-in-value intact, `parse_query`, `-Rs` remove, batch), zypper (`parse_search_xml`
skips `<solvable-list>` container, dep-free `attr`, non-interactive root plans), nix (`parse_search`
exact-`pname` over near-names, unprivileged flake install/upgrade). Earlier coverage: homebrew
(formula→candidate, unprivileged
plan, `brew list --versions`, batch), snap (candidate + classic detection, root plan, `--classic`,
batch merge-vs-decline, `snap list`), github `.AppImage` acceptance (no-`linux` token, wrong-arch/
`.zsync` rejection), `info`/`search` rendering helpers
(`recommendation_reasons` source-agnostic rationale, `one_line`, `candidate_line`),
`group_by_source` (first-seen order), batch remove/update merges (dnf root remove+upgrade,
cargo uninstall, flatpak update), dnf/flatpak parsers, ranking,
registry (incl. `record_update` version refresh + `Update` history), cache, privilege
elevation prefixing, the executor (sha256 digest,
verification accept/reject/case-insensitive/fail-closed, place+mode+remove, tar.gz **and
zip** extract + member selection, unknown-format rejection, run_action), github
(owner/repo, release JSON, asset selection incl. `.zip`/tar.gz preference, checksums,
plan shapes), copr (search parsing, exact-name + fedora/arch chroot selection, two-step
root plan), cargo (binary-crate vs library-only candidate filtering, unprivileged plan
shape, `cargo install --list` parsing), npm (CLI vs library-only filter incl. bin-as-
string, user-prefixed plan shape, `npm ls -g --json` parsing), pipx (candidate shape,
install/upgrade plans, `pipx list --json` parsing), go (candidate shape, unprivileged
`go install @latest` plan, binary-name derivation incl. `/v2` major-version skip, proxy
uppercase→`!x` escaping), **batch merge** (dnf/cargo/go `plan_install_many` collapse a
group into one command), audit (verification resolution +
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
- **`cli/mod.rs`** (~1700 lines after U4–U7 — spec parsing, the wizard, Friendly preview,
  doctor Tier 1, `recommend`, system update) holds every command handler inline. It has now well crossed the
  "unwieldy" line flagged in ADR-0024; splitting into `cli/commands/*` (one module per subcommand
  + a shared helpers module) is the next structural cleanup, best done between UX slices so it
  doesn't collide with in-flight feature work.
- **recommend-catalog is hand-curated + Fedora-only (ADR-0033).** The `data/recommend/catalog.toml`
  entries (package names, the RPM Fusion command) are authored by hand and **not verified on a
  clean VM**; verify in the T7 clean-VM pass. Non-Fedora entries are deliberately empty until
  verified on a real host. `manual` (repo-enable) entries are shown, never run — a real repo-enable
  capability (previewable plan) is a follow-up.
- **System update doesn't refresh the registry (ADR-0034).** Bare `jii update` runs each manager's
  bulk upgrade (`dnf upgrade`, `flatpak update`, …), which upgrades packages beyond JII's ledger, so
  those plans are **not** recorded — only the per-record fallbacks (github/cargo/go) refresh the
  registry. Consequence: after a system update, `jii list` may show a stale version for a
  bulk-updated *tracked* dnf/flatpak package. Re-querying every tracked version per update is
  expensive; accepted for MVP. The non-Fedora `plan_update_all` impls are unverified on a live host.
- **pipx/go offer libraries (ADR-0023, by design):** PyPI/Go expose no reliable
  program-vs-library signal, so `pipx`/`go` don't pre-filter (cargo/npm do). They offer
  the package; the tool rejects a non-app at install. Accepted — a visible false positive
  beats silently hiding a real app. Add a filter only if reliable metadata appears.
- **Engine↔UI seam (ADR-0022):** `Engine::install`/`remove` take `&crate::ui::Renderer`
  so the executor can print progress — the one `ui` type reaching into the engine. Fine
  now (single CLI frontend), but it must be decoupled (a progress-event/`ProgressSink`
  trait) **before** a GUI/second frontend or a workspace split. Meanwhile: **do not add
  new `ui` types to engine signatures.**
- **nix provider is untested on a live host + version-fragile (T4).** Implemented against the
  modern flakes CLI; no Nix host was available here. `nix profile` remove/list schemas have
  shifted across Nix versions — `list_installed` returns empty and `is_installed` checks
  `~/.nix-profile/bin/<name>` (go-style; name==binary caveat). Verify search/install/remove/
  upgrade on a real Nix/NixOS box in the T7 clean-VM pass.
- **apt/pacman version = first search stanza (T4).** `apt-cache show`/`pacman -Si` list versions
  highest-first, so the first stanza is taken as the candidate version. It is informational; the
  actual `apt-get`/`pacman` install resolves the real candidate regardless. Fine for MVP.
- **apt non-interactive relies on `-y` only.** No `DEBIAN_FRONTEND=noninteractive` is set (the
  `Action` model runs argv without env); revisit if a package's postinst prompts.

## Where things live

```
src/
  model.rs       core types (Action, InstallPlan, PackageCandidate, TrustLevel…)
  provider/      Provider trait + http_client/get_json_opt/command_plan/run_capture[_lax] +
                 dnf, copr, apt, pacman, zypper, nix, flatpak, snap, github, cargo, npm,
                 pipx, go, homebrew
  engine/        orchestration (search→rank→plan→execute) + ranking.rs;
                 any_source_available() = source-based "supported" (ADR-0029)
  exec.rs        plan executor (the one place that runs a plan's actions)
  privilege.rs   sudo/pkexec elevation (prime + run)
  cache.rs       on-disk TTL search cache (stale-on-error)
  registry.rs    JSON install registry
  recommend.rs   recommend-catalog: typed model + embedded-TOML loader + distro filter
  cli/, ui/, config.rs, platform.rs, error.rs
data/            recommend/catalog.toml — the curated recommend-catalog (embedded at build)
docs/            ARCHITECTURE (canonical) · ROADMAP · TASKS · DECISIONS · this file
AGENTS.md        tool-neutral onboarding entry (read first); CLAUDE.md = Claude's copy
LICENSE          MIT
```

To add a source: implement `Provider` (or a declarative TOML later) — never edit the
core. Use the `/new-provider` skill.
