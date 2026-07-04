# JII — Tasks

Actionable checklist derived from [ROADMAP.md](ROADMAP.md). Check items off as they
land. Keep tasks small enough to complete and verify in one sitting.

---

## Phase 0 — Skeleton 🎯

- [ ] `cargo init` single crate; add deps (clap, tokio, serde, toml, reqwest+rustls,
      anyhow, thiserror, indicatif, owo-colors, directories, async-trait).
- [ ] `error.rs`: `JiiError` (thiserror) + `Result` alias.
- [ ] `model.rs`: `Query`, `QueryKind`, `TrustLevel`, `Health`, `PackageCandidate`,
      `Step`, `Verification`, `InstallPlan`, `InstalledRecord`.
- [ ] `platform.rs`: detect distro (Fedora), arch, PATH entries, TTY vs graphical.
- [ ] `config.rs`: struct + defaults + TOML load/merge + validation (unknown source id → error).
- [ ] `cli/mod.rs`: clap commands + global flags (`-y/-n/--auto/--source/--profile/--dry-run/-v/--json/--no-color`).
- [ ] `ui/mod.rs`: renderer facade (respects `--json`, `--no-color`).
- [ ] `main.rs`: wire config → engine → cli.
- [ ] **Verify:** `jii fastfetch` runs, prints placeholder; `--json` emits JSON; config loads.

## Phase 1 — DNF end-to-end 🎯

- [ ] `provider/mod.rs`: finalize `Provider` trait + provider registry.
- [ ] `provider/dnf.rs`: `is_available`, `search`, `plan_install`, `list_installed` (dnf5 machine output).
- [ ] Unit tests for the dnf output parser on **fixed sample output**.
- [ ] `privilege.rs`: detect sudo/pkexec; batched elevation; print exact commands.
- [ ] `engine/mod.rs` + `engine/plan.rs`: `search → rank → plan → execute` (single provider).
- [ ] `ui/prompt.rs`: `[Y/n]` default-yes; trust barrier hook.
- [ ] `--dry-run` renders the plan and exits without side effects.
- [ ] **Verify:** `jii <dnf-pkg> --dry-run` previews; `jii <dnf-pkg>` installs it.

## Phase 2 — State, remove, why 🎯

- [ ] `registry.rs`: JSON store under `~/.local/state/jii/`; load/save; write **only on success**.
- [ ] Verification: reconcile registry with `dnf list installed`.
- [ ] `cli/commands/remove.rs`: resolve source → plan_remove → execute.
- [ ] `cli/commands/{list,why,history}.rs`.
- [ ] **Verify:** install → `list`/`why` reflect it → `remove` uses the recorded source.

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
