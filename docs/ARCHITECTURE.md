# JII — Architecture

> **Status:** Design agreed, MVP in progress.
> This document is the **source of truth** for JII's architecture. Do not redesign
> what is described here unless implementation reveals a concrete, real problem.

---

## 1. What JII is (and is not)

JII (**Just Install It**) is a **smart universal package installer** for Linux. It is
**not** a package manager. It sits *on top of* existing package managers and
installation sources (DNF, COPR, Flatpak, GitHub Releases, Cargo, npm, pipx, Go…),
searches all of them, ranks the results, and installs the best one — while always
explaining *why*.

**Philosophy — the user thinks about software, not package managers.**

> "I want Docker." → JII decides *how* to install it and explains *why*.

### Design principles (binding)

1. **Fast startup** (target < 100 ms cold; no work until needed).
2. **Offline-first** where possible; degrade gracefully when a source is down.
3. **Explain every important decision** (`why`, reasons in every recommendation).
4. **Never sacrifice security for convenience.**
5. **Never hide the installation source.**
6. **Everything is previewable** — every action can run under `--dry-run`.
7. **Every provider is replaceable** — the core never hardcodes a source.
8. **Extensibility without overengineering.**
9. **Predictable behavior** — deterministic ranking, no hidden ML.
10. **Beautiful CLI UX.**

---

## 2. Scope of the MVP

| Aspect | MVP decision |
|--------|--------------|
| Target platform | **Fedora only** (dnf5, COPR). Cross-distro is future work. |
| Crate layout | **Single Cargo crate**, modular. Migrate to a workspace only if the project grows significantly. |
| State storage | **JSON state file** under XDG paths. Migrate to SQLite later without changing the public architecture. |
| Semantic search | **Out of scope.** MVP does name search + light full-text over metadata. |
| Privilege model | Point elevation via **`sudo` / `pkexec`**. JII is never fully run as root. |

Everything below is designed so these MVP choices can evolve (SQLite, workspace,
more distros, semantic search) **without** breaking the module boundaries or the
public `Provider` / `Engine` contracts.

---

## 3. High-level architecture

Clean layered architecture. The **core never contains `if source == "dnf"`** — it
operates only on the `Provider` trait and the `PackageCandidate` / `InstallPlan`
model.

```
┌───────────────────────────────────────────────┐
│  cli/     — clap commands, flag parsing         │
├───────────────────────────────────────────────┤
│  ui/      — output, prompts, spinners, tables   │
├───────────────────────────────────────────────┤
│  engine/  — orchestrator                        │
│   search() → rank() → plan() → execute()        │
├──────────────┬───────────────┬──────────────────┤
│ provider/    │ registry.rs   │ privilege.rs      │
│ (Provider    │ (JSON state   │ (sudo/pkexec,     │
│  trait +     │  + verify)    │  batched elevate) │
│  sources)    │               │                   │
├──────────────┼───────────────┼──────────────────┤
│ cache.rs     │ config.rs     │ model.rs / error  │
├──────────────┴───────────────┴──────────────────┤
│  platform.rs — distro / arch / PATH detection    │
└───────────────────────────────────────────────┘
```

### The pipeline (single mental model for every command)

```
        ┌─────────┐   ┌──────┐   ┌──────┐   ┌─────────┐
Query → │ search  │ → │ rank │ → │ plan │ → │ execute │ → Outcome
        └─────────┘   └──────┘   └──────┘   └─────────┘
         parallel      priority   FIRST-     privileged,
         fan-out,      + tie-     CLASS      writes registry
         graceful      breakers   concept    only on success
```

**`Plan` is a first-class concept.** No command mutates the system directly.
Every action (install / remove / update) first builds an `InstallPlan`, which can be
displayed (`--dry-run` or when confirmation is warranted), and only then executed.
`why`, `audit`, and `--dry-run` all read the same `InstallPlan`.

---

