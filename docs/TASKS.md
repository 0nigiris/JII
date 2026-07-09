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
