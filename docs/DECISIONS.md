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

---

## ADR-0013 — Minimal CI enforces the DoD; other infra deferred deliberately

**Status:** Accepted

**Decision:** Add GitHub Actions CI that runs, on every push/PR,
`cargo clippy --all-targets -- -D warnings` (which also proves the build) and
`cargo test`, both `--locked`. Add Dependabot (weekly, grouped) for Cargo crates and
Actions. Add `.editorconfig`. **Deliberately do not add** (for now): a rustfmt check,
`rust-toolchain.toml`, `CONTRIBUTING.md`, `SECURITY.md`, issue/PR templates,
`CODEOWNERS`, `deny.toml`/`cargo-audit`, or a release workflow.

**Reason:** The handoff guarantee ("build clean, clippy clean, tests pass") was
enforced only by discipline; CI makes it automatic and visible to any future agent.
Dependabot matters because JII downloads and installs software (supply-chain
hygiene). `.editorconfig` keeps hand-formatting consistent because rustfmt is not on
the dev host.

**Alternatives considered / why the rest is skipped:**
- **rustfmt check in CI** — rustfmt is not installed on the dev host and the code is
  hand-formatted; a fmt gate would fail and fmt is not part of our DoD.
- **`rust-toolchain.toml`** — the dev host uses system Rust (no rustup), so the file
  is inert locally; CI already pins the toolchain via `dtolnay/rust-toolchain@stable`.
- **`CONTRIBUTING.md`** — [AGENTS.md](../AGENTS.md) already is the onboarding/workflow
  doc; a second one would duplicate and drift.
- **`SECURITY.md`, issue/PR templates, `CODEOWNERS`** — the repo is private,
  pre-release, single-maintainer; these pay off with external contributors/a public
  release. The security *model* already lives in ADR-0005/0006/0007.
- **`deny.toml` / `cargo-audit`** — overlaps Dependabot and adds config upkeep and
  false-positive CI failures; revisit if supply-chain risk grows.
- **Release workflow** — no releases yet; distribution is planned for Phase 7.

**Consequences:** CI is the automated backbone of the DoD. The runner has no
dnf5/flatpak, so CI covers unit logic only (parsers, ranking, exec fileops, elevation
prefixing); end-to-end `--dry-run`/install verification stays a manual step on
Fedora. Revisit the deferred items when JII goes public or starts cutting releases.

---

## ADR-0014 — GitHub provider: `owner/repo` queries, network in `search`, raw binaries first

**Status:** Accepted

**Decision:** The GitHub Releases provider resolves a query of the form `owner/repo`
to that repo's latest release. It does **all** network I/O in `search` — fetching the
release and any checksums file — and embeds the download URL, filename, size and
resolved sha256 into the candidate's `raw`, so `plan_install` is pure. It selects a
single **raw executable** asset for the host arch (Linux, musl preferred over gnu),
plans `Download`→`Place` into `~/.local/bin` (no root), and classifies the source as
`untrusted`.

**Reason:**
- **`owner/repo` only** — GitHub has no reliable name→repo search; guessing the repo
  for a bare name is a separate, error-prone problem. An explicit slug is honest and
  unambiguous now; broad resolution can come later without changing the model.
- **Network in `search`** — mirrors how dnf/flatpak stash everything the plan needs in
  `raw`; keeps `plan_install` deterministic and unit-testable, and confines failure
  modes to the search step the engine already degrades gracefully (ADR-0010).
- **Raw binaries first** — the execution model has `Download`/`Place` but no extractor,
  and most `.tar.gz`/`.zip` releases need one. Shipping the raw-binary slice proves the
  whole download→verify→place→trust path end-to-end (e.g. `jqlang/jq`) without
  prematurely expanding the model.
- **`untrusted` trust** — a third-party binary from an arbitrary repo is exactly the
  `untrusted` tier (ADR-0006): always explicitly confirmed, even under `--auto`. A
  verified sha256 raises confidence but not the trust tier.

**Alternatives considered:**
- Shell out to `curl | tar` in a `RunCommand` — rejected: unverifiable, leaks source
  specifics into argv, no enforced checksum (the whole point of ADR-0007).
- Fetch checksums lazily in `plan_install` — rejected: puts network in planning and
  makes the plan non-deterministic/harder to test.
- Treat GitHub as `community` when a checksum verifies — rejected: provenance, not
  integrity, defines the trust tier; the binary is still arbitrary code.

**Consequences:**
- **Archives:** `.tar.gz`/`.tgz` are now supported via `Action::Extract` (ADR-0016);
  `.zip`/`.tar.xz`-only releases still yield no candidate.
- **`jii remove` for GitHub installs** — resolved via `Provider::is_installed(record)`:
  the default checks `list_installed`, github overrides it to test that
  `~/.local/bin/<name>` exists. This confirms a file-based install without a manifest
  or a new `InstalledRecord` field, and without source branching in the core. (The
  install path is deterministic, so it need not be stored.)
- `is_available` returns `true` (GitHub is remote, no local binary); real rate-limit /
  reachability health for `doctor` is a later slice.

