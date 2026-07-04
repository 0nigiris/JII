---
name: new-provider
description: Scaffold a new JII source provider — either a native Rust provider (impl Provider) or a declarative data-driven source (data/sources/*.toml). Use when adding support for a new installation source (a package manager, a repo, a GitHub project).
---

# new-provider — add a source to JII

Adds a new installation source to JII. First decide **which kind** of provider fits;
they have very different amounts of work.

## Step 1 — choose the kind

- **Declarative (data-driven)** — for a *simple, specific* source: one COPR/RPM repo,
  one GitHub repo with a predictable asset pattern, a single well-known package. No
  Rust code, no recompile. **Prefer this whenever it is sufficient.**
- **Native (Rust)** — for a *whole ecosystem* with its own search/install/list
  semantics (a new package manager like apt, a registry like crates.io/npm). Requires
  implementing the `Provider` trait.

If unsure, ask the user which they mean.

## Step 2a — declarative source

Create `data/sources/<id>.toml` following the shape used in `docs/ARCHITECTURE.md` §5:

```toml
[source]
id = "<unique-id>"          # e.g. "spotify-repo"
type = "<dnf-repo|github-release|…>"
trust = "<official|community|untrusted>"
# type-specific fields, e.g.:
repo_url = "https://…/x.repo"       # for dnf-repo
# owner_repo = "owner/name"         # for github-release
# asset_pattern = "*-x86_64.tar.gz" # for github-release
provides = ["<name>", "<alias>…"]   # package names this source can satisfy
```

The generic `DeclarativeProvider` (`src/provider/declarative.rs`) loads these — no
code change needed. Add a fixture-based test if the source's matching logic is
non-trivial.

## Step 2b — native provider

1. Create `src/provider/<id>.rs` implementing the `Provider` trait
   (see `src/provider/mod.rs` and the reference impl in `src/provider/dnf.rs`).
   Required: `id`, `trust`, `is_available`, `search`, `plan_install`,
   `plan_remove`, `plan_update`, `list_installed`.
2. Register it in the provider registry (`src/provider/mod.rs`).
3. Add its default rank position to config defaults (`src/config.rs`) if it should
   participate in ranking out of the box.

### Non-negotiable rules (from CLAUDE.md / ARCHITECTURE.md)

- **Plan, never execute privileged actions.** Return `Step { needs_root, argv, … }`;
  escalation happens centrally in `privilege.rs`. Never call sudo/pkexec here.
- **Prefer machine-readable output** over parsing human text; isolate the parser and
  **unit-test it on a fixed sample** of the tool's output.
- **Assign a correct `TrustLevel`** and set `signed` / verification honestly — the
  trust barrier and `--auto` behavior depend on it.
- **Filter incompatible arch/libc** in `search` (set `arch_ok`).
- `search` must **not panic on network failure** — return `Result`; the Engine tags a
  failed source and continues.
- **No source-specific branching leaks into the core** — all specifics stay in this
  module.

## Step 3 — verify

- `cargo clippy` clean, `cargo fmt`.
- Unit test for any parser/matching logic.
- `jii <name> --source <id> --dry-run` shows a correct plan without side effects.
