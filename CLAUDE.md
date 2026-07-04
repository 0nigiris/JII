# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

JII ("Just Install It") — a smart universal package **installer** (not a package
manager) for Linux, written in Rust. It searches multiple sources (DNF, COPR,
Flatpak, GitHub Releases, Cargo, npm, pipx, Go…), ranks them, installs the best, and
explains why.

## Source of truth

**`docs/ARCHITECTURE.md` is canonical.** Read it (and `docs/ROADMAP.md`,
`docs/TASKS.md`) before proposing design changes. **Do not redesign the agreed
architecture** unless implementation reveals a concrete, real problem — then say so
explicitly and justify it.

## Binding MVP constraints (do not silently change)

- **Fedora-first**: dnf5, COPR, Flatpak, GitHub. Cross-distro is future work behind
  the `platform` abstraction.
- **Single Cargo crate**, modular layout — NOT a workspace (migrate later if it grows).
- **JSON state file** for the registry — NOT SQLite yet (migrate later, same API).
- **`Plan` is first-class**: every action builds an `InstallPlan` before executing;
  everything is previewable via `--dry-run`.
- **`default_yes` is a trust threshold** (`default_yes_max_trust`), not a global bool.
- **JII is never fully run as root** — only concrete steps escalate via sudo/pkexec,
  batched, exact commands shown first. Providers plan but never execute privileged
  actions themselves; escalation lives in `privilege.rs`.
- **Trust levels** (official/community/untrusted); **auto mode never installs
  untrusted automatically**.
- **The core never branches on the source** (no `if source == "dnf"`). It operates
  only on the `Provider` trait and the `PackageCandidate` / `InstallPlan` model.
- Add a new source by implementing `Provider`; simple sources are declarative TOML in
  `data/sources/` (use the `/new-provider` skill).

## Working style

- Build the MVP **incrementally, phase by phase** per `docs/TASKS.md`.
- Prefer machine-readable tool output (dnf5 structured output, `flatpak --columns`)
  over parsing human text; isolate parsers and unit-test them on fixed samples.
- Definition of Done: `cargo clippy` clean, `cargo fmt`, tests for non-trivial logic,
  behavior verified at least via `--dry-run`.
- **Respond in Russian.**
