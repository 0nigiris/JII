# JII — Roadmap

Incremental delivery. Each phase produces something **compilable and runnable**.
The architecture ([ARCHITECTURE.md](ARCHITECTURE.md)) does not change between phases —
each phase fills in more of the same contracts.

Legend: 🎯 MVP · 🔭 post-MVP · 🌅 future

---

## Phase 0 — Skeleton 🎯

**Goal:** a single crate that compiles, parses `jii <name>`, and prints a stub.

- Single Cargo crate, modular layout (see ARCHITECTURE §4).
- `platform.rs`: detect Fedora, arch, PATH, TTY/graphical session.
- `config.rs`: load/merge/validate TOML with defaults.
- `error.rs`, `model.rs`: core types.
- `cli/`: clap wiring, global flags.
- `ui/`: renderer facade honoring `--json` / `--no-color`.

**Done when:** `jii fastfetch` runs, loads config, prints a placeholder plan.

---

## Phase 1 — DNF end-to-end 🎯

**Goal:** actually install a real package via one source.

- `Provider` trait finalized.
- `provider/dnf.rs`: `search` / `plan_install` / `list_installed` using **dnf5**
  machine output.
- `privilege.rs`: batched `sudo`/`pkexec`, exact-command display.
- `engine/`: `search → rank → plan → execute` (single provider, full model).
- `ui/prompt.rs`: `[Y/n]` with default, trust barrier.
- `--dry-run` shows the plan and exits.

**Done when:** `jii <dnf-package>` installs it; `--dry-run` previews the plan.

---

## Phase 2 — State, remove, why 🎯

**Goal:** JII remembers and can reverse.

- `registry.rs`: JSON state store (intentions) + verification against dnf.
- `jii remove` (registry → verify → plan → remove).
- `jii list`, `jii why`, `jii history`.
- Write registry **only on success**.

**Done when:** install → `list`/`why` reflect it → `remove` uses the right source.

---

## Phase 3 — Multiple sources & ranking 🎯

**Goal:** real choice between sources; tie-breakers matter.

- `provider/flatpak.rs`, `provider/copr.rs`.
- `engine/ranking.rs`: priority + tie-breakers + mandatory explanations.
- Parallel fan-out with per-source timeouts + graceful degradation.
- `cache.rs`: TTL cache, stale-on-error.
- `jii doctor` (availability, latency, health).

**Done when:** a package present in DNF+Flatpak+COPR is ranked with a clear "why".

---

## Phase 4 — GitHub Releases & trust 🎯

**Goal:** the hard, security-sensitive source.

- `provider/github.rs`: name→repo resolution, arch/libc asset filtering,
  checksum/signature verification, `GITHUB_TOKEN` support.
- Trust levels enforced end-to-end; `untrusted` always confirmed even in `--auto`.
- `jii audit` (signatures, sha256, GPG, sigstore, source, trust).

**Done when:** installing a GitHub release verifies the artifact and respects trust.

---

## Phase 5 — User-space sources & update 🔭

- `provider/cargo.rs`, `npm.rs`, `pipx.rs`, `go.rs` (no root; `~/.local/bin` PATH check).
- `jii update [<name>]` across all managers.
- `jii undo`, `jii benchmark`.

---

## Phase 6 — Declarative sources & catalog 🔭

- `provider/declarative.rs` + `data/sources/*.toml`.
- `data/catalog.toml`: name aliases (`vscode → code`, `node → nodejs`).
- Light full-text search over package metadata (Stage 3).
- Fuzzy name search (Stage 2).

---

## Phase 7 — Hardening 🔭

- Full test matrix (unit ranking/parsers on fixed samples; integration dry-run).
- Docs polish, `--json` stability, error-message quality pass.
- Distribution: COPR repo + signed GitHub binary.

---

## Future 🌅

- SQLite migration (behind the same registry API).
- Cargo workspace split.
- Semantic / AI search (Stage 4).
- Cross-distro: apt, pacman, zypper, nix, AUR, snap.
- Windows (winget), macOS (Homebrew).
- GUI frontend, plugin SDK.
