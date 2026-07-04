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

**Start every session by reading `docs/AI_CONTEXT.md`** — it is the current-state
snapshot (phase, last work, next task, build/test status). Decisions and their
rationale live in `docs/DECISIONS.md`. The repository — not any AI conversation — is
the single source of truth; no important project knowledge may exist only in a chat
window.

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

## AI Handoff Policy (mandatory — every session)

JII must be continuable by **any** agent (Claude Code, another AI, or a human) with
minimal context loss. So no task is complete until the repository reflects reality.
At the **end of every work session**, before considering the task done, always:

1. **Update `docs/TASKS.md`** — check off what landed; add notes on deviations.
2. **Update `docs/AI_CONTEXT.md`** — it must describe the *current* state: phase,
   last completed work, current task, next recommended task, blockers, build/test
   status. If the current task changed, or a phase completed, reflect it immediately.
   Keep it a concise snapshot — no accumulated history.
3. **Update `docs/DECISIONS.md`** — if any architectural decision was made, add an ADR
   (decision, reason, alternatives, status, consequences). Never leave a
   design-affecting decision only in the conversation or a commit message.
4. **Update `README.md`** — only if user-visible behavior changed.
5. **Ensure the project builds** — `cargo build` clean, `cargo clippy` clean.
6. **Ensure tests pass** — `cargo test` green; add tests for new non-trivial logic.
7. **Commit** — a small, descriptive commit capturing the change.