## 4. Module structure (single crate)

```
jii/
├─ Cargo.toml
├─ README.md
├─ docs/               ARCHITECTURE.md · ROADMAP.md · TASKS.md
├─ data/
│  ├─ catalog.toml     name aliases + category → packages
│  └─ sources/*.toml   declarative (data-driven) sources
├─ src/
│  ├─ main.rs          entrypoint, wiring
│  ├─ error.rs         thiserror error types
│  ├─ model.rs         Query, PackageCandidate, InstallPlan, Step, TrustLevel…
│  ├─ config.rs        TOML config load/merge/validate
│  ├─ platform.rs      distro/arch/PATH/session detection
│  ├─ cache.rs         HTTP + metadata cache (TTL, stale-on-error)
│  ├─ registry.rs      JSON install state store + verification
│  ├─ privilege.rs     sudo/pkexec escalation, batched
│  ├─ engine/
│  │  ├─ mod.rs        Engine struct, public API
│  │  ├─ search.rs     parallel fan-out, timeouts, health
│  │  ├─ ranking.rs    priority + tie-breakers, explanations
│  │  └─ plan.rs       plan assembly & rendering
│  ├─ provider/
│  │  ├─ mod.rs        `Provider` trait + registry of providers
│  │  ├─ dnf.rs        (MVP)
│  │  ├─ copr.rs
│  │  ├─ flatpak.rs
│  │  ├─ github.rs
│  │  ├─ cargo.rs · npm.rs · pipx.rs · go.rs · appimage.rs
│  │  └─ declarative.rs  generic provider driven by data/sources/*.toml
│  ├─ ui/
│  │  ├─ mod.rs        renderer facade (respects --json / --no-color)
│  │  ├─ prompt.rs     [Y/n] prompts, trust barriers
│  │  ├─ progress.rs   spinners / progress bars (indicatif)
│  │  └─ table.rs      candidate & reason tables
│  └─ cli/
│     ├─ mod.rs        clap definitions, global flags
│     └─ commands/     install · remove · update · search · info ·
│                      why · doctor · history · undo · audit · config
└─ tests/              integration tests (dry-run, ranking, parsers)
```

> **As built (current, honest).** The tree above is the *target* layout; the code is
> deliberately flatter until a module earns a split (YAGNI). Today: `engine/` is
> `mod.rs` + `ranking.rs` only — search fan-out lives inline in `engine/mod.rs` (no
> `search.rs`), and plan assembly is each provider's `plan_*` (no `plan.rs`). `ui/` is
> `mod.rs` + `prompt.rs` (no `progress.rs`/`table.rs`; the renderer prints inline).
> `cli/` is a single `mod.rs` — every command handler is a method on `Cli`, not a
> `commands/` module (split it when it grows unwieldy — see AI_CONTEXT tech debt).
> `provider/` has the eight native sources (dnf, copr, flatpak, github, cargo, npm, pipx,
> go) plus shared helpers; `declarative.rs`, `appimage.rs`, and `data/sources/*.toml` are
> **not built yet** (declarative providers are future work). `data/catalog.toml` aliases
> are likewise pending.

---

## 5. Source providers