---

## ADR-0015 — Frontend-agnostic engine; frontends stay thin

**Status:** Accepted

**Decision:** All business logic — search, ranking, planning, the trust model,
execution, and the registry — lives in the engine and operates purely on the model.
Frontends are **thin**: they parse input, call the engine, and render results. The CLI
(`cli/` + `ui/`) is exactly this today, and any future frontend (notably the GUI in
[ROADMAP.md](ROADMAP.md) "Future ideas") must be the same. A frontend **never**
duplicates or reimplements engine logic; if it needs behavior the engine doesn't
expose, the *engine* grows and both frontends share it.

**Reason:** Recorded now — while there is only one frontend — because the value of the
rule shows up the moment there are two. A GUI that reimplements ranking or the trust
barrier would drift from the CLI, doubling bugs and splitting the security model. One
engine, many thin frontends keeps behavior identical everywhere and keeps the
security-sensitive logic in a single audited place.

**Alternatives considered:**
- Let each frontend own some logic for convenience — rejected: guarantees drift and
  duplicate trust/verification code paths (the exact thing that must stay singular).
- A cross-process API (daemon) as the only integration path — deferred: the engine is
  already a library operating on the model; a GUI can link it directly. Introduce a
  service only if a real need (privilege separation, multi-client) appears.

**Consequences:**
- Keep `cli/` and `ui/` free of decision-making: no ranking, trust, or plan logic
  there — only input parsing and presentation. New behavior belongs in the engine.
- The engine's public API is a supported contract; review it before adding a frontend.
- Frontend-only concerns the GUI will need (icons, screenshots, progress events) must
  be surfaced *through the model* by providers/engine, not fetched ad hoc in the UI.

---

## ADR-0016 — `Action::Extract`: locate the binary by name, `.tar.gz` first

**Status:** Accepted

**Decision:** Add an `Extract { archive, member, dest, mode }` action to the execution
model. The handler decompresses a **gzip tarball** in memory and installs one member:
the entry whose file-name matches `member`, or — failing that — the sole executable
file in the archive. The `Download` step verifies the archive first, so extraction
runs on trusted bytes. Only `.tar.gz`/`.tgz` is handled for now; a raw binary is still
preferred when a release offers both.

**Reason:**
- **Extract by binary name, not internal path** — release tarballs have wildly varying
  layouts (`rg`, `bin/rg`, `ripgrep-14.1.0-.../rg`). The provider can't know the layout
  without downloading during `search` (which we forbid, ADR-0014). Naming the wanted
  binary and letting the executor find it keeps the plan declarative and network-free.
- **Sole-executable fallback** — most CLI tarballs contain one binary plus docs/
  completions; the executable bit disambiguates when the name doesn't match (e.g. repo
  `ripgrep` → binary `rg`).
- **`.tar.gz` first** — by far the dominant format for Linux release binaries. Doing
  one format well (with `flate2` + `tar`, both pure-Rust) beats a shallow multi-format
  attempt. `.zip`/`.tar.xz` slot in behind the same action later.
- **Verify before extract** — reusing `Download`'s checksum enforcement means the
  archive is trusted before we read it; no new verification surface.

**Alternatives considered:**
- Encode the internal member path in the plan — rejected: requires inspecting the
  archive at plan time (network in `search`) and is brittle across releases.
- Shell out to `tar`/`gunzip` in a `RunCommand` — rejected: unverifiable, leaks to the
  system tool, and breaks the "each action has a focused, testable handler" rule.
- Support `.zip`/`.tar.xz` now — deferred: more deps and surface for little immediate
  gain; add when a real target tool needs them.

**Consequences:**
- The installed file is named after the repo (`~/.local/bin/<repo>`), even when the
  archive's binary basename differs — acceptable while repo==binary is the common case.
- Extraction is in-memory; pathologically huge tarballs would use proportional memory
  (fine for CLI tools). Revisit with streaming if it ever matters.
- Adding a format = one more branch in the extractor + widening `classify` in github;
  the action and provider contracts don't change.

---

## ADR-0017 — COPR provider: exact project-name match, two-step root plan

**Status:** Accepted

