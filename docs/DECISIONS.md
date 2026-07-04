# JII — Architecture Decision Records

This file records **why** JII is built the way it is. Each entry is an ADR: a
decision that shapes future development, the reasoning behind it, the alternatives
weighed, its current status, and its consequences.

Rules:

- **Never leave an architectural decision only in an AI conversation or a commit
  message.** If a decision affects how future code is written, it belongs here.
- Keep entries short and durable. Detail lives in [ARCHITECTURE.md](ARCHITECTURE.md);
  this file is the *why*, not the *how*.
- Do not rewrite history: to reverse a decision, add a new ADR that supersedes the
  old one and set the old one's status to `Superseded by ADR-XXXX`.

Statuses: `Accepted` · `Superseded` · `Deprecated` · `Proposed`.

---

## ADR-0001 — Single Cargo crate, not a workspace

**Status:** Accepted

**Decision:** Ship JII as one binary crate with a modular layout (`provider/`,
`engine/`, `cli/`, `ui/`, …), not a multi-crate Cargo workspace.

**Reason:** The MVP is small enough that a workspace adds coordination overhead
(version bumps, inter-crate APIs, build graph) without payoff. Module boundaries
give us the separation we need now.

**Alternatives considered:** Workspace with `jii-core` / `jii-providers` / `jii-cli`
crates — deferred until the code actually needs independent compilation or reuse.

**Consequences:** Refactoring across module boundaries is cheap. A future split is
possible because modules already respect clean boundaries. Recorded as future work
in [ROADMAP.md](ROADMAP.md).

---

## ADR-0002 — JSON state file, not SQLite

**Status:** Accepted

**Decision:** Persist the install registry as a JSON file under
`~/.local/state/jii/state.json`, behind the `Registry` API.

**Reason:** The registry records *intentions* ("JII installed X via source Y") and
is verified against the real package manager on use, so it is a small hint store,
not a source of truth. JSON is dependency-light, human-inspectable, and trivial to
diff.

**Alternatives considered:** SQLite — more robust for large/concurrent state, but
premature; the dataset is tiny and single-writer.

**Consequences:** Migration to SQLite later stays behind the same `Registry` API, so
callers do not change. The registry must always be treated as a hint, never trusted
blindly (verification against the manager is mandatory — see ARCHITECTURE §state).

---

## ADR-0003 — `Plan` is a first-class, previewable concept

**Status:** Accepted

**Decision:** Every action builds an `InstallPlan` (a list of `Action`s + reasons)
*before* touching the system. `--dry-run` renders the plan and exits; the same
`describe_action` rendering is printed as each action executes.

**Reason:** Transparency and safety are core product values. The user must be able to
see exactly what will happen — commands, downloads, file placements, elevation — and
`why`, before consenting.

**Alternatives considered:** Execute-as-you-go with inline prompts — rejected: it
cannot show the whole picture up front and makes `--dry-run` and `why` second-class.

**Consequences:** Providers must express *everything* as declarative `Action`s and
may not perform side effects during planning. What is previewed is exactly what runs.

---

## ADR-0004 — The core never branches on the source

**Status:** Accepted

**Decision:** All source-specific logic lives behind the `Provider` trait. The engine,
ranking, executor, and UI operate only on the model (`PackageCandidate`,
`InstallPlan`, `Action`, `TrustLevel`). There is no `if source == "dnf"` anywhere in
the core.

**Reason:** Adding a source must not require touching the core. This keeps the
system open for extension (GitHub, COPR, cargo, npm…) and closed for modification.

**Alternatives considered:** A central match over source ids — rejected: it turns
every new source into a core edit and a merge magnet.

**Consequences:** New sources implement `Provider`; simple ones become declarative
TOML in `data/sources/`. Any behavior a source needs must be expressible through the
model — when it is not, the model evolves for *all* sources (see ADR-0007), never by
special-casing one.

---

## ADR-0005 — JII is never fully run as root

