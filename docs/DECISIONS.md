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
- **Archives:** `.tar.gz`/`.tgz` and `.zip` are supported via `Action::Extract`
  (ADR-0016); `.tar.xz`-only releases still yield no candidate.
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

**Update (2026-07-04):** `.zip` is now supported exactly as this ADR predicted — the
executor dispatches on the archive's file-name extension into `read_tar_gz` / `read_zip`
(both yielding the same `ArchiveFile` list, so member selection and writing are
format-agnostic); github's `classify` gained an `AssetKind::Zip`, ranked below `TarGz`
(which preserves unix modes) but above nothing. No change to the `Action`/`Provider`
contracts, confirming the format seam. Also hardened `classify` to reject delta-patch
assets (`.bsdiff`/`.patch`/`.delta`/`.zsync`) that otherwise masqueraded as raw binaries
(surfaced by `denoland/deno`, which ships a `*.bsdiff` alongside its Linux `.zip`).
`.tar.xz` remains unsupported (would add an xz decoder dependency for little gain yet).

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

---

## ADR-0020 — JII is a universal layer, not another package manager

**Status:** Accepted (foundational — restates the project's reason to exist)

**Decision:** JII is a **unifying layer** over the package sources that already exist
(DNF, COPR, Flatpak, GitHub Releases, and later Cargo/npm/pipx/Go/Homebrew/Nix/…). It
provides one interface for **search, choice, trust, install, and management** across
all of them. It deliberately does **not**:

- become another package manager or dependency resolver;
- invent a new package format;
- maintain its own package archive or replace any ecosystem;
- ask users to change their habits or migrate away from the tools they use.

Every source is reached through its **own native mechanism** (dnf5, flatpak, the
GitHub API, …); JII orchestrates, it does not re-implement. When JII installs from a
source, the artifact remains a first-class citizen of that source (an rpm is still an
rpm, a Flatpak is still a Flatpak) and can still be managed by that source's own tools.

**Reason:**
- **This is the product.** The value is *unification with honesty* — "here is the best
  way to get X, from Y, and here's why" — not another silo competing with the others.
  Fighting DNF/Flatpak/Homebrew or minting a JII format would recreate the very
  fragmentation JII exists to hide.
- **It keeps the architecture small and safe.** Because JII never owns packaging, it
  never inherits a package manager's hardest problems (dependency solving, conflict
  resolution, an archive to host and sign). It plans and delegates; the ecosystems keep
  doing what they are good at.
- **It is the test for every future feature.** The guiding question is *"does this make
  the user's life easier without making the architecture heavier?"* A feature that
  pulls JII toward owning packaging, or toward a per-source special case in the core,
  fails that test.

**Alternatives considered:**
- A JII-native package format / store — rejected: maximal complexity, maximal
  ecosystem friction, directly opposed to the mission.
- A thin meta-CLI that only shells out with no model — rejected: loses trust, ranking,
  provenance and the "why", which are the actual value.

**Consequences:**
- Reinforces the load-bearing rules: **core never branches on the source** (ADR-0004),
  everything is a **provider** behind one trait, and **`Plan` is first-class** (ADR-0003)
  so JII delegates rather than executes packaging itself.
- New sources are *additive*: a `Provider` (native or, later, declarative TOML). Growth
  is in breadth of coverage, not in a growing core.
- Frontends (CLI now, GUI/software-center later) are thin over this one engine
  (ADR-0015); the "universal layer" is the engine, not any single UI.

---

## ADR-0021 — Integrate external backends only through their stable public API

**Status:** Accepted (forward-looking; no code yet)

**Decision:** When JII integrates another project as a backend for some capability
(the motivating case is **UPAC** / a future `libupac`, but this is the general rule),
it does so **only through that project's stable, public API** — never against its
internals, private types, or unreleased interfaces. Such an integration is modelled as
just **another `Provider`** (or a small adapter behind an existing trait). Concretely:

- JII depends on a **published, versioned** interface of the external project, and pins
  a compatible version range like any other dependency.
- The two projects stay **independent** — neither absorbs nor forks the other. Each
  evolves on its own; JII uses the other where it genuinely solves a problem better.
- **If the stable public API does not exist yet, JII implements nothing** — the
  interaction is only designed on paper (this ADR) until the API is real.
- No JII behavior may depend on undocumented behavior or internal data structures of
  the external project; if JII needs something the public API can't express, the
  request goes upstream to that project's API, it is not reached around.

**Reason:**
- **Coupling to internals is a trap** — it makes both projects fragile and turns
  cooperation into a maintenance burden, the opposite of the intended collaboration.
- **The `Provider` boundary already is the right seam** — JII was built so that any
  source is reachable behind one narrow trait; an external library is just such a
  source. No core change is needed to accommodate one (ADR-0004/0020).
- **Design-before-code** matches the project's rule: architecture first, and here the
  prerequisite (a stable public API) is outside our control, so we wait.

**Alternatives considered:**
- Vendor or fork the external code — rejected: violates the "stay independent" premise
  and couples releases.
- Depend on internal crates/types directly for speed — rejected: brittle, and it would
  leak another project's model into JII's core.

**Consequences:**
- A future `libupac`-backed provider is a drop-in once the API is published; nothing in
  the core needs to know it exists beyond registering the provider.
- Until then this is a *documented intent*, not a dependency — JII ships and evolves
  with no reference to UPAC internals.
- The same rule governs any future "use library X as a backend" (e.g. a compatibility
  layer for the experimental cross-distro idea): public API only.

---

## ADR-0022 — Phase 5 readiness: grow via optional Provider capabilities; keep the engine UI-free

**Status:** Accepted (architecture re-evaluation before Phase 5)

**Context:** Before starting Phase 5 (user-space sources), a full re-evaluation checked
the live code against the design. Finding: the load-bearing structure is sound
(`Provider` seam, plan-as-`Action`, trust threshold, registry-as-hint). Adding
cargo/npm/pipx/go needs **no model change** — they are the same shape as github
(user-space, no root, `Download`/`Place`/`RunCommand` into `~/.local/bin`). The
re-evaluation also named the future pressure points, and this ADR records how they must
be handled so later work stays honest.

**Decision:**

1. **New capabilities are optional `Provider` methods with safe defaults, never a fat
   required trait and never core branching.** This follows the existing precedent
   (`probe`, `is_installed` are already default methods). Concretely, when they land:
   version management (`list_versions` / install-at-version), provider metadata
   (`fetch_metadata`), and manager bootstrap (`bootstrap_plan`) are added this way — a
   provider that can't do one simply inherits the default (empty / "not supported"),
   and the engine/CLI need no per-source `if`.

2. **The engine stays UI-free.** The multi-frontend future (GUI, KDE Discover, GNOME
   Software, TUI, Web — ideas 4/5/12) requires the engine to be a library any frontend
   can drive. The one identified coupling is that `Engine::install`/`remove` currently
   take a `&crate::ui::Renderer` to print execution progress. That seam must be
   decoupled (execution emits progress via a small `ProgressSink`/event trait the CLI
   implements) **before a second frontend or a workspace split** — **not now** (YAGNI;
   there is no second consumer yet, and adding the abstraction early is exactly the
   over-complication ADR-0020's test forbids). Until then: **do not add new `ui`
   dependencies to the engine**, so the eventual decoupling stays a one-seam change.

3. **Versioning, metadata and rollback live in the provider or the registry, not the
   core.** Version *ordering* is a provider capability (sources are heterogeneous —
   reaffirms ADR-0009, no core version algebra). Rollback is a **registry-model
   extension** (retain enough to reinstall a prior version), added only when version
   management is built. Metadata is a lazy, on-demand fetch for frontends that need it —
   it is **not** eagerly loaded in `search` nor stored on `PackageCandidate`, so CLI
   startup stays fast (design principle §1).

4. **`ARCHITECTURE.md` is synced to the evolved execution model** (this session):
   `Step` → `Action` enum + `exec.rs` (ADR-0007), verification inside `Download` and
   recorded on `InstalledRecord` (ADR-0016/0018). A stale *canonical* doc is an active
   hazard, so it is corrected rather than left describing the pre-Phase-4 model.

**Reason:**
- **The default-method pattern already proved itself** (`probe`/`is_installed`): it lets
  the trait grow in breadth without forcing every provider to implement everything, and
  without leaking capability checks into the core.
- **Naming the UI seam now, fixing it later** is the disciplined middle path: the risk is
  recorded so it can't rot silently, but the abstraction isn't paid for before it earns
  its place.
- **Keeping versions/metadata out of the core** preserves the "small, understandable,
  modular" engine that the whole platform vision (one engine, many frontends) depends on.

**Alternatives considered:**
- Add version/metadata/bootstrap as required trait methods now — rejected: fat trait,
  stubs everywhere, and speculative (no consumer yet).
- Decouple `Renderer` from the engine in this pass — rejected as premature: it adds an
  abstraction with a single existing caller shape; revisit when a second frontend or
  Phase 5 `update` makes it concrete.
- Give `PkgVersion` a global ordering in the core — rejected: reaffirms ADR-0009;
  heterogeneous sources make a core version algebra wrong.

**Consequences:**
- **Phase 5 can start with no model change** — cargo/npm/pipx/go are pure new
  `Provider`s. This is the recommended next work.
- Future capability work has a prescribed shape (optional method + default), so reviews
  can reject fat-trait or core-branch approaches by citing this ADR.
- The engine's public API gains a documented constraint: **no `ui` types in engine
  signatures** going forward; the existing `Renderer` parameter is grandfathered debt
  tracked in AI_CONTEXT until the pre-frontend decoupling.

---

## ADR-0023 — Program-vs-library detection is best-effort per provider (prefer false positives)

**Status:** Accepted

**Decision:** JII installs *programs*, so a source provider should offer only candidates
that install an executable — but **how well it can tell a program from a library depends
on the source's metadata, and JII does not force a uniform filter.** Two rules:

1. **Filter when the registry exposes it reliably.** crates.io gives `bin_names` and npm
   gives `bin`, so `cargo`/`npm` drop library-only packages (e.g. `serde`, `lodash`)
   before offering them.
2. **When it does not, offer the package and let the underlying tool be the authority.**
   The PyPI JSON API exposes no entry-points/console-scripts field; the only proxy,
   the `Environment :: Console` classifier, is ~40% unreliable (measured: `poetry`,
   `twine`, `pre-commit`, `awscli` are real pipx apps that omit it). So `pipx` does **not**
   pre-filter — it offers the package and `pipx install` rejects a non-app at execution
   with a clear message (the plan is fully previewable first).

The governing principle: **a false positive (offering a library that the tool then
rejects, visibly) is safer than a false negative (silently hiding a real, installable
app).** Silently hiding an installable program violates "never hide"; a visible,
previewable plan that the tool declines does not.

**Reason:**
- **Discoverability beats tidiness.** Filtering pipx on the unreliable classifier would
  make JII silently omit ~40% of legitimate Python CLIs — a worse failure than showing a
  library candidate the user can see and that pipx will refuse.
- **It stays per-provider, no core change.** The filter (or its absence) lives entirely
  in the provider's `search`; the engine and model are untouched (ADR-0004/0022).
- **It generalizes.** `go install` only builds `main` packages, and the module proxy
  doesn't cheaply reveal which are `main`; `go` will follow the same rule (offer, let
  `go install` be the authority) rather than invent an unreliable heuristic.