The core defines **one trait**. Network providers are async (`tokio`).

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable id: "dnf", "flatpak", "github"…
    fn id(&self) -> &'static str;

    /// Base trust level of this source.
    fn trust(&self) -> TrustLevel;

    /// Is this source usable on this machine (binary present / network)?
    async fn is_available(&self) -> bool;

    /// Find candidates. MUST NOT panic on network failure — returns Result;
    /// the Engine renders a failed source as "✗ timeout" and continues.
    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>>;

    /// Build an InstallPlan WITHOUT executing (packages, steps, privileges,
    /// download size, verification). Never performs privileged actions itself.
    async fn plan_install(&self, c: &PackageCandidate) -> Result<InstallPlan>;
    async fn plan_remove(&self, r: &InstalledRecord) -> Result<InstallPlan>;
    async fn plan_update(&self, r: &InstalledRecord) -> Result<InstallPlan>;

    /// OPTIONAL (default None): one plan installing many candidates at once
    /// (e.g. `dnf install a b c`), for batch install. None ⇒ engine falls back
    /// to one plan_install per candidate. (ADR-0025)
    async fn plan_install_many(&self, cs: &[&PackageCandidate]) -> Result<Option<InstallPlan>>;

    /// What is actually installed via this source (used to verify the registry).
    async fn list_installed(&self) -> Result<Vec<InstalledRecord>>;
}
```

> **Evolved since Phase 4.** The trait has grown **optional methods with safe defaults**
> — `is_installed(record)` (default: look up in `list_installed`; file-based sources
> like github override it) and `probe()` (default: local availability; network sources
> report reachability/rate-limit). This **default-method pattern is how the trait grows
> in breadth without a fat interface or core branching** (ADR-0022): future capabilities
> — version enumeration, provider metadata, manager bootstrap — are added the same way,
> and a provider that can't do one inherits the default. `plan_install` returns a plan
> of declarative `Action`s (see §9), not raw `Step`s.
>
> **Shared helpers (not trait methods).** Recurring *implementation* shapes — extracted
> only once a third/fourth provider proved them, to cut maintenance cost, not lines — live
> as free functions in `provider/mod.rs`: `http_client()` (the registry User-Agent /
> transport), `get_json_opt()` (GET → `Ok(None)` on 404, else typed JSON — the exact-name
> registry lookup shared by cargo/npm/pipx/go), and `command_plan()` (a one-`RunCommand`
> `InstallPlan`, also used by dnf's root plans). Deliberately **not** extracted: the
> `PackageCandidate` builder (per-provider; a shared one would leak trust/arch semantics)
> and each source's `list_installed` parser (genuinely divergent per tool).

### Two kinds of providers

1. **Native (code).** `dnf`, `flatpak`, `github`, `cargo`… implement the trait
   directly and own their quirks: parsing (prefer machine output — **dnf5** exposes
   structured output; `flatpak --columns`), arch/libc filtering, API pagination.
2. **Declarative (data-driven).** One generic `DeclarativeProvider` reads
   `data/sources/*.toml`. Simple sources (a specific COPR/RPM repo, a specific
   GitHub repo with an asset pattern) are added **without recompiling**:

   ```toml
   # data/sources/spotify.toml
   [source]
   id = "spotify-repo"
   type = "dnf-repo"
   trust = "community"
   repo_url = "https://…/spotify.repo"
   provides = ["spotify", "spotify-client"]
   ```

### Key contract

A provider **plans but never executes privileged actions**. Execution and privilege
escalation are centralized in the `Engine` + `privilege.rs`, so security and logging
live in one place.

### Source health

Each provider reports health (`Healthy` / `Slow` / `Offline` / `RateLimited`).
Health is surfaced by `doctor`/`benchmark` and used as a ranking tie-breaker.

---

## 6. Ranking

Two stages: **priority (coarse) → tie-breakers (fine)**. Deterministic, explainable,
no hidden ML.

```
score(candidate) =
    priority_rank(source)      // from config: dnf=0, copr=10, flatpak=20, github=30…
  + trust_penalty(trust)       // untrusted → large penalty
  − official_bonus             // "Official Fedora package"
  − freshness_bonus            // version newer than others / upstream
  + install_cost               // large dependency footprint → penalty
  + health_penalty             // slow/rate-limited source ranked lower
```

Rules:
1. **Primary key is `priority_rank`** from config — deterministic and explainable.
2. **On ties / near-ties, tie-breakers decide:** trust → official → freshness →
   user profile → source health → size/speed.
3. **Hard filters before scoring:** incompatible arch/libc, and `untrusted` in
   `--auto` without explicit permission, are dropped.
4. **Explanations are mandatory.** Every recommendation renders its reasons:
   ```
   Recommended: DNF — Official Fedora package, v2.21 (latest)
   Also available: Flatpak (v2.21, sandboxed), GitHub (v2.22, ⚠ unsigned)
   ```

### Profiles (presets over the ranking)

Profiles are simply named priority/tie-breaker presets:

| Profile | Effect |
|---------|--------|
| `stable`  | Prefer distro repositories (default). |
| `latest`  | Freshness beats priority. |
| `sandbox` | Prefer Flatpak. |
| `minimal` | Prefer smallest dependency footprint. |

> **Implemented as of Phase 3:** ranking is `source priority → trust`, with a profile
> adjustment (`sandbox` floats Flatpak up; `stable` is the default). The
> freshness/official/size/health terms above are the target design; `latest` and
> `minimal` are reserved until comparable version and dependency-footprint data are
> collected. Flatpak performs its own polkit elevation, so its steps are not marked
> `needs_root` (JII does not wrap them in sudo/pkexec). Flatpak packages are
> identified by application id (e.g. `org.gimp.GIMP`).

---

## 7. Trust & security model

Three trust levels, attached to the source/repo:

| Level | Examples | Default behavior |
|-------|----------|------------------|
| `official` 🟢 | DNF (Fedora repos), verified Flathub | Installs on `Y`, including `--auto`. |
| `community` 🟠 | COPR, known third-party repos, crates.io | Installs, but `--auto` shows the source; confirmation configurable. |
| `untrusted` 🔴 | Arbitrary GitHub binary, unknown repo/URL | **Always explicit confirmation**, even with `--auto` / `default_yes`. Shows URL and signature status. |

On top: **artifact verification** where the source provides it — GPG-signed RPM,
sha256 from a release, sigstore/cosign, cargo checksum. No signature → `⚠ unsigned`
tag and a raised barrier.

**`default_yes` is not a global boolean** — it is a *trust threshold*
(`default_yes_max_trust`). Below that threshold JII still asks.

> **Auto mode must NEVER install an untrusted source automatically.**

---

## 8. Resilience (network & sources)

- **Parallel fan-out** across available providers, each with its own timeout.
- **Graceful degradation:** a failed/slow source drops out of the results tagged
  `✗ timeout`; the rest still return. Search always yields something.
- **Cache** (`cache.rs`): API/metadata responses cached with a TTL; **stale cache
  is used when a source is unavailable**; background refresh where sensible.
- **GitHub rate-limit** (60/h anonymous) mitigated by cache + optional
  `GITHUB_TOKEN`.

---

## 9. Privilege escalation

Centralized in `privilege.rs`. Providers emit declarative **`Action`s** (see §11); a
`RunCommand` action flagged `needs_root` requests elevation. Providers never call sudo
themselves — the executor (`exec.rs`) does, via `privilege.rs` (`prime()` + `run()`).

> **Evolved (ADR-0007).** The original `Step { argv, needs_root, cwd }` became the
> `Action` enum (`RunCommand` / `Download` / `Place` / `Extract` / `RemoveFile`), each
> with a focused handler in `exec.rs`. `Download` enforces artifact verification before
> the bytes are used; there is no generic "do anything" step. `needs_root` is now a
> derived property of a plan (`InstallPlan::needs_root()`), computed from its
> `RunCommand` actions.

Algorithm:
1. Build the plan. If **no** step needs root (cargo/npm/pipx/`flatpak --user`) →
   run as-is, **no password prompt**.
2. If any step needs root → **show the exact commands**, then escalate **once for
   the whole batch** (not per step).
3. Escalation mechanism detected at runtime: `sudo` when a TTY is present,
   `pkexec` for a graphical session / no TTY. Chosen in `platform.rs`.
4. **JII is never fully run as root.** Only the concrete `dnf install …` runs
   privileged — minimizes the attack surface.
5. Ctrl-C and root errors are handled cleanly; the **registry is written only on
   success** (verify-after-install).

---

## 10. State: registry + verification

- JII keeps a **JSON registry** of *intentions* (`name → source → version → date`)
  under XDG state (`~/.local/state/jii/`).
- The registry is a hint, **not the source of truth** — before `remove`/`update`,
  JII **verifies against the real manager** (`flatpak list`, `dnf list installed`)
  to resolve ownership and dedupe.
- Powers `remove`, `update`, `list`, `history`, `undo`, `why`.

---

## 11. Internal API (core model)

> The block below reflects the **current** code (`src/model.rs`, `src/engine/mod.rs`).
> It evolved from the original design via ADR-0007 (`Action` execution model),
> ADR-0016 (`Extract`) and ADR-0018 (verification recorded on the install record).

```rust
pub struct Query { pub raw: String, pub kind: QueryKind } // Name | Description
pub enum QueryKind { Name, Description }
pub enum TrustLevel { Official, Community, Untrusted }    // Ord: Official < … < Untrusted
pub enum Health { Healthy, Slow, Offline, RateLimited }
pub struct PkgVersion(pub String);                        // raw string, NOT semver (ADR-0009)

pub struct PackageCandidate {
    pub name: String,
    pub source_id: String,
    pub version: Option<PkgVersion>,
    pub trust: TrustLevel,
    pub arch_ok: bool,
    pub signed: bool,
    pub summary: Option<String>,
    pub raw: serde_json::Value,   // source-specific payload for plan_install (opaque to core)
}

pub enum Verification { Sha256(String), Gpg, Sigstore, None }

/// One declarative step of a plan; each has a focused handler in exec.rs (ADR-0007).
pub enum Action {
    RunCommand { argv: Vec<String>, needs_root: bool },
    Download   { url: String, dest: PathBuf, verify: Verification },
    Place      { src: PathBuf, dest: PathBuf, mode: u32 },
    Extract    { archive: PathBuf, member: String, dest: PathBuf, mode: u32 }, // .tar.gz/.zip
    RemoveFile { path: PathBuf },
}

pub struct InstallPlan {
    pub candidate_ref: String,
    pub source_id: String,
    pub actions: Vec<Action>,       // verification lives inside Download; needs_root() is derived
    pub download_size: Option<u64>,
    pub reasons: Vec<String>,       // WHY this was recommended
}

pub struct InstalledRecord {
    pub name: String,
    pub source_id: String,
    pub version: Option<PkgVersion>,
    pub installed_at: DateTime<Utc>,
    pub verification: Option<String>, // how it was verified at install (None = manager-signed)
}

pub struct Engine { /* providers, config, registry, cache, privilege */ }
impl Engine {
    pub async fn search(&self, q: &Query) -> SearchResult;                 // parallel, graceful
    pub fn rank(&self, cands: Vec<PackageCandidate>) -> Vec<PackageCandidate>;
    pub async fn plan_remove(&self, r: &InstalledRecord) -> Result<InstallPlan>;
    pub async fn plan_update(&self, r: &InstalledRecord) -> Result<InstallPlan>;
    // install is uniformly batch (N≥1): group by source, merge where the source can.
    pub async fn plan_install_batch(&self, cs: Vec<PackageCandidate>) -> Result<Vec<BatchPlan>>;
    pub async fn install_batch(&mut self, batch: &[BatchPlan], r: &Renderer) -> Result<()>;
    pub async fn remove(&mut self, p: &InstallPlan, rec: &InstalledRecord, r: &Renderer) -> Result<()>;
    // update = execute plan_update, then refresh the registry record (new version, logged Update).
    pub async fn update(&mut self, p: &InstallPlan, rec: &InstalledRecord,
                        new_version: Option<PkgVersion>, r: &Renderer) -> Result<()>;
    pub async fn resolve_installed(&self, name: &str) -> Result<InstalledRecord>;
    pub async fn diagnose(&self) -> Vec<SourceHealth>;                     // backs `doctor`
    pub fn audit(&self) -> Vec<AuditEntry>;                                // backs `audit`
}
```

`Engine::search` fans out over providers via `tokio`, each with a timeout; failed
sources are tagged, not fatal. `install`/`remove`/`update` run the plan through `exec.rs`
— the **only** place with privileges and the only place that writes the registry (on
success). `update` reuses the search→rank path to re-resolve the latest version from the
owning source, then runs its `plan_update` through the same preview → confirm → execute
flow as install; there is no per-source branching (the engine resolves the provider by
`source_id`).

> **Known seam (ADR-0022).** `install`/`remove` currently take a `&Renderer` so the
> executor can print progress — the one place a `ui` type reaches into the engine. To
> enable multiple frontends (GUI, Discover, TUI, Web), this is to be replaced by a small
> progress-event/`ProgressSink` trait **before** a second frontend lands — not earlier
> (YAGNI). No new `ui` types may enter engine signatures meanwhile.

---

## 12. Configuration

TOML at `~/.config/jii/config.toml` (optional — sane defaults). Precedence:
**CLI flag > env > config > default**.

```toml
[sources]
priority = ["dnf", "copr", "flatpak", "github", "cargo", "npm", "pipx", "go"]
disabled = []

