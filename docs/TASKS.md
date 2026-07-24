# JII — Tasks

Actionable checklist derived from [ROADMAP.md](ROADMAP.md). Check items off as they
land. Keep tasks small enough to complete and verify in one sitting.

---

## Phase 0 — Skeleton 🎯 ✅

- [x] `cargo init` single crate; add deps (clap, tokio, serde, toml, reqwest+rustls,
      anyhow, thiserror, indicatif, owo-colors, directories, async-trait; + chrono, semver).
- [x] `error.rs`: `JiiError` (thiserror) + `Result` alias.
- [x] `model.rs`: `Query`, `QueryKind`, `TrustLevel`, `Health`, `PackageCandidate`,
      `Step`, `Verification`, `InstallPlan`, `InstalledRecord`.
- [x] `platform.rs`: detect distro (Fedora), arch, PATH entries, TTY vs graphical.
- [x] `config.rs`: struct + defaults + TOML load/merge + validation (unknown source id → error).
- [x] `cli/mod.rs`: clap commands + global flags (`-y/-n/--auto/--source/--profile/--dry-run/-v/--json/--no-color`).
- [x] `ui/mod.rs`: renderer facade (respects `--json`, `--no-color`).
- [x] `main.rs`: wire config → engine → cli.
- [x] **Verify:** `jii fastfetch` runs, prints placeholder; `--json` emits JSON; config loads. 8 unit tests pass.

> Notes: crate-level `#![allow(dead_code)]` is set during scaffolding (model/provider
> API defined ahead of use); tighten as later phases consume it. `cargo clippy` is not
> installed on this host (Fedora system Rust) — `cargo build` is warning-clean instead.

## Phase 1 — DNF end-to-end 🎯 ✅

- [x] `provider/mod.rs`: finalize `Provider` trait + provider registry.
- [x] `provider/dnf.rs`: `is_available`, `search`, `plan_install`, `list_installed` (dnf5 machine output).
- [x] Unit tests for the dnf output parser on **fixed sample output**.
- [x] `privilege.rs`: detect sudo/pkexec; batched elevation; print exact commands.
- [x] `engine/mod.rs` (+ `engine/ranking.rs`): `search → rank → plan → execute` (single provider).
- [x] `ui/prompt.rs`: `[Y/n]` default-yes; trust barrier hook.
- [x] `--dry-run` renders the plan and exits without side effects.
- [x] **Verify:** `jii <dnf-pkg> --dry-run` previews; `jii <dnf-pkg>` installs it (verified with a
      real `cowsay` install+remove via pkexec). 19 unit tests pass.

> Notes:
> - Model change: `PkgVersion(String)` replaces `semver::Version` — RPM EVR
>   (`2.63.1-1.fc44`) is not semver, and sources are heterogeneous. Cross-source
>   version comparison lands in Phase 3 where it is needed.
> - Bug fix: TTY detection now uses `std::io::IsTerminal` (the earlier char-device
>   heuristic misclassified piped stdin), so prompts/color behave correctly.
> - `#![allow(dead_code)]` narrowed from crate-wide to `model.rs` plus a few
>   targeted, phase-labelled items; cli/engine/ui/config/privilege are allow-free.

## Phase 2 — State, remove, why 🎯 ✅

- [x] `registry.rs`: JSON store under `~/.local/state/jii/state.json`; load/save; write
      **only on success**; install/remove history log (+ 4 unit tests).
- [x] Verification: `Engine::resolve_installed` uses the registry as a hint but verifies
      against `dnf repoquery --installed`; falls back to scanning providers when stale.
- [x] `remove`: resolve source → `plan_remove` → confirm → execute → record.
- [x] `list`, `why`, `history` (added a `History` command to the CLI surface).
- [x] **Verify:** install → `list`/`why`/`history` reflect it → `remove` uses the recorded
      source; scan-fallback and "not installed" paths checked. 23 unit tests pass.

> Note: `list`/`why`/`history`/`remove` are implemented as methods on `Cli` in
> `cli/mod.rs` rather than separate `cli/commands/*.rs` files — the command surface is
> still small. Split into per-command modules if `cli/mod.rs` grows unwieldy.

## Phase 3 — Multiple sources & ranking 🎯 ✅