**Alternatives considered:**
- Filter pipx on `Environment :: Console` — rejected: ~40% false negatives, silently
  hiding real apps.
- Download the wheel/module to read entry points during `search` — rejected: network in
  `search` must stay light (ADR-0014); downloading every candidate's artifact to classify
  it is far too heavy.
- Offer nothing unless certain — rejected: PyPI/Go can never be "certain" cheaply, so
  this would neuter two whole ecosystems.

**Consequences:**
- A new contributor sees an intentional asymmetry (cargo/npm filter; pipx/go don't) with
  the rationale recorded here, so they won't "fix" it by adding a brittle classifier
  filter.
- pipx/go may surface a library candidate; it is honest (a previewable plan) and the tool
  rejects it clearly at install. Documented as a known, accepted limitation.
- If PyPI/Go ever expose reliable entry-point metadata, the provider can add a filter
  with no core change — the rule is "filter when reliable", not "never filter".
- **Follow-up landed 2026-07-09 (#9):** the cargo/npm filter used to make a library name (`serde`,
  `lodash`) read as a bare "not found". An optional `Provider::explain_miss(&query) -> Option<String>`
  (default `None`, ADR-0022 growth) now lets those two sources say *why*: on a **total** search miss the
  engine (`explain_miss`, gated on `is_available`) asks each source, and cargo/npm re-check the registry —
  if the exact name exists but ships no executable, they return "'X' is a library — nothing to install as a
  program." Rendered under the miss in install/info/search. Off the hot path (miss-only), no core knowledge
  of the source, and the decision keys on the already-tested `candidate(...).is_none()` signal.

---

## ADR-0024 — 8-provider architecture review: no changes warranted; Homebrew is the next provider

**Status:** Accepted

**Context:** After eight providers (dnf, copr, flatpak, github, cargo, npm, pipx, go) and
`jii update`, a full-project architecture review was run (recorded in the session; summary
below) to decide whether the load-bearing structure needs change before adding more
sources, and which source is the right next one.

**Decision:**

1. **The architecture is healthy and needs no code change.** The `Provider` seam
   (ADR-0004), `Plan`/`Action` model (ADR-0003/0007), trust threshold (ADR-0006),
   registry-as-hint, and optional-method growth (ADR-0022) all held across three provider
   *classes* (system-root, file-based, registry user-space) and a whole new command with
   zero engine/model edits. No refactor is justified by this review.

2. **Two debts are now "pressing but non-blocking", and are recorded, not acted on:**
   - **Version comparison.** `PkgVersion(String)` (ADR-0009) has no ordering, so `jii
     update` can only detect "already newest" by *exact string equality*, and the
     `latest`/`minimal` profiles + freshness tie-breaker stay reserved. When version-aware
     work is next needed, add a provider-computed normalized comparison key **beside** the
     raw string (not a global semver — sources differ), as an optional capability.
   - **`cli/mod.rs` size.** ~615 lines; split into `cli/commands/*` when it next grows.
   - Minor: `PackageCandidate.raw: serde_json::Value` is used by only dnf/github; tolerated
     (typing it would parameterize a hot model type for little gain).

3. **The next provider is Homebrew** (`brew`, Linux). Rationale in full below; in short:
   it completes the user-space/community-ecosystem coverage, slots into the *proven*
   registry-user-space shape (`get_json_opt` + `command_plan`, community trust, no root,
   `brew list`/`brew upgrade`) at **zero new architectural axis**, and is the empirical
   test point for whether that pattern has stabilised enough to extract a thin
   `RegistryProvider` scaffold (prior: still separate files — verify at Homebrew, don't
   force it). Doing a bounded, proven-pattern consolidation immediately after a
   clean-bill-of-health review is deliberately lower-risk than opening a new axis.

**Alternatives considered (next provider):**
- **Declarative `.repo`/COPR provider (`data/sources/*.toml` + `DeclarativeProvider`)** —
  the higher-*leverage* choice: it's a promised-but-unbuilt core capability (ARCHITECTURE
  §5), closes the biggest doc-vs-code gap, and hits the most common real Fedora-user gap
  (vendor `.repo` apps: VSCode/Chrome/Docker). **Deferred to right-after Homebrew**, not
  dropped: it is a larger, fuzzier design with genuine new security surface (writing
  `/etc/yum.repos.d`, importing vendor GPG keys as root — only copr's two-step root plan
  is precedent), and it deserves its own design pass with a concrete seed use-case. Right
  move: do the bounded consolidation first, then this.
- **apt/pacman/zypper** — highest long-term value but **premature**: the binding MVP
  constraint is Fedora-first, cross-distro behind the `platform` abstraction (future).
  These are the first real test of that seam and should wait until it's scoped.
- **AppImage** — low architectural value: it repeats github's Download+Place path with a
  weak/again-URL-driven search (no real registry), risking the "give me a URL" untrusted
  pattern without teaching the model anything new. Later, if at all.

**Consequences:**
- A future agent has a recorded verdict ("architecture healthy, don't refactor") so the
  review isn't re-litigated, and a recorded next-two-steps (Homebrew → declarative).
- The version-comparison debt has a prescribed shape when it's next touched, so it won't
  be solved by bolting semver onto every source.
- Homebrew is expected to be a near-clone of the four registry providers; if it is, that
  is the signal to *evaluate* (not assume) a shared `RegistryProvider` scaffold — decided
  with five data points, per the 2/3/4-copies rule.

---

## ADR-0025 — Batch install is a first-class operation via an optional `plan_install_many` + engine grouping (no model change)

**Status:** Accepted

**Context:** `jii install a b c …` must install many packages as one operation (one
preview, one confirmation, one root escalation, one run), and — where a source supports it
— *merge* same-source installs into a single command (`dnf install a b c` instead of three
runs). The requirement was explicitly to make batch a **natural extension** of the model,
not per-provider batch code, and to stop and propose if it needed breaking the engine.

**Decision:** Batch fits the existing model with **no change to `InstallPlan` or the
Executor**. Four additions, all along existing seams:

1. **One optional trait method** `Provider::plan_install_many(&[&PackageCandidate]) ->
   Result<Option<InstallPlan>>`, default `Ok(None)`. A source that can batch overrides it
   to assemble **one** multi-package command (dnf/cargo/npm/go do; each builds its own
   argv via the shared `command_plan`); a source that can't inherits `None`. This is the
   same optional-method growth as `is_installed`/`probe` (ADR-0022) — the engine **never
   branches on the source**, it just uses the returned plan or falls back.
2. **Engine grouping** `plan_install_batch(candidates) -> Vec<BatchPlan>`: group by
   `source_id` (ranked order preserved); a **group of one** uses `plan_install` (richer
   reasons, identical to a plain single install); a group of 2+ asks `plan_install_many`
   and falls back to per-candidate `plan_install` on `None`. `BatchPlan { plan,
   candidates }` keeps each plan paired with the installs it covers.
3. **Engine execution** `install_batch`: prime privilege **once** across all plans, run
   them in order, and **record each plan's candidates as it succeeds** — so a mid-batch
   failure still leaves the registry accurate for what actually installed.
4. **Executor primitives**: `run_plan` was split into `prime_for(&[&InstallPlan])` (prime
   once if any needs root) + `run_actions(plan)` (run one plan, no priming); `run_plan`
   is now a thin wrapper. This is the whole "one escalation across many plans" mechanism —
   a small helper, not a new concept.

Supporting decisions:
- **Single install is now a batch of one.** The old `Engine::install` and the
  `plan_install` engine wrapper were removed — one install write-path (`install_batch`),
  no duplicated recording logic to drift.
- **Trust barrier for a batch is governed by its least-trusted candidate**
  (`prompt::confirm_install_batch`): if anything in the batch is below the auto-confirm
  threshold, the whole batch needs an explicit answer even under `--auto` (ADR-0006 holds,
  now for a set).
- **A not-found package never cancels the rest**: misses are reported, and if anything did
  resolve the user is offered to continue with the remainder.
- **Bootstrap (installing a missing manager once for a group) is deferred, not faked.** It
  needs the manager-install capability that does not exist yet (an ADR-0024 future
  direction, with its own trust policy). The per-source grouping in `plan_install_batch` is
  exactly the seam it will hook into; until then, a package whose only source's tool is
  absent is simply reported not-found.

