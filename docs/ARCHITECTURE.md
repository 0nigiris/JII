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

    /// What is actually installed via this source (used to verify the registry).
    async fn list_installed(&self) -> Result<Vec<InstalledRecord>>;
}
```

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

Centralized in `privilege.rs`. Providers return steps flagged `needs_root`; they
never call sudo themselves.

```rust
pub struct Step {
    pub argv: Vec<String>,   // exact command
    pub needs_root: bool,
    pub cwd: Option<PathBuf>,
}
```

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

```rust
pub struct Query { pub raw: String, pub kind: QueryKind } // Name | Description
pub enum QueryKind { Name, Description }
pub enum TrustLevel { Official, Community, Untrusted }
pub enum Health { Healthy, Slow, Offline, RateLimited }

pub struct PackageCandidate {
    pub name: String,
    pub source_id: String,
    pub version: Option<Version>,
    pub trust: TrustLevel,
    pub arch_ok: bool,
    pub signed: bool,
    pub summary: Option<String>,
    pub raw: serde_json::Value,   // source-specific payload for plan_install
}

pub enum Verification { Gpg, Sha256(String), Sigstore, None }

pub struct InstallPlan {
    pub candidate_ref: String,
    pub steps: Vec<Step>,
    pub verification: Vec<Verification>,
    pub download_size: Option<u64>,
    pub needs_root: bool,
    pub reasons: Vec<String>,     // WHY this was recommended
}

pub struct InstalledRecord {
    pub name: String,
    pub source_id: String,
    pub version: Option<Version>,
    pub installed_at: DateTime<Utc>,
}

pub struct Engine { /* providers, config, registry, cache */ }
impl Engine {
    pub async fn search(&self, q: &Query) -> SearchResult;             // parallel, graceful
    pub fn rank(&self, cands: Vec<PackageCandidate>) -> Ranked;        // priority + tie-breakers
    pub async fn plan(&self, c: &PackageCandidate) -> Result<InstallPlan>;
    pub async fn execute(&self, p: InstallPlan, d: Decision) -> Result<Outcome>;
    pub async fn resolve_installed(&self, name: &str) -> Result<InstalledRecord>;
}
```

`Engine::search` fans out over providers via `tokio`, each with a timeout; failed
sources are tagged, not fatal. `execute` is the **only** place with privileges and
the only place that writes the registry.

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
| `jii <name>` / `jii install <name>` | search → rank → plan → confirm → install |
| `jii remove <name>` | resolve source (registry→verify) → plan → remove |
| `jii update [<name>]` | update one/all via the correct manager |
| `jii search <query>` | show candidates only, no install |
| `jii info <name>` | availability, versions, trust, size |
| `jii why <name>` | explain how/why it was (or would be) installed |
| `jii doctor` | source availability, latency, rate limits, problems |
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
- SQLite, workspace split, GUI, plugin SDK — future, no architecture change needed.