- [x] `provider/flatpak.rs` (`--columns` machine output; best-match selector; installs
      via Flatpak's own polkit, so `needs_root=false` — no JII sudo/pkexec).
- [x] `engine/ranking.rs`: source priority + trust tie-breaker + profile adjustment;
      CLI prints the recommendation plus an "also available" list (+ ranking tests).
- [x] Parallel fan-out with per-source timeouts; failed source tagged (e.g. `timeout`).
- [x] `cache.rs`: TTL cache + stale-on-error (search results per source/query).
- [x] `jii doctor`: availability, latency, health.
- [x] Unit tests for ranking + flatpak parser/best-match + shared installed parser.
- [x] **Verify:** gimp → dnf recommended + flatpak alternative; `--source`/`--profile`
      honored; warm cache ~4 ms. 31 unit tests pass.
- [~] `provider/copr.rs` — **moved to Phase 4** (see note).

> Notes:
> - **COPR deferred to Phase 4:** `dnf5 copr` has no search; resolving which COPR
>   provides a package needs the COPR web API — the same fuzzy name→project problem as
>   GitHub, plus root repo-enable + trust. Grouped with GitHub in Phase 4.
> - **Flatpak identified by appid** (`org.gimp.GIMP`): `jii remove org.gimp.GIMP`
>   works, but `jii remove gimp` for a Flatpak may not resolve. Documented tech debt —
>   revisit with a name/id split if it becomes painful.
> - **`latest`/`minimal` profiles + freshness/health ranking tie-breakers** are
>   reserved: they need comparable versions / dependency-footprint data not yet collected.
> - `cli.rs` is ~410 lines; still readable. Split into `cli/commands/*` if it grows.

## Phase 4 — GitHub Releases, COPR & trust 🎯

- [x] **Execution model evolution** (prerequisite): replace argv-only `Step` with an
      `Action` enum (`RunCommand`/`Download`/`Place`/`RemoveFile`) + a plan executor
      (`exec.rs`) that dispatches each action to a focused handler. Download enforces
      verification (sha256; gpg/sigstore fail closed). `privilege.rs` reduced to
      `prime()`+`run()`. DNF/Flatpak unchanged. See [DECISIONS.md](DECISIONS.md) ADR-0007.
- [x] `provider/github.rs` (raw-binary slice): `owner/repo` → latest release, arch/OS
      asset filter (musl-preferred), sha256 from a checksums asset, `Download`+`Place`
      into `~/.local/bin` (no root), `GITHUB_TOKEN` support. All network in `search`;
      `plan_install` pure. Trust `untrusted` (always confirmed). See DECISIONS ADR-0014.
- [x] Artifact verification: **sha256 enforced** in the executor; `⚠ unverified` shown
      when no checksum is published. GPG / sigstore still fail-closed (later slice).
- [x] Trust enforcement: `untrusted` always confirmed, even with `--auto`
      (barrier in `ui/prompt.rs`; verified — `--auto` on github aborts non-interactively).
- [x] `GITHUB_TOKEN` support to lift rate limits. Rate-limit health in `doctor`: **done** (below).
- [x] `jii remove` for file-based (github) installs: `Provider::is_installed(record)`
      (default = list lookup; github overrides to check `~/.local/bin/<name>` exists),
      so `resolve_installed` confirms file-based installs without a manifest. No core
      branching, no new record field. Verified: real jq install→remove cycle.
- [x] **`Action::Extract` + `.tar.gz` release assets** (exec.rs): download+verify the
      archive, then extract the binary (found by name, else the sole executable file)
      into `~/.local/bin`. github selects tarballs (raw binary still preferred when
      both exist). Verified: real `sharkdp/fd` install→run→remove. See ADR-0016.
- [x] **`.zip` release assets** (exec.rs): `extract` dispatches on the archive's
      file-name extension into `read_tar_gz` / `read_zip` (both yield `ArchiveFile`, so
      selection + writing stay format-agnostic); github `classify` gained `AssetKind::Zip`
      (ranked below `TarGz`), and now rejects delta-patch assets (`.bsdiff`/`.patch`/…)
      that masqueraded as raw binaries (surfaced by `denoland/deno`). Verified: real-release
      dry-run picks `deno-…-linux-gnu.zip` → Extract; zip round-trip unit-tested. ADR-0016 update.
- [ ] **github follow-ups (deferred):** `.tar.xz` (needs an xz decoder dep); broad
      name→repo resolution + release pagination — reframed as the interactive **GitHub
      repository selection** future idea (never silently install the wrong repo).
- [x] `provider/copr.rs`: COPR API `project/search` → exact project-name match that
      builds for the host Fedora/arch (prefer most chroots); two-step root plan
      (`dnf5 -y copr enable owner/project` → `dnf5 -y install <name>`); community trust;
      `is_installed` via rpm. Integrates through ranking, no engine special-case.
      Verified via real API search + `--dry-run` (privileged install not run — system
      change). See ADR-0017.
- [x] `jii audit`: per installed record show source, trust, verification and concerns
      (human table + `--json`). Verification is recorded at install time on
      `InstalledRecord` (from the plan's Download step; `None` = manager-verified).
      Engine owns the logic (`audit()`), CLI renders. See ADR-0018.
- [x] Rate-limit / reachability health in `doctor` (GitHub, COPR): `Provider::probe()`
      reports raw facts (`reachable`, `rate_limited`, `detail`); the engine maps them via
      pure `health_from()` (Offline → RateLimited → Slow → Healthy). github probes
      `/rate_limit` (shows `remaining/limit`, flags `RateLimited` at 0), copr pings
      `project/search`. `detail` shown in human + `--json`. Verified live (github
      `58/60 req left` healthy, copr reachable-but-slow). See ADR-0019.
- [x] **Verify:** installing a GitHub release verifies the artifact & respects trust —
      real `jqlang/jq` install in an isolated HOME: sha256 matched, binary runs, registry
      recorded; `--dry-run`/`-n`/`--auto` paths checked. 49 unit tests pass.

## Phase 5 — User-space sources & update 🔭

> **Readiness (ADR-0022):** the pre-Phase-5 architecture re-evaluation confirmed the
> model needs **no change** — cargo/npm/pipx/go are pure new `Provider`s (user-space, no
> root), same shape as github. New *capabilities* (versions, metadata, bootstrap) go in
> as **optional trait methods with defaults**, never a fat trait or core branch. Keep the
> engine **UI-free** (no new `ui` types in engine signatures).

- [x] `provider/cargo.rs`: `is_available` (cargo present), `search` (crates.io
      `crates/{name}` API — **only offers crates that ship a binary**; library-only
      crates like `serde` yield no candidate), `plan_install` = `RunCommand cargo install
      <crate>` (`needs_root=false`), `list_installed` (`cargo install --list` parser),
      `plan_remove` (`cargo uninstall`), `plan_update` (reinstall newest), community trust.
      No core change, no engine special-case (registered in `provider/mod.rs` like the
      rest). Verified: real crates.io search via JII (ripgrep offered v15.1.0, serde
      rejected as library-only), dry-run plan (single unprivileged command), multi-source
      ranking (dnf recommended, cargo shown as alternative), 5 unit tests. A from-source
      `cargo install` compile was not run (disproportionate; the unprivileged `RunCommand`
      path is already covered by dnf/copr/flatpak/exec tests) — same precedent as COPR.
- [x] `provider/npm.rs`: mirrors cargo. `search` (npm registry `/<pkg>/latest` — only
      offers packages with a non-empty `bin`; library-only like `lodash` yields nothing),
      `plan_install`/`remove`/`update` = single unprivileged `npm … --global --prefix
      $HOME/.local <pkg>` (forces the user prefix so it never needs root, binaries →
      `~/.local/bin`), `list_installed` (`npm ls -g --json`, tolerant of npm's benign
      non-zero exits), community trust. No core change. Verified: real registry search via
      JII (prettier offered v3.9.4, lodash rejected), dry-run, multi-source ranking. 6 tests.
- [x] **Shared `provider::http_client()`** extracted (was copied 3× in copr/github/cargo;
      npm would be the 4th) — one place for the registry User-Agent / transport policy.
- [x] `provider/pipx.rs`: mirrors cargo (PyPI `/<pkg>/json`, `pipx install`/`uninstall`,
      first-class `pipx upgrade`, `pipx list --json`, installs to `~/.local/bin`, no root,
      community trust). **No app-filter** — PyPI exposes no reliable program signal (the
      `Environment :: Console` classifier is ~40% unreliable, measured), so it offers the
      package and lets `pipx install` reject non-apps (ADR-0023: prefer a visible false
      positive over silently hiding a real app). Verified: real PyPI search via JII
      (black + requests both offered), dry-run. 4 tests.
- [x] `provider/go.rs` (no root; `go install <mod>@latest`, `~/go/bin`/`$GOBIN`/`$GOPATH`).
      Search via the Go module proxy (`{proxy}/<mod>/@latest`, uppercase→`!x` escaping); no
      app-filter (only `main` packages install; the proxy can't cheaply tell — ADR-0023).
      `plan_remove` deletes the installed binary (Go has no uninstall, like github);
      `list_installed` empty (no cheap module→binary list; registry + file-existence
      `is_installed` track it); `is_available` uses `go version` (not `--version`, which
      exits non-zero). Community trust. Verified: real proxy search (fzf→v0.73.1;
      BurntSushi/toml resolves via `!burnt!sushi`), dry-run (one unprivileged command), 4
      unit tests. Registered in `provider/mod.rs`; no engine/model change (ADR-0022 holds).
- [x] **Helper evaluation (done at `go`, the 4th user-space provider):** the search
      404-dance and the one-`RunCommand` plan had each reached 3× (→ 4× with go). Extracted
      `provider::get_json_opt` (GET → `Ok(None)` on 404, else typed JSON — replaces cargo/
      npm/pipx/go's `error_for_status`+`json` dance) and `provider::command_plan(source_id,
      name, argv, needs_root, reasons)` (each provider assembles its own argv; also absorbs
      dnf's `root_plan` and npm's `--prefix` argv). Commit `f2e8377`. Deliberately did **not**
      extract `PackageCandidate` construction (per-provider, would leak trust/arch_ok) or the
      tolerant "read stdout regardless of exit status" spawn (only npm + pipx = 2×, go didn't
      need it). Reduced maintenance cost, not line count. No new model (ADR-0022).
- [x] **`jii update [<pkg>]`** (in `cli/mod.rs`, not a separate file — the handler is
      thin). One package or every registry record → for each, re-search the **owning**
      source (normal search→rank path, filtered by `source_id`) for the latest version,
      skip provably-current ones (exact version equality → clean no-op), then run the
      provider's `plan_update` through the same preview → confirm (one batch prompt) →
      execute pipeline as install/remove. No per-source branching (engine resolves the
      provider). Engine gained `plan_update`/`update`; registry gained `record_update`
      (logs history `Update`, refreshes the stored version) sharing an `upsert` helper
      with `record_install`. Verified: end-to-end `--dry-run` (go: v0.60.0→v0.73.1
      transition + plan), no-op path ("already up to date"), missing-package error.
- [x] **Batch install** — `jii install a b c …` (and bare `jii a b c`). Each package runs
      the normal search→rank→pick; the engine then groups the chosen candidates by source
      and **merges same-source installs into one command** where the source can
      (`dnf/cargo/npm/go install a b c`), via a new optional `Provider::plan_install_many`
      (default `None` → per-candidate fallback; ADR-0025). One grouped preview, one
      trust-governed confirmation (least-trusted candidate rules), one root escalation, one
      run; records written as each plan succeeds. A not-found package is reported and does
      not cancel the rest (offer to continue). Single install is now a batch of one (old
      `Engine::install`/`plan_install` wrapper removed — one write-path). Executor split
      into `prime_for` + `run_actions`. Bootstrap-a-missing-manager **deferred** (needs the
      manager-install feature). Verified: dnf/cargo merges, mixed sources, not-found
      continue, single-package UX unchanged. 99 tests.
- [ ] `cli/commands/{undo,benchmark}.rs`.

## Phase 6 — Declarative sources & catalog 🔭

- [ ] `provider/declarative.rs` + `data/sources/*.toml` loader.
- [ ] `data/catalog.toml` aliases (`vscode→code`, `node→nodejs`, `chrome→google-chrome`).
- [ ] Full-text metadata search (Stage 3) + fuzzy name search (Stage 2).

## Phase 7 — Hardening 🔭

- [ ] Integration tests (dry-run flows).
- [ ] Error-message quality pass (actionable hints).
- [ ] `--json` output schema stability.
- [ ] Distribution: COPR repo + signed GitHub binary; self-install docs.

## Terminal 1.0 — CLI completion plan 🎯 (ADR-0026)

Finish the whole terminal version before the first public Beta. Ordered T1→T8.

- [x] **T1 — Read-only honesty layer**: `jii search`, `jii info`, `jii sources`. Pure
      rendering over the engine's existing `search`/`rank` — no new architecture. Engine
      gained `source_catalog()` (enabled providers + trust + live availability). `search`
      lists ranked candidates (top marked `→`); `info` lists sources + recommendation with a
      **source-agnostic** rationale (`recommendation_reasons`, no branching on source id);
      `sources` groups active vs enabled-but-unavailable. Summaries collapsed to one line
      (`one_line`). README de-lied (`config` removed; search/info/sources documented as real).
      Verified live on Fedora (dnf/cargo/npm ranked for ripgrep; pipx shown unavailable).
      104 tests.
- [x] **T2 — Batch symmetry**: `jii update a b c`, `jii remove a b c` (and `jii update`
      = all). Optional `plan_remove_many`/`plan_update_many` (dnf/copr/flatpak/cargo/npm +
      go-update; go-remove/github/pipx inherit `None` → per-record fallback). Engine gained
      a generic `group_by_source`, `RecordOp`, `plan_record_batch` (→ `RecordBatch { plans,
      unplannable }` — an un-updatable package is reported, not fatal), and
      `remove_batch`/`update_batch` mirroring `install_batch` (prime once, run in order,
      record as each succeeds). CLI `Remove`/`Update` widened to `Vec`; update carries the
      **post-update** record (version = refreshed target), engine stamps installed_at/
      verification. Single = batch of one (old `Engine::remove`/`update`/`plan_remove`/
      `plan_update` + `exec::run_plan` removed — one write-path). Verified via isolated
      `XDG_STATE_HOME` dry-runs: merged `dnf5 remove/upgrade -y a b`, mixed dnf+cargo
      grouping, version transitions, single-package richer plan. **109 tests.**
- [x] **T3 — Provider breadth:** Homebrew ✅ → Snap ✅ → AppImage ✅ (as a github asset kind).
      - [x] **Homebrew** (`brew`, Linuxbrew): formula API (`formulae.brew.sh/api/formula/<n>.json`)
            via `get_json_opt`; unprivileged `brew install/uninstall/upgrade` (+ `_many`);
            `brew list --versions`; community trust; no library filter (ADR-0023). Registered
            in config (`KNOWN_SOURCES` + default priority). **Empirical scaffold verdict
            (ADR-0027): NO shared `RegistryProvider`** — after 5 providers the identical part is
            ~8 lines of boilerplate; search/plans/list are irreducibly per-provider; the real
            sharing already lives in the free-function helpers. 115 tests.
      - [x] **Snap** (`snap`, snapd): **system** provider (root, unlike registry ones) —
            store info API (`api.snapcraft.io/v2/snaps/info/<n>?fields=…`, needs
            `Snap-Device-Series` header, so `http_client` directly). `snap install/remove/
            refresh` (+ `_many`). Detects **classic** confinement → adds `--classic` and
            declines batch-merge when mixed (verified against the live API: `confinement`
            is only returned as an explicit `fields` item). `snap list` parse; community
            trust. Registered in config. 121 tests.
      - [x] **AppImage** — **not a standalone provider (ADR-0028)**: it has no manager/API and
            its catalog has no download URLs. It is a *delivery format over GitHub releases*, so
            `github::classify` now accepts `.AppImage` assets as raw binaries without the `linux`
            token (arch still required; `.AppImage.zsync` rejected). `jii owner/repo` installs an
            AppImage today; by-name discovery folds into T5 (repo chooser). Reserved `"appimage"`
            id removed from `KNOWN_SOURCES`. 123 tests.
- [x] **T4 — Cross-distro system providers:** `apt.rs` ✅, `pacman.rs` ✅, `zypper.rs` ✅,
      `nix.rs` ✅ behind the relaxed platform seam. **ADR-0029 enacted** (Accepted): removed
      `Platform::is_supported`/`require_supported` + `JiiError::UnsupportedPlatform`; `Platform`
      is now pure host facts; "supported" = `Engine::any_source_available` (≥1 usable source),
      guarded at the 5 CLI entry points via `ensure_usable_source`. Providers self-gate on their
      **binary** (no distro branch, ADR-0029 showed a distro-aware `is_available` was unneeded).
      Fedora behaviour verified unchanged. Shared `run_capture_lax` (stdout even on non-zero
      exit: apt-cache 100 / pacman 1 / zypper 104 = "no candidate"). apt=deb822 `apt-cache show`,
      pacman=`pacman -Si`, zypper=`--xmlout search` (dep-free attr parse) + `rpm -qa`, nix=modern
      flakes CLI (`nix search --json` / `nix profile`, user-space, no root). **Debt:** nix
      profile CLI is version-fragile and untested on a live Nix host — revisit in T7.
- [~] **T5 — Interactive choosers:**
      - [x] **Candidate chooser** (`ui/prompt::choose`): a single interactive install with
            more than one candidate now presents a numbered source menu (recommendation
            pre-selected as the default — Enter installs it) instead of silently taking the
            top rank; picking a source is itself the consent, so a trusted pick skips the
            redundant `[Y/n]` (an untrusted pick still hits the trust barrier, ADR-0006).
            Batch installs, `--source`/`--auto`/`--yes`/`--no`, non-TTY and `--json` all
            skip the chooser (they already express intent / can't prompt). **No ADR / no
            engine change:** the multi-candidate model already existed (`search` returns
            `Vec`, engine ranks the lot since Phase 3) — the chooser is pure `cli`/`ui` over
            the existing ranked list. Pure `parse_choice` unit-tested; the three live paths
            (Enter→dnf, `2`→cargo, `n`→abort) + `--auto`-bypass verified on a real pty. 150 tests.
      - [~] **GitHub repository chooser** — by-name repo discovery (github `/search/repositories`)
            feeding the candidate chooser. **Design done, implementation DEFERRED** (ADR-0030
            Proposed/deferred): after clean-VM dogfooding the user re-prioritised to a UX-polish
            pass (no new features). Resume after the UX pass.
      - [ ] **Version chooser** — `--version`; optional `Provider::available_versions`,
            provider-ordered. Own ADR for the version growth (per ADR-0026). **Paused** (same
            re-prioritisation).
- [x] **UX-polish pass (COMPLETE 2026-07-06):** real-use feedback from a clean Fedora VM →
      16 UX problems + a first-run wizard, evaluated in **[UX_EVALUATION.md](UX_EVALUATION.md)**
      (10 pure polish over existing seams, 6 need small optional-capability/UI designs). Delivery
      order U0–U8. Doctor scope decided: **Tier 1 + recommend-catalog, both in 1.0** (own catalog
      ADR). Folds in much of T7 (error quality) and T8 (first-impression).
      - [x] **U0** measured; **U1** unavailable-spam killed + single-pkg preview de-duped; **U2**
            search timeout 8→5s (streaming search is the better long-term fix, UX_EVALUATION §A);
            **U3** already-installed pre-check + multi-owner remove.
      - [x] **CLI grammar LOCKED (ADR-0031):** the package spec **`name[:source][@ref]`** is the
            language of JII — source/version/channel belong to the *spec*, not flags; `@ref` is
            source-interpreted (ADR-0004/0009). Durable principle: *"package or command?"* — package
            attributes extend `PackageSpec`, never a new flag. Implemented in **U4** (pure
            `PackageSpec::parse`, clap untouched; `:source` suppresses the chooser — one clause;
            `@ref` parsed but explicitly rejected until the version chooser lands). `--auto`→`-y`,
            `--profile`→config, `--source` demoted to whole-command synonym *(flag-shed still TODO)*.
      - [x] **U4** chooser presentation (#4) + **D5** source-supplied recommendation highlights +
            the `PackageSpec` grammar (ADR-0031), universal across install/remove/update/info.
      - [x] **U5** Friendly/Advanced verbosity (D8: `OutputMode`, `-v` forces Advanced, Friendly hides
            secondary-source noise + collapses the install preview) + first-run wizard/`jii setup`
            (DW: `Config::save`, `first_run_completed`) + a clap fix (global flag before a subcommand
            no longer misparses as install). 165 tests green.
      - [x] **U6** actionable errors (D7, ADR-0032: pure `JiiError::remedy()`) + doctor Tier 1
            (system checks: `~/.local/bin` on PATH, `GITHUB_TOKEN`) + Tier 2 recommend-catalog
            (D6, ADR-0033: `data/recommend/catalog.toml` embedded, distro-filtered; `jii recommend`
            lists, `jii recommend <id>` applies via the normal install path; `manual` repo-enables
            shown not run). 175 tests. Fedora catalog entries unverified on a clean VM (T7 debt).
      - [x] **U7** system-wide `update` (D10, ADR-0034): optional `Provider::plan_update_all`
            (default None); bare `jii update` aggregates every manager's bulk upgrade + per-record
            fallback for sources without one (github/cargo/go). Implemented for all bulk managers
            (dnf/flatpak/apt/pacman/zypper/snap/nix/brew/pipx/npm). Fedora-verified; non-Fedora
            impls unverified (T7). 177 tests.
      - [x] **U8** first-run walkthrough polish (final track). Played the whole CLI as a new user and
            fixed the awkward edges (#15): aligned, headered ledger tables for `list`/`history`/`audit`
            (one data-driven `table_lines` helper; `Action::label()` humanises history verbs) and
            `jii update <not-installed>` no longer follows the `Not installed` error with a misleading
            `Nothing installed yet` ledger claim. 180 tests, Fedora-verified (incl. pty first-run
            wizard replay).
      - **Deferred follow-ups (not blocking 1.0):** **streaming/progressive search** (UX_EVALUATION §A,
            own ADR) — the real fix for perceived speed; lets the timeout be raised again. Structural
            cleanup queued: **split `cli/mod.rs`** (~1800 lines) into `cli/commands/*`. Recommend
            follow-ups: interactive multi-pick, skip already-satisfied entries, a real repo-enable
            capability. Friendly install: "Also available" prints just before the recommendation line
            (mildly backwards; reordering would restructure the preview flow). Update debt: a
            bulk-updated tracked package can show a stale version in `jii list`.
- [x] **UX-WAVE 2 — clean-VM feedback (COMPLETE 2026-07-09, owner-set 2026-07-06).** Preceded cutting
      Beta by owner decision; product/UX polish over architecture. Agreed order **①→②→③→④** with
      `recommend` folding into doctor and `list`+`audit` merging — **all landed and pushed.** See
      AI_CONTEXT "Current task". Next: Beta prep resumes (BETA_ROADMAP.md).
      - [x] **① arrow-key TUI choosers** (`dialoguer` Select — setup + source chooser + multi-owner
            remove; ↑↓/Enter/Esc; pty-verified). Plus #3/#10/#12/#13 crisp-output polish, #6 `-d`
            alias. #11 already shipped in U7.
      - [x] **#9 helpful "it's a library" message** (2026-07-09, follow-up to ADR-0023). Optional
            `Provider::explain_miss` (ADR-0022 growth); cargo/npm explain that a bin-less name (`serde`,
            `lodash`) is a library, not a program. Engine asks only on a total miss, gated on `is_available`;
            shown under the miss in install/info/search. 185 tests, live-verified.
      - [x] **② doctor-as-system-helper.** (all three slices landed — ADR-0035)
            - [x] *Slice 1 — read-only diagnostics.* Added checks: internet reachability (critical),
                  git/curl presence (advice → `jii git`/`jii curl`), `~/.cargo/bin` on PATH
                  (conditional on cargo), Flathub remote (conditional on Flatpak). Concurrent
                  `gather_system_facts`; pure unit-tested `system_checks(&SystemFacts)`; summary line.
                  180 tests, clippy clean, live-verified.
            - [x] *Slice 2 — `--fix`.* `jii doctor --fix` offers the fixable checks: git/curl route
                  through the normal install path (previews + confirms itself); the Flathub remote is
                  a plain command shown before it runs (Flatpak elevates via its own polkit — no JII
                  root). `--dry-run` previews every fix without asking or changing anything; plain
                  `jii doctor` nudges "run --fix" only when something is fixable. 183 tests.
            - [x] *Slice 3 — fold recommend into doctor* (ADR-0035). Catalog now shows at `doctor`'s
                  tail as a compact "Suggestions for your system" section (title — why · command);
                  standalone `jii recommend` + apply-by-id **removed** — apply by running the shown
                  command. `Recommendation.id` dropped from runtime (uniqueness moved to `title`).
                  **② doctor complete.** 183 tests.
            - [x] *Slice 4 — doctor becomes an interactive setup questionnaire* (ADR-0041). Reverses
                  the read-only/advice stance at the owner's request: bare `jii doctor` now asks a
                  yes/no per actionable item (fixable checks + catalog suggestions: RPM Fusion, codecs,
                  fonts, VLC, PATH…) and applies on "yes". New `Fix::PathExport` edits the shell rc for
                  `~/.local/bin`/`~/.cargo/bin`; `install`→`install_inner(assume_yes)` +
                  `PromptFlags::with_yes` so one question is the consent (trust barrier still holds).
                  Read-only under `--json`/`-n`/no-TTY; `--fix` kept as a hidden no-op. 194 tests.
      - [x] **Smart search matching** (ADR-0042). Exact-first, broaden only on a miss: prefix search
            (`ayugram` → `ayugram-desktop`) + a ≤2-char trailing-trim typo fallback (`ayugramm` still
            resolves); shows "No exact match — closest: …" then the normal confirm (the "did you mean").
            `MatchMode` on `Query` (dnf appends `*`, others rely on native breadth); name-aware
            `rank(config, query, cands)` (exact>prefix>substring tier, then priority/trust/shorter-name);
            `Engine::broaden_search`; capped, name-showing "Also available". Exact queries stay
            noise-free (avoided `*git*` = ~1300 hits). 197 tests, live-verified on Fedora (ayugram-desktop).
      - [x] **③ providers/marketplace** (ADR-0036). `jii providers` lists the ecosystem managers
            (npm/cargo/brew/flatpak/snap/pipx/go/nix) with installed-vs-available status; `jii providers
            add <name>` bootstraps a missing one. Ecosystem-ness is optional `Provider::ecosystem`
            metadata (ADR-0022 growth); `Bootstrap::Packages(&[…])` is an ordered cross-distro candidate
            list resolved by `Engine::first_available_package` → the normal install path; `Bootstrap::Script`
            (brew/nix) is shown, never run (trust boundary). 184 tests, live-verified on Fedora.
            **Note:** install/remove/update of a manager beyond `add` (e.g. `providers remove`) deferred
            as a follow-up to keep the slice small.
      - [x] **④ info app-card** (ADR-0037). `jii info` is now a card: name → description → aligned
            metadata block (Source/Version/License/Homepage/Repository/Author, present fields only) →
            source list + recommendation. Optional `async Provider::describe -> Option<PackageInfo>`
            (ADR-0022 growth), called only for the recommended candidate on `info`. dnf implements it
            fully (`dnf5 info` + pure tested `parse_info`); github gives a cheap repo+author card; other
            sources degrade gracefully. `--json` → `{candidates, recommended, info}`. 185 tests.
            **Follow-ups:** richer cargo/npm/flatpak cards + GitHub repo-metadata fetch (description/license).
      - [x] **list + audit merge** (ADR-0038). `jii list` gained `--audit`: bare = plain
            NAME/SOURCE/VERSION; `--audit` = the security view (trust/verification/concerns). Standalone
            `jii audit` **removed** (rendering → private `audit_view`; engine `audit()` + model untouched).
            185 tests, live-verified.
      - **Note:** this unfreezes `doctor --fix` (was in the frozen backlog) by explicit owner
        reprioritisation; the rest of the freeze list stays frozen.
- [x] **POST-TESTING UX WAVE — 10-point owner audit (COMPLETE 2026-07-10).** After more dogfooding the
      owner filed 10 points (analyse → architecture → implement). All landed as ADRs, each green + pushed:
      - [x] **#1 doctor analyses system state** (ADR-0043). `Engine::installed_index` gathers installed once;
            doctor drops already-satisfied suggestions (no offering an installed VLC). Catalog `check` field
            for non-obvious identities (Flatpak app-id, repo-release pkg).
      - [x] **#2 search speed** (ADR-0044). Per-source failure **circuit breaker** in the disk cache
            (`network.failure_cooldown_secs`, default 120) — a timing-out source (COPR) is skipped. ~5s→~1.1s.
      - [x] **#3 nix `list_installed`** (ADR-0047). Tolerant `nix profile list --json` parser (map+array
            schemas). *Fixture-tested only — needs a real Nix host to confirm live JSON.*
      - [x] **#4 bare manager name → bootstrap** (ADR-0046). `jii npm` installs the npm *manager*, not a
            package called npm; `Engine::ecosystem_ids` + `install_inner` `route_managers` loop guard.
      - [x] **#5 library messaging** (folded into ADR-0045). `serde`/`lodash` explain "it's a library, not a
            program" — philosophy KEPT (programs, not libraries), only the wording clarified.
      - [x] **#6 info ≠ install** (ADR-0045). `Provider::reference`/`Engine::reference` + `Reference` model;
            `jii info lodash` shows a real card + library note, using no install logic.
      - [x] **#7 localization** (ADR-0050). **Zero user-facing string literals left in Rust code** — all in
            `locales/en.toml`/`ru.toml`, `t!("key", arg=…)`, en fallback, parity test, `--lang`. `label()`
            kept as stable JSON id; parallel localized `display()`. `#[error]` prefixes stay English by
            design. 216 tests, live-verified in Russian across every command.
      - [x] **#8 forge abstraction** (ADR-0049). `Forge` trait + generic `ForgeProvider`; GitHub is
            `GithubForge`, one forge among peers. Adding Codeberg/Gitea/GitLab = implement `Forge`.
      - [x] **#9 principle** (ADR-0048). "JII cooperates with the system; it is not the centre of the world."
      - [x] **#10 release test plan** (docs/RELEASE_TESTPLAN.md). Manual per-release checklist.
      - [x] **First-run setup before ANY command** (ADR-0051). The onboarding wizard now greets the first
            *interactive* use of JII for any task (`jii fastfetch`), announces which command runs after the
            optional setup, then continues with it. pty-verified. 216 tests.
      - [x] **Colour + mouse polish** (ADR-0052). Semantic `Palette` (source cyan, trust green/yellow/red,
            versions/secondary dimmed, ✓/→ green, bold headings/table headers), colour-gated + alignment-safe.
            Candidate chooser reimplemented on **crossterm** with **mouse** (hover/click/scroll) + keys;
            dialoguer dropped. 216 tests, pty + pipe verified.
      - [x] **GitHub by-name search** (ADR-0053). A bare name that misses normal sources opens an
            interactive **repo picker** (GitHub `/search/repositories` → `owner/repo — desc ★stars`, a
            "↓ Show more" that pages forever); picking resolves the release into the normal install flow
            (untrusted → confirmed). Optional forge capability, no core source-branch. 218 tests, pty+pipe
            verified.
      - [x] **Typo tolerance for GitHub search** (ADR-0053). On top of GitHub's own fuzzy matching, a
            verbatim miss retries cheap edit-distance-1 variants (`cli::typo_variants`: deletions then
            adjacent transpositions) and adopts the first that hits, paging the corrected term and telling
            the user. `exeteragram` → `exteragram` recovers locally. 219 tests, pty-verified.
      - [x] **Setup GitHub-token help.** Setup wizard now explains the token (rate-limit benefit + create +
            export), mitigating the tighter search rate limit.
      - [x] **Void (XBPS) provider** (ADR-0054) — first step of the cross-platform program. `provider/void.rs`,
            id `void`, Official (RSA-signed repos), self-gates on `xbps-install`. Exact-name search
            (`xbps-query -R`, like `pacman -Si`), root plans (`xbps-install -Sy` / `xbps-remove -Ry` /
            `xbps-install -Suy`), `xbps-query -l` list, full `_many` + `plan_update_all`. Pure `split_pkgver`.
            No core source-branch. 228 tests. Fixture-tested only — unverified on a live Void host (T7).
      - [x] **doctor enables a required repo before its dependents** (ADR-0055). Field report: skipping
            RPM Fusion in `jii doctor`, then accepting codecs/VLC, gave a bare "not found". Data-driven fix:
            `Recommendation.requires` (+ reads `id`); codecs & VLC `requires = "rpmfusion"`; pure
            `recommend::prerequisite(...)`; `doctor_offer` enables the prerequisite (command shown, dry-run
            honoured, parent consent) before the dependent. 233 tests. Follow-ups: direct `jii <pkg>` doesn't
            resolve prerequisites yet; openh264 needs the Cisco repo; VLC "hang" not reproduced.
      - **Cross-platform expansion (ADR-0054, ACTIVE):** grow past Fedora-first. Risk-ordered:
            **Void ✅ → declarative-Nix Etap A ✅ → Gentoo ✅ → … → Windows/macOS (separate epic).**
            - [x] **Declarative Nix — Etap A (snippet-first)** (ADR-0054). Optional
                  `Provider::install_strategies` + model `InstallStrategy`/`StrategyKind`; engine dispatch;
                  CLI chooser for a single-package interactive install. Nix detects existing config files
                  (NixOS `configuration.nix`→`environment.systemPackages`; home-manager `home.nix`→`home.packages`)
                  and offers only those + imperative `nix profile`, each with a hint; no config → no menu.
                  A declarative pick **SHOWS** the snippet + file + apply cmd + backup note and **installs
                  nothing** ("show, never run"). Pure detection/snippet/guidance unit-tested; menu→print path
                  pty-verified. 231 tests. New `[nix]` locale section (en+ru).
            - [x] **Declarative Nix — Etap B (parser-driven auto-edit)** (ADR-0056). A user-owned
                  home-manager `home.nix` is now **actually edited**. `StrategyKind::EditFile{path,new_content,
                  diff,apply}`; provider parses with `rnix` 0.14 (rowan CST) and **splices the package into the
                  original source bytes** (no reflow) — mirrors style (multi-line/inline/empty), preserves
                  comments, detects already-present, returns `NotFound` → Etap A snippet fallback (attr absent /
                  not a plain list / unparseable). Root-owned `configuration.nix` stays snippet-only → **no
                  escalation** (one user file). CLI: diff → confirm (`--yes/--no/--auto`; `--dry-run` never
                  writes) → back up `<path>.jii-bak` → write → print `home-manager switch`.
                  `insert_package`/`find_list`/`line_diff`/`write_nix_config` unit-tested. 253 tests. New
                  `[nix]` `edit_*` locale keys (en+ru). **T7:** full menu→edit→apply flow unverified on a live
                  home-manager host. **Follow-up remaining:** `configuration.nix` auto-edit (privileged).
            - [x] **Declarative install preference + batch/scripted routing** (ADR-0057). Closes the
                  ADR-0056 "wire the edit into non-interactive/batch installs" follow-up. `[install]
                  prefer_declarative = ask|always|never` (`config::DeclarativePref`, default `ask`) + per-run
                  `--nix-config`/`--nix-imperative` (mutually exclusive). `ask` = unchanged single menu (batch
                  stays imperative, no prompt-storm); `never` = always imperative; **`always` routes every
                  candidate with an auto-editable `EditFile` into the config edit — single, batch *or*
                  scripted** (each diff→`.jii-bak`→write; snippet for root-owned `Manual`), non-Nix/no-config
                  falls through to imperative. Source-agnostic (`[install]`, no core branch). Shared
                  `apply_edit_file` guarantees `--dry-run` writes nothing. `declarative_pref` + dry-run
                  no-write unit-tested; flag conflict + non-Nix no-op verified live. 255 tests. New
                  `nix.edit_dry_run` locale key (en+ru).
            - [x] **Declarative Nix — Etap C (privileged `configuration.nix` auto-edit)** (ADR-0058).
                  Closes the last ADR-0056 follow-up. `strategy_for_target` now emits an `EditFile` for
                  **any** readable/parseable config (not just home-manager); `StrategyKind::EditFile` gains
                  `needs_root` (`= !home`), unreadable/unparseable still → `Manual`. CLI `apply_edit_file`
                  branches on the flag, not the source: user file → `write_nix_config` (direct); root file
                  → `write_nix_config_root` — stage `new_content` in an `O_EXCL` temp, then two **explicit**
                  elevated `cp` commands via `privilege.rs` (`cp -a -- <dest> <dest>.jii-bak`, then
                  `cp -- <tmp> <dest>`), `prime`d once, argv **printed first**; `--dry-run` shows them,
                  writes/stages nothing. JII never fully root — only the two `cp`s escalate. 259 tests
                  (+4: `needs_root` class, root dry-run no-write/no-stage, exact argv, unreadable→Manual).
                  New `nix.edit_root_cmds` locale key (en+ru). **T7:** live escalated write unverified on a
                  real NixOS host.
            - [x] **Gentoo (Portage/emerge)** (ADR-0054). `provider/gentoo.rs`, id `gentoo`, Official
                  (GPG-verified ::gentoo tree), self-gates on `emerge`. Atom-based (`category/package`):
                  exact search `emerge --search "^name$"` (one candidate per `cat/name`, atom in `raw`),
                  root plans `emerge --ask=n`/`--unmerge`/`--update`/`-uDN @world`, `_many` batching,
                  `/var/db/pkg` list, pure `split_pf`. Builds from source (slow, inherent). No core
                  source-branch. 243 tests. Fixture-tested only — unverified on a live Gentoo host (T7).
            - [ ] **Windows/macOS** — a **separate later epic**, not "another provider": breaks `privilege.rs`
                  (no sudo/pkexec), path handling, packaging, CI. Scope on its own.
            - [x] **"Install-easy" epic (2026-07-11).** `install.sh` native installs (ADR-0059:
                  `JII_METHOD=auto|native|portable`, default `auto` asks-then-native on a TTY, portable in
                  pipes/CI, exact `sudo … install` shown first; Arch/AUR not wired yet). Packaging bumped to
                  v0.1.5-beta (`packaging/aur/PKGBUILD` real sha256sums + `packaging/jii.spec`), ready to
                  publish (owner's AUR/COPR accounts). `docs/SUPPORTED_SYSTEMS.md` cross-system test matrix;
                  README rewrite (two methods; `$ `-prompt copy-paste fix). **T7:** live native `sudo` install
                  unverified; AUR/COPR unpublished; Arch pacman native path pending the AUR publish.
            - [x] **"JII everywhere" — packaging recipes for every channel (2026-07-12, ADR-0060).**
                  `packaging/jii.spec` made multi-arch (one SRPM → every x86_64+aarch64 COPR/OBS chroot;
                  Fedora/EPEL=CentOS/Rocky/Alma/openSUSE). Added prebuilt-binary recipes:
                  `homebrew/jii.rb` (Linuxbrew), `alpine/APKBUILD` (musl-native), `void/template`,
                  `gentoo/jii-bin-*.ebuild`, `nix/jii.nix`. crates.io metadata in `Cargo.toml`
                  (`cargo publish --dry-run` clean → `cargo install jii`). Cross-system test guide
                  published as an Artifact for a friend. **Owner actions:** publish each (their accounts)
                  + one real build per off-Fedora recipe; exotic arches wait on the CI cross-compile epic.
            - [x] **Cross-system testing fixes — batch 1 (2026-07-12).** Owner tested every distro but
                  Gentoo/NixOS (report + screenshots in `~/Documents/suka/`), filed ~26 issues. Fixed:
                  (1) **`install.sh` checksum** — verify by hash not filename; GitHub rewrites `~`→`.` in
                  asset names so `sha256sum -c` failed on the `~beta` sidecar name, breaking native
                  `.deb`/`.rpm` install on Ubuntu+openSUSE (portable was fine). Live on `master`.
                  (2) **TTY/Unicode fallback** — `Platform::unicode` drives `+/x/!/-` when `TERM=linux`
                  or non-UTF-8 locale (was `▪` tofu on Void console); centralised behind `Palette::mark_*`.
                  (3) **Prompt UX** — doctor setup defaults to yes (`[Y/n]`), single-keypress y/n (no Enter).
                  (4) **`jii doctor`** shows the config path. (5) **`jii lang [en|ru|auto]`** view/set UI language.
                  (6) **first-run `jii doctor`** now onboards (was skipped, left first-run unmarked).
                  (7) **GitHub ranked strictly last** — part A of pt.17 (ADR-0061); `github` moved to the
                  end of default `priority`, below cargo/npm/pipx/go/brew/nix.
            - [x] **Cross-system fixes — batch 2 (2026-07-12).** pt.17 **part A** (github strict-last) +
                  **part B stages 1-2** (ADR-0061): `Provider::can_search` (cargo/npm/pipx/go network search +
                  Flatpak→Flathub v2 API), uninstalled-source search + bootstrap-before-install prompt; verified
                  `jii obsidian`→Flatpak, not github. Dotted app-ids match on their tail (`firefox`==
                  `org.mozilla.firefox` — the openSUSE "closest" papercut). `apt` update runs `apt-get update`
                  before upgrade. `jii cache [clear]`. `Provider::web_url` shown in the single-install preview
                  ("have a look first"). `install.sh` always speaks to PATH after a portable install.
            - [x] **Cross-system fixes — batch 3 (2026-07-12).** pt.17 part B **stage 3** (Snap+Homebrew
                  can_search → part B complete for all network sources). `-s` alias for `--source`. Flatpak
                  installs `--user` (no sudo/polkit; fixes the Void-live system-bus failure). Arch doctor
                  suggestions (VLC/codecs/fonts/Steam). `jii sources` hides other-distros' native managers
                  by default (`--all` to show; `SourceEntry.relevant`, pure-capability).
            - [x] **Cross-system fixes — batch 4 (2026-07-12).** `jii sources disable|enable <id>`
                  (flip `[sources] disabled`, validated vs KNOWN_SOURCES). All remaining output glyphs
                  (`⭐ ℹ ❯`) now TTY-safe via `palette.mark_*` — Unicode fallback complete.
            - [x] **Cross-system fixes — batch 5 (2026-07-12, ADR-0062).** **AUR provider** (`provider/aur.rs`,
                  Arch-family only via new `Platform::arch_like`): AUR RPC search + install/remove/update through
                  a helper (paru/yay, `needs_root=false`), `pacman -Qm` list; `jii yay`/`jii paru` bootstrap a
                  helper (makepkg shown, never run). **`jii providers` merged into `jii sources`** (hidden alias):
                  one view annotates ecosystem managers with `[add:…]`/`[remove:…]`. **`jii sources add <id>`**
                  (bootstrap) + **`jii sources remove <id>`** (uninstall a manager via the host system manager —
                  `SysManager` dnf/apt/pacman/zypper/xbps/portage, installed-only targets, exact elevated command
                  shown, default-no confirm; **system managers refused**; script-installed brew/nix → manual;
                  yay/paru → `pacman -Rs`). 268 tests, clippy clean; verified on Fedora (merge view, refuse dnf,
                  dry-run flatpak/go removal, `add yay` off-Arch refused).
            - [x] **`jii update` output polish (owner, 2026-07-12, ADR-0063).** The whole-system update now
                  **captures** each bulk manager's output and shows one line per source (`  <source>  ✓
                  <headline>` + notes) via `exec::summarize_update` (source-agnostic: nothing-to-do / `changed N
                  packages` / `N upgraded` / deprecation count / EOL count) instead of flooding the terminal;
                  failures show `✗` + an output tail. The JII self-update GitHub check runs **in parallel** with
                  the system update (near-instant). `Privilege::run_captured` + `Engine::run_plan_captured`. 271
                  tests (+3). **T7:** eyeball the live summary on a real update.
            - [x] **Cross-system fixes — batch 6 (2026-07-12).** **Mid-word fuzzy:** `broaden_search` gained a
                  3rd stage trying edit-distance-1 `typo_variants` (moved to `engine`, shared with the GitHub
                  picker) as exact searches, so `jii pipix` → `pipx` (deletions cover a doubled key). **`-d`/`-n`
                  disambiguated:** sharpened all three flag help texts + added `--preview` alias for `--dry-run`
                  (behaviour unchanged). 271 tests.
            - [x] **Owner-reported Fedora bugs — batch 7 (2026-07-13, ADR-0064).** (1) **`jii doctor` hid
                  foreign native managers:** `Engine::diagnose` now applies the shared
                  `source_relevant(available, provider)` predicate (extracted from `source_catalog`), so
                  `doctor` shows the same host-relevant set as `jii sources` — a Fedora box no longer lists
                  apt/pacman/aur/zypper/void/gentoo. (2) **Codec "not found" after enabling RPM Fusion:**
                  `doctor_offer` now runs `refresh_repo_metadata` (best-effort non-root `dnf5 makecache`,
                  dnf5-guarded, skipped in dry-run) right after enabling a prerequisite repo, so the dependent
                  install sees the new repo's packages. New locale key `doctor.refreshing_meta` (en+ru). 271
                  tests, clippy clean. **T7:** verify the codec flow end-to-end on a fresh RPM-Fusion enable.
            - [x] **Owner audit prompt — items #2/#5/#7/#14 (2026-07-13).** **#2** on a total miss (after
                  broadening + the repo picker) `install` now prints browse links — GitHub repo search + Flathub
                  search, per missed name — via a dependency-free unit-tested `url_query_encode`; skipped in JSON /
                  when `--source` pinned. **#5** `jii --help` now ends with the real XDG-resolved config path
                  (`main::parse_cli` injects `after_help` dynamically). **#7** `jii doctor` shows the `setup`
                  GitHub-token guidance, but only when actionable (GitHub in play + no configured token). **#14**
                  already resolved by design: the Flatpak provider is entirely `--user` (never `needs_root`), so
                  JII never triggers a flatpak polkit/sudo prompt — no password detection needed. 272 tests.
            - [x] **Owner audit #13 = T6 bootstrap (2026-07-13, ADR-0065).** `bootstrap_missing_managers` (`cli`)
                  runs on the chosen set before planning: a chosen candidate from an **uninstalled** ecosystem
                  manager (Flatpak/Snap/cargo/npm/pipx/go — they `can_search` without their CLI, so they outrank
                  the last-resort GitHub binary) triggers "set up {manager} and install {app}?" (default yes,
                  asked once per manager). `Packages` managers install via the normal path + `Engine::source_
                  available` confirms it landed; Flatpak also gets its Flathub user remote added idempotently.
                  `Script` managers (brew/nix) are shown-never-run and their apps skipped. `--dry-run` previews
                  both phases. Replaces the incomplete ADR-0061-partB loop; removed the dead `[bootstrap]` locale
                  section. 272 tests, clippy clean. **Verified (dry-run):** `httpie:pipx` → pipx-via-dnf then
                  httpie-via-pipx; `wget:brew` → shows the Homebrew script + skips; `obsidian` (flatpak present) →
                  plain flatpak, no bootstrap. **T7:** live on a manager-less host.
            - [x] **Owner testing round #1 (2026-07-15, ADR-0066).** Ten reports from live testing of
                  v0.1.7-beta on Fedora + an apt host, all landed. **(1)** Bootstrap resolves the manager's
                  package *and* its source, restricted to sources usable right now, and pins it
                  (`first_available_package` → `first_bootstrap_package`) — kills the "install pipx via pipx" /
                  "npm via npm" chooser ADR-0065 left behind. **(2)** brew/nix: their upstream script is shown
                  in full and **run on an explicit answer, default yes** (was shown-never-run, a dead end);
                  `--auto`/`--yes` never consent for it, non-TTY only prints it. **(3)** `ui::Spinner` +
                  `exec::run_actions_quiet`: install/remove/update show live progress over captured output
                  (`jii update` read as a hang), remove's preview is one line per package, "Searching…" is a
                  spinner. **(4)** New `--run` + `Provider::launch_command` (default = package name; Flatpak →
                  `flatpak run <id>`), verified to exist before running, `exec`s. **(5)** `changed_count`
                  counts per line — apt's "will be upgraded:" prose ate every count, so apt always reported a
                  bare "updated"; dnf5's summary counted too. **(6)** `jii sources` lists sources you disabled
                  (they're absent from the registry) + a disable/enable footer. **(7)** `jii man` formats via
                  `man(1)` on a TTY, raw roff when redirected. **(8)** `jii providers` removed (duplicated
                  `jii sources`). 276 tests, clippy clean. **Verified live on Fedora:** sources view, the two
                  bootstrap dry-runs, npm install/remove/`--run`, `--run` on a font + a batch, `man -l`.
                  **Open (owner asked, answered not coded):** registry name-squats — `htop` on PyPI is an
                  unrelated project, so an explicit `htop:pipx` installs junk and pip fails. Only bites on an
                  explicit pin (ranking puts dnf/apt first); a relevance heuristic is unscoped.
            - [x] **Full-project audit + fix-everything round (2026-07-16/17, ADR-0067/0068).** Owner asked for
                  a line-by-line audit; every P1–P3 finding fixed in one wave. **P1:** the github source resolves
                  prerelease-only repos (`/releases?per_page=20` + `pick_release`, was a 404 from `/releases/latest`);
                  `jii update` / self-update exit non-zero when a bulk source or the release check failed (was a
                  silent success). **P2:** remove-chooser & forge errors localized; `@ref` pins honoured in
                  `route_managers`; `sources --json` schema stable (explicit nulls); Flatpak plans idempotently add
                  the **user-scope Flathub remote** first. **P3:** Russian `д/н` accepted at every y/n prompt;
                  the chooser menu scrolls on short terminals; `jii how` shows **every** owner of a name
                  (`Registry::get_all`); cache entries pruned after 30 days; self-update warns when the published
                  tag looks like a **downgrade** (`selfupdate::looks_like_downgrade`); `record_remove` matches
                  name+source. **Junk filter (ADR-0067):** `ranking::mark_suspicious` — community candidates from
                  network registries with near-zero popularity (cargo/npm downloads), no summary, `0.0.x`, or a
                  provider pre-mark (pipx: sole release >5 years old) → `untrusted` + red `install.suspicious`
                  warning; never touches official/local/path-style names; auto mode therefore never picks them.
                  **Hidden tester checklist:** `jii yes-I-am-dev-and-want-to-test` (`src/devtest.rs`, hidden from
                  help/README, English-only) — 12 scripted steps incl. real install/remove of htop, per-step
                  Y/n/s verdicts, full log with username/hostname scrubbed, one-key upload (0x0.st →
                  paste.c-net.org fallback), pre-filled GitHub-issue link; exits non-zero on any FAIL. Tester
                  guide: **docs/TESTING.md**. **Win/mac:** plan only, no code — ADR-0068 (three waves,
                  macOS-first) + ROADMAP entries (incl. landing page / launch content). `install.sh` no longer
                  prints a spurious `curl: (23)` while resolving the tag. 285 tests, clippy clean. **Verified
                  live:** `search htop` shows pipx/cargo red-untrusted; `htop:pipx --dry-run` prints the warning;
                  the checklist runs end-to-end and is absent from `--help`.
            - [x] **Owner feedback round #2 (2026-07-25, ADR-0069/0070) → `v0.1.10-beta`.** Two owner asks.
                  (1) *Live progress.* Friendly-mode installs/updates/downloads now draw a real bar with a
                  percentage read from the source's own output — new source-agnostic `src/progress.rs`
                  (`[done/total]` counter or bare `NN%`, strict parsing rejects dates/prose), new
                  `Privilege::run_streamed` (line-streaming, replaces `run_captured`), `Spinner` +
                  `ProgressReporter` drawing `████░░░░ 45% [3/41]`, chunk-streaming `download_reported` for exact
                  byte % (reqwest `stream` feature), and `run_plan_streamed` for the whole-system update.
                  (2) *Flatpak update-all* dropped `--user` → `flatpak update -y` updates system-wide apps too
                  (the Discover bug); still root-free (flatpak's polkit). 297 tests, clippy clean. **Found, not
                  fixed:** `jii install` can dead-end when the top pick's manager needs bootstrapping and that
                  fails, instead of falling back to a listed alternative (see AI_CONTEXT batch 9 — next release).
      - **Next (owner to steer):** the **install-easy epic** landed; declarative-Nix **complete** through Etap C.
            **Next core work (owner directive, 2026-07-11):** unfreeze **T6 — bootstrap a missing manager**
            (offer to install e.g. Flatpak, then the app; engine today *skips* `!is_available()` sources, so
            T6 must surface them + prepend a bootstrap plan step) **and rank GitHub strictly last** (default
            `priority` in `src/config.rs` currently puts `github` above `cargo/npm/pipx/go/brew/nix`; owner
            wants it the absolute last resort — confirm scope). Then: publish AUR/COPR + wire pacman native;
            Windows/macOS (separate epic, deferred); richer info cards; live verification of non-Fedora
            providers and the Nix + native-install flows on real hosts (T7). GUI is **parked**.
- [~] **BETA-READINESS — FEATURE FREEZE (ACTIVE, owner-set 2026-07-06).** New large features are
      **frozen**; drive to the first public Beta. Full plan + parked backlog in
      **[docs/BETA_ROADMAP.md](BETA_ROADMAP.md)**. Priority order:
      - [x] **1. CI** — already present (`.github/workflows/ci.yml`): `clippy -D warnings` (which
            also builds) + `test`, both `--locked`, on push/PR. (No `fmt --check` by design — ADR-0013;
            rustfmt isn't part of the DoD.)
      - [ ] **2. Integration tests** — CLI-level `assert_cmd`/`predicates` over isolated `XDG_*`
            (search/info/list/history/sources/--json/--dry-run/setup + not-installed/empty edges) +
            a registry-partial-failure test. All 180 tests today are unit-level. *(agent)*
      - [ ] **3. Clean-VM verification** on Arch/Ubuntu/Debian/openSUSE — the whole cross-distro
            layer is written but never run live. **The one Beta blocker an agent can't close alone**
            (needs the owner's real hosts; agent scripts a repeatable smoke test). *(owner + agent)*
      - [ ] **4. Public docs & assets** — README polish, CONTRIBUTING/SECURITY *(agent)*; asciinema
            script *(agent writes, owner records)*; logo + screenshots *(owner/designer)*.
      - [x] **5. Public release — `v0.1.0-beta` PUBLISHED (2026-07-09).** Released end-to-end after two CI
            fixes: nfpm installed from the goreleaser apt repo (taiki-e/install-action can't do nfpm); nfpm.yaml
            rendered via `envsubst` (nfpm left `${BIN}` unexpanded). Release carries 12 assets — tarball/.deb/.rpm
            for x86_64 + aarch64 (+ sha256). **Still unverified on a live host** (install/run, esp. non-Fedora +
            arm64 — risk #1 open).
      - [x] **Self-update / uninstall** (ADR-0040, owner-requested). `jii` reserved as the tool's own name:
            `jii update jii` self-updates from the newest release (user-space → atomic binary swap via new
            `Action::Replace`, no root; package → dnf/apt install shown first), `jii uninstall`/`jii remove jii`
            self-remove, bare `jii update` updates everything (system + JII itself, self-update last, still prompts).
            `src/selfupdate.rs` (pure builders unit-tested) + `Engine::run_self_plan`.
            Cargo version → `0.1.0-beta`. Fetch+swap exercised only on the next real tag; pure parts tested + `--dry-run`.
      - [~] **5b. (was 5) Packaging pipeline (ADR-0039).** `release.yml`
            reworked: a `v*` tag builds **static musl** binaries for **x86_64 + aarch64** (via `cross`)
            and publishes, on the GitHub Release: checksummed tarballs, native **.deb** + **.rpm** (nfpm,
            `packaging/nfpm.yaml`, bundling **man page + bash/zsh/fish completions**). Added **`install.sh`**
            (`curl|sh`, arch-detect, sha256-verified, → ~/.local/bin) and hidden `jii completions <shell>` /
            `jii man` (clap_complete/clap_mangen, no build.rs). `[profile.release]` = lto/codegen-units=1/strip
            (behavior unchanged). README **Install** rewritten (one-liner + .rpm/.deb + AUR + tarball + source).
            COPR (`packaging/jii.spec`) + AUR (`packaging/aur/PKGBUILD`) prepared turnkey (`packaging/README.md`)
            — need owner accounts. 186 tests, clippy clean; locally validated (tarball layout, install.sh
            extraction/checksum, completions/man, spec parse). **Owner action to cut Beta:** `git tag
            v0.1.0-beta && git push origin v0.1.0-beta` (the workflow does the rest). Only signing/COPR/AUR
            publish remain owner-side.
      - **Absorbs the old T6/T7/T8:** T7 (hardening) = items 1–3 above; T8 (public polish) = items
        4–5. **T6 (bootstrap a missing manager) is FROZEN** — parked in BETA_ROADMAP, post-Beta.
      - **Frozen backlog (do NOT start pre-Beta):** doctor --fix, catalog aliases, version chooser,
        GitHub repo chooser, bootstrap (T6), undo, streaming search, declarative providers, plus the
        tech-debt items (cli/mod.rs split, flag-shed, update-staleness, model.rs dead_code). See
        BETA_ROADMAP.md "Frozen" — post-Beta feedback reorders and promotes them.

---

### Definition of Done (every task)

1. Compiles with no warnings (`cargo clippy` clean).
2. Formatted (`cargo fmt` — **not installed on the current dev host**; match the
   surrounding style by hand until it is available).
3. Has a test where logic is non-trivial (parsers, ranking).
4. Behavior verified end-to-end (`--dry-run` at minimum).
5. No provider-specific branching leaked into the core.
6. AI Handoff Policy done: `AI_CONTEXT.md` updated, any decision recorded in
   `DECISIONS.md`, small descriptive commit (see [../CLAUDE.md](../CLAUDE.md)).