**Alternatives considered:**
- **Merge at the argv level in the engine** (splice package names into existing plans'
  commands) — rejected: it would require the engine to know each source's command shape
  (where npm's `--prefix` sits, that go needs `@latest`), leaking source knowledge into
  the core and violating ADR-0004. The optional method keeps that knowledge in the
  provider.
- **A new `BatchPlan`-as-model-type executed specially** — rejected: unnecessary. A merged
  plan is just a normal `InstallPlan` with one multi-arg `RunCommand`; the executor is
  unchanged. `BatchPlan` is only an engine-internal pairing (plan + its candidates), not a
  new executable concept.

**Consequences:**
- The model got **stronger, not bigger**: "install" is uniformly N≥1, with one write-path
  and one confirmation path. No `InstallPlan`/Executor change.
- **`batch update` and `batch remove` now need no new architecture** — the batch machinery
  (group → optional `plan_*_many` → prime-once → run → record-as-you-go) is reusable.
  `jii remove a b c` / `jii update a b c` would add symmetric optional methods
  (`plan_remove_many`/`plan_update_many`) where merging helps (e.g. `dnf remove a b c`) and
  an engine `remove_batch`/`update_batch` mirroring `install_batch`. The CLI for `update`
  already takes many names; `remove` would widen to a `Vec`. Recorded here so this is not
  re-litigated when Batch Operations lands.
  **Landed as predicted (T2, ADR-0026):** exactly these additions, plus a generic
  `group_by_source` helper (the grouping was now used 2×) and a `RecordOp`-driven
  `plan_record_batch` that unifies remove+update planning (branches on the *operation*, never
  the source). `RecordBatch { plans, unplannable }` reports an un-actionable package (e.g. a
  github install has no update path) instead of aborting the batch — the `SearchResult`
  facts-not-failures shape. No `InstallPlan`/Executor/model change, confirming the prediction.

## ADR-0027 — No shared `RegistryProvider` scaffold after five registry providers (evidence-based)

**Status:** Accepted

**Context:** Homebrew (`brew`) is the 5th "registry-user-space" provider after cargo, npm, pipx,
go — all shaped alike (a network `search`, unprivileged `plan_*` commands, a `list_installed`
parse, `community` trust). The standing question (ADR-0024): once the pattern recurs a 5th time,
does a thin shared `RegistryProvider` scaffold pay off? The decision was to be made **from the
real code**, not assumed.

**Decision:** **Do not build a `RegistryProvider` scaffold. Keep the providers as separate
files.** Measured against the five actual implementations, the only *identical* code is ~8 lines
of boilerplate per file (`id`/`new`/`Default`, `trust() = Community`, `is_available() =
which(BIN)` — and go even overrides `is_available`). Everything substantive is **irreducibly
per-provider**:
- **`search`** — different URL shape (`crates/{name}`, `/{pkg}/latest`, `/pypi/{pkg}/json`, the go
  proxy with `!x` escaping, `/{name}.json`), different JSON structs, and different candidate
  construction (cargo filters on `bin_names`, npm on `bin`, pipx/go/brew deliberately don't
  filter — ADR-0023; go derives a module path; version fields differ).
- **`plan_install`/`remove`/`update` (+`_many`)** — different verbs and argv shape (npm's
  `--global --prefix`, go's `@latest`, brew's `install/uninstall/upgrade`). Their *shared*
  part — assembling one `RunCommand` plan — is **already** the `command_plan` helper.
- **`list_installed`** — completely different parsing (cargo's indented `--list`, npm/pipx JSON,
  go's empty, brew's `list --versions`).

What recurs is the **trait structure** (methods with the same names), which Rust's trait already
mandates — not duplicated *logic*. The genuinely shared logic is already extracted as small
stateless free functions in `provider/mod.rs` (`http_client`, `get_json_opt`, `command_plan`,
`run_capture`, `which`, `nonempty_lines`, `parse_installed_records`). That is the right
granularity of sharing.

**Alternatives considered:**
- **A `RegistryProvider` trait/struct** parameterised by a URL-builder, a candidate-parser, a
  verb map, and a list-parser (closures or associated types). Rejected: each provider would still
  supply all the varying parts (which are the substance), now wrapped in a second abstraction
  everyone must learn. It trades ~8 boilerplate lines/provider for a new indirection layer — a net
  readability loss, i.e. abstraction for symmetry, not for maintenance cost (violates the standing
  rule: reduce maintenance cost, not line count).
- **A shared `*_many` argv helper** across cargo/npm/brew. Rejected: the 5-line "extend argv with
  names, join reasons" idiom varies (npm's prefix, go's `@latest`), so a shared helper would need
  those as parameters and read worse than the inline version. 2–4 small copies with real variation
  is within the duplication tolerance.

**Consequences:**
- New registry providers stay one self-contained file each, mirroring an existing one (cargo is
  the reference), reusing the free-function helpers. The `/new-provider` guidance stands.
- If a *future* provider introduces genuinely shared **logic** (not structure) — e.g. several
  sources needing identical version-list fetching for the T5 version chooser — that specific logic
  gets its own free-function helper when it hits the 3×/4× threshold, not a god-trait.
- Revisit only if the identical surface grows well beyond boilerplate; today it does not.

## ADR-0028 — AppImage is a GitHub-release asset kind, not a standalone provider

**Status:** Accepted

**Context:** Terminal 1.0 (ADR-0026) T3 lists AppImage after Homebrew/Snap. Investigating the
real ecosystem showed AppImage does not fit the `Provider`-as-a-source shape the way a package
manager does:
- **No manager, no install command, no download API.** "Installing" an AppImage is just
  *download a file + place it + `chmod +x`* — which the **github provider already does**
  (`Download` + `Place` with an exec mode).
- **No usable search source.** The only catalog, `appimage.github.io/feed.json`, is a discovery
  index of ~1388 names with descriptions/icons but **no download URLs** and frequently
  `links: null` (e.g. Inkscape). When a link exists it points to a **GitHub repository** — i.e.
  resolving an AppImage by name is the *same* name→repo→release-asset problem the github provider
  already faces (and the T5 repository chooser will solve).
- **github already classifies `.AppImage` as an installable binary** when the asset name carries
  `linux` + an arch token; it only missed the common `App-x86_64.AppImage` naming (no `linux`).

**Decision (user-approved):** **Do not build a standalone `appimage` provider.** Treat AppImage as
a **delivery format over GitHub releases**:
1. `github::classify` now accepts a `.AppImage` asset as a raw `Binary` (download + place + chmod)
   **without** requiring the `linux` token — AppImages are Linux-only by definition — while still
   requiring an arch match (never install a wrong-arch build). `.AppImage.zsync` updater deltas
   stay rejected (the `.zsync` token). So `jii <owner>/<repo>` installs an AppImage release today.
2. **AppImage-by-bare-name** folds into **T5 (repository chooser)** — the same name→repo
   disambiguation, resolved visibly by the user, not a separate source.
3. Removed the reserved `"appimage"` id from `KNOWN_SOURCES` — there is no such source.
4. The catalog's icons/screenshots/metadata are a **GUI-era** concern (ROADMAP "Provider-supplied
   metadata"), not a CLI necessity.

**Alternatives considered:**
- **A catalog-backed `appimage` provider** (search feed.json → GitHub repo → release asset).
  Rejected: it duplicates github's release-asset resolution, and covers only the subset of catalog
  entries that *have* a link and ship an `.AppImage` (Inkscape has neither in the feed). Half
  coverage plus duplication — the opposite of the ADR-0027 principle.
- **Defer AppImage entirely to T5.** Reasonable, but the github `.AppImage` acceptance is a
  correct, ~6-line, independently-useful improvement, so it lands now; only *by-name discovery*
  waits for T5.

**Consequences:**
- AppImage releases install through the existing github path — no new source, no duplication, one
  trust story (untrusted binaries, always confirmed). T3's "AppImage" item is satisfied this way.
- The "install an AppImage by friendly name" experience arrives with T5's repository chooser.
- Confirms the ADR-0026/0027 stance: breadth is added by reusing seams, and a thing that isn't a
  managed source doesn't get forced into the `Provider` mould.
- **Further plan-merging across *different* sources is intentionally not pursued.** Merging
  is only ever within one source (that's the only place a single command is meaningful);
  cross-source "one super-command" is impossible and undesirable. The current granularity
  (one plan per source group) is the right and final level — no deeper merging is planned.

## ADR-0026 — Terminal 1.0: complete the CLI before Beta; grow only via optional capabilities

**Status:** Accepted

**Context:** The priority shifted from "ship a narrow honest Beta now" to **finish the whole
terminal version first — call it CLI 1.0 — and only then cut the first public Beta** (tested
on clean Fedora/Arch/Ubuntu/Debian/openSUSE VMs, then a polished public repo). The scope named:
the read-only `search`/`info`/`sources` commands, batch `update`/`remove`, more providers
(Homebrew, Snap, AppImage, Nix), cross-distro system providers (Apt, Pacman, Zypper), an
interactive GitHub **repository chooser**, a **version chooser**, and **bootstrapping a missing
manager**. GUI/daemon/Discover/plugins stay out (kept ready, not built).

Almost all of this was already anticipated in `ROADMAP.md` → "Future ideas" **with hard
architectural rules already written** (repo chooser, version management, bootstrap, breadth-as-
additive, cross-distro-as-Provider-behind-the-platform-seam). So this is **promotion into an
ordered delivery plan, not a redesign** — the load-bearing decisions (ADR-0004 core-never-
branches, ADR-0022 grow-via-optional-methods, ADR-0006 trust barrier) are unchanged.

**Decision:** Deliver CLI 1.0 as an ordered sequence of tracks, each a small increment that
keeps the build/tests green (the order minimises architectural risk — cheap read-only honesty
first, the biggest cross-distro push only after the model is well-exercised by additive
breadth):

- **T1 — Read-only honesty layer:** `jii search`, `jii info`, `jii sources`. Engine already
  exposes `search`/`rank`; these are **pure rendering**, zero new architecture. Closes the
  Product Review's #1 blocker (README advertises `search`/`info`/`config` that are stubs/absent).
- **T2 — Batch symmetry:** `jii update a b c`, `jii remove a b c`. Exactly the ADR-0025
  machinery — optional `plan_update_many`/`plan_remove_many` + engine `update_batch`/
  `remove_batch`, CLI widened to `Vec`. No new architecture (ADR-0025 pre-committed this).
- **T3 — Provider breadth (proven shape):** Homebrew, then Snap, then AppImage — additive
  `Provider`s (ADR-0004/0020). Empirical check at Homebrew (5th user-space provider): does a
  shared `RegistryProvider` scaffold finally pay off? Decide from evidence, don't assume.
- **T4 — Cross-distro system providers:** Apt, Pacman, Zypper, Nix behind the platform seam.
- **T5 — Interactive choosers:** GitHub repository chooser (paged select) and version chooser.
- **T6 — Bootstrap a missing manager:** offer-then-install, strongest consent.
- **T7 — Hardening:** CLI-level integration tests (`assert_cmd`), registry-partial-failure test,
  error-message quality, clean-VM runs on all five distros (the Product Review's Etap B/C).
- **T8 — Public polish:** professional README, logo, screenshots/asciinema, architecture
  diagram, CONTRIBUTING/SECURITY, limitations — the first-impression pass. Then cut Beta.

This ADR commits to **three deliberate, minimal architecture growths** (each optional/data-
driven, none a core branch), to be detailed in their own ADRs when their track lands:

1. **Platform seam relaxes (T4):** `Platform::is_supported` stops meaning "distro == Fedora"
   and starts meaning "at least one native system provider is available here." System
   providers gate themselves via **distro-aware `is_available`** (dnf is unavailable on Arch,
   pacman is available). The core still never branches on the source; Fedora behaviour is
   untouched. A dedicated ADR will define the "native system provider per distro" concept.
2. **Versions are surfaced by the provider, ordered by the provider (T5):** an optional
   `Provider::available_versions` (or versions on the candidate); the engine stays
   version-agnostic and never invents an ordering for `PkgVersion(String)` (ADR-0009 holds).
   This is the source-provided answer to the recorded version-comparison debt — not a
   jii-invented semver.
3. **Bootstrap is a plan step, not engine special-casing (T6):** an optional
   `Provider::bootstrap_plan() -> Option<InstallPlan>`; the engine offers it when a chosen
   candidate's source is unavailable. Installing a manager demands the strongest consent
   (own previewable plan, official install method, never `curl|sh`, never `--auto`), and
   bootstrapping does not launder trust (ADR from ROADMAP "Bootstrapping" hard rule).

**Alternatives considered:**
- **Go straight to Homebrew (the prior ADR-0024 "next").** Rejected *as the next step*: the
  Product Review showed the CLI advertises commands it doesn't have — adding a 9th provider
  before `search`/`info` exist widens the honesty gap. T1 comes first.
- **Ship the narrow Beta now, defer the rest.** Superseded by the user's explicit decision to
  finish the CLI first. The narrow-Beta work is not lost — it folds in as T1 (search/info),
  T7 (tests, clean-VM, errors) and T8 (README) under the larger arc.
- **A cross-distro mega-refactor of the engine up front.** Rejected: cross-distro is additive
  providers + a platform-seam relaxation, not an engine change. Doing T3 (additive breadth)
  before T4 exercises the model so T4 stays a provider-and-platform change, not a core one.

**Consequences:**
- A single, ordered definition of "done" for the terminal version, recorded in the repo (not
  a chat): CLI 1.0 = T1–T6 implemented, T7 hardened, T8 polished, then Beta.
- Each track is independently shippable and reviewable; the build stays green throughout.
- The three growths are pre-declared so they are not re-litigated per track — each still gets
  its own ADR when it lands, but the *shape* (optional method, no core branch) is fixed here.
- GUI/daemon/Discover/plugins remain out of scope; the engine↔UI seam (ADR-0022) must be
  decoupled **before** any second frontend, which is a post-1.0 concern, not a T-track.

---

## ADR-0029 — The platform seam: `Platform` is host *facts*; "supported" is "≥1 usable source" and lives in the engine, not in a distro check

**Status:** Accepted 2026-07-05 (refines ADR-0026 growth #1). Foundation for Terminal 1.0
T4 (cross-distro: apt, pacman, zypper, nix). Enacted: `Platform::is_supported`/
`require_supported` and `JiiError::UnsupportedPlatform` removed; `Platform` is now pure
host facts; the CLI's five source-touching commands guard on
`Engine::any_source_available` (source-based "supported"). Fedora behaviour verified
unchanged. The `id`/`id_like` predicate remains deferred to its first consumer (T6).

**Context:** T4 adds non-Fedora system providers. A full audit of the platform layer (real
code, not assumptions) found that **the entire codebase couples to the distribution in exactly
one place**: `Platform::is_supported()` returns `matches!(self.distro, Distro::Fedora)`, and its
wrapper `require_supported()` guards five CLI entry points (install/remove/update/search/info).
Everything else is already distro-agnostic:

- Every provider self-gates on a **binary**, not a distro: `dnf`/`copr` = `which("dnf5")`,
  `snap` = `which("snap")`, `brew` = `which("brew")`, registries = `which(<tool>)`. On Arch,
  `dnf5` is absent, so dnf/copr simply drop out — no distro check needed or present.
- `Platform`'s other fields — `arch`, `is_tty`, `path_dirs`, `elevation_kind()` (sudo/pkexec) —
  are cross-distro host facts consumed by github/copr (arch), prompts/color (tty) and
  `privilege.rs` (elevation). None are Fedora-specific.
- `Distro` is the only type that privileges one distro: `Fedora | Other(String) | Unknown`.
  Fedora is first-class; every other distro is a second-class string.

So relaxing "Fedora-only" is **not** an engine refactor and not a platform rebuild — it is
removing one artificial wall and de-privileging one enum. ADR-0026 pre-declared keeping the
gate on `Platform` ("`is_supported` starts meaning ≥1 system provider"); the audit showed a
cleaner placement, so this ADR refines that.

**Decision:**

1. **`Platform` becomes a pure host-facts value object — it loses all policy.** Remove
   `is_supported()` / `require_supported()` from `Platform`. `Platform` answers only *"what is
   this machine?"* (distro, arch, tty, PATH, elevation mechanism). It gets **dumber**, not
   smarter. Providers and config-defaults may *read* `distro`; **the core never branches on it.**

2. **"Supported" is redefined as "≥1 usable install source" and moves to the engine/registry.**
   The question is not "which distro is this?" but "does JII have any working source here?" —
   which only the provider set can answer. The engine exposes an availability guard (built on the
   existing `is_available` fan-out already used by `source_catalog`) and the CLI calls it where
   `require_supported()` was. This is strictly better than the distro wall on Fedora too: if a
   user disables every source, they get a clear "no usable source" message instead of a false
   green light. Absence of a *native system* manager (e.g. only cargo+github in a container) is
   **not** an error — it is a soft note surfaced by `jii sources`/`doctor`.

3. **Distro identity is a family predicate, introduced on first use — not a fat enum.** The
   durable target for `Distro` is `id` + `id_like` chain with `is("debian")` / `is_like("debian")`
   predicates (handles the unbounded set of derivatives: nobara→fedora, mint→ubuntu→debian),
   with **no privileged variant**. This is built the moment the first consumer needs it (bootstrap
   T6, or a family-scoped provider), **not speculatively now** — for T4 the providers self-gate on
   their binary and need no distro logic at all.

**Responsibility split (the load-bearing part):**
- **Platform** — host facts only, zero policy.
- **Provider** — its own availability (self-gating), trust, plans, later `bootstrap_plan`. May
  read `Platform`; the core never reads `distro` to pick a provider.
- **Engine/Registry** — owns "is any source usable here?".
- **Config** — user policy (priority/disabled); may be *seeded* from distro at first run, then it
  is just data.

**Five-year shape:** with 15+ managers and dozens of distros, the core stays **O(1) in distros
and O(1) in managers**. The only thing that grows is the provider list in the registry (linear,
one obvious place). No new core branch is ever required to add a distro or a manager, because
distro never drives core control flow, a manager is a self-gating `Provider`, `Platform` is
immutable facts, and policy lives in config.

**Alternatives considered:**
- **Keep the gate on `Platform`, just redefine it (the ADR-0026 pre-declaration).** Rejected:
  "is any source usable?" is a fact about the provider set, not about the host; keeping it on
  `Platform` makes `Platform` a mini-policy engine and couples it to the registry. Moving it out
  makes `Platform` a clean value object — the cleaner refactor the audit surfaced.
- **Central `distro → provider` map in the core.** Rejected outright: reintroduces source
  knowledge into the core and breaks "core never branches on the source" (CLAUDE.md, ADR from
  ARCHITECTURE §5). Self-gating providers already solve this with zero core coupling.
- **Expand `Distro` into one enum variant per distribution now.** Rejected: brittle (derivatives
  are unbounded), and speculative (no consumer in T4). The `id`/`id_like` predicate is the right
  shape, added when a consumer appears.
- **Just delete `require_supported()` and rely on "no candidates".** Rejected: a dedicated
  early "no usable source" message is clearer than an empty search result; the guard has value,
  it just belongs in the engine with a source-based definition.

**Consequences:**
- Fedora behaviour is **unchanged**: dnf/copr still gate on `dnf5`, the same commands run, the
  same trust/escalation applies. Non-Fedora hosts stop being walled off and get whatever sources
  are actually present (flatpak/cargo/npm/github/snap today; apt/pacman/zypper/nix as T4 lands).
- `Platform` sheds a responsibility (net simpler), and the "supported" concept gains an honest,
  source-based definition that also improves the all-sources-disabled case on Fedora.
- T4 providers are pure additive `Provider` impls that self-gate on their binary; none needs a
  distro check. The `id`/`id_like` predicate is deferred to its first real consumer (T6 bootstrap).
- This ADR gates code: `platform.rs`/`engine`/`cli` change only after acceptance; apt is the
  first provider to follow (Debian/Ubuntu — largest audience).

---

## ADR-0030 — GitHub by-name discovery: search resolves cheaply, releases resolve lazily at plan time

**Status:** Proposed 2026-07-06, **deferred**. Terminal 1.0 (ADR-0026) T5, second slice — the
"GitHub repository chooser." Builds on the interactive candidate chooser (T5 slice 1, no ADR).
**Deferred** the same day: after real dogfooding on a clean Fedora VM the user re-prioritised
Terminal 1.0 to a **UX-polish pass** ("no new features, no new providers") — by-name GitHub
discovery is a new feature, not one of the reported UX problems, so its implementation waits until
the UX pass lands. The analysis below stays valid and is the design to build against when it
resumes. See docs/UX_EVALUATION.md for the re-prioritisation.

**Context:** Today the github provider only answers an explicit `owner/repo` query; a bare
`jii ncdu` returns nothing from github. To make `jii <name>` reach GitHub-only tools, github
must map a name to repositories via `/search/repositories`. Two forces constrain the design:

1. **The core never branches on the source (ADR-0004).** The engine calls *every* provider's
   `search` for *every* install and ranks the union; it cannot tell github "skip, dnf has it."
   So whatever github does on a bare name, it does on **every** install — including `jii firefox`
   that dnf satisfies. github candidates are always `Untrusted`, so they already rank **last**
   (ADR-0006 trust barrier) and never outrank a native package.
2. **Anonymous GitHub API budget is small:** ~60 core requests/hour. A `latest-release` GET is a
   core request; `/search/repositories` is on a **separate** search limit (~10/min). Probing the
   release of every search hit during `search` would mean *K release GETs on every install* —
   ~10 installs and an unauthenticated user is rate-limited, most of it wasted on queries a
   trusted source already answered.

Eagerly probing releases in `search` (to filter to repos that actually ship an installable Linux
asset) is the accurate option but collides with both forces: it is the most expensive path and it
runs unconditionally. A core-side "only search github if nothing trusted matched" gate would fix
the cost but **is exactly the forbidden branch-on-source**.

**Decision:** Split github resolution across the two phases so the common case stays cheap and no
core branch is needed:

- **`search` (bare name) does only the cheap call.** It issues one `/search/repositories?q=<name>
  in:name fork:false&sort=stars` request (on the search limit, not the core limit), then filters
  and ranks the hits **without** fetching any release. It returns up to **`K = 5`** lightweight
  candidates carrying `{ owner, repo, by_name: true, stars, description }` in `raw`, `version:
  None`, `arch_ok: true` (optimistic), `trust: Untrusted`. Best-effort: any search/network/rate
  error yields **zero** github candidates (never an error that blocks other sources).
- **Ranking/filter policy (pure, unit-tested):** drop archived repos; keep hits whose repo name
  contains the query token; order **exact name match first, then by stars descending**; take the
  top `K`. Since all share `source_id="github"` and `Untrusted`, Rust's stable `sort_by` in the
  engine preserves this relevance order within the github group — **no engine change**.
- **`plan_install` resolves the release lazily.** For a `by_name` candidate it fetches the latest
  release, runs the existing `select_asset`/checksum logic, and builds the plan. If the repo ships
  no installable Linux asset for this arch, it errors *at plan time* with a clear message. Thus a
  release GET (core budget) is spent **only for the one repo the user actually installs / previews**
  — zero when a trusted source wins and github is never picked.

This deliberately relaxes the module's "all network in `search`, `plan_install` is pure" invariant
**for by-name candidates only**. The explicit `owner/repo` path is unchanged (still resolves in
`search`). The plan-*building* stays pure and unit-tested; only the async `plan_install` wrapper
gains a release fetch. Release resolution is factored into one `resolve_release_candidate(owner,
repo)` helper shared by both the `owner/repo` search path and the lazy plan path (removes the
duplication that existed between them).

**Security:** every github candidate stays `Untrusted`, so the ADR-0006 trust barrier always fires
— a by-name pick is never auto-installed, even under `--auto` (unless `allow_untrusted_auto`). The
chooser shows `owner/repo ★stars — description` so the user recognises the real project (e.g.
`BurntSushi/ripgrep`) rather than a typosquat. github ranks last, so by-name suggestions only
surface when the user opens the chooser or nothing more trusted matched.

**Alternatives considered:**
- **Eagerly probe the top-K releases in `search` and filter to installable repos.** Rejected: most
  accurate (never lists a repo with no Linux asset) but K release GETs on *every* install blows the
  60/hr anonymous budget on queries trusted sources already answer. Accuracy is recovered cheaply
  at plan time; a rare "no installable asset" repo failing at plan time is an acceptable 1.0
  trade-off (noted as debt — a token or a `HEAD`/asset-list cache could pre-filter later).
- **Gate github-by-name in the engine (only when no trusted candidate).** Rejected outright:
  reintroduces source knowledge into the core — the forbidden branch-on-source (ADR-0004).
- **A `--github <name>` opt-in flag instead of automatic discovery.** Rejected as the default: it
  fights "just install it." The lazy design already makes automatic discovery cheap enough that an
  opt-in is unnecessary; an opt-out / config knob can come later if needed.
- **Rank by-name repos with a github-specific popularity key in the engine.** Rejected: the engine
  stays version/popularity-agnostic (ADR-0009). Provider-supplied relevance order + stable sort
  achieves the same with zero engine coupling.

**Consequences:**
- `jii <bare-name>` now reaches GitHub-only tools; combined with the T5 chooser, multiple repos are
  presented (never silently installing the wrong one), the recommended (trusted) source stays the
  default, and github options follow in stars order.
- Common-case cost is one search-limit request; a core release GET is spent only on an actual
  github install/preview — no rate-limit regression for the dnf/apt-satisfied majority.
- New debt: a picked repo can fail *at plan time* if its latest release has no Linux asset for the
  arch (the search phase cannot know). Message is explicit; revisit with caching/token in T7.
- No engine or core change; the growth is entirely inside the github `Provider` (ADR-0004/0022).

---

## ADR-0031 — The JII package spec `name[:source][@ref]` is the language of JII; package-belonging attributes extend the spec, not the flags

**Status:** Accepted 2026-07-06. Locks the Terminal 1.0 CLI *surface*. Lands with Terminal 1.0 (ADR-0026) T5/U4 (UX pass). Full first-principles evaluation and critical pass in [UX_EVALUATION.md](UX_EVALUATION.md) §E/§E.1.

**Context:** Dogfooding raised a first-principles question about the CLI: rather than "which flag deserves a shorter alias?", ask "**should this even be a flag?**". JII's dominant interaction is *"install this software"* (`jii <name>`); flags are overrides and scripting knobs, not the everyday path. Several current globals describe **the package**, not **the command**: `--source` (which provider), and — deferred — version/channel selection. Traditional package managers scatter these across flags and per-command choosers; copying that convention is not a goal (ADR-0020: JII is a universal layer, and its ergonomics should feel designed for it, not inherited).

Two separate questions were deliberately kept apart. A **flag's spelling** (`-y`, `--dry-run`) is where convention *is* usability (muscle memory, `--help`, shell completion), so reinventing it (e.g. single-dash long flags) costs more than it saves and clap v4 fights it anyway (rejected in §B). But **what deserves to be a flag at all** is fair game — and that is where the real win is.

**Decision:** Introduce a universal **package specification** and treat it as the language of JII:

```
name[:source][@ref]
```

- **`name`** — the package (the only required part).
- **`:source`** — the owning provider (an id in `KNOWN_SOURCES`), e.g. `firefox:flatpak`. Per-package and unambiguous (`jii firefox:flatpak cava:dnf`).
- **`@ref`** — a **source-interpreted** version/channel/branch reference: `node:brew@22` (a version), `firefox:flatpak@stable` (a flatpak branch), snap channels, etc. The **core never interprets `@ref`** — the owning provider resolves it (ADR-0004; ADR-0009's "versions are opaque to the core" extends to refs). This folds *channel* into `@ref`, so no third separator is needed. `@` is reserved for the ref because every other ecosystem (npm/pip/go/cargo) has trained users that `@` means version; using it for source would mis-train them.

The spec is **universal across every verb** that names a package — install, `remove firefox:flatpak` (which is the non-interactive answer to the multi-owner remove chooser, ADR-scoped as UX #11), `info`, `update node:brew@22`. One grammar unifies the source disambiguation that today is split between `--source` and two different interactive choosers.

**Explicit intent suppresses the matching question:**
- `:source` present → **skip the source chooser** (install) and the owning-source chooser (remove) — implemented as one added clause on the existing `offer_choice` gate.
- `@ref` present → skip any version prompt and pin the ref.
- `firefox@120` (ref, no source) → resolve in the *recommended* source; only if it lacks the ref fall back to sources that have it.
- an explicit source with **no match** (`firefox:flatpak` where flatpak has no firefox) → an honest error, **never a silent substitution** to another source (the cooperation principle).

**Flag taxonomy after the spec** (the "truly global" set shrinks hard):
- **Kept, conventional (truly global):** `-y/--yes`, `-n/--no`, `--dry-run`, `-v/--verbose`, `--json`.
- **Into the spec:** source (`:source`), version/channel (`@ref`).
- **Into the chooser:** source selection when unspecified.
- **Into config / `jii setup`:** `--profile` (a standing preference, not a per-run choice).
- **Eliminated / inferred:** `--auto` folds into `-y` (both merely skip the trusted-confirm; the trust barrier still governs untrusted, ADR-0006); `--no-color` inferred from `NO_COLOR`+tty, kept only as an explicit override.
- **Demoted but kept:** `--source` as the *whole-command* sweep (`jii a b c --source flatpak`) and a scriptable/discoverable synonym.

**The durable principle (this is the point of the ADR).** Before adding any future flag, ask: **"Does this belong to the package itself, or to the command?"** If it belongs to the package (source, version, channel, …), it **extends `PackageSpec`**, not the flag set. If it modifies the command/output/consent (`--dry-run`, `--json`, `-y`), it may be a flag. This one rule keeps the CLI small and memorable for years and is binding on every future feature.

**Implementation shape (fits with zero core changes):** a pure, unit-tested `PackageSpec::parse` (the ADR-0012 "isolate + test parsers" pattern); **clap is untouched** — the spec is an ordinary positional value JII parses itself. Parse rules of note: an npm **scoped name starts with `@`** (`@angular/cli`), so a ref is split only on a **non-leading, last** `@` (`@angular/cli@18` → name `@angular/cli`, ref `18`); a source is validated against `KNOWN_SOURCES` with a did-you-mean suggestion; a literal `:` in a name (vanishingly rare) uses `--source` as the escape hatch. **Parse the full grammar now, but reject an unimplemented `@ref` explicitly** ("pinning a version/channel is coming in a later release") rather than silently dropping a version pin — this locks a forward-compatible surface without half-building version selection.

**Alternatives considered:**
- **Single-dash long flags (`-source`).** Rejected (§B): clap v4 has no single-dash-long; only an argv-normalising shim could fake it, leaving help/errors/completions inconsistent — reinventing flag *spelling* for no real gain.
- **Keep `--source`/`--version` as flags, just add short aliases.** Rejected as the primary path: these describe the *package*, so a flag is the wrong home; the spec is more consistent and readable, and unifies disambiguation across all verbs. `--source` survives only as the whole-command sweep/synonym.
- **A third separator for channel** (e.g. `name:source/channel@version`). Rejected: `@ref` is already source-interpreted, so channel and version share the one slot — simpler, and the provider (which knows whether it has channels) decides.
- **Do nothing / pure traditional flags.** Considered and rejected on the merits (not merely tradition): the spec measurably reduces typing *and* reads better *and* removes two redundant choosers' worth of inconsistency, at additive/non-breaking cost.

**Consequences:**
- The everyday surface becomes **`jii name[:source][@ref]`** plus a handful of global switches — dramatically easier to hold in the head than a dozen flags.
- **Additive and non-breaking:** every existing flag still works; the spec is the new *preferred* and *taught* form, `--source` the synonym.
- No core/engine change and no clap change; a new pure parser is the only addition, and the "skip chooser when `:source` given" rule is one clause on existing gating.
- Binds future work via the "package or command?" principle — new package attributes extend `PackageSpec`, keeping the CLI clean.
- Locks the Terminal 1.0 CLI surface, so `@ref` is parsed (and cleanly rejected until version selection lands) rather than deferred, avoiding a later breaking grammar change.

## ADR-0032 — Actionable errors: a pure `JiiError::remedy()` maps failures to next steps

**Status:** Accepted 2026-07-06. Lands with Terminal 1.0 (ADR-0026) U6 (UX pass, problem D7). Small, additive.

**Context:** An error message that only states *what went wrong* leaves a first-time user stuck ("unknown source: dfn" — and now what?). The UX evaluation (D7) asked every failure to also say *what to do next*, in JII's own voice, ideally the exact next command. JII's `JiiError` enum is deliberately thin (`Config`, `UnknownSource`, `Io`, and a catch-all `Other(anyhow)` that wraps opaque text from dozens of call sites).

**Decision:** Add a **pure** method `JiiError::remedy(&self) -> Option<String>` that maps a *typed* error to a concrete next step, rendered by the caller on the line below the error (`  → …`). It is pure (no I/O) so it is unit-tested against fixed inputs (the ADR-0012 "isolate + test" discipline applied to error copy). Coverage:
- `UnknownSource(id)` → names the id, lists `KNOWN_SOURCES`, points at the config `priority`/`disabled` list and `jii setup`.
- `Config(_)` → points at the config file and `jii setup`.
- `Io { path, source }` → branches on `ErrorKind` (`NotFound`/`PermissionDenied` get specific advice; other kinds get none).
- `Other(_)` → **`None`**. Deliberately no string-sniffing: `Other` wraps free-form text from many sites, so keyword-matching it ("rate limit", "not found") would be fragile and frequently wrong. Better to stay silent than invent a misleading remedy.

Rendered in `main.rs::report` (not the `Renderer`) because the highest-value case — a bad config — fails *before* a renderer exists.

**Alternatives considered:**
- **A rich `Remedy { summary, next_command }` struct.** Over-built for the current thin error set; a single formatted string is enough and easy to render. Revisit if remedies need to carry a runnable command the UI offers to execute.
- **String-sniff `Other` for known phrases (GitHub rate limit, missing tool).** Rejected for now: fragile and coupling error copy to remedy logic. The right fix is to *promote* those conditions to typed variants (or surface them where they arise — e.g. the GitHub rate-limit remedy belongs to `doctor` Tier 1, ADR-0033), then map them here. Left as a forward hook, not a guess.
- **Fold remedies into each `#[error("…")]` string.** Rejected: mixes the terse *what* with the verbose *how-to-fix*, and can't adapt to `ErrorKind`. Keeping `remedy()` separate lets the UI show or suppress it (e.g. `--json`) independently.

**Consequences:**
- Errors now teach: a typo in a config source, a missing config path, a permission problem each print an exact next step.
- Adding a remedy for a new failure is a one-arm `match` + a unit test; the discipline ("no invented remedies for opaque errors") keeps it honest.
- Forward hook: as high-value `Other` cases (GitHub rate limit, a source's tool missing → bootstrap offer) become typed or move to `doctor`, they slot into `remedy()`/Tier 1 without a redesign.

## ADR-0033 — The recommend-catalog is a data subsystem, distro-filtered, read-only (Analyze → Explain)

**Status:** Accepted 2026-07-06. Lands with Terminal 1.0 (ADR-0026) U6 (UX pass, problem D6 Tier 2 — pulled into 1.0 by the user's 2026-07-06 scope decision). Two slices, both landed: (1) the catalog + `jii recommend` reporting; (2) guided per-entry apply (`jii recommend <id>`). **Amended by ADR-0035 (2026-07-07):** the catalog data subsystem is unchanged, but its presentation folded into `jii doctor`'s tail and the standalone `jii recommend` command (incl. apply-by-id) was removed — suggestions are now applied by running the shown command.

**Context:** D6 split `doctor`/onboarding into two tiers. Tier 1 (system checks about JII working) shipped as part of `doctor` (ADR-0033-adjacent, in that commit). Tier 2 is the *curated recommendations* — codecs, RPM Fusion, GPU drivers, fonts, Steam/Wine, battery — which the UX evaluation flagged as "a real content subsystem, not polish," deferred in the ROADMAP but pulled into 1.0 by the user. Two hard constraints frame it: (1) **the core never branches on distro** (ADR-0029) — yet these recommendations are inherently distro-specific; (2) **Analyze → Explain → Ask → Apply, never auto-modify** (ROADMAP) — yet several entries (RPM Fusion) cross a trust boundary a plain package install can't express.

**Decision:** Model the catalog as a **data subsystem**, not code and not a `Provider`:
- The catalog is **authored as TOML** (`data/recommend/catalog.toml`), **embedded** via `include_str!` so the binary is self-contained (establishing the `data/` pattern CLAUDE.md reserves for declarative providers). Parsing lives in `src/recommend.rs` behind a small typed model (`Catalog`/`Recommendation`), unit-tested against the shipped file (valid TOML, unique ids, every entry actually does something).
- **Distro-awareness is data, not branching.** Each entry *declares* the distro ids it applies to (empty = all); `Catalog::for_distro(id)` filters on that. The one new host fact is `Distro::id()` (a plain string accessor — the recommend-catalog is the first real consumer of distro-awareness that ADR-0029 deferred to "its first consumer"). No `if fedora` anywhere; adding a distro is adding data.
- **`jii recommend` is read-only** (Analyze → Explain): it groups the applicable entries by category and shows, per entry, *what it is*, *why you'd want it*, and *the exact way to get it* — a `jii <specs>` install for anything installable through JII, or, for a step a package install can't express (enabling a third-party repo), the **documented command shown for the user to run themselves** (`manual`). JII never runs `manual`, never edits repos, never `curl|sh`. Notes surface trust boundaries explicitly (RPM Fusion = "you are extending who you trust").
- Entries resolve to **normal JII specs** (`steam:flatpak`, `vlc`), so *applying* a recommendation is just the existing install path — the catalog adds no new install machinery and the engine still never learns about "recommendations".

**Alternatives considered:**
- **Hardcode recommendations in Rust with `if distro == Fedora` / hardware probes.** Rejected head-on: it violates ADR-0029, buries auditable content in code, and makes community contribution a code change. Data + a declared-distro filter is the whole point.
- **Make `recommend` auto-apply (install the whole set).** Rejected for slice 1: apply must be per-entry, previewable, and explicitly confirmed (Analyze → **Ask** → Apply), and repo-enabling crosses a trust boundary that deserves its own careful plan. Reporting first is the honest, shippable floor; guided per-entry apply is the next slice.
- **Model repo-enabling (RPM Fusion) as a fake package.** Rejected: JII's dnf provider installs by *name*, not a remote `.rpm` URL, and pretending otherwise would either fail or hide a trust decision. Showing the official documented command is honest and keeps the user in control.
- **A `recommend` `Provider`.** Rejected: recommendations aren't a search source; they're curated content that *points at* real sources. Forcing them onto the `Provider` trait would distort it.

**Consequences:**
- Fedora users get a genuinely useful "round out my fresh install" surface; the catalog grows by editing TOML, reviewable in isolation.
- Distro coverage expands by adding entries with the right `distros` — no code change, no core/distro branching.
- The trust story stays visible: third-party repos are labelled, and JII never enables them silently.
- **Apply (landed, slice 2):** `jii recommend <id>` routes the entry's `packages` through the normal install path (preview → confirm → execute — reusing `self.install`, so the U3 already-installed pre-check and U5 preview come for free); `manual`-only entries stay "run this yourself" (JII shows the command, never runs it); an entry meant for another distro is refused honestly, never silently substituted. **Remaining follow-ups:** an interactive multi-pick ("apply these three"), detecting already-satisfied entries to skip them in the listing, and a real repo-enable capability so RPM Fusion can become a previewable plan instead of a shown command.
- **Debt noted:** the shipped Fedora entries (package names, the RPM Fusion command) are curated by hand and unverified on a clean VM here; verify in the T7 clean-VM pass. Non-Fedora entries are deliberately empty until verified on a real host.

## ADR-0034 — System-wide update: bare `jii update` aggregates each manager's bulk upgrade via an optional `plan_update_all`

**Status:** Accepted 2026-07-06. Lands with Terminal 1.0 (ADR-0026) U7 (UX pass, problem D10 / #15 "the universal update command"). Slice 1: dnf + flatpak (Fedora-verified); the remaining managers add `plan_update_all` incrementally.

**Context:** `jii update` (no args) previously updated only the packages in JII's own registry — a handful the user installed *through* JII. But the real ask (#15) is *"update my whole system with one command"*: the distro packages, the Flatpaks, everything the host's managers own — not just JII's slice. Doing that must not violate the two invariants: the core never branches on the source (ADR-0004), and growth happens through optional capabilities with safe defaults (ADR-0022/0025).

**Decision:** Add an **optional** `Provider::plan_update_all(&self) -> Result<Option<InstallPlan>>` (default `None`): "upgrade everything this source owns", as one plan (`dnf5 upgrade -y`, `flatpak update -y`, later `pipx upgrade-all`, `apt-get upgrade`, …). Bare `jii update` asks the engine to **aggregate** every available provider's `Some(plan)` into the usual batched, previewable, single-confirmation, single-escalation run (`Engine::plan_update_all` → `SystemUpdate { plans, sources }`; `Engine::run_system_update` primes privilege once across the mixed root/user plans and runs them). A provider with no first-class bulk upgrade (github, cargo, go) returns `None` and simply doesn't participate — the engine never branches on the source, it just uses whatever plans come back.

**Non-regression — the per-record fallback.** Bare update must not *lose* the ability to update JII-installed packages from sources without a bulk path (a github binary, a cargo crate). So bare `jii update` also collects the registry records whose source is **not** among the aggregated `sources`, runs them through the existing per-record update path (`refresh_for_update` + `plan_record_batch(Update)`), and appends them to the same run. Result: the bulk managers upgrade the system; the niche sources still update per-record; nothing is missed and nothing is double-covered (a source is in exactly one bucket). Named `jii update <pkg>` is unchanged — it keeps the registry path (and `<pkg>:source` still pins the copy, ADR-0031).

**Recording.** The bulk plans upgrade the whole system, well beyond JII's registry, so they are **not** recorded (JII's registry tracks JII-installed packages, not the OS). Only the per-record fallbacks refresh the registry (via `record_update`, as before). Consequence: after a system update, `jii list` may show stale versions for JII-tracked *dnf/flatpak* packages (they were upgraded by the bulk plan, not re-queried). Accepted for MVP — re-querying every tracked package's version on every system update is expensive; noted as debt.

**Alternatives considered:**
- **Keep bare `jii update` = registry-only.** Rejected: it answered the wrong question (#15 wants the *system*, not JII's slice), and users were surprised a bare update left the OS un-upgraded.
- **A `--system` flag / a separate `jii upgrade` command.** Rejected on the ADR-0031 principle — the bare, most-common invocation should do the obviously-wanted thing; a flag to opt *into* "actually update my system" is backwards. Named update covers the "just this package" case.
- **Aggregate but drop the per-record fallback (pure ADR text).** Rejected as a silent regression: it would stop updating github/cargo/go packages under bare update with no replacement. The fallback keeps the promise "never break existing behavior without explaining why".
- **Record the bulk upgrade against the registry** (re-query versions after). Rejected for now: costly and out of scope; the registry is a JII-install ledger, not a system inventory.

**Consequences:**
- `jii update` now means "update my whole system", the intuitive behavior — Fedora gets `dnf upgrade` + `flatpak update` in one previewable, single-confirmation run.
- Coverage grows by adding `plan_update_all` to more providers (pipx `upgrade-all`, apt/pacman/zypper/snap/brew/nix) — one method each, no core change.
- **Debt:** bulk-updated tracked packages can show a stale version in `jii list` (not re-queried); the non-Fedora `plan_update_all` impls are unverified until a clean-VM host (T7).

## ADR-0035 — `doctor` becomes the system helper; the `recommend` catalog folds into it and the standalone command is removed

**Status:** Accepted 2026-07-07. Lands in UX-wave 2 (owner-set, clean-VM feedback) as item ② of the agreed order ①→④. Supersedes the surface of ADR-0033 (the catalog *data subsystem* is unchanged; only its entry point moves). Unfreezes `doctor --fix`, which BETA_ROADMAP had parked — by explicit owner reprioritisation to polish before cutting Beta.

**Context:** Two VM-feedback points converged. (#2) `doctor` reported per-source health but wasn't the *"doctor of my system"* the user wanted — it never checked the environment (network, common tools, PATH, Flathub) nor offered to fix anything. (#14) `jii recommend` was disliked: a long, rarely-typed command for a catalog the user would more naturally meet while checking system health. The owner's decision (AskUserQuestion): **fold recommend into doctor, remove the standalone command.**

**Decision:**
- **`doctor` gains real host system checks** beyond the two Tier-1 ones: internet reachability (a fast HTTPS HEAD; failure reads critical), `git`/`curl` presence, `~/.cargo/bin` on PATH (only when cargo is present or the dir exists), and the Flathub remote configured (only when Flatpak is installed). Facts are gathered concurrently (`tokio::join!`) in `gather_system_facts`; the verdict/wording logic stays a **pure, unit-tested** `system_checks(&SystemFacts)`. This keeps the ADR-0004/0029 invariants: these are *environment facts*, not per-provider branching, and the core still never branches on a source.
- **`doctor --fix`** turns the fixable checks into actions, Analyze → Explain → Ask → Apply: `git`/`curl` route through the normal install path (`self.install`, which previews and confirms itself — JII installing its own prerequisites); the Flathub remote is a plain `flatpak remote-add` **shown before it runs** (Flatpak elevates via its own polkit, so JII wraps no sudo/pkexec — consistent with how the flatpak provider treats installs). `--dry-run` previews every fix without asking or changing anything. Manual-only checks (PATH, token, internet) carry no fix — JII will not edit your shell rc or invent a token. Each fix is data on the check (`Fix::Install` / `Fix::Command`), kept pure and unit-tested.
- **The recommend catalog folds into `doctor`'s tail** as a compact, informational "Suggestions for your system" section (title — why · the exact command to run). The **standalone `jii recommend` command and its apply-by-id path are removed.** Applying a suggestion is now simply *running the shown command* (`jii vlc`, or the documented `manual` command) — more transparent than the `recommend <id>` indirection. The `Recommendation.id` slug is no longer read at runtime (the uniqueness invariant moved to `title`); it remains in the TOML as an authoring anchor.

**Alternatives considered:**
- **Keep `jii recommend` as a thin alias / subcommand.** Rejected: the owner explicitly disliked the separate command; a fold, not a rename, was the ask.
- **Gate suggestions behind a `--suggest` flag.** Rejected: they enrich exactly the moments `doctor` is run (health check, first-run setup); showing them compactly is the value, and they stay silent when the catalog has nothing for the distro, so they never nag.
- **Keep apply-by-id inside `doctor`.** Rejected as needless indirection now that every entry shows the exact `jii …` command; `doctor --fix` already owns the *health* fixes, and suggestions stay purely informational (never auto-installed).
- **Auto-edit `~/.bashrc` for the PATH checks.** Rejected: editing a user's shell rc is exactly the kind of silent, hard-to-undo change JII avoids; PATH stays advice-only.

**Consequences:**
- `doctor` is now the single "is my system healthy, and what's worth adding?" surface: sources → system checks → suggestions, with `--fix` for the actionable ones.
- The command surface shrinks by one (`recommend` gone); help and docs updated (README, ARCHITECTURE command table).
- The catalog subsystem (ADR-0033) is untouched as *data*; only its presentation moved. `manual` entries are still shown, never run.
- **Follow-ups (unchanged from ADR-0033):** interactive multi-pick, skipping already-satisfied entries, a real repo-enable capability. **Debt:** the Fedora catalog entries are still unverified on a clean VM (T7).

---

## ADR-0036 — `jii providers`: the ecosystem *marketplace*; bootstrap a missing manager via optional `Provider::ecosystem`

**Status:** Accepted 2026-07-09. Lands in UX-wave 2 (owner-set, clean-VM feedback) as item ③ of the agreed order ①→④. Pure ADR-0022 optional-method growth — no change to the `Provider` trait's required surface, the model, or the executor.

**Context:** Two VM-feedback points (#7, #8). JII installs *packages* through managers (npm, cargo, brew, Flatpak…), but never let you see or manage **the managers themselves**: which ecosystems exist on this host, and — when one is missing — how to get it. #8 was concrete: `jii <some-npm-tool>` on a box without npm found nothing, with no hint that the fix is "install Node.js/npm first". The managers are exactly the kind of thing JII should install *for* you.

**Decision:**
- **A new read-only surface `jii providers`** lists the installable *ecosystem* managers with their presence on this host (installed vs available-to-install) — base system repos (dnf/copr/apt/pacman/zypper) and non-managers (github) are deliberately absent: you don't install those, they *are* the system.
- **Ecosystem-ness is provider metadata, not a core list.** A new **optional** `Provider::ecosystem() -> Option<Ecosystem>` (default `None`) lets a manager declare a human `label`, its `binary`, and a `Bootstrap`. The engine's `ecosystem_catalog()` aggregates whoever declares one and never branches on the source id (ADR-0004). This is the same optional-method growth pattern as `highlights`/`plan_update_all` (ADR-0022) — adding an ecosystem is a per-provider edit, not a core change.
- **`Bootstrap` has exactly two honest shapes:**
  - `Bootstrap::Packages(&[names])` — the manager lives in a distro repo (npm, cargo, go, pipx, flatpak, snap). The names are an **ordered cross-distro candidate list** (npm is `nodejs-npm` on Fedora, `npm` on Debian/Arch; go is `golang`/`golang-go`/`go`). `Engine::first_available_package` searches them in order and returns the first that resolves — JII's own search does the per-distro work, **no distro branch** (ADR-0004/0029).
  - `Bootstrap::Script(cmd)` — the manager bootstraps via its own upstream installer (Homebrew, Nix). JII **shows the command, never runs it**: piping an installer script into a shell is precisely the trust boundary JII refuses to cross (ADR-0005/0006).
- **`jii providers add <name>`** bootstraps a missing manager. For `Packages`, the resolved package is handed to the **normal install path** (`self.install`) — so bootstrapping a manager gets the same preview → confirm → execute → record flow as any package (the `doctor --fix` reuse pattern, ADR-0035). For `Script`, the command is shown with an explicit "JII won't run this for you". Already-installed and unknown-ecosystem cases answer clearly.

**Alternatives considered:**
- **Hardcode the ecosystem list + per-distro package names in the CLI.** Rejected: that is exactly the source-branching the architecture forbids, and it would rot per distro. Metadata on the provider + JII's own search keeps it declarative.
- **Auto-run the Homebrew/Nix installer scripts.** Rejected: `curl … | sh` is the canonical untrusted action; JII shows it and stops (ADR-0005/0006).
- **Fold this into `doctor`.** Rejected: `doctor` diagnoses *JII's own* health and offers small fixes (git/curl/Flathub); managing the whole ecosystem marketplace is a distinct, browsable surface. (Flatpak/Snap appear in both — `doctor` cares about the Flathub *remote*, `providers` about the manager's presence.)
- **A generic `jii install <manager>` special-case.** Rejected: install operates on packages; a manager isn't a package in the ledger sense, and `providers` gives the discoverable listing `install` can't.

**Consequences:**
- Adding a future ecosystem = implement `ecosystem()` on its provider; the listing and bootstrap come for free. The trait's required surface is unchanged.
- `Bootstrap::Packages` correctness depends on the candidate name lists; they are hand-curated and **unverified on clean non-Fedora VMs** (T7 debt, same class as the recommend catalog). A wrong/missing name degrades gracefully to an honest "couldn't find a package for X".
- **Follow-ups:** `jii providers remove <name>` (uninstall a manager) and richer per-ecosystem detail (version, count of packages JII installed through it) are natural next slices, deferred to keep this one small.

---

## ADR-0037 — `jii info` is an app *card*; rich metadata via optional `Provider::describe`

**Status:** Accepted 2026-07-09. UX-wave 2 item ④ (agreed order ①→④). Pure ADR-0022 optional-method growth — no change to the required `Provider` surface, the model's core types, or the executor.

**Context:** VM-feedback #4 — `jii info` printed a source list + recommendation reasons, but not the *"what is this app"* card a user expects: a description, homepage/repository, license, author. The data isn't on the hot search path (`PackageCandidate.raw` is minimal — enough to plan an install, not to describe a project), so the card needs a richer, on-demand metadata pull.

**Decision:**
- **A new `PackageInfo`** value type (all fields `Option`: description, homepage, repository, license, author) carries the card. It renders only the fields present, so a sparse source degrades gracefully.
- **An optional `async Provider::describe(&candidate) -> Option<PackageInfo>`** (default `None`) lets a source assemble the card. It is **async on purpose** (unlike the pure `highlights`): a source may do one extra call to fill it. The engine (`candidate_info`) calls it **only for the recommended candidate on `jii info`** — never on search — so the extra latency lands only where the user asked "tell me more".
- **dnf (the Fedora-first platform) implements it fully:** one `dnf5 info <name>` call, parsed by a pure, unit-tested `parse_info` (`Key : Value` with folded `      : …` continuations, first stanza wins) → Description/URL/License/Vendor. **github implements a cheap card** from what search already captured (`owner/repo` → repository URL + owner as author), no extra API call. Every other source inherits `None` and shows the basic card (name, summary, version, trust, source).
- **The card layout:** name → description (falls back to the candidate's one-line summary) → an aligned metadata block (Source, Version, License, Homepage, Repository, Author — present fields only) → the existing source list + recommendation. `--json` now returns an object `{ candidates, recommended, info }` (was a bare array) — richer and self-describing for a card command.

**Alternatives considered:**
- **Enrich `PackageCandidate` at search time** so `info` needs no second call. Rejected: it would slow every search to serve one command, and most fields (dnf License/URL, a repo's license) aren't in the cheap search response anyway.
- **A synchronous `describe`.** Rejected: dnf needs a subprocess call; forcing it sync would either block or push the I/O back into search. Async keeps it lazy and honest.
- **Fetch the GitHub repo-metadata endpoint for description/license.** Deferred: it is a second authenticated call per `info`; the cheap repo+author card ships now, the richer fetch is a follow-up.

**Consequences:**
- Adding a card for another source = implement `describe` on its provider; the rest is free. The required trait surface is unchanged.
- `jii info`'s JSON shape changed from an array to an object — acceptable now (no external consumers; the object is the correct shape for a card), noted for anyone scripting against it.
- **Follow-ups:** richer cargo/npm cards (their registry manifests carry homepage/repository/license/author — capture at search or a small extra fetch), the GitHub repo-metadata fetch (description/license), and flatpak AppStream metadata. **Debt:** dnf's `License`/`Vendor` are whatever the RPM declares (e.g. Fedora's `LicenseRef-Callaway-…` SPDX-ish strings) — shown verbatim, not normalized.

---

## ADR-0038 — `audit` folds into `jii list --audit`; the standalone command is removed

**Status:** Accepted 2026-07-09. UX-wave 2 item #5 (the last of the wave). A command-surface merge; no engine/model change (the engine's `audit()` and the `AuditEntry` model are untouched).

**Context:** VM-feedback #5 — `jii list` (what JII installed) and `jii audit` (the same installs, with trust/verification/concerns) are **two views of one dataset**: the install ledger. Two top-level commands for one ledger is more surface than the feature needs, and a user browsing "what did I install" is exactly who wants the security view one flag away.

**Decision:** `jii list` gains a `--audit` flag: bare `jii list` prints the plain NAME/SOURCE/VERSION table; `jii list --audit` prints the security table (NAME/SOURCE/TRUST/VERIFIED/STATUS + a "N need attention" summary). The **standalone `jii audit` command is removed.** The rendering moved verbatim into a private `audit_view(&engine, renderer)` helper; the engine's `audit()` computation and `AuditEntry`/`AuditConcern`/`AuditVerification` model are unchanged. Same fold-a-command-into-a-flag pattern as ADR-0035 (`recommend`→`doctor`).

**Alternatives considered:**
- **Keep `jii audit` as an alias.** Rejected: the ask was to consolidate the surface, and `--audit` reads naturally as "the audit view of my list". (An alias also keeps the redundant top-level command in `--help`.)
- **Make audit a separate `--security`/`--concerns` flag name.** Rejected: `--audit` matches the prior command name, so muscle memory and docs carry over cleanly.

**Consequences:**
- One fewer top-level command; `jii audit` now falls through to installing a package literally named `audit` (consistent with any non-command word). Docs (README, ARCHITECTURE) and in-code comments updated to `jii list --audit`.
- The `--audit` JSON keeps the former `audit` array shape (per-install rows) — no consumer breakage for the security view; bare `jii list` keeps its own records array.
- **UX-wave 2 is complete** with this merge (①→④ + the two folds #2/#14 and #5). Beta prep resumes next.

---

## ADR-0039 — Distribution: prebuilt static-musl binaries + native packages, no user compile

**Status:** Accepted 2026-07-09. Beta-readiness (BETA_ROADMAP item 5). Owner asked for "convenient install for every distro without building." Delivery/infra decision — no change to the crate's code architecture.

**Context:** Before the first public Beta, users must be able to install JII on any distro **without a Rust toolchain and without compiling**. The prior `release.yml` built a single x86_64 glibc binary (Ubuntu 22.04) and attached a tarball — portable, but not "native" per distro, no ARM, and no one-line install.

**Decision:**
- **Static musl binaries for `x86_64` and `aarch64`.** A `*-unknown-linux-musl` build is fully static, so one binary runs on every distro (glibc or musl, old or new) and on ARM (Raspberry Pi, ARM servers, Asahi). JII shells out to the system's managers, so it bundles nothing else. Built in CI via `cross` (Docker), so no per-distro build host is needed.
- **Native `.deb` and `.rpm` via [nfpm](https://nfpm.goreleaser.com)** — one YAML (`packaging/nfpm.yaml`), both formats, both arches, assembled on the CI runner (nfpm just packs files; needs no dpkg/rpm/target host). Packages bundle the binary, a man page, and bash/zsh/fish completions. Attached to the GitHub Release; users `dnf/apt install ./jii.*`.
- **`install.sh`** (`curl … | sh`) — detects arch, downloads the matching musl tarball from the latest release, **verifies its sha256**, installs to `~/.local/bin` (never root). POSIX sh, curl-or-wget, jq-free.
- **Shell completions + man page** are generated by the binary itself via **hidden** `jii completions <shell>` / `jii man` subcommands (clap_complete / clap_mangen over the existing derive `Command`) — no build.rs/workspace needed (respects the single-crate constraint), and they're arch-independent so CI emits them once with a host build.
- **Official-repo scaffolding is prepared, not published** (needs the owner's accounts): `packaging/jii.spec` (a **binary repack** of the release tarball, so a COPR build Just Works with no compile) and `packaging/aur/PKGBUILD` (`jii-bin`). `packaging/README.md` is the turnkey publish checklist. Publishing to COPR/AUR is deliberately left to the owner (accounts, signing, hosting) and is a post-Beta nicety on top of the already-working GitHub-release install.
- **Leaner release profile:** `[profile.release]` gains `lto`, `codegen-units = 1`, `strip` (default opt-level and unwinding kept — behavior unchanged), trading CI compile time for a smaller, faster binary.

**Alternatives considered:**
- **glibc build (status quo).** Rejected: pins a minimum glibc, needs a matrix of build images for broad compatibility; static musl sidesteps the whole libc-version problem with one artifact.
- **A from-source RPM spec (Fedora rust-packaging macros).** Deferred: correct for a real Fedora submission but heavy (vendored deps / network-in-mock); the binary-repack spec gives the owner a working COPR immediately. Revisit if JII is submitted to Fedora proper.
- **build.rs / a separate xtask crate for completions.** Rejected: build.rs can't easily use the crate's own derive `Command`, and a workspace violates the single-crate constraint. A hidden runtime subcommand is the least-friction path and doubles as a user-facing feature.
- **`goreleaser` for the whole pipeline.** Rejected for now: it's Go-centric and heavier than needed; hand-written workflow + nfpm is transparent and enough.

**Consequences:**
- One `git tag v*` push produces, for both arches: a checksummed tarball, a `.deb`, and a `.rpm`, plus the `install.sh` one-liner — "install on any distro without building" is satisfied end-to-end by the agent-owned pipeline.
- The full release workflow (musl cross-build, nfpm, publish) **cannot be run without pushing a tag**, so it is verified by construction + local checks (host release build with the new profile; tarball layout + `install.sh` extraction/checksum; completions/man non-empty; spec parses); first real run is the owner's tag push. The `install.sh` and package layouts were validated against a locally-assembled tarball.
- Completions/man add two small deps (`clap_complete`, `clap_mangen`) and two hidden subcommands (`cli_definition_is_valid` test guards the tree).
- COPR/AUR go live only when the owner runs the documented steps; until then the GitHub-release `.deb`/`.rpm`/tarball/`install.sh` already cover every distro.

---

## ADR-0040 — JII self-update/uninstall: `jii` is a special name that manages the tool itself

**Status:** Accepted 2026-07-09. Owner-requested during Beta (self-update unfrozen, like `doctor --fix`) — a Beta needs an easy path to the next Beta. Pure ADR-0022-style additive growth: one new `Action`, one self-contained module; no change to the `Provider` trait or the core model's meaning.

**Context:** People install JII several ways (install.sh/tarball → `~/.local/bin`, `cargo` → `~/.cargo/bin`, or a `.rpm`/`.deb` → `/usr/bin`), and a downloaded `.rpm`/`.deb` is **not in a repo**, so `dnf/apt upgrade` won't move it. JII already knows how to install software the right way for its source; it should treat *itself* the same. Owner decisions (AskUserQuestion): **fold self-update into `jii update`** (the literal name `jii` means "the tool itself"), **add `jii uninstall` / `jii remove jii`**, and for the package case **"do whatever's best for the user."**

**Decision:**
- **`jii` is a reserved package name** meaning JII itself. `jii update jii` self-updates; `jii remove jii` and `jii uninstall` self-remove; a bare `jii update` runs the system update and then *nudges* if a newer JII exists (a message, not a surprise self-install mid-update). When mixed with real names (`jii update jii ripgrep`), the self action runs first, then the rest go through the normal path.
- **The mechanism follows how jii was installed** (`selfupdate::detect_install`, via `current_exe()` + `rpm -qf`/`dpkg -S` ownership; anything under `$HOME` is user-space by definition):
  - **user-space binary** → download the matching static-musl tarball, verify sha256, extract, and **atomically swap** it over the running binary. A new `Action::Replace { src, dest }` does an `fs::rename` — copying over a live executable fails with `ETXTBSY`, but a rename gives the new file a fresh inode while the running process keeps its old one. **No root.**
  - **package install** → download the matching `.rpm`/`.deb` and install it via `dnf`/`apt` as a previewable **root** step (escalated through `privilege.rs`, exact command shown first). JII never clobbers a packaged file behind rpm's back — the package database stays consistent ("cooperate, don't clobber").
- **Everything is an `InstallPlan`** built in the `selfupdate` module and run through the existing executor (`Engine::run_self_plan`), so `--dry-run` previews it and the download/verify path is the usual one. Version comparison is a deliberate **plain "different tag → offer"** (versions are opaque, ADR-0009) — no fragile semver ordering. `Cargo.toml` version was aligned to `0.1.0-beta` so the binary's reported version matches the release tag.

**Alternatives considered:**
- **A separate `jii self update` namespace.** Rejected: owner chose to fold it into `update` with `jii` as the special name — fewer commands, reads naturally.
- **Always self-replace the binary, even for `/usr/bin`.** Rejected: it desyncs the rpm/dpkg database (`rpm -V` would flag a modified file) — the exact "clobbering the system" JII refuses to do.
- **Semver-compare versions to decide "newer".** Rejected: versions are opaque (ADR-0009); "different published tag → offer, you decide" is honest and simpler.
- **Auto-self-update on every `jii update`.** Rejected: a surprise self-install mid system-update is startling; a nudge respects consent (Analyze → Explain → Ask).

**Consequences:**
- A real package literally named `jii` on some source is now shadowed by the self-management path — acceptable (JII is the tool; that name belongs to it).
- `Action::Replace` is a small general primitive (atomic rename into place); the executor, `describe_action`, and the JSON schema all handle it.
- The self-update fetch + swap **can't be exercised without a real newer release**; the plan-building and asset selection are pure and unit-tested, the network fetch + atomic swap are verified by construction + `--dry-run`. First true end-to-end self-update happens when the owner cuts the *next* tag.
