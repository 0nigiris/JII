# AGENTS.md — Onboarding for any agent (AI or human)

You are continuing work on **JII** ("Just Install It"), a smart universal package
**installer** for Linux, written in Rust, Fedora-first. This file is the tool-neutral
entry point: it works whether you are Claude Code, OpenCode, GPT, Gemini, or a human.

**The repository is the single source of truth.** No project knowledge lives only in
a chat window. Everything you need to continue is in these files.

## Read these first (in order, ~5 minutes total)

1. **[docs/AI_CONTEXT.md](docs/AI_CONTEXT.md)** — the current state: phase, last work,
   current/next task, blockers, build/test status. **Start here every time.**
2. **[CLAUDE.md](CLAUDE.md)** — binding MVP constraints and the working style. These
   apply to **every** agent, not just Claude Code; the filename is only a convention.
3. **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** — canonical design. Do not redesign
   it without a concrete, real implementation problem (and then record why).
4. **[docs/DECISIONS.md](docs/DECISIONS.md)** — ADRs: *why* the architecture is the way
   it is. Read before proposing to change a load-bearing decision.
5. **[docs/TASKS.md](docs/TASKS.md)** / **[docs/ROADMAP.md](docs/ROADMAP.md)** — the
   phased plan and the actionable checklist.

## Golden rules (the short version)

- **Never branch on the source in the core** — all source logic lives behind the
  `Provider` trait. No `if source == "dnf"` outside `provider/`.
- **`Plan` is first-class** — build an `InstallPlan` of declarative `Action`s before
  touching the system; everything is previewable via `--dry-run`.
- **JII is never fully run as root** — only concrete steps escalate, via `privilege.rs`.
- **Trust is a threshold, not a boolean** — `untrusted` is always confirmed, even `--auto`.
- **Prefer machine-readable tool output**; isolate parsers as pure functions and
  unit-test them on fixed samples.
- **Build incrementally, phase by phase** (docs/TASKS.md). Keep commits small.

## Build / run / test

```console
cargo build            # must be warning-clean
cargo clippy           # must be clean (installed on the dev host)
cargo test             # all tests must pass
cargo run -- install <pkg> --dry-run   # preview a plan without side effects
```

> **Note:** `cargo fmt` / `rustfmt` are **not installed** on the current dev host.
> Match the surrounding code style by hand; do not assume `cargo fmt` is available.

## AI Handoff Policy (mandatory — end of every session)

Before considering any task complete, so the next agent loses no context:

1. Update **docs/TASKS.md** (check off what landed; note deviations).
2. Update **docs/AI_CONTEXT.md** (current phase, last work, current/next task,
   blockers, build/test status — a *snapshot*, no accumulated history).
3. Update **docs/DECISIONS.md** if any architectural decision was made (add an ADR).
4. Update **README.md** only if user-visible behavior changed.
5. `cargo build` clean · `cargo clippy` clean.
6. `cargo test` green (add tests for new non-trivial logic).
7. Make a small, descriptive commit.

The full, canonical version of this policy lives in [CLAUDE.md](CLAUDE.md#ai-handoff-policy-mandatory--every-session).

## Adding a new source

Implement the `Provider` trait in `src/provider/` (see `dnf.rs` / `flatpak.rs` as
worked examples). Simple sources become declarative TOML in `data/sources/` later.
Never edit the core to add a source.