[install]
profile = "stable"                  # stable | latest | sandbox | minimal
default_yes = true
default_yes_max_trust = "community" # below this trust level, still ask
auto = false

[trust]
require_signature = "untrusted"
allow_untrusted_auto = false

[network]
timeout_secs = 8
cache_ttl_secs = 3600
github_token_env = "GITHUB_TOKEN"

[ui]
color = "auto"      # auto | always | never
locale = "auto"
```

---

## 13. Commands (overview)

| Command | Purpose |
|---------|---------|
| `jii <name…>` / `jii install <name…>` | one or many: search → rank → plan → group/merge per source → one confirm → one run (ADR-0025) |
| `jii remove <name>` | resolve source (registry→verify) → plan → remove |
| `jii update [<name>]` | named: update that package via its source; bare: update the whole system — aggregate every manager's bulk upgrade + per-record fallback (ADR-0034) |
| `jii search <query>` | show candidates only, no install |
| `jii info <name>` | availability, versions, trust, size |
| `jii why <name>` | explain how/why it was (or would be) installed |
| `jii doctor` | source availability, latency, rate limits + Tier-1 system checks (PATH, token) |
| `jii recommend [<id>]` | curated, distro-aware suggestions (list); apply one via the normal install path (ADR-0033) |
| `jii setup` | first-run wizard: output mode + optional system check |
| `jii history` | installation history |
| `jii undo` | undo last install / remove / update |
| `jii audit` | signatures, sha256, GPG, sigstore, source, trust |
| `jii list` | what JII installed (from the registry) |
| `jii config <get\|set\|edit>` | manage configuration |

Global flags: `-y/--yes`, `-n/--no`, `--auto`, `--source <id>`, `--profile <p>`,
`--dry-run`, `-v/--verbose`, `--json`, `--no-color`.

---

## 14. Distribution of JII itself

Primary channel: a **COPR repository** (`copr enable …/jii`), plus a signed static
binary on GitHub Releases. JII installs the same way it recommends — "the cobbler
wears shoes."

---

## 15. Non-goals (MVP)

- Semantic / AI search (Stage 4) — architecturally reserved, not built.
- Non-Fedora distros — reserved behind the `platform` abstraction.
- SQLite, workspace split, plugin SDK — future, no architecture change needed.
- GUI / other frontends — future; the model is ready, but the engine must first be made
  **UI-free** (decouple the `Renderer` execution seam) and likely exposed as a library
  (workspace split). Tracked in ADR-0022; not started.