**Decision:** COPR has no package search, only `project/search`. The provider resolves
a query to the COPR project whose **name equals the query** (so the package name is
known) and that **builds for the host Fedora/arch**, preferring the one with the most
Fedora chroots. It plans the two privileged steps a user runs by hand —
`dnf5 -y copr enable <owner>/<project>` then `dnf5 -y install <name>` — at `community`
trust. All network is in `search`; `is_installed` verifies via rpm; `list_installed`
is empty (COPR packages are ordinary RPMs and can't be attributed to COPR).

**Reason:**
- **Exact project-name match** — COPR project search is noisy (matches descriptions,
  returns 50 loosely-related projects). Requiring `projectname == query` means the
  package to install is known (`== name`) and avoids installing something unrelated.
- **Fedora/arch chroot filter** — only offer projects that actually build for the
  running system, so the plan won't fail at `dnf5 install`.
- **Two root `RunCommand`s** — this is exactly what a user does manually; it fits the
  existing command-execution model with no new action type. `-y` is a global dnf5
  option, so it precedes `copr` (`dnf5 -y copr enable …`).
- **`community` trust** — COPR repos are user add-ons; below-official but not arbitrary
  binaries. The exact `owner/project` is shown in the plan for confirmation.

**Alternatives considered:**
- Query a package-list API per project to confirm the package — rejected: N extra
  calls per search; exact project-name match is a good-enough, cheap proxy.
- Accept substring name matches — rejected: the package name becomes unknown and the
  install target ambiguous.
- Rank same-named projects by real popularity — no such metric in `project/search`;
  the chroot-count heuristic is the best cheap signal, backed by the visible-plan +
  confirmation safety net.

**Consequences:**
- **Ambiguous picks are possible** among identically-named projects (a widely-building
  fork can win the heuristic). Mitigated by showing `owner/project` and requiring
  confirmation; a better signal is future work.
- No version in the candidate (COPR search gives none); the plan shows the repo and
  package, not a version.
- `search` hits the COPR API on every query; results are cached by the engine and the
  call is bounded by the per-source timeout with graceful degradation (ADR-0010).

---

## ADR-0018 — `jii audit`: record verification at install time

**Status:** Accepted

**Decision:** Add a `verification: Option<String>` field to `InstalledRecord`, set at
install time from the plan's `Download` step (`Verification::label()`), and back it
with `jii audit`, which reports per install: source, trust (from the owning source),
verification, and any concerns (untrusted source / no checksum / disabled source).
`None` verification means the install ran through a self-verifying package manager
(dnf/copr GPG, flatpak). The engine computes the audit (`audit()` plus pure
`resolve_verification`/`audit_concerns`); the CLI only renders (human table + `--json`).

**Reason:**
- **Record provenance, don't guess it** — whether a github binary was sha256-verified
  or unverified depends on what the release published; it is only known at install
  time. Storing it is the only way `audit` can report it faithfully.
- **`None` = manager-verified** — command-based installs have no `Download` step, and
  all our command-based sources self-verify (rpm GPG, flatpak signatures). So `None`
  is a truthful, source-agnostic category, and old registry entries (pre-field) read
  correctly via `#[serde(default)]`.
- **Engine owns the logic** (ADR-0015) — trust resolution and concern rules are
  business logic; the CLI stays a thin renderer.

**Alternatives considered:**
- Derive verification purely from the source at audit time — rejected: can't
  distinguish a verified from an unverified github install, which is the whole point.
- A separate audit store — rejected: the registry already records installs; one more
  optional field is simpler and versioned with the record.

**Consequences:**
- The registry schema grew one optional field; existing state files still load.
- `audit` is registry-based and fast (no live provider calls); it does not currently
  verify the package is still present (that would need provider calls) — a possible
  future "installed but missing" check.
- New verification methods (GPG/sigstore) flow through automatically: their label is
  recorded and shown, and they count as `Checksum` (verified) in concerns.

## ADR-0019 — `jii doctor` health: providers probe, engine judges

**Status:** Accepted

**Decision:** Add a `Provider::probe() -> Probe { reachable, rate_limited, detail }`
trait method (default: local binary availability). Network sources override it —
github hits `/rate_limit` (surfacing `remaining/limit` as `detail` and setting
`rate_limited` when the budget is 0), copr pings `project/search` for reachability.
`diagnose()` times each probe and maps its raw facts to a `Health` via a pure
`health_from(reachable, rate_limited, latency)` (Offline → RateLimited → Slow →
Healthy, in that precedence). `SourceHealth` gained an optional `detail`, rendered in
both the human table and `--json`.

**Reason:**
- **Providers report facts, the engine decides** (ADR-0015) — "reachable at 7 s" and
  "0 requests left" are facts a source knows; whether that means `Slow` or
  `RateLimited` is a product judgement that belongs in the engine, testable in
  isolation (`health_from` has a unit test; no network needed).
- **Rate limit is a real GitHub failure mode** — unauthenticated API access is 60
  req/h; a user hitting the wall needs to *see* it (and that a token lifts it), not
  get an opaque "search failed" later. `RateLimited` is reachable-but-degraded, so it
  ranks below `Offline` but above `Slow`.
- **`detail` keeps it honest** — showing `58/60 req left` explains *why* a source is
  healthy-but-watch, without the engine inventing prose.

**Alternatives considered:**
- Reuse `is_available()` for health — rejected: it only answers "binary present",
  can't distinguish reachable/rate-limited/slow for a network API.
- Compute rate-limit health in the provider — rejected: that is the decision logic
  ADR-0015 keeps in the engine; the provider only reports the raw budget.

**Consequences:**
- Adding a health signal to a source is a small `probe()` override; the engine and CLI
  need no per-source branches.
- `probe()` for github spends one API request on `/rate_limit` — which itself does not
  count against the limit, so `doctor` is free to run.
- `Health::RateLimited` is wired end-to-end but only reproducible live by exhausting
  the budget; the mapping is covered by `health_from`'s unit test instead.
