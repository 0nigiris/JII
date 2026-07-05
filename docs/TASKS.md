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
- [ ] **T2 — Batch symmetry:** `jii update a b c`, `jii remove a b c`. Optional
      `plan_update_many`/`plan_remove_many` + engine `update_batch`/`remove_batch`; CLI
      widened to `Vec`. (ADR-0025 pre-committed: no new architecture.)
- [ ] **T3 — Provider breadth:** `provider/homebrew.rs` → `snap.rs` → `appimage.rs`. Empirical
      check at Homebrew: is a shared `RegistryProvider` scaffold now worth it?
- [ ] **T4 — Cross-distro system providers:** `apt.rs`, `pacman.rs`, `zypper.rs`, `nix.rs`
      behind the platform seam. Relax `Platform::is_supported`; distro-aware `is_available`.
      Own ADR for the "native system provider per distro" concept. Never break Fedora.
- [ ] **T5 — Interactive choosers:** GitHub repository chooser (paged select in `ui/prompt`;
      engine ranks/heuristics) + version chooser (`--version`; optional
      `Provider::available_versions`, provider-ordered). Own ADR for the version growth.
- [ ] **T6 — Bootstrap a missing manager:** optional `Provider::bootstrap_plan`; engine offers
      it when a chosen source is unavailable. Strongest consent, never auto. Own ADR.
- [ ] **T7 — Hardening:** CLI integration tests (`assert_cmd`), registry-partial-failure test,
      error-message quality pass, clean-VM runs on Fedora/Arch/Ubuntu/Debian/openSUSE.
- [ ] **T8 — Public polish:** professional README, logo, screenshots/asciinema, architecture
      diagram, CONTRIBUTING/SECURITY, examples, limitations. Then cut the first public Beta.

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
