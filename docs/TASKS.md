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
- [ ] **github follow-ups:** more archive formats (`.zip`, `.tar.xz`); broad name→repo
      resolution; release pagination.
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

- [ ] `provider/{cargo,npm,pipx,go}.rs` (no root; `~/.local/bin` PATH warning).
- [ ] `cli/commands/update.rs` across managers.
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