**Status:** Accepted

**Decision:** JII runs as the user. Only concrete privileged steps escalate, via
`sudo` (on a TTY) or `pkexec` (graphical), batched and shown before running.
Escalation lives solely in `privilege.rs`; providers plan privileged actions but
never execute them.

**Reason:** Running an installer entirely as root is a large, unnecessary attack and
mistake surface. Least privilege plus visible, exact commands keeps the user in
control.

**Alternatives considered:** Re-exec the whole process under sudo — rejected: broad
privilege, worse auditability.

**Consequences:** The executor primes credentials once (`sudo -v`) so a batch prompts
at most once. Flatpak handles its own polkit elevation, so its actions are
`needs_root=false`. Any new privileged step must route through `privilege.rs`.

---

## ADR-0006 — Trust levels drive consent; `default_yes` is a threshold

**Status:** Accepted

**Decision:** Every candidate carries a `TrustLevel` (`official`/`community`/
`untrusted`). The auto-confirm setting is a trust *threshold*
(`default_yes_max_trust`), not a global boolean. `untrusted` is always confirmed
explicitly, even under `--auto`.

**Reason:** "Install without asking" is only safe up to a trust level. A single
global yes would silently install arbitrary binaries; a threshold encodes the real
policy.

**Alternatives considered:** Global `--yes` boolean — rejected as unsafe. Per-source
allowlists — more config for the same effect the threshold gives declaratively.

**Consequences:** Ranking and the confirmation barrier both read `TrustLevel`. New
sources must classify their trust honestly; anything unverifiable defaults to
`untrusted`.

---

## ADR-0007 — Expressive execution model: `Action` enum + plan executor

**Status:** Accepted (supersedes the original argv-only `Step`)

**Decision:** Replace the command-only `Step` with an `Action` enum —
`RunCommand`, `Download`, `Place`, `RemoveFile` — and a dedicated plan executor
(`exec.rs`) that dispatches each variant to a focused handler. `Download` enforces
`Verification` (sha256 now; gpg/sigstore fail closed until implemented).
`privilege.rs` is reduced to a single responsibility: `prime()` once + `run()` one
command.

**Reason:** GitHub Releases needs download → verify → place, which an argv-only step
cannot express, and the old `Verification` field was passive/unenforced. This is the
kind of model change reserved for when a *real* implementation problem appears (it
did, entering Phase 4) rather than speculative redesign.

**Alternatives considered:**
- Keep `Step` and shell out to `curl`/`install` — rejected: unverifiable, leaks
  source specifics into argv, no enforced checksum.
- A single generic "do anything" executor step — rejected: each action must have one
  clear responsibility and be previewable/auditable on its own.

**Consequences:** Every action is previewable (`--dry-run`), explainable (`why`), and
individually verified. DNF and Flatpak keep emitting `RunCommand` and are unchanged.
Adding GitHub means emitting `Download`+`Place` — no executor special-casing. Real
GPG/sigstore verification is now a localized change in `verify_bytes`.

---

## ADR-0008 — COPR grouped with GitHub in Phase 4, not Phase 3

**Status:** Accepted

**Decision:** Defer the COPR provider from Phase 3 to Phase 4, alongside GitHub
Releases.

**Reason:** `dnf5 copr` has no search subcommand, so resolving *which* COPR project
provides a package requires the COPR web API — the same fuzzy name→source resolution
problem as GitHub, plus root repo-enable and trust handling. The two belong together.

**Alternatives considered:** Ship a name-only COPR enable in Phase 3 — rejected: no
discovery, poor UX, and it would model trust/root differently from GitHub.

**Consequences:** Phase 3 shipped DNF + Flatpak ranking; Phase 4 tackles the shared
name→source resolution problem once, for both COPR and GitHub.

---

## ADR-0009 — `PkgVersion(String)` instead of `semver::Version`

**Status:** Accepted

