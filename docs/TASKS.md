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

## Phase 3 — Multiple sources & ranking 🎯

- [ ] `provider/flatpak.rs` (`--columns` machine output, `--user` = no root).
- [ ] `provider/copr.rs`.
- [ ] `engine/ranking.rs`: priority + tie-breakers + `reasons` on every candidate.
- [ ] Parallel fan-out with per-source timeouts; failed source tagged `✗ timeout`.
- [ ] `cache.rs`: TTL cache + stale-on-error.
- [ ] `cli/commands/doctor.rs`: availability, latency, health.
- [ ] Unit tests for ranking (fixed candidate sets → expected order + reasons).
- [ ] **Verify:** a multi-source package ranks correctly with a printed "why".

## Phase 4 — GitHub Releases & trust 🎯

- [ ] `provider/github.rs`: name→repo resolution, arch/libc asset filter, pagination.
- [ ] Artifact verification: sha256 / GPG / sigstore where available; `⚠ unsigned` tag.
- [ ] Trust enforcement: `untrusted` always confirmed, even with `--auto`.
- [ ] `GITHUB_TOKEN` support to lift rate limits.
- [ ] `cli/commands/audit.rs`.
- [ ] **Verify:** installing a GitHub release verifies the artifact & respects trust.

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
2. Formatted (`cargo fmt`).
3. Has a test where logic is non-trivial (parsers, ranking).
4. Behavior verified end-to-end (`--dry-run` at minimum).
5. No provider-specific branching leaked into the core.
