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

## Phase 3 — Multiple sources & ranking 🎯 ✅

**Goal:** real choice between sources; tie-breakers matter.

- `provider/flatpak.rs` (installs via Flatpak's own polkit — no JII root).
- `engine/ranking.rs`: source priority + trust tie-breaker + profiles, with an
  "also available" explanation in the CLI.
- Parallel fan-out with per-source timeouts + graceful degradation.
- `cache.rs`: TTL cache, stale-on-error.
- `jii doctor` (availability, latency, health).

**COPR moved to Phase 4:** `dnf5 copr` has no search, so finding which COPR provides
a package needs the COPR web API — the same fuzzy name→project resolution problem as
GitHub Releases, plus root repo-enable and trust handling. Best done alongside GitHub.

**Reserved:** `latest`/`minimal` profiles and freshness/health ranking tie-breakers
need comparable versions / dependency-footprint data we do not collect yet.

**Done:** a package in DNF+Flatpak is ranked with a clear recommendation + alternatives.

---

## Phase 4 — GitHub Releases, COPR & trust 🎯

**Goal:** the hard, security-sensitive sources that share a name→source resolution
problem.

- `provider/github.rs`: name→repo resolution, arch/libc asset filtering,
  checksum/signature verification, `GITHUB_TOKEN` support.
- `provider/copr.rs`: COPR web-API project search, root repo-enable, trust handling.
- Trust levels enforced end-to-end; `untrusted` always confirmed even in `--auto`.
- `jii audit` (signatures, sha256, GPG, sigstore, source, trust).
- Rate-limit health in `doctor` (GitHub).

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
- Plugin SDK.
- **GUI frontend** — see "Future ideas" below.

---

## Future ideas

Captured so they are not forgotten. **Not scheduled and not started.** Before acting
on any of these, revisit the engine's public API and record decisions in
[DECISIONS.md](DECISIONS.md).

### GUI frontend — a cross-provider "Discover"

**Vision:** a Discover-like desktop application that is *not* limited to a single
ecosystem. It transparently searches, compares, and installs across every enabled
provider (DNF, Flatpak, GitHub, COPR, Cargo, npm…), showing the same recommendation
and "why" the CLI gives.

**Non-goal:** the GUI does **not** replace the CLI, and it is **not** a second
implementation. It is *another frontend over the same engine*.

```
CLI ─┐
     ├── Core Engine  (search · rank · plan · trust · execute · registry)
GUI ─┘
```

**Hard architectural rule:** the GUI is a **thin frontend**. It reuses the exact
search, ranking, planning, trust model, and execution logic of the engine and
**never duplicates business logic**. Any behavior it needs must live in the engine
and be shared with the CLI — if the GUI wants something the engine can't express, the
engine grows, not the GUI. (See [DECISIONS.md](DECISIONS.md) ADR-0015.)

**Potential features** (all backed by existing or extended engine capabilities):

- Universal Linux software catalog; search across every enabled provider.
- Rich listings: application icons, screenshots, descriptions, version info.
- Source comparison side-by-side, with the engine's **"why this source?"** rationale.
- Trust indicators (official / community / untrusted) and signature/verification status.
- Dry-run preview of the plan before anything runs.
- Update management, installed applications, history, and audit — the same commands
  the CLI exposes, rendered visually.

**Implications to weigh when it is time (not now):**

- Metadata the CLI doesn't need yet — icons, screenshots, long descriptions — must be
  produced by providers through the model, not fetched ad hoc in the GUI.
- The engine's API must be callable as a library (it already operates purely on the
  model); a GUI likely links the crate directly or talks to a thin local service.
- Long-running/streamed operations (download progress) may need the engine to surface
  progress events without the GUI reaching into execution internals.