**Decision:** Represent versions as an opaque `PkgVersion(String)` newtype, not a
parsed semver value.

**Reason:** Sources are heterogeneous — RPM uses EVR (`2.63.1-1.fc44`, with epoch and
release), GitHub uses tags (`v2.63.1`), Flatpak its own scheme. `semver::Version`
cannot parse EVR, and forcing one scheme loses fidelity. Discovered during Phase 1
implementation.

**Alternatives considered:** `semver` (failed on real RPM EVR); a per-source version
enum (premature — cross-source comparison is not needed until a freshness
tie-breaker exists).

**Consequences:** Versions display faithfully. Cross-source version comparison is
deferred to when a freshness ranking tie-breaker actually needs it (reserved in
Phase 3 notes); it will be added as comparison logic over `PkgVersion`, not by
changing the type.

---

## ADR-0010 — Parallel search with per-source timeouts and stale-on-error cache

**Status:** Accepted

**Decision:** Search all providers concurrently (fan-out), each bounded by a
per-source timeout. Results are cached on disk with a TTL; on error/timeout a stale
cache entry is served if present. Failed sources are tagged (e.g. `timeout`) and
reported without failing the whole search.

**Reason:** Sources have very different latencies (local dnf vs. network GitHub). One
slow or offline source must not block or fail the others; graceful degradation beats
all-or-nothing.

**Alternatives considered:** Sequential search — too slow. Hard-fail on any source
error — brittle offline and against slow mirrors.

**Consequences:** The engine owns concurrency and caching; providers stay simple and
synchronous-looking. `jii doctor` surfaces availability/latency/health so users can
see why a source was skipped.

---

## ADR-0011 — Repository is the single source of truth; AI-agnostic handoff

**Status:** Accepted

**Decision:** All knowledge required to continue development lives in the repo:
[ARCHITECTURE.md](ARCHITECTURE.md) (design), [ROADMAP.md](ROADMAP.md) +
[TASKS.md](TASKS.md) (plan/progress), this file (decisions), and
[AI_CONTEXT.md](AI_CONTEXT.md) (current state). No important project knowledge is
allowed to exist only inside an AI conversation window.

**Reason:** JII should be continuable by *any* agent — Claude Code, another AI, or a
human — with minimal context loss. Conversation context is ephemeral and
model-specific; the repository is durable and shared.

**Alternatives considered:** Rely on AI conversation summaries / memory — rejected:
lost on context reset, not shared across tools or people, not reviewable.

**Consequences:** Every work session ends by updating TASKS.md and AI_CONTEXT.md,
recording any architectural decision here, and committing — the mandatory **AI
Handoff Policy** in [CLAUDE.md](../CLAUDE.md).

---

## ADR-0012 — Machine-readable tool output; parsers are pure and unit-tested

**Status:** Accepted

**Decision:** Providers drive underlying tools in their machine-readable mode and
parse *structured* output, never human-formatted text. Concretely: `dnf5 repoquery`
with an explicit `--queryformat` using a real tab separator; `flatpak search
--columns=…`. All parsing lives in pure functions (e.g. `parse_candidates`,
`parse_rows`, `parse_installed_records`) that are unit-tested on fixed sample output.

**Reason:** Human-facing output is unstable across tool versions and locales and is
painful to parse reliably. Structured output is stable and testable, and pure parsers
can be verified without invoking the real tool.

**Alternatives considered:** Scrape default human output — rejected: brittle,
locale-sensitive, untestable offline. A structured library/API binding per tool —
none stable enough for dnf5/flatpak today; the CLI machine formats are the contract.

**Consequences:** Every new provider must (a) request the tool's machine format and
(b) put parsing in a pure function with a unit test over a captured sample. This is
part of the Definition of Done (see [TASKS.md](TASKS.md)). Runtime tool invocation is
funneled through shared helpers (`run_capture`, `which`, `nonempty_lines`) in
`provider/mod.rs`.
