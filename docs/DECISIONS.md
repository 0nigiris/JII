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
- **`jii` is a reserved package name** meaning JII itself. `jii update jii` self-updates; `jii remove jii` and `jii uninstall` self-remove; a bare `jii update` **updates everything** — the whole system and then JII itself (self-update runs last, so the atomic binary swap happens after the system upgrade; it still previews and prompts via the normal confirm, so it isn't silent). When mixed with real names (`jii update jii ripgrep`), the self action runs first, then the rest go through the normal path. *(Revised from the original "nudge only" design at the owner's request — `jii update` should update everything; see the alternative below.)*
- **The mechanism follows how jii was installed** (`selfupdate::detect_install`, via `current_exe()` + `rpm -qf`/`dpkg -S` ownership; anything under `$HOME` is user-space by definition):
  - **user-space binary** → download the matching static-musl tarball, verify sha256, extract, and **atomically swap** it over the running binary. A new `Action::Replace { src, dest }` does an `fs::rename` — copying over a live executable fails with `ETXTBSY`, but a rename gives the new file a fresh inode while the running process keeps its old one. **No root.**
  - **package install** → download the matching `.rpm`/`.deb` and install it via `dnf`/`apt` as a previewable **root** step (escalated through `privilege.rs`, exact command shown first). JII never clobbers a packaged file behind rpm's back — the package database stays consistent ("cooperate, don't clobber").
- **Everything is an `InstallPlan`** built in the `selfupdate` module and run through the existing executor (`Engine::run_self_plan`), so `--dry-run` previews it and the download/verify path is the usual one. Version comparison is a deliberate **plain "different tag → offer"** (versions are opaque, ADR-0009) — no fragile semver ordering. `Cargo.toml` version was aligned to `0.1.0-beta` so the binary's reported version matches the release tag.

**Alternatives considered:**
- **A separate `jii self update` namespace.** Rejected: owner chose to fold it into `update` with `jii` as the special name — fewer commands, reads naturally.
- **Always self-replace the binary, even for `/usr/bin`.** Rejected: it desyncs the rpm/dpkg database (`rpm -V` would flag a modified file) — the exact "clobbering the system" JII refuses to do.
- **Semver-compare versions to decide "newer".** Rejected: versions are opaque (ADR-0009); "different published tag → offer, you decide" is honest and simpler.
- **Nudge-only on bare `jii update` (the original ADR-0040 design).** Superseded at the owner's request: a bare `jii update` is expected to update *everything*, so it now runs the self-update inline after the system upgrade. Consent is preserved through the usual confirm prompt (respecting `--auto`/`-y`/`-n`), so it is Analyze → Explain → Ask, not a silent mid-update self-install.

**Consequences:**
- A real package literally named `jii` on some source is now shadowed by the self-management path — acceptable (JII is the tool; that name belongs to it).
- `Action::Replace` is a small general primitive (atomic rename into place); the executor, `describe_action`, and the JSON schema all handle it.
- The self-update fetch + swap **can't be exercised without a real newer release**; the plan-building and asset selection are pure and unit-tested, the network fetch + atomic swap are verified by construction + `--dry-run`. First true end-to-end self-update happens when the owner cuts the *next* tag.
- **Update (v0.1.3-beta):** `latest_release()` must use the **list** endpoint (`/releases`, take the first non-draft), not `/releases/latest` — the latter 404s on a repo whose only releases are pre-releases (every JII beta tag is a prerelease). Any binary built before this fix is stuck on the 404 and needs one manual update (install.sh / rebuild) to get a working checker; self-update works from then on.

---

## ADR-0041 — `jii doctor` is an interactive setup questionnaire (not a list of advice)

**Status:** Accepted (supersedes the read-only-doctor stance of ADR-0034-era U6/D6).

**Context:** `doctor` diagnosed the system and *advised* — it printed "add it with: `jii git`", "run: `flatpak remote-add …`", and a passive "Suggestions for your system" list (RPM Fusion, codecs, fonts…). The user had to copy/paste each command. The owner's explicit ask: doctor should **do** it, not describe it — "Want RPM Fusion? [y/N]" → yes → it runs the command; "Add cargo to PATH? [y/N]" → yes → it edits your shell rc. A questionnaire, not a wall of tips.

**Decision:**
- **`jii doctor` (bare) is interactive by default.** After the source diagnostics + system checks, in an interactive terminal it walks every actionable item as a plain **yes/no question** (default **no** — Enter skips) and, on "yes", **applies it on the spot**: install a package, run a documented command, or fix `PATH`. The old passive suggestions list and the old `--fix` behavior are folded into this one flow.
- **What it can now set up:** the fixable system checks (`git`/`curl`, the Flathub remote, **and now `~/.local/bin` / `~/.cargo/bin` on `PATH`**) *plus* every distro-appropriate catalog suggestion (RPM Fusion, multimedia codecs, fonts, VLC, Steam, Wine, power tuning…).
- **PATH fixes edit the shell rc** (`Fix::PathExport`): JII appends the correct line for the user's `$SHELL` — `fish_add_path <dir>` for fish, `export PATH="<dir>:$PATH"` for bash/zsh — idempotently (skips if the rc already references the dir). This **reverses the earlier "JII won't edit your shell rc" boundary**, but only on an explicit per-item "yes".
- **The single question is the consent.** Installs carry a `with_yes` through the normal install path so they don't ask twice; the **trust barrier still holds** — an untrusted source is never auto-confirmed by the questionnaire's yes (ADR-0006). Catalog `manual` commands run via `sh -c` (they use `$(rpm -E %fedora)` and carry their own `sudo`, whose prompt is visible). `--dry-run` shows what each "yes" *would* do and changes nothing.
- **Read-only is preserved** for the non-interactive contexts: `--json` (unchanged machine schema), `-n/--no`, and no-TTY all skip the questionnaire and just report + list the catalog. The first-run wizard runs the interactive doctor (prime onboarding moment), gated behind its own opt-in prompt.
- **`--fix` is now a hidden no-op** kept only so existing `jii doctor --fix` invocations don't error.

**Alternatives considered:**
- **Keep advice-only + `--fix` for applying.** Rejected: the owner wants the default doctor to act; copy/pasting commands is exactly the friction JII exists to remove.
- **Refuse to touch the shell rc (the old boundary).** Rejected at the owner's request — but constrained to an explicit per-item yes and made idempotent, so it's a consented edit, not a silent one.
- **Default each question to "yes".** Rejected: a fresh-system questionnaire that installs on Enter is too aggressive; default-no keeps the user in control (press `y` to opt in).
- **Bare doctor stays read-only; add `jii setup`/`--interactive`.** Rejected: the bare, most-common invocation should do the obviously-wanted thing (same principle as ADR-0034's bare `update`).

**Consequences:**
- `doctor` now mutates the system (with consent) — the JSON/`--no`/non-TTY read-only paths are the contract for scripting.
- New `Fix::PathExport` primitive + a pure `path_export_edit(shell, dir)` helper (unit-tested); `install` split into `install_inner(assume_yes)` with a thin `install` wrapper, and `PromptFlags::with_yes`.
- The `data/recommend/catalog.toml` entries are now *actionable* prompts, not just text — their `packages`/`manual` fields drive real applies, so they must stay conservative and correct.

---

## ADR-0042 — Search matching: exact-first, broaden on a miss (prefix + trailing-typo)

**Status:** Accepted.

**Context:** Providers searched by **exact name**, so `jii ayugram` found nothing even though `ayugram-desktop` exists, and a typo like `ayugramm` was hopeless. The owner wants `ayugram` to resolve to `ayugram-desktop`, and a near-miss to still be found or offered ("did you mean …?"). But naive broadening is dangerous: `dnf5 repoquery '*git*'` matches **~1300** packages; even `'git*'` is 68. So we cannot just substring-match everything on every query.

**Decision:**
- **Exact-first, broaden only on a miss.** The normal search stays exact (noise-free, fast — `jii git` is unaffected). Only when the exact search returns **nothing** does the engine broaden (`Engine::broaden_search`):
  1. **Prefix** — re-query with `MatchMode::Prefix` (dnf sends `<term>*`), so `ayugram` → `ayugram-desktop`. Prefix, not substring, keeps the fallback focused (68 vs 1300 for `git`).
  2. **Trailing-typo fallback** — if the prefix finds nothing, trim up to two trailing characters and retry the prefix search, so `ayugramm` still reaches `ayugram*`. A stem under four characters isn't tried (too little signal).
- **Match mode is a `Query` field**, set by the engine and honored per-provider (dnf appends `*`; providers with no glob support ignore it — their native search is already broad, and ranking filters afterwards). The core still doesn't branch on the source: it sets one uniform mode and lets each provider interpret it.
- **Ranking is now name-aware** (`rank(config, query, candidates)`): the primary sort key is a **name-match tier** — 0 exact · 1 prefix · 2 substring · 3 unrelated (case-insensitive) — then the existing source-priority and trust, then shorter-name-first. So an exact match on a *lower-priority* source still beats a prefix match on a higher-priority one, and a broadened result never recommends a random longer name over the closest one.
- **The recommend + confirm is the "did you mean".** When the best match isn't what was typed, install prints `No exact match for '<typed>'. Closest: <name>.` before the normal preview + confirmation, so a broadened result is never installed silently — the user sees the substituted name and can decline. `search`/`info` broaden the same way. "Also available" is capped (6) and now shows names, since alternatives can differ from the pick.

**Alternatives considered:**
- **Always substring-match (`*term*`).** Rejected: ~1300 hits for `git` — slow, noisy, and it buries the exact match. Exact-first with a prefix fallback gives the recall without the noise.
- **Full fuzzy/edit-distance "did you mean" over all package names.** Rejected for now: JII has no local name index, and dnf/repoquery can't edit-distance match; enumerating every package to score locally is far too expensive on the hot path. The trailing-trim heuristic covers the most common slip cheaply; broader fuzzy is future work (a follow-up ADR if it earns its keep).
- **Auto-install the closest match without asking.** Rejected: a broadened match is a *guess*; the existing confirm (with the "closest" note) keeps the human in the loop, matching Analyze → Explain → Ask.

**Consequences:**
- New `MatchMode` enum + `Query::with_match_mode`; `Engine::broaden_search`; name-aware `rank` (all four call sites updated); capped, name-showing "Also available".
- Broadening costs an extra provider round-trip **only on an exact miss** — the common case pays nothing.
- Only dnf honors `Prefix` today (the confirmed case); COPR keeps its project-name resolver, and other providers rely on their native breadth. Extending explicit prefix support to more providers is incremental and needs no core change.

---

## ADR-0043 — Doctor analyses system state before suggesting (skip what's already done)

**Status:** Accepted (slice 1 of the post-testing UX wave; refines ADR-0041).

**Context:** `doctor`'s questionnaire offered every distro-appropriate catalog suggestion unconditionally — it proposed installing **VLC even though it was already installed**. That breaks the whole point of `doctor`: it must *diagnose the system*, not read out a canned list. The owner's principle (#9): JII cooperates with the system, it doesn't live beside it.

**Decision:**
- `doctor` gathers the **installed set once** (`Engine::installed_index` — a `HashSet` of source-native names/app-ids, one `list_installed` per available provider) and **filters out every suggestion the user has already done** before offering anything. If a fix and every suggestion are satisfied, it says "you're all good."
- Each catalog entry declares how to tell it's done: an optional **`check`** identifier (a Flatpak app-id like `com.valvesoftware.Steam`, or a repo's release package `rpmfusion-free-release`) when the installed name differs from the install spec; otherwise satisfaction is derived from `packages` (bare names, `:source`/`@ref` stripped). An entry with no derivable identifier is never auto-hidden (offer beats wrongly hiding).
- `Recommendation::is_satisfied(&installed)` is pure and unit-tested; the I/O (the scan) lives in the engine.

**Alternatives considered:**
- **Per-suggestion targeted "is X installed" calls.** Rejected: N suggestions × per-provider `list_installed` is O(N·providers) process spawns (dnf `repoquery --installed` each time). One shared index is a handful of calls total.
- **Check via `which <name>`.** Rejected: catalog items (codecs, fonts, repos) aren't PATH binaries; only the source's installed list is authoritative.
- **Verify installed state inside the pure catalog module.** Rejected: keeps `recommend` pure data (ADR-0033); the engine owns the I/O, the module owns the match logic.

**Consequences:**
- `doctor` now costs one installed-scan on the interactive path (acceptable — it *is* the diagnosis). The read-only `--json`/`-n` listing stays a plain catalog reference for now.
- New `Recommendation.check` field + `satisfied_ids`/`is_satisfied`; `Engine::installed_index`; `doctor_offer` takes `&Engine`. Live-verified on Fedora: with VLC/codecs/fonts/Steam/RPM Fusion all present, doctor offers none of them.
- Flatpak-pinned or repo-style entries need an accurate `check`; a wrong/missing one just means the entry is offered when already done (safe, not harmful).

---

## ADR-0044 — Search speed: a per-source failure circuit breaker

**Status:** Accepted (slice 2 of the post-testing UX wave).

**Context:** Search felt slow. Measured: even a *warm* `jii search` took **~5.06 s** wall at ~0.04 s CPU — one source (COPR, whose search API is ~9 s against the 5 s per-provider timeout) times out every time, and the fan-out `join_all` waits for the slowest source. Failures weren't cached, so every search re-paid COPR's full timeout.

**Decision:**
- Add a **disk-persisted, per-source failure table** to the search cache (a **circuit breaker**). When a source times out or errors, its id is stamped with the time (`mark_failure`); a source that failed within `network.failure_cooldown_secs` (default **120 s**) is **skipped without waiting** on later searches (`recently_failed`), serving a stale cache entry if present. A successful search clears the mark (`clear_failure`) — the circuit self-heals after the cooldown or on first success ("half-open" retry).
- It must persist to disk because each `jii` invocation builds a fresh `Engine`; an in-process breaker wouldn't survive between commands, and "every *separate* `jii` search waits 5 s" is exactly the complaint. The failure table rides in the existing `search-cache.json` (schema bumped to `{ entries, failures }`; an old plain-map file is simply discarded as a stale cache).
- A **missing local tool** (`is_available == false`) is *not* a failure — it's instant, and marking it would wrongly suppress a source the user might install mid-session.

**Alternatives considered:**
- **Lower the global timeout (5 → 2–3 s).** Rejected as the primary fix: it would permanently break COPR (genuinely ~9 s) even when it's working, and still pays the timeout on the first hit. The breaker keeps a working-but-slow source usable (it succeeds once, then is cached) while making repeats instant. (The timeout knob stays available for tuning.)
- **In-process circuit breaker only.** Rejected: doesn't survive between `jii` invocations, so it wouldn't speed up the common "run a few searches in a row" flow.
- **Early return once high-priority sources answer (don't await stragglers).** Deferred: a real improvement for the *first* search, but it changes result completeness/ranking and is a bigger change; the breaker delivers most of the felt speedup first.

**Consequences:**
- Verified on Fedora: first search 5.07 s (pays COPR once, marks it), every subsequent search **~1.1 s** (~4.5× faster) until the cooldown lapses.
- A source that's slow/down contributes nothing for up to `failure_cooldown_secs` even if it recovers early — acceptable; stale cache still serves its last-known results, and `jii sources`/`doctor` probe it directly.
- New `network.failure_cooldown_secs` config; `Cache` gains `recently_failed`/`mark_failure`/`clear_failure` (unit-tested); `search_one` consults and updates the breaker. The first search of a session still pays one timeout — the future early-return work (above) would address that.

---

## ADR-0045 — `info` is a *show*, separate from `install`/`search` (informational reference)

**Status:** Accepted (slice 3 of the post-testing UX wave).

**Context:** `jii info lodash` answered with install phrasing — effectively "nothing to install" — because `info` reused the install-oriented resolve (which filters to installable *programs*) and, on a miss, the install-path `explain_miss`. But `info` should *show information*, `search` should *search*, `install` should *install* — the commands must be logically separated (#6), and a library like lodash is real and describable even though JII won't install it as a program (#5).

**Decision:**
- New optional `Provider::reference(query) -> Option<Reference>` (ADR-0022 growth, default `None`): an **informational** card resolved from a name, **independent of installability**. npm implements it from the registry manifest (description, homepage, cleaned repository URL, version) plus a `note` when the package is a library.
- `Engine::reference(name)` asks each available source and returns the first card; `jii info` calls it **only when the installable resolve found nothing**, then renders it with `render_reference` — name, description, links, and the clarifying note — with **no install phrasing**. A true unknown now reads "No information found for '{name}'", not "nothing to install".
- The library explanation is one shared, actionable message (`library_note`): "`lodash` is an npm library — a code dependency, not a runnable program … run `npm install lodash`." Used by both the install-path `explain_miss` (#5) and the info `reference` note, so the wording is consistent.
- The "JII installs *programs*, not libraries" philosophy is unchanged (owner-confirmed) — only the messaging and the info/install separation change.

**Alternatives considered:**
- **Make `jii lodash` install libraries (a `--lib` mode).** Rejected by the owner — keep programs-only; fix the *experience* (clear message + real `info`).
- **Reuse `describe(candidate)` for info.** Rejected: it needs a `PackageCandidate`, which a library never produces (search filters it out); `reference` resolves from the bare name.
- **Only reword `explain_miss`.** Rejected as insufficient: `info` would still be install-framed and show nothing about the package; the point is `info` shows *what it is*.

**Consequences:**
- Verified: `jii info lodash` shows description/version/homepage/repository + note; `jii lodash` gives an actionable library message. `info` no longer borrows install/search logic.
- New `Reference` model + `Provider::reference` + `Engine::reference` + `render_reference`; npm gained `homepage`/`repository` manifest fields and `repository_url` (unit-tested). Other sources default to `None` (cargo is the obvious next `reference` implementer). 204 tests.

---

## ADR-0046 — A bare ecosystem-manager name installs the manager (no circular search)

**Status:** Accepted (slice 4 of the post-testing UX wave).

**Context:** `jii npm` searched for a *package named "npm"* — and, since the npm registry has one, offered to install `npm@12` **through npm itself** (a circular absurdity). The philosophy (#9): if a manager is missing, `jii npm` should install the *manager*; JII cooperates with the system, it doesn't hunt a package by the manager's own name.

**Decision:**
- The install path routes a **bare ecosystem-manager name** (npm, cargo, pipx, flatpak, snap, go, brew, nix) to the **bootstrap** flow — the same path as `jii providers add <m>`, now shared as `bootstrap_ecosystem`. `jii npm` on a box without npm bootstraps it; with npm present it says "Node.js (npm) is already installed — it's a package manager JII drives" (no pointless reinstall).
- A name counts as a manager only when **unpinned** (no `:source`, no `--source`); `jii npm:npm` still installs the real registry package, `jii npm:dnf` the distro one. The escape hatch keeps the rare "I really want the package called npm" case reachable.
- Detection is cheap: `Engine::ecosystem_ids()` is pure (no I/O), so an ordinary `jii vlc` pays only a name comparison — the `ecosystem_catalog` probe runs only when a name actually matches.
- **Loop guard:** a bootstrap package's own name can *be* a manager id (the Fedora `pipx` package for the pipx ecosystem; `npm` for npm). Installing it must **not** re-enter routing, so `install_inner` gained a `route_managers` flag — off for bootstrap installs and for doctor's explicit package installs, on for the user-facing `jii <name>`. (The async recursion install→route→bootstrap→install is `Box::pin`-ed.)

**Alternatives considered:**
- **Only `jii providers add npm`, never `jii npm`.** Rejected: the bare, most-natural invocation should do the obviously-wanted thing (same principle as bare `update`, ADR-0034) — a user types `jii npm` to get npm.
- **Hardcode the manager id list in the CLI.** Rejected: it would duplicate the `Ecosystem` metadata; `ecosystem_ids()` derives it from the providers, so the core never branches on a source and a new manager needs no CLI change.
- **Route inside the public `install` wrapper (build a second engine).** Rejected: that doubles engine/registry loads on every ordinary install; routing lives in `install_inner` on its single engine, gated by the cheap id check.

**Consequences:**
- Verified on Fedora: `jii npm`/`jii cargo` → "already installed" (no reinstall); `jii pipx` (absent) → resolves the dnf `pipx` and offers it via dnf, no loop; `jii npm:npm` → the registry package; `jii vlc` unaffected. 204 tests.
- New `Engine::ecosystem_ids`; `route_managers` + `bootstrap_ecosystem` in the CLI; `install_inner` gained a `route_managers` flag. A manager with only a `Bootstrap::Script` (brew/nix) shows its script, never runs it (trust boundary, ADR-0005).

---

## ADR-0047 — Nix `list_installed`: schema-tolerant `nix profile list --json`

**Status:** Accepted (slice for #3). *Unverified on a live Nix host — parser is fixture-tested.*

**Context:** The Nix provider left `list_installed` empty because `nix profile list --json`'s schema changed across versions, so JII couldn't see Nix packages installed *outside* jii — it only knew its own registry records. The owner wants correct detection of already-installed Nix packages (#3).

**Decision:**
- Parse `nix profile list --json` via `serde_json::Value` and tolerate **both** shapes: modern Nix (≥2.20) keys `elements` by the profile element **name** (a map); older Nix makes `elements` an **array** where the name is derived from `attrPath`'s last segment (`legacyPackages.x86_64-linux.ripgrep` → `ripgrep`), falling back to the store-path basename. Any unrecognised/empty/garbage shape yields **no records**, never an error — a broken `nix` must not break JII.
- Version is best-effort from the store path (`…-<name>-<version>`); absent it's just `None` (versions are opaque, ADR-0009).
- `is_installed` keeps its profile-symlink check as an independent verifier.

**Alternatives considered:**
- **Keep it empty, rely on the registry + symlink.** Rejected: it can't see non-jii installs, which is exactly the "cooperate with the system" gap (#3/#9).
- **Pin to one schema.** Rejected: it would silently return nothing on the other Nix version — the tolerant parser handles the field that's stable (`attrPath`/`storePaths`).

**Consequences:**
- Fixture-tested on both schemas + garbage; **needs a real Nix host to confirm** the live JSON matches (open risk — flagged in AI_CONTEXT). Pure helpers (`parse_profile_list`, `element_name`, `store_name`, `store_version`) are unit-tested.
- Nix now participates in `installed_index` (so `doctor` and cross-source "already installed" see Nix packages) and in remove/update owner resolution.

---

## ADR-0048 — Principle: JII cooperates with the system; it is not the centre of the world

**Status:** Accepted (guiding principle — the owner's #9; cross-references the ADRs that embody it).

**Context:** In real use, JII kept behaving as if only *it* mattered — `doctor` read out a canned list instead of inspecting the machine, `jii npm` searched for a package named npm instead of installing the manager, `jii info` spoke in install terms. The owner stated the principle plainly: **`jii update` means update the whole system; `jii remove` means remove the program however it was installed; `jii doctor` means analyse the whole system.** JII sits *on top of* the tools the user already has and cooperates with them — it does not live beside them.

**Decision (the principle, and how each command honours it):**
- **Read real system state, don't assume jii-only.** JII inspects what's actually installed across every source (`Engine::installed_index`, each provider's `list_installed`, incl. Nix now) rather than trusting only its own registry. `doctor` offers only what's genuinely missing (ADR-0043); "already installed" checks span sources.
- **The bare verb does the whole-system thing.** `jii update` upgrades every manager's packages *and* JII itself (ADR-0034/0040); `jii remove <x>` removes via whatever source owns it, chooser on multi-owner (UX #11); `jii doctor` diagnoses the host, not a script.
- **Install the manager, not a package named after it.** `jii npm` bootstraps/【notes】the npm manager (ADR-0046) — no circular "npm via npm".
- **Commands stay in their lane.** `info` shows, `search` searches, `install` installs (ADR-0045).
- **Cooperate, don't clobber.** Privileged steps only (never fully root, `privilege.rs`); packaged installs go through the native manager so the package DB stays consistent; self-update respects how JII was installed (ADR-0040). Third-party scripts (brew/nix bootstrap) are shown, never run (ADR-0005).
- **The core never branches on the source.** Distro/source knowledge lives in providers and data (ADR-0004/0029/0033), so "cooperation" is uniform, not a pile of `if fedora`.

**Consequences:**
- This ADR is the lens for future features: before adding behaviour, ask "does this cooperate with the system the user already has, or does it assume JII is the centre?" New commands/providers must read real state and keep verbs in their lane.
- No code of its own — it records the principle and points at ADR-0034/0040/0043/0045/0046/0047 as its concrete expressions.

---

## ADR-0049 — Forge abstraction: GitHub is one `Forge` among peers, not a hardcoded exception

**Status:** Accepted (slice for #8).

**Context:** GitHub Releases lived in `github.rs` with `api.github.com`, the GitHub JSON schema, its headers and the web URL all baked into the `Provider`. The owner wants Codeberg/Gitea, GitLab and similar forges to be easy to add later — so GitHub must not be a special case.

**Decision:**
- A new `Forge` trait captures **only** the host-specific bits: `id`, `label`, `repo_url(owner, repo)`, `async latest_release(...) -> Release` (normalised), and an optional `probe` (rate-limit). A generic `ForgeProvider` (in `provider/forge.rs`) implements `Provider` on top of it and owns everything forge-neutral: `owner/repo` parsing, arch-aware **asset selection** (raw binary / `.tar.gz` / `.zip`, musl-over-gnu, AppImage handling, OS/arch rejection), **checksum** discovery + verification, and the **user-space install plan** (`~/.local/bin`, no root, `untrusted` trust).
- `Release`/`ForgeAsset` are **normalised** types; each forge maps its native JSON onto them (`GithubForge::latest_release` parses GitHub's shape and calls `.normalize()`). So the shared code never sees a GitHub-specific field.
- GitHub becomes `GithubForge` (a ~120-line `Forge` impl). The registry builds it as `ForgeProvider::new(Box::new(GithubForge), token_env, arch)`. **Behaviour is identical** — same source id `"github"`, same candidates/plans, all prior tests moved and pass.

**Adding a forge (the payoff):** implement `Forge` (Gitea/Codeberg's releases API is close to GitHub's — `tag_name` + `assets[].{name,url,size}` — so it's nearly a drop-in; GitLab's differs more and just needs its own `latest_release` mapping), register it in `ProviderRegistry` with a new source id, add that id to `KNOWN_SOURCES` + the default priority, and select it per-spec via `owner/repo:codeberg` (ADR-0031). No core or shared-code change.

**Alternatives considered:**
- **Parameterise `Github` with a base URL only.** Rejected: GitLab's release API and asset shape differ enough that a URL swap isn't sufficient; a trait with a `latest_release` mapping is the honest seam.
- **A generic over `<F: Forge>` instead of `Box<dyn Forge>`.** Rejected: the registry stores `Box<dyn Provider>` and builds providers dynamically from config; a boxed forge keeps that uniform with no monomorphisation benefit here.

**Consequences:**
- `provider/forge.rs` holds the trait + `ForgeProvider` + all shared logic and tests; `github.rs` holds only `GithubForge` + GitHub JSON/rate-limit + its own tests. 210 tests green, clippy clean; `jii jqlang/jq` verified unchanged.
- Codeberg/Gitea/GitLab are now a well-scoped follow-up (implement `Forge` + wire the source id), not a rewrite. No live non-GitHub forge ships yet — the abstraction is in place and proven by GitHub riding on it.

---

## ADR-0050 — Localization: keys in code, strings in `locales/*.toml` (i18n framework)

**Status:** Accepted (slice 1 of #7 — the framework; string migration follows incrementally).

**Context:** All UI text (≈200 `renderer.*` calls + `#[error]` + provider reasons) was hardcoded in Rust. The owner wants multi-language support (English + Russian to start) with **no user-facing strings in the code** — code holds logic, text lives separately — and automatic language selection via `$LANG`/`$LC_MESSAGES` overridable by config/flag.

**Decision:**
- **Strings live in `locales/en.toml` / `locales/ru.toml`**, namespaced tables referenced by dotted key (`install.searching`, `error.unknown_source`). Files are **`include_str!`-embedded** at build time (single-binary constraint) and flattened to `dotted.key → value` at load.
- **`t!` macro** is the only call-site API: `t!("common.aborted")` and `t!("install.not_found", names = list)` (named `{placeholder}` interpolation). It expands to `i18n::tr`/`tr_args` — no formatting logic leaks to callers.
- **Language resolution (once, in `main` after config load):** `--lang` › config `[ui] locale` (unless `"auto"`) › `$LC_ALL`/`$LC_MESSAGES`/`$LANG` › English. Normalisation maps `ru_RU.UTF-8`→`ru`, `C`/`POSIX`/unshipped→`en`.
- **English is the source of truth and the fallback.** Lookup is active-lang → English → the raw key; a missing/renamed key never panics, it degrades. A **parity test** asserts `en` and `ru` have identical key sets, so no translation is silently missing.
- **Migration is incremental** (each area a commit) until zero hardcoded user-facing strings remain — the framework ships first (proven by migrating `common.aborted`, `install.searching`/`not_found`, `doctor.all_good` end-to-end), then CLI → errors → providers.

**Alternatives considered:**
- **A heavy i18n crate (`fluent`, `gettext`).** Rejected: `fluent` pulls a dependency tree and an ICU-ish syntax; JII's needs (two languages, `{name}` interpolation, TOML we already parse) are met by ~150 lines with no new heavy dep — matches the single-crate, minimal-surface constraints.
- **Runtime-loaded locale files from disk.** Rejected for the default: the binary must be self-contained (`include_str!`); a disk override dir is a possible future addition.
- **Enum of message variants instead of string keys.** Rejected: an exhaustive enum for ~200 messages is heavier to author/read than namespaced TOML, and gives no real safety over the parity test + fallback.

**Consequences:**
- New `src/i18n.rs` (+ `t!` macro, `#[macro_export]`), `locales/en.toml`/`ru.toml`, `--lang` global flag; `[ui] locale` config (already existed, default `"auto"`) now drives language.
- **Migration COMPLETE (2026-07-10):** zero user-facing string literals remain in Rust code (verified by sweep). The only English left in code is the low-level `#[error(...)]` Display prefixes (the technical cause line — thiserror derives them at compile time; the user-facing `remedy()` guidance *is* localized). `label()` methods (`TrustLevel`/`Health`/`Verification`) are kept as **stable JSON identifiers**; a parallel `display()` (calling `t!`) serves the human UI. The parity test guards the locale files; a lint/scan for stray literals is a possible later guard.
- 216 tests (incl. parity, normalise, interpolation, fallback). Verified: `--lang ru` and `LC_MESSAGES=ru_RU.UTF-8` both switch every string; English is the default.

## ADR-0051 — First-run setup runs before ANY command, then the command proceeds

**Status:** Accepted (2026-07-10).

**Context:** The onboarding wizard (ADR: U5/DW) only fired on a bare `jii`. A new user whose
first-ever invocation was a task — `jii fastfetch`, `jii search foo` — skipped onboarding
entirely (mode never chosen, doctor never offered). The owner wants the wizard to greet the
**first use of JII for anything**, then run the command the user actually typed.

**Decision:** Hoist the first-run check to the top of `Cli::run`, before command dispatch. When
`config.is_first_run() && interactive`, and the invocation is an *onboardable task*
(`onboarding_task_summary()` returns `Some`), JII: (1) tells the user up-front which command
will run after the optional setup, (2) runs the wizard, (3) reloads the saved config, (4)
rebuilds the renderer (the wizard may have changed the output mode), then (5) falls through to
the normal dispatch, which runs the original command. Excluded (return `None`, no pre-wizard):
`setup` (it *is* the wizard), `doctor` (runs its own setup — would double), `uninstall`, and the
hidden `completions`/`man`; bare `jii` keeps its dedicated welcome arm. Non-interactive / `--json`
/ piped first runs never trigger it (no TTY), so scripts are unaffected.

**Alternatives considered:** (a) keep onboarding bare-`jii`-only — rejected, misses the common
first-use path. (b) Run the command first, then offer setup — rejected: setup should shape how
the command behaves (mode, PATH), so it must come first.

**Consequences:** `run()` reloads config via `Config::load()` after the wizard (falls back to the
in-memory config if the reload fails). New pure helper `onboarding_task_summary()` (echoes
`jii <args>`); `renderer_for()` extracted so the renderer can be rebuilt. Verified under a pty:
`jii fastfetch` on a fresh `XDG_CONFIG_HOME` → first-use notice → wizard → "▶ Now running: jii
fastfetch" → the install runs. 216 tests green, clippy clean.

## ADR-0052 — Semantic colour palette + a mouse/keyboard chooser (crossterm)

**Status:** Accepted (2026-07-10).

**Context:** Owner asked for two "make it pretty / usable" polish items: colour in the human
output, and mouse control of the interactive chooser (click an option to pick it). The old
chooser used `dialoguer::Select` — arrow keys only, no mouse.

**Decision (colour):** A small `Copy` `Palette { enabled }` in `ui`, obtained from
`Renderer::palette()`, colours output **only** when the renderer's existing colour flag is on
(so `--no-color`/`NO_COLOR`/`--json`/no-TTY stay plain — one gate, already resolved in
`Renderer::new`). Every method returns the *plain* string when disabled, so callers never
branch on colour and column widths are unaffected. Semantic hues: source ids cyan, trust levels
official=green/community=yellow/untrusted=red, versions + secondary text dimmed, `✓`/`→`/`❯`
green, headings/table-header rows bold. **Alignment rule:** pad to width *before* colouring (ANSI
bytes must not count toward `{:8}`); free helpers like `candidate_line` take the `Palette`
explicitly rather than reaching for a global.

**Decision (chooser):** Replace `dialoguer` with a tiny inline `crossterm` menu in
`prompt::choose`. Raw mode + mouse capture; supports arrow keys (↑/↓, `j`/`k`, Home/End, Enter,
Esc/`q`, Ctrl-C) **and** the mouse (hover highlights, left-click a row picks it, scroll moves).
The terminal is **always** restored (mouse capture off, raw mode off, cursor shown) and the menu
region cleared, even on error — the fallible work runs in a closure whose result is handled after
an unconditional cleanup. The anchor row is measured (and its cursor-position report consumed)
**before** mouse capture is enabled, so the report can't race with mouse/key events; `n` lines
are reserved first so a menu near the bottom scrolls cleanly. Non-TTY/`--json` still returns the
default (unchanged consent semantics — picking is the consent; the untrusted trust barrier still
gates downstream, ADR-0006).

**Alternatives considered:** (a) keep dialoguer + add mouse — dialoguer/console expose no mouse
events, so not possible without a different backend. (b) A full-screen alternate-screen TUI —
rejected as heavier than warranted; the inline menu matches the old UX. (c) A process-global
colour flag for the free helpers — rejected; threading a `Copy` `Palette` is explicit and
test-friendly (`Palette::plain()`).

**Consequences:** New dep `crossterm` (0.29); `dialoguer` dropped (it was used only by `choose`).
`Palette` + `Renderer::palette()`/`heading()` added; `TrustLevel`/`Health` already had localized
`display()` (ADR-0050) which the palette colours. 216 tests green, clippy clean; verified under a
pty (menu renders in colour, keyboard nav selects the right source, terminal restored to cooked
mode afterwards) and piped (zero ANSI, default taken, no hang).

## ADR-0053 — By-name GitHub repo search: an interactive picker over a forge capability

**Status:** Accepted (2026-07-10).

**Context:** `jii <owner/repo>` installs a GitHub release, but a bare name a user only knows from
GitHub (`jii exteragram`) found nothing — the forge provider only answered explicit `owner/repo`.
Owner asked for free-text discovery: top matches, a "show more" that pages forever, and typo
tolerance. This is the long-deferred T5 GitHub repo chooser (ADR-0026/0030), now scoped.

**Decision:** Add by-name repo search as an **optional forge capability**, not a special case, so
it stays within ADR-0049 (GitHub is one forge among peers) and ADR-0022 (optional-method growth):
- `Forge::search_repos(client, query, per_page, page, token)` (default empty) — GitHub implements
  it via `/search/repositories` (relevance/"best-match" ranking, 1-based paging), normalising each
  item to a forge-neutral `model::RepoHit { source_id, slug, description, stars }` and dropping
  archived repos. `ForgeProvider::resolve`/`search`/`resolve_repo` are refactored to share the
  release→asset resolution, so a picked repo resolves exactly like an explicit `owner/repo`.
- `Provider` gains `search_repos`/`resolve_repo` (default empty) + `supports_repo_search()`
  (default false, forge = true). `Engine` gains `has_repo_search()`, `forge_repo_search(query,
  page)` (fan-out, concatenated in provider order — each forge keeps its own relevance), and
  `resolve_repo(source_id, slug)` (routes by id — dispatch, not a behavioural source-branch).
- **CLI hook:** in the install path, a **single, slash-free, unpinned** bare name that misses every
  normal source *in an interactive session with a forge available* opens `repo_picker`: it shows
  the top matches (`owner/repo — description  ★stars`) with a "↓ Show more" entry that fetches the
  next page and appends. Picking a repo resolves its latest release; if it has an installable Linux
  asset the candidate flows into the **normal** preview→confirm→install (untrusted, so still an
  explicit confirm, ADR-0006); if not, JII says so and re-shows the list. `owner/repo`, a pinned
  `:source`, any intent flag (`--source`/`--auto`/`--yes`/`--no`), a batch, `--json`, and non-TTY
  all skip the picker (unchanged behaviour). **Typo tolerance** — GitHub's own fuzzy matching handles
  the easy cases (`exteragram`); on top of it, when the verbatim term finds **nothing**, the picker
  retries with cheap edit-distance-1 variants (`cli::typo_variants`: single-char deletions first —
  the everyday extra-key slip — then adjacent transpositions; deduped, capped at 16) and adopts the
  first variant that hits, from then on paging *that* corrected term and telling the user
  (`install.gh_corrected`). So `exeteragram` → `exteragram` now recovers locally without a name index.

**Alternatives considered:** (a) fold GitHub repo hits into the normal ranked candidate list —
rejected: it would flood every search with untrusted repos and force a release lookup per repo on
the hot path. (b) Eagerly resolve each repo's release to *filter* the list to only-installable —
rejected for v1: 5 extra API calls per page (rate-limit heavy) and slower; lazy resolve-on-pick is
snappier and the "no Linux binary → pick another" message is clear. (c) A dedicated `jii gh <name>`
command — rejected: `jii <name>` "just works" is the goal.

**Consequences:** New `model::RepoHit`; forge/github/provider/engine gain the methods above;
`cli::repo_picker` + `repo_label`/`humanize_count`/`typo_variants` helpers. The crossterm menu now truncates each
item to the terminal width (`truncate_display`) so a long repo line can't wrap and desync the
per-row redraw/mouse mapping. 218 tests (github search-JSON parse, humanize_count); live-verified
under a pty (`jii exteragram` → GitHub picker with stars/descriptions; picking an APK-only repo
correctly reported "no installable Linux binary" and re-prompted) and piped (picker suppressed,
plain "not found"). **Debt:** rate limits bite harder here (GitHub search is 10/min unauthenticated)
— the setup token help (this session) mitigates it; cross-forge paging is concatenated, not merged.

---

## ADR-0054 — Cross-platform expansion: cheap imperative providers first, Void (XBPS) added; declarative Nix is snippet-first

**Status:** Accepted (2026-07-10). Void provider landed; the rest is a sequenced program.

**Context.** The owner decided to grow JII beyond Fedora-first toward a genuinely universal
installer: Nix (a *declarative* config path, not just the existing `nix profile`), then Gentoo
(emerge), then Void (XBPS), then possibly other distros and eventually Windows/macOS. This is a
multi-release program that touches CLAUDE.md's Fedora-first MVP constraint, so it is recorded here
rather than living in a chat. Two design questions had to be answered before writing code:
*(a) in what order*, and *(b) how the risky declarative-Nix path behaves*.

**Decision.**
1. **Sequence by risk, cheapest-first.** The declarative Nix config-edit is the single novel,
   high-risk item (it introduces a *new kind of action* — modifying a user's hand-written config —
   plus setup discovery). Gentoo/Void/Windows/macOS package managers are **imperative**, structurally
   identical to the existing apt/pacman/zypper providers ("just another `Provider`", the ADR-0022
   growth pattern), so they are cheap and safe. We therefore **prove the platform seam with a cheap
   imperative provider first**, then tackle declarative Nix. Between Void and Gentoo we start with
   **Void**: XBPS gives clean machine-readable output and maps directly onto the pacman model,
   whereas Gentoo's emerge drags in USE flags, source builds and the `world` file (a separate epic).
   **Windows/macOS is explicitly its own later epic**, not "another provider" — it breaks
   `privilege.rs` (no sudo/pkexec), path handling, packaging and CI, and must be scoped on its own.
2. **Void (XBPS) provider — landed.** `src/provider/void.rs`, id `void`, `TrustLevel::Official`
   (Void's official repos are RSA-signed), self-gates on the `xbps-install` binary (ADR-0029; no
   distro branch). Search uses `xbps-query -R <name>` — an **exact-name** property stanza (the
   analogue of `pacman -Si`), read via `run_capture_lax` (unknown package exits non-zero = "no
   candidate", not a source failure), and only emits a candidate when `pkgname` matches the query
   exactly (never installs a near-name). Plans: `xbps-install -Sy` (install, root), `xbps-remove -Ry`
   (remove + orphaned deps, root), `xbps-install -Suy [pkg]` (single/many/all update, root). Batching
   via `plan_install_many`/`plan_remove_many`/`plan_update_many` and bulk `plan_update_all` (bare
   `jii update`, D10). `list_installed` parses `xbps-query -l`. A pure `split_pkgver`
   (`name-version_revision` → name + display version, splitting on the final hyphen and dropping the
   `_revision`) backs both the stanza and list parsers. Reuses the shared `[reason]` keys with
   `mgr = "xbps"`; adds `reason.void_official`/`void_official_many`. Registered in
   `provider/mod.rs`, `KNOWN_SOURCES`, and the default priority (after zypper). **No core
   source-branch.** 9 unit tests; 228 total green, clippy clean. **Debt (T7):** unverified on a live
   Void host (same as apt/pacman/zypper/nix) — parsers are fixture-tested only.
3. **Declarative Nix is snippet-first (Etap A — LANDED), auto-edit deferred (Etap B).** The owner
   chose the **safe** design and it is now implemented. New optional `Provider::install_strategies(
   candidate) -> Vec<InstallStrategy>` (default empty; ADR-0022 growth) + model `InstallStrategy {
   label, hint, kind }` / `StrategyKind::{Imperative, Manual{guidance}}`. The engine exposes it as
   `install_strategies(source_id, candidate)` (dispatch, no source-branch); the CLI calls it **only
   for a single-package interactive install** and, if non-empty, shows a chooser. **Nix implements
   it:** it **detects which config files actually exist on this host** — NixOS
   `/etc/nixos/configuration.nix` → `environment.systemPackages` (apply `sudo nixos-rebuild switch`);
   standalone home-manager `~/.config/home-manager/home.nix` or `~/.config/nixpkgs/home.nix` →
   `home.packages` (apply `home-manager switch`) — and offers **only the ones present**, each with a
   one-line hint, alongside the default imperative `nix profile install`. **Crucially, when no config
   is detected it returns empty → no menu → plain imperative install as before** (a Nix-on-Fedora
   `nix profile` user is never nagged). A declarative pick is `Manual{guidance}`: the CLI **prints the
   exact snippet + the file + the apply command + a backup note and installs nothing** ("show, never
   run" — RPM Fusion / bootstrap `Script` precedent, ADR-0048). Detection (`detect_targets`, via an
   injectable existence predicate), the snippet builder and the guidance builder are pure and
   unit-tested; the interactive menu → print-guidance → install-nothing path was pty-verified with a
   stubbed `nix` + a temp home containing a `home.nix`. **Etap B** — actually editing the file via a
   real Nix parser (`rnix`) with **diff-preview → backup → confirm** — stays deferred; regex-editing
   `.nix` is ruled out (it will eventually corrupt a real config).

**Alternatives considered.** (a) *Start with declarative Nix* (owner's first instinct) — rejected:
begins the program with the hardest, riskiest, most novel piece. (b) *Start with Gentoo* (owner's
stated next-in-line) — deferred behind Void: emerge is materially more complex, a poor "prove the
seam cheaply" pick. (c) *Hardcode a fixed list of Nix config locations to ask about* — rejected: the
locations differ per user and most don't exist on a given host; offering a NixOS target to a
home-manager user produces a snippet that goes nowhere. JII must **detect** the real files and offer
only those. (d) *Auto-edit the config immediately* — rejected for now (Etap B): editing a
hand-written, git-tracked, module-split config from the first run is high-risk and needs a real
parser + diff + backup first.

**Consequences.** `void` is a first-class source everywhere (sources list, ranking, `--source void`,
batch, update-all). The cross-platform program is now on record with a risk-ordered plan: **Void
(done) → declarative-Nix Etap A (done) → Gentoo (done) → … → Windows/macOS (separate epic)**. The
pile of **live-host-unverified** system providers grows (apt/pacman/zypper/nix/void/gentoo) — the
owner running the existing ones on real hosts (T7) gains value before adding more. Fedora-first
remains the *default* posture; this ADR is the explicit, justified relaxation CLAUDE.md requires for
cross-distro work.

**Update (2026-07-10) — Gentoo (Portage/`emerge`) landed** as the next cheap imperative provider
(`src/provider/gentoo.rs`, id `gentoo`, Official). Portage is **atom-based** (`category/package`), so
`search` parses `emerge --search "^name$"` and emits one candidate per `category/name` (keeping the
atom in `raw`; a bare name in two categories yields two candidates); plans run `emerge --ask=n <atom>`
/ `--unmerge` / `--update` / `-uDN @world` (root) with `_many` batching; `list_installed` reads
`/var/db/pkg/<category>/<PF>` directly (no gentoolkit/portage-utils dependency); pure, revision-aware
`split_pf` derives name+version. Builds **from source** (slow — inherent to Gentoo, surfaced not
hidden). No core source-branch; new `gentoo_official`/`_many` reasons. 243 tests; fixture-tested only
(T7). **Windows/macOS remains the one non-trivial epic** (privilege/paths/packaging/CI).

---

## ADR-0055 — Recommend prerequisites: doctor enables a required repo before dependent suggestions

**Status:** Accepted (2026-07-10). Landed.

**Context.** A user's friend ran the released `jii` on a fresh Fedora, opened `jii doctor`, **skipped
the RPM Fusion suggestion**, then accepted "Multimedia codecs" and "VLC" — which live in RPM Fusion —
and got a bare `✗ Не найдено` (not found) for the codecs and an apparent hang on VLC. Root cause:
codecs/VLC depend on the RPM Fusion repo, but the catalog modelled that only as a prose `note`
("Needs RPM Fusion enabled first"); nothing enabled the repo or ordered it before its dependents, so
skipping RPM Fusion silently broke everything downstream. The owner's decision: **doctor should enable
the required third-party repos itself (with consent), before the things that need them** — "fix all
such cases where possible." (The interactive `doctor` questionnaire already *runs* a `manual`
repo-enable command via `run_shell_command`/`sh -c` on "yes" — superseding the stale ADR-0035 "shown,
never run" note — so the missing piece was the **dependency link + ordering**, not execution.)

**Decision.** Model the dependency **in the catalog data**, not in code. `Recommendation` gains
`requires: Option<String>` (the `id` of a prerequisite entry) and re-reads its `id` (previously
unread). `data/recommend/catalog.toml`: the codecs and VLC entries now declare `requires = "rpmfusion"`.
A new **pure** `recommend::prerequisite(chosen, all, installed, enabled) -> Option<&Recommendation>`
returns the prerequisite that must be enabled first — or `None` when there's none, it's already present
on the system (`is_satisfied`), or it was already enabled earlier this run (dedupe). In
`doctor_offer`, when the user accepts a suggestion, JII enables its prerequisite first: it prints the
prerequisite's title + trust `note` and runs its `manual` command through the existing
`apply_suggestion` (which **shows the exact command before running it**, honours `--dry-run`, and
carries the parent "yes" as consent). The full distro-filtered catalog is kept alongside the
offered-subset so a prerequisite that was *already satisfied* (and thus filtered out of the offered
list) is still found for the lookup. The core never branches on the source or distro — the dependency
is declared in data and resolved by a pure function.

**Alternatives considered.** (a) *Leave it as a prose note* — rejected: that is exactly what failed
the user. (b) *Auto-enable RPM Fusion unconditionally at startup* — rejected: a third-party repo is a
trust boundary; it must be tied to a consented action and shown. (c) *Ask a second, separate y/n for
the prerequisite* — rejected as needless friction: accepting codecs *is* accepting "set up codecs,"
which requires the repo; the command is still shown before it runs. (d) *Hard-code the codecs→RPM
Fusion link in Rust* — rejected: violates the data-driven catalog principle (ADR-0033) and the
no-distro-branch rule (ADR-0029).

**Consequences.** `doctor` now enables RPM Fusion before installing codecs/VLC when the user accepts
them, so the reported failure can't recur via that path. Pure `prerequisite` + the catalog wiring are
unit-tested (fires only when needed; dedupe; already-satisfied skip; dangling-`requires` safe); the
read-only `doctor` render was verified intact. **Known limitations / follow-ups:** (1) the direct
`jii <pkg>` install path (outside doctor) does **not** yet resolve prerequisites — a `jii vlc` on a
box without RPM Fusion still relies on another source (Flatpak) or misses; wiring prerequisites into
the general install path is future work. (2) `gstreamer1-plugin-openh264` lives in the Cisco
OpenH264 repo (usually enabled by default on Fedora Workstation), which RPM Fusion does not provide;
on a spin where it's disabled that one package can still miss. (3) The friend's VLC "hang" was not
reproduced (likely the no-RPM-Fusion miss falling back to a slow source search); enabling RPM Fusion
via doctor makes `dnf` resolve VLC directly — if a hang persists on a clean `jii vlc`, diagnose
separately.

## ADR-0056 — Declarative Nix Etap B: parser-driven auto-edit of a user-owned config (diff → backup → write)

**Status:** Accepted (2026-07-10). Landed.

**Context.** ADR-0054 shipped declarative Nix "Etap A" — when JII detects a Nix config the user
maintains (`/etc/nixos/configuration.nix` or a home-manager `home.nix`), the install menu offers a
"show me the snippet" path that **only prints** the block to add plus the apply command, never
touching the file. The owner's next step (chosen via the direction prompt: *"Nix этап B (авто-правка
конфига)"*) is to actually **make the edit** for the user: splice the package into the existing list,
show a diff, and write it — the first time JII modifies a user's hand-written configuration, which is
why it gets its own ADR.

**Decision.**
1. **Scope v1 = user-owned files only.** Auto-edit is offered **only** for a home-manager `home.nix`
   (under `$HOME`, `home.packages`). The root-owned NixOS `configuration.nix` stays Etap A
   (show-snippet): editing it needs privilege, and JII "is never fully run as root" (CLAUDE.md) — a
   privileged config rewrite is a separate, larger decision. So Etap B needs **no escalation**: it
   writes exactly one file the current user owns.
2. **New action shape, not a new privileged step.** `StrategyKind` gains
   `EditFile { path, new_content, diff, apply }` alongside `Imperative`/`Manual` (ADR-0054). The
   **provider precomputes everything** — it reads the file, produces the rewritten content and a
   rendered diff — so the core stays source-agnostic: the CLI just shows the diff, confirms, backs up,
   and writes, exactly as it already shows `Manual` guidance. No `if source == "nix"` anywhere.
3. **Edit via the concrete syntax tree, splice the original text.** `insert_package(source, attr, pkg)`
   parses with **`rnix` 0.14** (its lossless rowan CST), locates the `home.packages` list (unwrapping a
   leading `with pkgs;`), and splices `pkg` into the **original source bytes** at the right offset —
   it never re-prints the tree (which would reformat the whole file). It mirrors the existing style
   (multi-line list → new indented line after the last item, past any trailing comment; inline
   `[ a b ]` → space-separated; empty `[]` → `[ pkg ]`), detects an already-present package, and
   returns `NotFound` for anything it can't confidently edit (attribute absent, value isn't a plain
   list, or the file doesn't parse) so the caller **falls back to the Etap A snippet**. Comments and
   formatting elsewhere are preserved byte-for-byte.
4. **Safety rails on write.** The CLI shows the diff, asks a y/n (honouring `--yes/--no/--auto` and,
   because the strategy menu is already gated off under `--dry-run`, a dry run never writes), backs the
   file up to `<path>.jii-bak` **before** overwriting (so a failed write always leaves a recoverable
   copy), then prints the apply command (`home-manager switch`) for the user to run.

**Alternatives considered.** (a) *Hand-roll a Nix text editor (regex/string scan)* — rejected: doing
it correctly means re-implementing a comment- and string-aware Nix lexer, strictly more code and more
bug surface than leaning on the canonical parser; `rnix` is the smaller, safer dependency. (b)
*Pretty-print the edited AST back out* — rejected: it reflows the user's entire file; splicing into the
original text preserves their formatting exactly. (c) *Also auto-edit `configuration.nix` via
`sudo`/`pkexec`* — deferred: privileged config rewrites are a separate trust/UX decision; v1 stays
user-space. (d) *Auto-run `home-manager switch` after writing* — rejected: JII installs, it doesn't
activate a user's whole generation behind their back; we show the command instead.

**Consequences.** A home-manager user picking "Add to …/home.nix" now gets the package actually added,
with a previewed diff and a `.jii-bak` backup, then a one-line apply hint — while a plain `nix profile`
user still sees no menu, and the NixOS system config still only shows a snippet. `insert_package`,
`find_list`, `line_diff` and the backup/write helper are unit-tested across multi-line, inline, empty,
no-`with-pkgs`, comment-preserving, already-present, not-found and unparseable inputs. New runtime
dependency: `rnix` 0.14 (pulls `rowan`/`text-size`/`countme`). **Known limitations / follow-ups:**
(1) only the flat `home.packages = [ … ]` form is recognised; a nested `home = { packages = … }` or a
package list built by a function (`lib.optionals …`) falls back to the snippet. (2) The full
menu→edit→apply flow is unit-tested at the seams but not yet exercised on a live home-manager host
(T7 debt, shared with the Void/Gentoo providers). (3) `configuration.nix` auto-edit and wiring the
edit into non-interactive/batch installs remain future work.

---

## ADR-0057 — Declarative install preference: `prefer_declarative` config + per-run flags, and batch/scripted routing

**Status:** Accepted (2026-07-11). Landed.

**Context.** ADR-0054/0056 shipped the declarative Nix strategy, but its chooser was reachable **only
for a single-package interactive install** (`single && !--auto && !--yes && !--no && !--dry-run &&
tty`). Two real gaps followed from that gate: (a) a home-manager user who lives in their config still
got a silent imperative `nix profile install` whenever they installed **several** packages
(`jii install a b c`) or **scripted** one (`--yes`) — there was no way to reach the config edit
outside the interactive single case; (b) the strategy list is ordered *imperative-first*
(`nix.rs`: index 0 = `nix profile install`), so any "default without a menu" is imperative — meaning
the declarative path had no non-interactive entry point at all. This ADR closes ADR-0056 follow-up (3),
the "wire the edit into non-interactive/batch installs" half.

**Decision.**
1. **A standing preference in config, source-agnostic.** New `[install] prefer_declarative =
   ask | always | never` (`config::DeclarativePref`, default `ask`). It records *whether* to prefer a
   declarative strategy, never *which* — the CLI still acts only on whatever `install_strategies`
   returns (empty for every source but Nix-with-a-config), so there is **no core source-branch**. It
   lives in `[install]` (not a `[nix]` section) precisely because it is a general
   declarative-vs-imperative stance that any future declarative source inherits for free.
2. **Per-run override flags.** `--nix-config` forces `always` and `--nix-imperative` forces `never`
   for one invocation (mutually exclusive via clap `conflicts_with`); with neither, the config
   decides (`Cli::declarative_pref`). Flags beat config — the standard precedence.
3. **Behaviour per preference.** `ask` = the historical single-package interactive menu, unchanged; a
   **batch stays imperative** under `ask` to avoid a prompt-storm. `never` = always imperative (the
   historical fall-through). `always` = route **each** resolved candidate that offers an auto-editable
   `EditFile` into that edit — single, batch, *or* scripted — via the shared `apply_edit_file`
   (diff → confirm honouring `--yes/--no/--auto`/`default_yes` → `.jii-bak` backup → write; `--dry-run`
   shows the diff and writes nothing). A candidate that only exposes a root-owned `Manual` snippet
   (NixOS `configuration.nix`) prints the snippet and is likewise treated as handled; a candidate with
   no declarative strategy (any non-Nix source, or Nix with no detected config) falls through to the
   normal imperative batch. Handled packages simply leave the `chosen` list, so the imperative
   preview/confirm/install below covers exactly the remainder.

**Alternatives considered.** (a) *Flag only, no config* — rejected: a home-manager user's declarative
stance is a **standing** preference, not something to retype every install; a per-run flag alone forces
the friction they were trying to avoid. (b) *Config only, no flag* — rejected: a committed
declarative user still occasionally wants a one-off imperative install (or vice-versa) without editing
config. The owner chose **both** (config for the default, flags to override). (c) *Also fire the
chooser per-package in an interactive batch under `ask`* — rejected: N packages → N menus is the
prompt-storm ADR-0025/T5 deliberately avoided; a batch user who wants declarative sets `always` or
passes `--nix-config`. (d) *Source-named config key `[nix] declarative`* — rejected: it would bake a
source name into the config surface and read wrong the moment a second declarative source appears; the
source-agnostic `[install] prefer_declarative` avoids that (the CLI flags stay `--nix-*` for now only
because Nix is the sole strategy source — aliases can be added when a second one lands).

**Consequences.** `jii install firefox vlc` on a home-manager host with `prefer_declarative = always`
now adds **both** to `home.nix` (each with its own diff + backup), then installs any non-Nix packages
imperatively in the same run; `--nix-config` gives the same routing one-off, `--nix-imperative` opts
back out. `ask` and `never` behave exactly as before. `Cli::declarative_pref` (flag-vs-config
resolution) and `apply_edit_file`'s dry-run no-write guarantee are unit-tested; the live
batch→edit→apply flow still needs a home-manager host (same T7 debt as ADR-0056). Still open from
ADR-0056: `configuration.nix` auto-edit (privileged rewrite).

## ADR-0058 — Declarative Nix Etap C: privileged auto-edit of the root-owned `configuration.nix`

**Status:** Accepted (2026-07-11). Landed.

**Context.** ADR-0056 (Etap B) auto-edits a *user-owned* home-manager `home.nix` (parse → splice →
diff → `.jii-bak` → write), but deliberately left the root-owned NixOS system config
(`/etc/nixos/configuration.nix`) at Etap A: JII only **showed** a snippet (`Manual`) and changed
nothing, because writing it needs root and "JII is never fully run as root". That was the last
declarative gap: a NixOS user (as opposed to a standalone home-manager user) got no auto-edit at all.
The binding constraints already provide the mechanism — *only concrete steps escalate, via sudo/pkexec,
exact commands shown first, and escalation lives in `privilege.rs`* — so the remaining work is to route
the existing splice through that path rather than block on ownership.

**Decision.**
1. **`EditFile` grows a `needs_root: bool`.** `strategy_for_target` (nix.rs) no longer gates auto-edit
   on `t.home`: *any* config it can read and parse becomes an `EditFile`; the system target simply
   carries `needs_root: !t.home`. An unreadable/unparseable file still falls back to `Manual` (Etap A
   snippet). The provider still only **plans** — it neither escalates nor writes.
2. **The CLI branches on the flag, not on the source.** `apply_edit_file` writes a `needs_root == false`
   file directly (`write_nix_config`, unchanged); for `needs_root == true` it goes through
   `write_nix_config_root`: stage `new_content` in a user-owned temp file (`O_EXCL`, so a pre-planted
   path/symlink can't be clobbered), then run two **explicit** elevated commands via `privilege.rs` —
   `cp -a -- <dest> <dest>.jii-bak` (backup) then `cp -- <tmp> <dest>` (write). `Privilege::prime` runs
   once so the pair prompts at most once. The exact `sudo`/`pkexec` argv is **printed before** anything
   runs; `--dry-run` prints them and writes/stages nothing. The core stays source-agnostic — it acts on
   `needs_root`, never on `source == "nix"`.
3. **Backup symmetry.** The root file is backed up to the same `<path>.jii-bak` sibling as the
   home-manager case (owned by root, created by the elevated `cp -a`), so recovery is identical.

**Alternatives considered.** (a) *Pipe `new_content` to `sudo tee <dest>` in one command* — rejected:
`Privilege::run` inherits stdio (no stdin plumbing), and it would also skip the backup; adding a stdin
path to `privilege.rs` is more surface than a staged temp + two `cp`s. (b) *`sudo install -m … <tmp>
<dest>`* — rejected: a plain `cp` onto the existing file preserves the destination's owner/mode
already, and two obvious `cp`s read more clearly in the "commands shown first" prompt than `install`
flags. (c) *A dedicated `EditFileRoot` variant instead of a bool* — rejected: it duplicates four
identical fields and forces two match arms everywhere for a single behavioural bit; a `needs_root` flag
is the minimal honest signal. (d) *Keep it at Etap A (snippet only)* — rejected: it was the one
remaining declarative gap and the escalation machinery already exists; showing a snippet when JII can
safely splice-and-apply is needless friction for NixOS users.

**Consequences.** On a NixOS host, `jii install ripgrep` with `prefer_declarative = always` (or
`--nix-config`) now splices `ripgrep` into `environment.systemPackages`, shows the diff **and** the two
`sudo cp` commands, backs the file up, writes it, then reminds the user to `sudo nixos-rebuild switch`.
`--dry-run` shows all of that and touches nothing. `home.nix` behaviour is unchanged. Unit tests cover
the `needs_root` classification, the dry-run no-write/no-stage guarantee for the root path, and the
exact elevated argv; the live escalated write still needs a real NixOS host to exercise (extends the
ADR-0056/0057 T7 verification debt). This closes the last open item from ADR-0056.

## ADR-0059 — `install.sh` native-package install (opt-in via `auto`), portable stays the safe default

**Status:** Accepted (2026-07-11). Landed.

**Context.** `install.sh` (the `curl … | sh` one-liner) only ever downloaded the static-musl tarball
and dropped it in `~/.local/bin` — a rootless, universal install. A user testing on CachyOS objected
that this is "not a real install": no system integration, no man page/completions on the system path,
not removable via the package manager. JII already **builds and publishes** native `.rpm`/`.deb` on
every release (nfpm) and has an AUR `PKGBUILD`, so the packages exist; the installer just never used
them. The tension: a `curl | sh` pipe that silently runs `sudo` contradicts JII's own binding
principle — *"JII is never fully run as root; only concrete steps escalate, exact command shown first."*

**Decision.** `install.sh` gains a `JII_METHOD` selector (`auto` | `native` | `portable`; also
`--native` / `--portable` args), defaulting to **`auto`**:

1. **`auto`** — detect the native manager (`dnf`/`apt`/`zypper` → the matching `.rpm`/`.deb`). If one is
   present **and** escalation is available (`root` or `sudo`) **and** a controlling terminal exists
   (`/dev/tty` readable), *ask* the user (default **yes = native**) whether to install system-wide via
   that manager or portably to `~/.local/bin`. **No TTY (a real pipe / CI) → portable, no prompt, no
   sudo.** This mirrors the app's philosophy: native is offered up-front, but privilege escalation
   never happens without an explicit answer.
2. **`native`** — force the native package. The exact privileged command is printed first
   (`sudo dnf install -y …` etc.); `sudo` itself gates on the password. Falls back to portable (with a
   note) when there is no supported manager / no escalation / no native asset — it warns, never hard-fails.
3. **`portable`** — the original behaviour verbatim (unchanged), so existing users and CI see no change.

Native assets are discovered from the GitHub **release-by-tag** JSON (grep the `browser_download_url`
matching the arch + extension) rather than by reconstructing the nfpm filename — robust to release-number
and naming quirks. The downloaded package's `.sha256` (also on the release) is verified before install,
same as the tarball path. The manager's own command is built as an argv and printed verbatim before it
runs. **Arch/`pacman` is deliberately not wired to a privileged install:** its native path is the AUR
(`jii-bin`), which isn't published yet — for now `pacman` hosts get a note pointing at the AUR and a
portable fallback. Wiring `yay -S jii-bin` is a one-line follow-up once the AUR package is live.

**Alternatives rejected.** (a) *Default to native (`sudo` in the pipe).* Rejected — surprising root in a
`curl | sh` context is exactly the "no surprise escalation" line JII draws for itself; worse UX than a
clean rootless install. (b) *Keep portable-only, publish native repos only.* Rejected — the user wants
the one-liner itself to be able to do a real install; opt-in `auto` delivers that without the surprise.
(c) *Reconstruct the nfpm filename.* Rejected as brittle (release number `-1`, `~`/`-`/`.` version quirks);
API discovery is stable. (d) *`sudo tee`/`dpkg -i` improvised flows.* Rejected — use each manager's own
`install` verb so the package is tracked and removable natively.

**Consequences.** `curl … | sh` on Fedora/Debian/openSUSE now offers a tracked system install (removable
via the manager, with man page + completions) while staying rootless-by-default in pipes/CI. Portable is
untouched. The live native-install path (actual `sudo dnf/apt/zypper install`) is unverified on real
hosts — it extends the T7 live-verification debt, and Arch native waits on the AUR publish.

---

## ADR-0060 — "JII everywhere": prebuilt-binary recipes for more channels + crates.io metadata

**Status:** Accepted (2026-07-12). Recipes landed in-tree; publishing is owner action.

**Context.** Owner's stated goal: JII installable from *everywhere* — every package manager, every
COPR chroot, server distros (CentOS/Alma), "almost any distro". The multi-arch `jii.spec` (ADR just
prior, commit `1859bd9`) already unlocked every Fedora/EPEL/openSUSE COPR+OBS chroot in both arches.
What remained were the **non-RPM/deb channels**: Alpine, Void, Gentoo, Nix, Homebrew, and crates.io —
each has users who install *only* through their native tool. All of these can consume the existing
static-musl release tarball with **no compile** (crates.io compiles from source), so no new build
infrastructure is needed to reach them; only per-ecosystem packaging recipes.

**Decision.** Add prebuilt-binary recipes, one per channel, under `packaging/`, each repacking the
release tarball (same binary + man page + completions + LICENSE the proven `.rpm`/AUR paths ship):

- `packaging/homebrew/jii.rb` — Homebrew formula (**Linux/Linuxbrew**; macOS deferred, needs a native
  mac build), `on_intel`/`on_arm`, real sha256s.
- `packaging/alpine/APKBUILD` — Alpine aport. Musl-native → the static binary is a perfect fit for
  servers/containers. `sha512sums` left for `abuild checksum` (Alpine uses sha512, which this Fedora
  host can't pre-generate without downloading).
- `packaging/void/template` — Void `srcpkgs/jii`, real per-arch sha256 baked in (Void uses sha256).
- `packaging/gentoo/jii-bin-0.1.5_beta.ebuild` — Gentoo overlay `app-admin/jii-bin`, `RESTRICT=strip`,
  `Manifest` generated by the maintainer at publish time.
- `packaging/nix/jii.nix` — prebuilt derivation, SRI hashes baked in, `dontPatchELF`/`dontStrip` (a
  static musl binary needs no loader patching).
- **crates.io**: `Cargo.toml` gains `repository`/`homepage`/`readme`/`keywords`/`categories`/
  `rust-version`; `cargo publish --dry-run` packages + compiles cleanly. `cargo install jii` then works
  on **any OS with Rust** — the widest-reach channel, source-built, no per-distro maintainer.

**Alternatives rejected.** (a) *Wait and build native repos only via COPR/OBS.* Rejected — that never
reaches Alpine/Void/Gentoo/Nix/Homebrew/crates.io users, who don't consume RPM/deb. (b) *Homebrew/Nix
build-from-source formulae.* Deferred — a source build pulls the whole Rust toolchain and, for macOS,
JII isn't validated there yet (it drives Linux managers); the prebuilt-binary recipe is honest about
being Linux-first. (c) *Add exotic CPU arches now (ppc64le/s390x/i686).* Still deferred to the CI
cross-compile epic — every recipe here is x86_64+aarch64, matching the only binaries the release builds.

**Consequences.** JII now has a ready recipe for essentially every mainstream Linux packaging channel
plus crates.io, all from one static binary. Each still needs the **owner's account** to publish and
**one real build on the target distro** before going live (this dev host is Fedora-only, so the recipes
ship validated-by-construction, not build-tested on Alpine/Void/Gentoo/Nix/brew). Per-release upkeep
grows: bump the version + refresh checksums in each recipe (noted in `packaging/README.md`).

## ADR-0061 — GitHub strictly last + bootstrap an uninstalled source before it (part B design)

**Status:** Part A **Accepted & landed** (2026-07-12). Part B **Accepted**; **stages 1-2 landed**
(2026-07-12): `can_search` for cargo/npm/pipx/go + Flatpak (Flathub v2 API), uninstalled-source
search, and the bootstrap-before-install prompt — verified `jii obsidian`→Flatpak end-to-end. Stage 3
(Snap/brew network search) remains.

**Context.** Owner directive from cross-system testing: "search GitHub *really* last." Concretely
(owner's example): `jii obsidian` on a box where Flatpak isn't installed should **offer to install
Flatpak and get obsidian there**, rather than falling to a raw GitHub Releases binary — "and this
should work with everything, any package and any source."

**Part A (done).** `github` moved to the end of the default source `priority` (below cargo/npm/pipx/
go/brew/nix). Ranking already keys on source priority after the name-match tier, so among equally-good
name matches github now sorts last. This fixes the case where the other source **is installed**.

**Part B (the hard case): the preferred source's CLI isn't installed.** Today `Engine::search_one`
gates every provider on `is_available()` (= `which(cli)`), so an uninstalled source contributes
nothing and github can win by default. Key finding while scoping this: **search and install have
different needs.** cargo/npm/pipx/go already *search over the network* (crates.io / registry.npmjs.org
/ pypi.org APIs) and only need the CLI to *install*; flatpak/snap/brew search **through their CLI**, so
for the owner's own example (Flatpak) a network search means talking to the **Flathub API** directly.

**Proposed decision.**
1. Split provider capability into **`can_search`** (often network-only) vs **`is_available`** (CLI
   present, needed to install). `search_one` includes a source when it can search, even if its CLI is
   absent — tagging any resulting candidate `needs_bootstrap = true` (the manager must be installed
   first). github stays last by priority, so a real package source outranks it whenever one matches.
2. On installing a `needs_bootstrap` candidate, the plan **prepends the manager bootstrap** (reuse the
   existing T6 `bootstrap_ecosystem` / `Bootstrap` metadata): e.g. "install Flatpak, then obsidian via
   Flatpak" — one preview, one confirmation, exact commands shown, escalation batched as always.
3. Add **network search to the CLI-only sources** that have a public API: **Flatpak → Flathub API**
   (`flathub.org/api`), Snap → snapcraft API, Homebrew → formulae.brew.sh JSON. Each behind `can_search`
   so an uninstalled manager can still answer "do I have this?".

**Alternatives rejected.** (a) *Blindly offer to install a manager without knowing the package is
there.* Rejected — installing Flatpak on a maybe is a bad surprise; we must confirm the package exists
first (hence real network search). (b) *Only reorder priority (part A) and stop.* Insufficient — it
doesn't cover the owner's example where the better source isn't installed at all. (c) *Bootstrap then
re-run the normal search.* Simpler but wastes an install when the package isn't in that source; the
`can_search`-first approach only bootstraps once we know it's worth it.

**UX forks — resolved (owner, 2026-07-12):** (i) **Prompt to confirm**, always: when the best match is
a `needs_bootstrap` candidate, show "Found in Flatpak (not installed). Install Flatpak and get obsidian
there? [Y/n]" — the user stays in control; `--auto`/`--yes` still auto-confirm within the trust barrier.
(ii) **Incremental, implementer's call:** land the already-network sources first (cargo/npm/pipx/go —
cheap, no new search code), then add Flatpak (Flathub API) so the owner's obsidian example works, then
Snap/Homebrew. (iii) trust: a `needs_bootstrap` Flatpak app is still `community` — the normal trust
barrier (ADR-0006) applies unchanged.

**Consequences.** Delivers the owner's "github truly last, bootstrap the right source instead" vision
generally, not just for Flatpak. Cost: a `Provider` API change (`can_search`), a `PackageCandidate`
flag, new Flathub/Snap/brew search paths, and install-flow wiring — a multi-file change, so it lands as
its own focused pass rather than bolted onto the batch-1 fixes.

## ADR-0062 — AUR provider (Arch-only) + merge `jii providers` into `jii sources` with add/remove

**Status:** Accepted (2026-07-12).

**Context.** Two owner requests from the cross-system testing round: (1) `jii yay`/`jii paru`
should work, **only on Arch-family** systems, backed by a real AUR source; (2) the ecosystem
managers (`jii providers`) and the source list (`jii sources`) are two views of the same thing —
merge them, and let a user **disable** a manager (JII stops seeing it) or **remove** it from the OS
— but **never** remove a *system* package manager (that would break the OS).

**Decision.**

*AUR provider* (`provider/aur.rs`, id `aur`, Community). Searches the AUR RPC v5
(`aur.archlinux.org/rpc`) and installs/removes/updates via an AUR helper (`paru`/`yay`), with
`needs_root = false` — a helper must never run as root; it escalates to `pacman` itself (the
Flatpak-polkit precedent). **Every entry point self-gates on Arch:** new `Platform::arch_like`
(parsed from `/etc/os-release` `ID`/`ID_LIKE`, derivative-proof via the `arch` token) AND a helper
present. `search()` returns empty off-Arch; `ecosystem()` returns `None` off-Arch, so AUR never
shows in `jii sources` on Fedora/Debian/etc. Deliberately **not** `can_search` (unlike the language
registries): without a helper there's nothing to install with, and AUR hits are meaningless off-Arch.
`list_installed` = `pacman -Qm` (foreign packages). Ranked just below Flatpak/Snap, above the
language registries and github.

*Merge.* `jii providers` is now a hidden alias; `jii sources` is the single view. It annotates each
ecosystem manager inline with `[add: …]` (when missing) or `[remove: …]` (when installed); system
repos get no such hint. New subcommands: `jii sources add <id>` (bootstrap — the old `providers add`,
plus `yay`/`paru` showing the manual `makepkg` install, shown-never-run) and
`jii sources remove <id>`.

*Removal.* Reuses each ecosystem's existing `Bootstrap::Packages` as the OS package(s) to uninstall
(no new metadata): the host system manager is detected (`SysManager`: dnf/apt/pacman/zypper/xbps/
portage), removal is narrowed to the package(s) actually installed (per-manager `pkg_installed`
probe, so we never guess-remove a wrong name — go is `golang`/`go`/`golang-go` across distros), the
**exact elevated command is shown first**, confirmation defaults to **no**, and it runs through
`privilege.rs`. A `Bootstrap::Script` manager (Homebrew/Nix) can't be auto-removed → its own
uninstaller is pointed to. A **system** package manager id is refused outright. AUR helpers (yay/paru)
are removed via `pacman -Rs`.

**Alternatives rejected.** (a) *Add a `Removal` field to `Ecosystem`* — unnecessary; the bootstrap
package list already names the OS package. (b) *Guess-remove the first candidate name* — wrong across
distros; the installed-probe is safer. (c) *Two tiny yay/paru providers* — boilerplate; one Arch-gated
AUR provider + name aliases is enough. (d) *Let `jii remove` handle managers* — it operates on JII's
registry (things JII installed); the manager wasn't, so a dedicated path is clearer and safer.

**Consequences.** One `jii sources` view for everything; Arch users get real AUR + `jii yay`/`jii paru`;
removal is safe-by-construction (system managers refused, exact command shown, default-no, installed-only
targets). No core source-branch: gating lives in the AUR provider and in capability checks. `Platform`
gains a durable `arch_like` family predicate (the first real consumer, as ADR-0029 anticipated).

## ADR-0063 — Whole-system update: capture per-source output into a summary + parallel self-check

**Status:** Accepted (2026-07-12).

**Context.** Owner ran `jii update` and the bulk managers **flooded the terminal**: npm dumped every
deprecation warning + a `changed 984 packages` line, flatpak printed a wall of end-of-life notices.
The result buries the one thing that matters — *what actually changed, per source*. Owner asked for a
compact per-source summary (`npm ✓ 984 packages updated`, EOL runtimes collapsed to a note) and, since
JII knows its own version up front, to run the "newer JII?" GitHub check **in parallel** with the
system update so it feels instant.

**Decision.** The whole-system update (bare `jii update`) now **captures** each bulk plan's output
instead of streaming it (`Privilege::run_captured`, `Engine::run_plan_captured`) and renders one line
per source: `  <source>  ✓ <headline>` plus indented notes. The headline/notes come from
`exec::summarize_update(&output)` — a **source-agnostic** heuristic scanning universal textual signals
(`nothing to do`/`up to date`/`0 upgraded` → "nothing to update"; npm `changed N packages` / apt `N
upgraded` → "N packages updated"; count `deprecated` lines; count `end-of-life` lines) so there is **no
branch on the source id**. On failure the source is marked `✗` and a short tail of the captured output
is shown so errors aren't swallowed. The per-record *fallback* updates (github/cargo/…) still stream —
they're small and already quiet. Separately, bare `jii update` spawns `selfupdate::latest_release()`
as a task before the system update and awaits it in `self_update`, so the self-check is near-instant.

**Alternatives rejected.** (a) *Keep streaming* — the flood is the whole complaint. (b) *Per-provider
`summarize` trait method* — cleaner in theory but 10+ impls for a display heuristic; the universal-signal
scanner covers dnf/apt/flatpak/npm/… with one tested function and no core coupling. (c) *A spinner with
live tail* — more machinery; the owner explicitly wants *less* output, and each source prints its result
as it finishes, which is enough progress feedback.

**Consequences.** `jii update` output is now a scannable per-source ledger instead of a wall of manager
noise; the self-update check no longer waits on the system update. Trade-off: no live progress *within* a
long single source (e.g. a big dnf download) — acceptable given the goal is less noise, and the result
line lands as soon as that source finishes. `run_captured` requires priming first (done by `prime_for`),
so `sudo` never prompts with stdin captured.

## ADR-0064 — `jii doctor` shows only host-relevant sources + refresh metadata after enabling a repo

**Status:** Accepted (2026-07-13).

**Context.** Two owner-reported bugs on Fedora. (1) `jii doctor` listed **every** source, including
other distros' native package managers — `apt`, `pacman`, `aur`, `zypper`, `void`, `gentoo` — all
`offline`. A Fedora user must not have to reason about pacman (and an Arch user not about dnf); this
is the same principle `jii sources` already honours, where the `SourceEntry.relevant` predicate hides
a foreign native manager unless `--all`. But `Engine::diagnose` (which backs `doctor`) probed and
printed **all** enabled providers, ignoring relevance — so `doctor` and `sources` disagreed. (2) The
`doctor` codec setup enabled RPM Fusion and then immediately reported its packages
(`gstreamer1-plugins-ugly`, …) **"not found"**: the just-added repo had no local metadata yet, so the
dependent install queried a stale `dnf5 repoquery` cache and missed them.

**Decision.** (1) Factor the relevance rule out of `source_catalog` into a shared
`source_relevant(available, provider)` = `available || can_search() || ecosystem().is_some()`, and
apply it in `diagnose` too: a source that can neither run here nor be bootstrapped is **skipped**, so
`doctor` shows exactly what `jii sources` shows. Still no branch on a concrete source id — pure
capability (a foreign native manager is `available=false`, `can_search=false`, `ecosystem=None`; its
own distro flips `available`/`ecosystem`). (2) After the questionnaire enables a prerequisite **repo**
(RPM Fusion), call `refresh_repo_metadata` once before installing the dependent — a best-effort,
non-root `dnf5 makecache` guarded on dnf5 (a no-op off Fedora, the only distro with a repo
prerequisite today), skipped in dry-run. The following install then sees the new repo's packages.

**Alternatives rejected.** (a) *Gate provider registration by platform* (don't even construct apt on
Fedora) — larger blast radius (`from_config`/`Engine::new` would go async or hardcode binary names)
and it would also hide a source a user explicitly `--source`-pins; filtering at the *view* layer keeps
registration uniform and `--all`/pins working. (b) *Add `--refresh` to every dnf search* — slows the
hot path for one rare post-repo-enable case. (c) *Install curated catalog packages via a direct
`dnf install`* (bypassing JII's search) — reintroduces source branching in the doctor flow; a targeted
metadata refresh fixes the timing without special-casing the install path.

**Consequences.** `doctor` and `sources` now present one consistent host-relevant set; a Fedora box
never surfaces apt/pacman/zypper/void/gentoo/aur (and symmetrically for other families). The codec
setup succeeds on a fresh RPM Fusion enable. `refresh_repo_metadata` lives in the CLI doctor layer
(already distro-aware via the per-distro catalog), not the source-agnostic core.

## ADR-0065 — T6: bootstrap an uninstalled manager before its app instead of falling to GitHub

**Status:** Accepted (2026-07-13).

**Context.** Owner: "`jii obsidian` — if Obsidian is in Flatpak but Flatpak **isn't installed**, offer
to set up Flatpak and install it there, instead of searching GitHub. And this should work for *any*
source, not just Flatpak." Two halves already existed: GitHub is the strict last resort in the default
`priority`, and `can_search` sources (Flatpak, Snap, cargo, npm, pipx, go, brew) search **without**
their CLI, so an uninstalled-Flatpak hit already surfaces and outranks the GitHub binary. A first-cut
bootstrap loop (ADR-0061 part B) also existed — but it (a) never added Flatpak's Flathub remote, so
the app install still failed on a fresh Flatpak; (b) never checked the manager actually installed; and
(c) kept the app even for a `Script` manager (brew/nix) that JII refuses to auto-install, so the app
then failed anyway.

**Decision.** Replace that loop with `bootstrap_missing_managers` (`cli`), run on the chosen set before
planning. Per **distinct** manager (asked once, not once per app): a `Packages` manager (flatpak/snap/
cargo/npm/pipx/go) is offered for setup (default yes), installed via the normal `install_inner` path
(its own preview + privilege), then — for Flatpak — the Flathub user remote is added idempotently
(`remote-add --user --if-not-exists`, the one manager needing a post-install remote, localized like
doctor's Flathub fix); `Engine::source_available` confirms it landed before the app is kept. A `Script`
manager (brew/nix) is **shown, never run** (ADR-0005/0006), so its apps are skipped with a note. In
`--dry-run` both phases are previewed (set up the manager, then install the app) with nothing executed.
Candidates whose manager is already present, or that aren't ecosystem managers at all (github), pass
through untouched — no branch on a concrete source id except the single, well-marked Flatpak remote.

**Alternatives rejected.** (a) *One combined plan* (prepend the manager-install actions to the app's
`InstallPlan`) — a single confirm, but it splices two sources' plans (dnf-installs-flatpak + flatpak-
installs-app) into the batch model and complicates preview/privilege; two reused phases are lower-risk
and each keeps its correct elevation. (b) *A generic `post_bootstrap` provider hook for the remote* —
over-built for the one Flatpak case; revisit if a second manager needs it. (c) *Keep falling to GitHub* —
the explicit thing the owner rejected.

**Consequences.** On a host without Flatpak, `jii obsidian` now offers "set up Flatpak and install
Obsidian?" and does both, instead of installing a raw GitHub binary — and the same holds for Snap/cargo/
npm/pipx/go. brew/nix stay show-only. The dead `[bootstrap]` locale section was removed (superseded by
`install.bootstrap_*`). Verified via `--dry-run`: `httpie:pipx` previews pipx-via-dnf then httpie-via-
pipx; `wget:brew` shows the Homebrew script and skips. Live end-to-end on a manager-less host is T7.

---

## ADR-0066 — Owner testing round: bootstrap via a usable source, consent to a manager's own script, and progress you can see

**Status:** Accepted (2026-07-15).

**Context.** The owner tested v0.1.7-beta on Fedora and an apt host and reported ten things. Five were
design-affecting; the rest were bugs or polish fixed within the existing design.

**Decision.**

1. **A manager is bootstrapped only through a source that works right now.** ADR-0065 handed the
   manager's package to the normal install path with **no source pinned**, so on a pipx-less box
   `jii htop:pipx` opened a chooser headed *"install pipx via pipx"* (and npm via npm) — a
   `can_search` source answers over the network without its CLI, so an absent manager cheerfully
   offered to install itself. You cannot set a manager up with a manager that isn't there.
   `Engine::first_available_package` becomes `first_bootstrap_package`, resolving the package **and**
   its source, considering only candidates whose source is `is_available()`, and pins it (`pipx:dnf`)
   via the ADR-0031 spec grammar. No chooser (the user picked the *app*, not its plumbing), no
   self-bootstrap, no core source-branch.

2. **brew/nix: their own upstream script is offered, not refused.** ADR-0005/0006 made it
   shown-never-run; the owner's reply while testing was "why won't you run it?" — and he is right that
   refusing dead-ends the user, because for a manager with no distro package **the script is the only
   install path there is**. It is now shown in full and run on an explicit answer, defaulting to
   **yes** (owner's call: he asked for the manager). The trust rule that survives, per CLAUDE.md's
   "auto mode never installs untrusted automatically": `--auto`/`--yes` do **not** consent for it, and
   a non-interactive session only ever prints it. Run via `bash -c` (the upstream one-liners are
   bash-isms) and never elevated — these installers ask for sudo themselves, exactly as they would if
   the line were pasted. `privilege.rs` still owns JII's *own* escalation; this isn't JII's command.

3. **Progress is visible: a spinner over captured output.** Friendly mode hides a manager's chatter
   (U5), which left a silent terminal for the minutes `dnf upgrade` takes — reported as "it looks like
   it froze". `ui::Spinner` animates one line on **stderr** (stdout stays clean for pipes/`--json`),
   erases itself when the step ends, and shows elapsed seconds past three. It is inert without a TTY,
   in `--json`, and in Advanced — where actions are streamed anyway. `exec::run_actions_quiet` gives
   install/remove/update the same captured-with-spinner treatment the whole-system update already had,
   and remove's preview drops to one line per package like install's. Failures are never swallowed:
   the failing command plus a tail of its real output print, and `--dry-run`/Advanced keep every
   command.

4. **`--run` asks the source how to launch, via `Provider::launch_command`.** Default: the package's
   own name (right for anything that drops a program on `PATH`); Flatpak overrides with
   `flatpak run <app-id>`. The core assembles no command (ADR-0004). The caller **verifies the program
   exists** before running, so a package that installs none (a font, a library) says so rather than
   running something that isn't what was meant, and `exec`s on success so an interactive program owns
   the terminal and its exit code becomes JII's. `jii htop --run` on an already-installed htop just
   starts it — "install and run" with the install already done is "run".

5. **`jii providers` is gone.** ADR-0062 merged it into `jii sources` and left it as a hidden alias;
   the owner found it and asked why two commands do the same thing. One concept, one command.

**Alternatives rejected.** (a) *Bootstrap from the detected system manager* (reuse `sources remove`'s
`SysManager`) — needs a distro branch to name the manager, where "any source that is usable" is both
more general and already in the model. (b) *Keep brew show-only* — the owner overruled it; the
compromise that survives is that consent can't be delegated to `--yes`. (c) *Download the script and
show it in a pager first* — offered and declined as too many steps for the value; the URL is shown and
is the same one the project's own docs tell you to paste. (d) *A `--quiet` flag* rather than tying the
narration to Friendly/Advanced — a second axis for a distinction U5 already draws.

**Consequences.** `first_available_package` is gone (one caller shape, both migrated). The
`install.bootstrap_script_only` / `providers.script_only` / `providers.script_wont_run` locale keys are
replaced by `install.script_*`; `providers.{installed,available,add_hint}` were already dead and were
removed with the command. New `[exec]` locale section for the spinner labels. `jii sources` now lists
sources you disabled (they're absent from the provider registry, so the view could never show them)
with the command to restore each, plus a footer naming disable/enable — the answer to "how do I turn a
repository off?", which existed but nothing pointed at. `jii man` formats through `man(1)` at a
terminal and still emits raw roff when redirected (`jii man > jii.1`, how the packages build it).
`exec::changed_count` counts per line: it searched the whole blob for the first "upgraded", which lands
on apt's "The following packages will be upgraded:" prose rather than its tally, so every apt update
reported a bare "updated" — dnf5's transaction summary is counted too, and apt's "N not upgraded"
(held back) is not counted as changed.

## ADR-0067 — Junk-package heuristics: downgrade to untrusted + loud warning, never a hard block

**Status.** Accepted (2026-07-16).

**Context.** Language registries (PyPI, npm, crates.io) accept any name, so an obscure package can
shadow a well-known tool: the owner hit a PyPI `htop` ("A lhk 1st training project", one release from
2016) offered as if it were the process viewer, and a crates.io `htop` that is an HTML-to-PDF
converter. Ranking had **no relevance/popularity signal at all**, and pipx/go/brew deliberately offer
any existing package (no program-vs-library signal in their APIs — ADR-0023), so the semantic mismatch
is invisible exactly where users can't see it. The owner chose: filter heuristics that **downgrade,
with a red warning** — not a hard block (interview decision, 2026-07).

**Decision.** Two layers, no core source-branching (ADR-0004 holds):
1. `PackageCandidate` gains `popularity: Option<u64>` (recent downloads where a registry reports them
   cheaply: crates.io `recent_downloads` from the response already fetched; npm via one small
   `api.npmjs.org/downloads/point/last-month` call per hit) and `suspicious: bool`.
2. A **provider** may pre-mark registry-specific junk from facts only it understands — pipx flags a
   package whose newest release upload is older than 5 years (PyPI's honest junk marker; its download
   counts are mirror-inflated and its stats API rate-limited, so staleness beats popularity there).
3. The **engine** (`ranking::mark_suspicious`, run inside `Engine::rank` before sorting) applies the
   generic policy to candidates from network-registry sources (`Provider::can_search`, a trait — not
   an id check) at community trust with a non-path name: popularity < 1000 recent downloads, or no
   popularity signal plus thin metadata (no summary, or a `0.0.x` version), or a provider pre-mark →
   `suspicious = true` **and trust downgraded to untrusted**.
Effect: auto mode never installs it (ADR-0006 barrier), listings show red `untrusted`, and the install
preview prints a red warning naming the package, its source, and how to verify
(`jii info name:source`). The user can still pick it explicitly — a warning, not a wall.

**Alternatives.** (a) Hard-blocking junk — rejected by the owner: false positives would hide small
legitimate tools with no recourse. (b) pypistats.org for PyPI popularity — tried, reverted: 429s after
the first call and mirror bots give even junk ~1k downloads/month. (c) An ML/scoring model — against
the project's "deterministic and explainable" ranking principle.

**Consequences.** Cached candidates from before this change deserialize with `popularity: None`,
`suspicious: false` (serde defaults) and are re-marked on the next rank. Niche-but-legitimate registry
packages under the floor get a warning + explicit confirm; that is the accepted trade. The engine may
consult provider *traits* in ranking (can_search) — a precedent consistent with ADR-0004's "no concrete
source id" rule.

## ADR-0068 — Windows/macOS expansion: plan only (no code yet)

**Status.** Accepted (2026-07-16) — a plan, deliberately without implementation.

**Context.** The owner's horizon for JII is "everything, including Windows and macOS" (client PCs,
servers, phones eventually). The MVP constraint is Fedora-first Linux; the `platform` abstraction was
always the designated seam for cross-OS growth. The owner explicitly scoped this session to **plan +
ADR only** — code follows after the Linux beta is validated by external testers.

**Decision.** Cross-OS lands in three ordered waves, each behind the existing abstractions
(`Provider` for sources, `platform.rs` for host facts, `privilege.rs` for elevation) — the core stays
source- and OS-agnostic:

1. **Wave 1 — macOS (smallest step).** Homebrew already exists as a provider and is the canonical mac
   manager; the work is host-facts (`Platform::detect` for Darwin: no `/etc/os-release`, arch via
   `uname`, elevation = `sudo` only), asset selection for `-darwin`/`-apple` release artifacts in the
   forge provider (today hard-rejected), `~/Library` XDG mapping for config/state/cache, and CI
   (macOS runner + a `aarch64-apple-darwin` release artifact). No new trust tiers.
2. **Wave 2 — Windows (the real port).** New providers: **winget** (official tier) and **scoop /
   chocolatey** (community tier); elevation via UAC (`runas` / `Start-Process -Verb RunAs`) as a new
   `ElevationKind`; `%LOCALAPPDATA%` paths; forge assets `.exe`/`.msi`/`-windows.zip`; no `exec(2)`
   (`--run` becomes spawn+wait); shell integration = PowerShell completions. The Unix-only bits
   (`std::os::unix`) get `cfg(unix)`/`cfg(windows)` splits at the three call sites (exec's modes,
   `--run`'s exec, privilege).
3. **Wave 3 — phones et al.** Explicitly out of scope until 1 and 2 ship; recorded only so the
   ambition is not lost (Termux/Android would ride the apt/pkg providers).

**Gate.** Wave 1 starts only after the Linux beta's external-tester round (the
`yes-I-am-dev-and-want-to-test` command) reports no criticals — "мало жалоб" is the readiness bar the
owner set.

**Alternatives.** (a) Start Windows first (biggest market) — rejected: it forks the execution model
(no exec, UAC) before the Linux core is proven. (b) A cross-platform rewrite around a
`trait Platform` object — rejected: `Platform` is a value object of host facts by design (ADR-0029);
`cfg` splits at the handful of OS-specific call sites are smaller and honest.

**Consequences.** No code changes now. `docs/ROADMAP.md` gains the wave ordering; the forge asset
classifier keeps its Linux-only rejection list until Wave 1 flips it per-OS.

## ADR-0069 — Live progress bars: stream the source's own output, parse universal signals

**Status.** Accepted, implemented in v0.1.10-beta.

**Context.** Friendly mode captured a manager's output and showed only a timed spinner (ADR/UX #6).
For a long `dnf install` or a big download the owner saw a spinner and an elapsed clock but no sense
of *how far along* or *how much is left* — "выглядит как обновление и время, всё". The managers
themselves already print progress (dnf5's `[ 3/41]` step counter, download bars' `NN%`); JII was
throwing that signal away by waiting for the whole command to finish (`.output()`).

**Decision.** Read progress from the source and draw a real bar, without ever branching on the source
id (ADR-0004 holds):
1. **Stream, don't capture-then-parse.** `Privilege::run_streamed` spawns with piped stdout+stderr,
   reads them concurrently line by line, hands each line to a callback *as it arrives*, and still
   returns `(success, combined_output)` for the error tail / update summary. It replaces the old
   `run_captured` (removed — its one caller moved over).
2. **A source-agnostic parser** (`src/progress.rs`) turns one line into an optional reading from only
   two universal shapes: a bracketed `[done/total]` counter (preferred — monotonic across a whole
   transaction) or a bare `NN%` (a download bar's current value). A manager JII has never met still
   animates a real bar if it speaks either dialect; one that speaks neither falls back to the timed
   spinner — never a wrong number. Strict bracket parsing keeps dates/prose ratios out.
3. **The `Spinner` grows a `ProgressReporter`** (a cloneable handle onto a shared reading) and draws
   `████████░░░░  45%  [3/41]` when a reading is present, elapsed time otherwise. Downloads report an
   exact byte percentage (`downloaded / Content-Length`) via a streaming `reqwest` body — the honest
   number for a GitHub-release install where there is no manager to emit a counter.

**Alternatives.** (a) Pull in `indicatif` for the bar — rejected: it draws on its own schedule and
would fight the existing self-erasing spinner/`renderer.info` line discipline; the reading/drawing we
need is a dozen lines. (The unused `indicatif` dep is left untouched, not adopted.) (b) Parse per
source (`if dnf { … }`) — rejected outright (ADR-0004). (c) Keep capturing and fake a percentage from
elapsed time — rejected: dishonest, and the real number was already on the wire.

**Consequences.** `install`, `update` (per-package and whole-system) and downloads all show live
progress in Friendly mode. `--json`/piped/Advanced output is unchanged (the spinner is inert there).
Progress bars that redraw with `\r` on a TTY are not seen here because piping makes managers
line-buffer plain text — exactly the newline framing the streamer reads.

## ADR-0070 — `jii update` updates every Flatpak scope, not just per-user

**Status.** Accepted, implemented in v0.1.10-beta.

**Context.** The Flatpak provider does everything `--user` so installs never need root. But
`plan_update_all` also carried `--user`, so `flatpak update --user -y` skipped every **system-wide**
app — the ones KDE Discover / GNOME Software install under `/var/lib/flatpak`. The owner reported
`jii update` saying Flatpak was done while Discover still listed a pile of updates.

**Decision.** `plan_update_all` drops the scope flag: `flatpak update -y` updates *all* installations
(per-user **and** system-wide). "Update everything" has to mean everything. JII still never runs
itself as root here — the system-scope portion is authorized by flatpak's own polkit agent, not by
JII wrapping the command in sudo (`needs_root` stays false). Install/uninstall/single-update stay
`--user`: JII only tracks what it installed itself (always user-scope), so those paths are unaffected.

**Alternatives.** (a) Two explicit steps (`--system` then `--user`) — rejected: `flatpak update` with
no scope already means "all installations", and one command matches the user's mental model. (b) Keep
`--user` and document the limitation — rejected: it silently under-delivers on the command's promise.

**Consequences.** On a desktop, a system-scope update triggers the polkit GUI prompt (as Discover
does). On a headless session with no polkit agent the system portion fails with "not authorized",
which JII surfaces via its loud-failure path rather than hiding.

## ADR-0071 — Untrusted candidates are never presented as "recommended"

**Status.** Accepted, implemented post-v0.1.10-beta (batch 10).

**Context.** Ranking sorts primarily by name-match closeness (ADR-0042), so an *exact*-name candidate
outranks a mere prefix/substring match even from a lower-priority source. For a query like `google`
that meant an untrusted name-squat crate (`google` v0.0.0, "Reserved for use by Google.") sorted to
index 0 and the install chooser unconditionally starred index 0 as "⭐ recommended". So JII was
actively recommending an untrusted package — directly at odds with ADR-0006 (auto mode never installs
untrusted). The owner reported it: "`jii google` should give me something normal, not this."

**Decision.** Separate *ordering* from *recommendation*. Ranking is unchanged (an exact name is still
the closest match and stays at the top of the list — an explicit pick still works). But the
"recommended" star is now placed by `ranking::recommended_index` = the first candidate that is **not**
`Untrusted` and **not** `suspicious`. When that is `None` (every match is untrusted/suspicious), the
chooser stars nothing, defaults the cursor to the top, and prints `install.no_trusted_match`
("nothing trusted matches '{name}' — pick explicitly only if you're sure"). Never a dead-end: the
untrusted candidates are still listed and pickable (memory: "never refuse without an offer").

**Alternatives.** (a) Demote untrusted below trusted in the *ordering* itself — rejected: it would
bury a knowingly-typed exact name under unrelated trusted substrings, and the list order should still
reflect name-closeness. (b) Hard-filter untrusted out of the chooser — rejected: an explicit pick of a
known name-squat is a legitimate (if rare) user choice; JII warns, it doesn't forbid. (c) A curated
"did you mean google-chrome?" catalog (offered to the owner) — declined this round as extra data; the
generic honest message covers the reliability need without hard-coding package names (ADR-0004).

**Consequences.** The star/default now marks the best *trustworthy* option, or nothing when there is
none, with an honest heads-up. The auto/non-interactive path is unchanged (ADR-0006 already forces an
explicit answer for an untrusted top candidate). Pure presentation policy — no source-branching.

## ADR-0072 — Achievements: a cosmetic, decoupled ledger with a secret-install hook

**Status.** Accepted, implemented post-v0.1.10-beta (batch 11). First half of the owner's
"secret Sans-fight installer" idea; built first so the reward has somewhere to land.

**Context.** The owner wants a playful `jii achievements` command with unlockable badges, including
one hidden `???` entry granted only by a forthcoming *secret* install path (a Sans battle served
locally by the `secret` branch's `secret_install.sh`). Achievements must never affect what JII
installs, rank, or recommend, and must survive across runs.

**Decision.** A standalone `achievements` module mirroring `registry`: a JSON ledger at
`$XDG_STATE_HOME/jii/achievements.json` (falls back to the data dir) holding only `{id → unlocked_at}`.
A static `CATALOG` of `Achievement { id, icon, secret }` defines the set; titles/descriptions are
**localized**, not stored (ADR-0050) — locale keys `achieve.<id>.title` / `achieve.<id>.desc`, keyed by
the stable id (never renamed). `unlock(id)` is idempotent and returns "newly unlocked" so callers can
show a one-time toast; unknown ids are ignored so the store can only ever hold catalog ids. Granting is
**best-effort and cosmetic** — every load/save failure is swallowed so an achievement can never break a
real command. Secret+locked entries render as `???` with a teaser (and are `null` in `--json`), so a
secret is never spoiled. The secret install path is decoupled via a **sentinel file**
(`$XDG_STATE_HOME/jii/secret-install`) the installer drops; `Achievements::take_sentinel` consumes it
once on JII's next run and grants `sans`. Initial catalog: `first-install`, `doctor` (real events
already wired), `sans` (secret).

**Alternatives.** (a) Store the badge text in the ledger — rejected: violates ADR-0050 and would freeze
English into state files. (b) A hidden `jii __unlock <id>` subcommand for the installer to call —
rejected: needs `jii` already on PATH mid-install and couples the installer to a private CLI; a sentinel
file is simpler and order-independent. (c) Fold achievements into the registry — rejected: unrelated
concerns; a cosmetic feature must not risk the install ledger's integrity.

**Consequences.** `jii achievements` (alias `achievement`) lists progress; new achievements are one
`CATALOG` entry + two locale keys each. The `sans` reward is ready before the secret installer exists —
the installer just drops the sentinel. No source-branching, no effect on the install core. 4 unit tests.

## ADR-0073 — The secret Sans-fight installer (second half of ADR-0072)

**Status.** Accepted, implemented post-v0.1.10-beta (batch 11). Lives on an **orphan `secret`
branch**, not master. The reward plumbing (the `sans` achievement + sentinel) is ADR-0072.

**Context.** The owner wanted a hidden install path: `curl -fsSL …/secret/secret_install.sh | sh`
opens the classic Sans battle, and JII installs only once you win — unlocking the secret `sans`
achievement. The fight is a compiled Construct 2 browser game (`c2runtime.js`/`data.js`), and a
terminal `| sh` cannot render it, nor can a page on `github.io` (https) signal a local shell
(mixed-content blocks `fetch('http://127.0.0.1')`).

**Decision.** Serve a **self-hosted fork locally** and bridge victory over same-origin HTTP:
`secret_install.sh` downloads a `game.tar.gz` bundle, starts a **python3 one-shot server** on
`127.0.0.1:<random>` (SimpleHTTPRequestHandler subclass), `xdg-open`s it, and blocks. The bundle's
`index.html` is a cleaned copy of the deployed page — Yandex portal SDK, AdSense, gtag and the
service worker stripped, with no-op stubs for the ad hooks the compiled game still calls
(`ShowAd()`, `ysdk.features.*`) so they don't throw — plus a **victory poller** that reads the C2
runtime's Text-object `.text` for Sans's concession line ("you win"); **no game code is patched**.
On detection the page calls `/claim?token=<random>` (a token templated into the served HTML gates
against other local processes); the server writes a marker, replies, and `os._exit`s (the
`shutdown()` dance proved flaky). The shell then drops the `secret-install` sentinel (ADR-0072) and
delegates the actual install to master's canonical `install.sh`. **Fallbacks are mandatory and
honest:** no TTY / no `$DISPLAY` / no `python3` (3.7+) / no browser → a normal install, no fight, no
achievement (never a dead end). The Undertale fan-game assets are redistributed on the `secret`
branch per the repo owner's explicit decision (grey-zone fan content); audio (`media/*.ogg`) is
bundled so the fight isn't silent.

**Alternatives.** (a) Host the fork on GitHub Pages and signal localhost — rejected: https→http
mixed-content is blocked, so victory can't reach the shell. (b) Patch the compiled win event in
`data.js` — rejected: fragile against the C2 data model; polling rendered text is robust and
touches nothing. (c) A `nc`/shell server — rejected: netcat variants differ and binary assets
(audio/sprites) serve incorrectly. (d) A bundled Rust server — rejected: per-arch build/ship
overhead for an easter egg; python3 is near-universal on desktop Linux.

**Consequences.** A self-contained, decoupled easter egg: master carries only the reward plumbing;
the `secret` branch is a thin delivery vehicle (installer + bundle). Verified end-to-end headlessly
(server/token/claim/shutdown, sentinel drop, the no-TTY fallback) and in a real browser (game boots
to MainMenu, audio decodes with no console errors, the poller detects an injected "you win" on the
live runtime). Not tested: an actual human playthrough — that's the owner's to run.

---

## ADR-0074 — Achievement expansion + anti-tamper signing

**Status.** Accepted, implemented post-v0.1.11-beta (batch 12). Builds on ADR-0072
(achievements ledger) and ADR-0073 (secret Sans installer).

**Context.** Two owner asks. (1) The v0.1.11 catalog had only three entries
(first-install, doctor, secret sans); the owner wanted a fuller set — everyday
badges you stumble into, several you must hunt for, and two extreme grinds. (2) The
ledger is plain JSON the user owns, so the secret 💀 (and anything else) could be
forged by hand-editing `achievements.json` or dropping the `secret-install` sentinel.
The owner wanted tampering to be caught and mocked, not silently rewarded — while
accepting that a local file can never be truly tamper-proof.

**Decision — catalog.** Ten new entries join the three, ordered easy→hard in `CATALOG`
(the display order): everyday (explorer/cleaner/fresh), hunt-for
(self-made/bootstrapper/night-owl/polyglot/centurion), extreme
(millennium/completionist), plus the existing secret `sans`. Counter-based badges need
persisted state, so the ledger grew a `counters` map (`installs` total) and a `sources`
set (distinct source ids ever installed from); `polyglot` = 5 distinct sources,
`centurion`/`millennium` = 100/500 installs, `night-owl` = an install in 00:00–04:00
local, `completionist` = every **non-secret** badge (so the secret is never required to
finish). Hooks live at each command's success point in `cli/mod.rs`
(`record_install` for the install-driven ones, `grant_achievement` for the rest); the
core still never branches on a source **for behaviour** — recording a source id in a
cosmetic ledger isn't a ranking decision (ADR-0004 intact).

**Decision — anti-tamper.** Every `save` writes an **HMAC-SHA256** over the ledger's
canonical JSON (stable `BTreeMap`/`BTreeSet` order), keyed by a constant baked in the
binary and bound to this host's `/etc/machine-id`. On `load`: a present-but-wrong
signature, or a v2-shaped file (`counters`/`sources` present) with its `sig` stripped,
is **tampering** → the ledger is wiped in memory and flagged; `run()` reacts once, in a
Sans-flavoured line, and persists the clean re-signed ledger (so it doesn't nag every
command). A pre-signing legacy file (only `unlocked`, no `sig`) is **grandfathered** in
once and re-signed — so honestly-earned v0.1.11 badges (the owner's hard-won 💀)
survive the upgrade. HMAC is hand-rolled on the existing `sha2` dep (no new crate).

**This is deterrence, not security — stated plainly.** The user owns the machine and the
binary; the key is extractable by reverse-engineering, so a determined cheater can forge
a valid signature. The design only has to defeat a text editor: a casual hand-edit
(the overwhelming case) leaves the stale signature in place and trips instantly, and
stripping the signature also trips. The residual hole (extract key → recompute) is
accepted by the owner as out of scope for an easter-egg ledger.

**Alternatives.** (a) Leave it forgeable — rejected: the owner explicitly wanted a
reaction. (b) Server-side verification — rejected: JII is offline-first; no backend. (c)
Encrypt the ledger — rejected: the key is still in the binary, and an opaque blob is
worse UX (users can't read their own progress) for no real gain over a signature. (d) A
separate `hmac` crate — rejected: HMAC-SHA256 is ~15 lines on the `sha2` we already ship.

**Consequences.** 13 achievements; the ledger is signed and machine-bound; casual
cheating is caught and mocked; legacy ledgers migrate cleanly. Unit-tested end to end
(round-trip verifies, hand-edit flagged+wiped, stripped-sig flagged, legacy
grandfathered) and checked live (fresh list renders 0/13 with 💀 hidden; a forged
ledger is scolded, wiped and re-signed on the next command). `counters`/`sources` are a
ledger-format change, back-compatible via the grandfather path; no registry impact.
ADR-0004/0006 unaffected.

---

## ADR-0075 — Install one-liner served from sudonit.com (site-hosted installer)

**Status.** Accepted, implemented 2026-08-15 (batch 12, owner directive). Cross-repo:
the site lives in `0nigiris/sudonit` (Astro, GitHub Pages, domain `sudonit.com`).

**Context.** The owner wanted JII presented on their personal site and the canonical
`curl … | sh` command to point at their own domain rather than a raw GitHub URL — a
cleaner, brandable install line — without giving up GitHub's reliability.

**Decision.** The installer `install.sh` is copied into the site's `public/` so GitHub
Pages serves it verbatim at `https://sudonit.com/install.sh`. The canonical one-liner
becomes `curl -fsSL https://sudonit.com/install.sh | sh`; the
`raw.githubusercontent.com/0nigiris/JII/master/install.sh` URL is kept working and named
as an explicit **fallback** in every doc (README, install.sh header, TESTING,
SUPPORTED_SYSTEMS, JII_EXPLAINED). The script is unchanged — it still downloads the
**binaries from GitHub Releases** — so only the *entry-point* moved; the heavy artifacts
stay on GitHub's CDN. JII itself also gained a dedicated page on the site (a `projects`
collection entry → `/jii`, `/en/jii`, `/es/jii`, trilingual) with a terminal install
set-piece; that's site-repo work, not core JII.

**Alternatives.** (a) Hard-switch to sudonit.com only — rejected: a single point of
failure (Pages/DNS outage would block installs); the fallback costs nothing. (b) Host
the binaries on the site too — rejected: large per-arch artifacts belong on Releases'
CDN, and Pages has size/bandwidth limits. (c) A redirect from sudonit.com to the raw URL
— rejected: Pages is static (no server redirects without a meta/JS hop that `curl` won't
follow); serving the file directly is simpler and honest.

**Consequences.** Two copies of `install.sh` now exist (JII repo = source of truth; the
site's `public/install.sh` is a synced copy) — they must be kept in step on any installer
change. The canonical command is brandable and the install path is resilient (site down →
GitHub fallback still documented). Verified live: `https://sudonit.com/install.sh` and
`/jii/` both return 200 and serve the real script/page.

---

## ADR-0076 — The Jevil "Chaos Simulator" installer + the `jevil` achievement

**Status.** Accepted; in progress (batch 12/13). The in-binary half (the `jevil`
achievement + the `chaos-install` sentinel) ships in **v0.1.13-beta**; the fight
installer lives on a new orphan **`chaos`** branch (like ADR-0073's `secret`). Sibling
of ADR-0072/0073 (the Sans secret installer).

**Context.** After the Sans-fight installer (ADR-0073) the owner wanted a second, even
better one: beat **Jevil** (Deltarune) in the "Chaos Simulator", and JII installs whether
you **spare or kill** him. The found build is a **TurboWarp/Scratch project packaged as an
Electron app** (`resources/app/` unpacked: `electron-main.js` + `preload` + the Scratch
runtime + `project.json`), ~242 MB. Unlike the browser-based Sans game, this is already a
native desktop window — the owner explicitly wanted it launched "as an application".

**Decision.** Reuse the sentinel pattern, but drop the whole local-HTTP-server/token dance
(ADR-0073) — Electron's main process is Node, so it writes the marker directly. The flow:
`chaos_install.sh` (on `chaos`) downloads the **modified Electron bundle** (hosted as a
**GitHub Release asset** — a >100 MB file can't live in git), launches `./chaos-simulator`
as a window, and waits. The app is patched in three small places: the renderer watches the
Scratch VM for the end of the fight (spare = the `battler.spare`/`joker.spare` broadcasts;
kill = `battler.health%` reaching 0; the player dying via `CutScene.GameOver.*` is **not**
a win), `preload` exposes a one-way `contextBridge` channel, and `electron-main.js` adds an
`ipcMain` handler that writes `$XDG_STATE_HOME/jii/chaos-install` (contents `spare`|`kill`)
then quits. The shell then drops nothing else — JII consumes that sentinel on its next run
via `Achievements::take_chaos_sentinel()` → unlocks the secret **`jevil`** 🃏 and records the
ending (a `jevil-spare`/`jevil-kill` counter drives the shown description; `completionist`
excludes secrets, so `jevil` is never required to finish). Mandatory honest fallbacks (no
`$DISPLAY` / no GUI libs / non-x86_64-Linux / download fails → a normal install, no fight,
no achievement). The Deltarune fan-game assets are redistributed per the owner's standing
grey-zone decision (ADR-0073).

**Alternatives.** (a) The Sans HTTP-server bridge — rejected: Electron's Node main is a
direct, simpler channel. (b) Bundle the 242 MB in git / on the `chaos` branch — rejected:
GitHub blocks >100 MB files; a Release asset (≤2 GB) is the right home. (c) A cross-platform
web build like Sans — considered; the owner also found the game's source
(`CherrySodaPop/Jevil-VGB`), so a lighter rebuild may be possible later, but the ready-made
Electron bundle ships first. (d) Reuse the `sans` achievement — rejected: Jevil earns his
own 🃏 (the "second easter egg" slot noted in ADR-0074).

**Consequences.** JII now has 14 achievements, two of them secret. The `jevil` plumbing is
live and unit-verified (kill/spare markers each unlock it with the matching ending text);
the fight installer + the modified bundle + the Release asset are the remaining `chaos`-branch
work. Linux-x86_64 only (the Electron build) — acceptable for a secret path, with a normal
install everywhere else. A real human playthrough (spare and kill) is the owner's to run.

---

## ADR-0077 — Generic boss sentinels + the two VGB fights (Jevil-VGB, Spamton NEO)

**Status.** Accepted (2026-08-15).

**Context.** ADR-0076 shipped one secret fight with one hard-wired sentinel
(`take_chaos_sentinel`). The owner then found two more fan games by the same author —
`CherrySodaPop/Jevil-VGB` and `CherrySodaPop/Spamton-NEO-VGB`, both Deltarune fights
recreated "handheld-console style" in **Godot 3.6, GPL-3.0, full source published**. A
per-boss method in `achievements.rs` plus a per-boss branch in `cli/mod.rs` would grow
linearly with every new fight, and the first Chaos-Simulator detector had already proved
that guessing the ending from the wrong health variable is easy to get wrong.

**Decision.** Two parts.

*Plumbing.* Replace `chaos_sentinel_path`/`take_chaos_sentinel` with a generic
`Achievements::boss_sentinel_path(file)` / `take_boss_sentinel(file)` plus named constants
(`JEVIL_SENTINEL = "chaos-install"`, `SPAMTON_SENTINEL = "spamton-install"`), and replace
`grant_jevil` with `grant_boss(id, variant, renderer)`. `run()` loops over a
`(sentinel, achievement id)` table, so a new fight is one table row, one catalog entry and
one locale block — no new code paths. `achievement_desc_key` treats every boss id the same
way (`<id>-spare` / `<id>-kill` counters pick the description).

*The fights.* Both games are **built from source** rather than repacked: a 24-line
`jii_marker.gd` writes `$XDG_STATE_HOME/jii/<marker>` with `spare`|`kill`, and each fight
script calls it at the exact branch the game already distinguishes — Jevil: `health <= 0`
(kill) vs `sleepHealth <= 0` (pacified); Spamton NEO: `health <= 0` (kill) vs
`wireHealth <= 0` (strings cut, "real boy"). No health guessing, no VM hooks, no bridge —
the win conditions are named in the game's own code. Exported with Godot 3.6 as a single
embedded-PCK Linux binary (~43 MB each), so the installer is a plain download + `chmod +x`
+ run, with no tarball and no `--no-sandbox` caveat. Jevil-VGB writes the **same**
`chaos-install` marker as the Chaos Simulator: two Jevil fights, one 🃏. Spamton NEO gets
his own 🎭 `spamton` (secret, so `completionist` still ignores it).

*Hosting.* `vgb_install.sh` joins `chaos_install.sh` on the `chaos` branch; Spamton gets an
orphan `spamton` branch and a `spamton-game` release. Because these are **GPL-3.0** works we
redistribute in binary form, each release also carries the complete modified source
(`*-source.tar.gz`) — the licence obligation the Chaos Simulator (unknown licence, ADR-0073
grey zone) doesn't let us discharge.

**Alternatives.** (a) Keep per-boss methods — rejected: linear growth, and the loop is
smaller than what it replaces. (b) Ship the HTML5 export and reuse the Sans browser bridge —
rejected: a native window is what the owner asked for, and the Godot export removes the
bridge entirely. (c) Bundle the Godot editor and run the project unexported — rejected:
bigger download, worse UX. (d) Give Jevil-VGB its own achievement — rejected: same boss,
same badge; the ending text still says which way it went.

**Consequences.** JII has 15 achievements, three secret. Adding a fourth fight now costs one
table row. Linux-x86_64 only (the exports), with the usual honest fallback to a normal
install. The kill/spare branches are verified in the games' own source, and `jii_marker.gd`
is runtime-tested, but a real playthrough of each ending remains the owner's to run.

---

## ADR-0078 — One badge per boss ending + a revealed-goal tier

**Status.** Accepted (2026-08-16).

**Context.** Owner ask: "a badge for every boss *and* every way of beating them, and add more
besides". Beating a boss had granted exactly one badge, with the *description* rewritten to
name the ending you got (ADR-0076/0077) — so playing both ways showed nothing new, and the
second run felt unrewarded. The obvious fix (a badge per ending) collides with how secrets are
displayed: every locked secret is a `???` row, so six new ending badges would have added six
anonymous rows to `jii achievements`, spoiling that fights exist while telling nobody anything.

**Decision.** Two changes, one to the model and one to the display.

*Model.* `Achievement` gains `revealed_by: Option<&'static str>`, and `achievements::visible()`
filters the catalog by it. An entry with `revealed_by: Some(boss)` is **omitted from the list
entirely** — not even a `???` — until that boss's badge is unlocked; from then on it shows with
its real title and description, unlocked or not. So the endings you haven't played read as
*named goals* ("Sweet Dreams — put Jevil to sleep instead of cutting him down"), which is the
only place in the ledger where a locked entry is legible. `earned`/`total` count the visible set,
so the total doesn't leak hidden rows. The friendly view and `--json` follow the same rule.

*Grants.* `grant_boss` now awards up to three badges at once: the boss's own, the one for that
ending (`<boss>-<ending>`), and `<boss>-both` once every ending in `ENDINGS` has been seen (the
existing `<boss>-<ending>` counters already recorded this). `maybe_completionist` additionally
grants `boss-slayer` when every id in `BOSSES` is unlocked. The per-ending `desc-spare`/`desc-kill`
locale keys are dropped: the base badge is neutral again, and each ending speaks for itself.

*More badges.* Eight everyday ones, each hooked to an existing command with no new plumbing:
`wizard` (finish setup), `paper-trail` (`jii how`), `dry-runner` (`--dry-run`), `auditor`
(`jii list --audit`), `sniper` (an explicit `name:source`), `haul` (`HAUL_AT = 5` packages in
one command), `translator` (`jii lang <code>`), `early-bird` (install 05:00–07:59, the mirror of
`night-owl`). Catalog: 15 → 30.

**Alternatives.** (a) Keep one badge per boss and vary only the text — rejected: that's what
prompted the ask. (b) Show ending badges as `???` like other secrets — rejected: six anonymous
rows is noise that spoils *that* there are secrets without being a goal anyone can act on.
(c) Reveal them from the start — rejected: it spoils the fights outright. (d) Make the endings
non-secret — rejected: `completionist` would then require beating every boss both ways, and the
crown is deliberately earnable without the easter eggs.

**Consequences.** 30 achievements, 10 secret, of which 6 are revealed-goal entries. The crown
now needs eight more everyday badges — all reachable from ordinary commands. Adding a fight
still costs one row in `BOSSES` plus its catalog/locale entries; adding an *ending* costs one
entry in `ENDINGS` and per-boss catalog rows. A ledger from an older version keeps everything it
had: nothing is renamed, only added.

---

## ADR-0079 — Release notes in the binary: `jii changelog` + a post-update summary

**Status.** Accepted (2026-08-16).

**Context.** Owner ask: "`jii update jii` should end by saying what changed, and the same
notes should be readable per version via `jii changelog`". Until now the only user-facing
record of a release was the GitHub release page (auto-generated commit lists) and the RPM
`%changelog` — neither reachable from the terminal, both written for packagers rather than
users. Self-update ended on a bare "updated to v0.1.16-beta", which says nothing about what
the user actually got.

**Decision.** A new `data/changelog.toml`, embedded with `include_str!` like the locales, and
a thin `changelog.rs` over it (`releases()` / `find()` / `since()` / `current()`). Each entry
is `version`, ISO `date`, and the bullets in both shipped languages. `jii changelog` prints
the running version; `jii changelog <version>` any past one (a bare `0.1.12` matches
`0.1.12-beta`); `--all` the history; `--since <version>` everything newer. `--json` emits
`{version, date, current, notes}` rows.

*Notes live with the version, not in `locales/*.toml`.* This is a deliberate, narrow
exception to ADR-0050: a per-bullet locale key would mean inventing
`changelog.0-1-15.line3` for every bullet of every release forever, in two files, with the
text divorced from the version it describes. The command's own chrome (header, hints,
errors) is localized normally.

*The post-update summary re-invokes the new binary.* The running binary only carries notes up
to its own release, so it cannot describe the version it just installed. After a successful
self-update JII runs `<exe> changelog --since <the version we were>` — the new binary is
already at that path, and it knows notes this one never could. Best-effort: if the child
can't run, JII prints "run `jii changelog`" rather than ending on "updated". Suppressed under
`--json` (a second document would corrupt the first).

*The release checklist is enforced by a test.* `this_build_has_release_notes_at_the_top`
asserts the first entry equals `CARGO_PKG_VERSION`, so shipping a version whose notes nobody
wrote fails `cargo test` instead of printing an empty changelog to a user. Companion tests
check descending order, en+ru presence, and ISO dates.

**Alternatives.** (a) Fetch release bodies from the GitHub API — rejected: needs network for
something that must work offline, is rate-limited and unauthenticated, and the auto-generated
bodies are commit lists, not user-facing notes. (b) Ship `CHANGELOG.md` and parse Markdown —
rejected: a parser for prose we control anyway, and no place for translations. (c) Have the
*old* binary print notes for the new version — impossible without a network fetch; the
re-invocation is what makes it work offline. (d) Generate the notes from git history —
rejected: commit subjects are written for maintainers ("refactor: Forge abstraction"), which
is exactly the register the owner asked us to leave behind.

**Consequences.** Every release now has two obligations instead of one: bump the version *and*
add its notes (the test enforces both). The RPM `%changelog` stays as-is for packagers — it is
allowed to say more technical things. Adding a language means adding a key per entry, with an
English fallback if it is missed. Notes for versions older than this file's creation were
reconstructed from the spec changelog and git tags (0.1.0 – 0.1.15).

---

## ADR-0080 — A bootstrap that finishes, and failures a source can explain

**Status.** Accepted (2026-08-17, v0.1.16-beta).

**Context.** Owner feedback from the cross-distro test round exposed three variants of the same
flaw: JII stops one step short and hands the rest to the user, which is exactly the dead end
CLAUDE.md's UX rules forbid.

1. `jii sources add brew` ran Homebrew's installer, then reported "installed, but isn't on this
   shell's PATH — follow the installer's last instructions". The user's verdict: *"why do I,
   as a user, still have to read and type commands myself?"* They then pasted the three lines
   Homebrew prints (two `.zshrc` edits and `dnf group install development-tools`) by hand.
2. `jii snap` installed `snapd` and called it done, leaving a socket that ships disabled outside
   Ubuntu and no `/snap` symlink — a manager that then refuses every install.
3. `pipx install affinity` failed with pipx's own "No apps associated with package affinity. Try
   again with `--include-deps`… Dependent package 'numpy' contains 2 apps", printed raw under a
   ✗. Accurate, useless, and its only actionable suggestion is a trap (installing numpy's
   `f2py`). ADR-0023 knowingly accepts these rejections — PyPI exposes no entry-point data — but
   promised "a clear message", which was never delivered.

Also: `Achievements` only awarded `bootstrapper` on the T6 install path, so setting a manager up
through `jii sources add` earned nothing.

**Decision.** Three source-agnostic trait methods, so the core keeps knowing nothing about any
particular manager (ADR-0004), plus the plumbing to use them.

*`Provider::plan_post_bootstrap`* returns the steps that make a just-installed manager **usable**,
as a normal previewable `InstallPlan`: Flatpak adds the Flathub remote (this replaces the
`if source_id == "flatpak"` special case in the CLI), Snap enables `snapd.socket` and links
`/snap` when absent. Root steps escalate through `privilege.rs` and are printed like any other,
and `--dry-run` previews them under "Then, to make Snap usable:". Both bootstrap paths (T6 and
`jii sources add`) run it, and both now grant `bootstrapper`.

*`Bootstrap::Script` carries an optional `ShellSetup`* (`bins`, `rc_line`). Homebrew declares its
two standard prefixes and `eval "$({bin} shellenv)"`. Two things follow: the provider resolves
`brew` by absolute path when it isn't on PATH (`homebrew::brew_bin()`, deliberately uncached —
brew may be installed *during* this run), so JII keeps working immediately; and JII offers to
append the shell line to the user's rc itself (new `shellrc.rs`: `$SHELL` → `.zshrc`/`.bashrc`/
`.kshrc`, `None` for fish, whose syntax differs; idempotent, append-only, shown before writing,
explicit yes required). Declining prints the line to paste — never a dead end. `jii doctor` gained
a Homebrew-only compiler check (`Fix::Install("gcc")`), so the build-tools half of Homebrew's
closing notes is something JII offers to do rather than something the user reads.

*`Provider::explain_failure`* turns a failed command's captured output into a `FailureNote`
(message + hints). `exec.rs` no longer prints the failure itself: it returns the new
`JiiError::StepFailed { command, output }`, and `Engine::report_step_failure` asks the plan's own
source first, falling back to the raw 12-line tail when it has nothing to say. pipx recognizes
"No apps associated with package X" and answers: *X is a Python library, not a program*, plus
`pip install X` (in a venv) and `jii search X`. `main.rs` no longer re-prints a `StepFailed`,
which would repeat the command in English under a second ✗.

**Alternatives.** (a) Mutate the process `PATH` after a script bootstrap — rejected: `set_var` is
`unsafe` in edition 2024 for good reason (a data race against any concurrent `getenv` in the
tokio pool), and resolving the binary in the provider is both safe and more honest. (b) Write the
shell line without asking — rejected: JII doesn't own that file. (c) Match failure text in
`exec.rs` — rejected: that is the core branching on a source id. (d) Pre-filter PyPI libraries
out of search — still rejected for ADR-0023's original reason (no reliable signal); explaining
the rejection is the half we owed.

**Consequences.** Adding a manager that needs finishing steps, or a source that can explain its
own failures, is now an override rather than a patch to the CLI. `exec::run_actions_quiet` no
longer renders errors, so any future caller must report `StepFailed` (the two engine paths do).
`shellrc.rs` deliberately supports only shells whose rc syntax matches the line the manager
prints; fish users get the line to paste. The `untrusted` trust level is now *displayed* as
"unverified" / «без проверки» (owner: "untrusted sounds very scary") — the machine label,
ranking and auto-mode rule are unchanged, and README now states plainly that JII does not vouch
for third-party software.

## ADR-0081 — A fourth boss, and a table that knows what a boss is

**Status.** Accepted (2026-08-18, v0.1.16-beta).

**Context.** The owner handed over a downloaded copy of **Omega Flowey Simulator** — Undertale's
final neutral-route boss, built as a Scratch project and packaged for the desktop with the
TurboWarp Packager — to become the fourth secret install path, after 💀 Sans, 🃏 Jevil and
🎭 Spamton NEO.

Two things stood in the way. First, the boss machinery ADR-0077 generalized was only *half*
general: `BOSSES` was a list of ids, `ENDINGS` was one global `["spare", "kill"]`, and
`take_boss_sentinel` hard-coded that same pair when normalizing what an installer wrote. A fight
whose two paths are *not* mercy and murder had nowhere to say so. Second, this game offers no
mercy choice at all: you either beat Omega Flowey or you don't. What it does have is a hard mode
in its own menu.

**Decision.** *Endings belong to the fight, not to the codebase.* `BOSSES` becomes a table of
`Boss { id, endings, sentinel }`, so a fight declares its own paths (`MERCY_ENDINGS` for Jevil and
Spamton, `FLOWEY_ENDINGS = ["normal", "hard"]` for Flowey) and its own sentinel file. Sans keeps
an empty `endings` and no sentinel — he has one path and an older contentless marker.
`take_boss_sentinel` now takes the `Boss` and normalizes against *its* endings, falling back to
the first one, so a half-written marker still grants the fight rather than nothing. The CLI's
`(sentinel, id)` literal and its `ENDINGS`-based "both ways" check both read the table instead.
Adding the fifth boss is one row, four badges and their locale keys — and a new i18n test fails
if the text is missing.

*The game reports its own win, and JII trusts nothing else.* The project already knows when the
fight is over: it sets the stage variable `flowey hp` to 9950 when the battle starts and
broadcasts `flowey death` the moment it drops below 1. The patch (one added file,
`jii-marker.js`, plus a two-line call) watches that same variable from Electron's **main**
process via `executeJavaScript` — the page keeps its sandbox, its context isolation and its
original code — and writes `normal` or `hard`, read from the game's own `hard mode?` toggle,
into `$XDG_STATE_HOME/jii/flowey-install`. It arms only after seeing a health bar above zero, so
a project that never started cannot look like a win. The only other change to the game is that
its connection to TurboWarp's cloud-variable servers is removed: an installer has no business
phoning anywhere.

*Losing is not a dead end.* Omega Flowey is the hardest fight JII hides, and most people will
lose. Where the Spamton installer exits 1 on a loss, this one offers a plain install anyway
(default yes) — the same rule that governs the rest of JII: never refuse without an offer.

**Alternatives considered.**

* *Ship the game as the web build and open the user's browser* (~48 MB instead of ~121 MB). It
  is a TurboWarp package, so it runs in a browser as-is — but a browser page cannot write a
  marker, so it would need a local HTTP server (a Python dependency JII does not otherwise have)
  to catch the win, and it trades a real window for a tab. Rejected: heavier machinery and a
  worse fight for a lighter download.
* *Patch the Scratch project itself* to broadcast into a JS hook. Rejected: editing a 1.6 MB
  serialized `project.json` is unreviewable, while the variable it already maintains is public.
* *Give Flowey `spare`/`kill` anyway* by inventing a choice the game doesn't offer. Rejected as
  a lie about the fight; hard mode is the duality this game actually has.

**Consequences.** 👺 `boss-slayer` now needs four fights instead of three — nobody loses a badge
they earned, but the crown moved. The ledger gains `flowey`, `flowey-normal`, `flowey-hard` and
`flowey-both`. The `flowey` orphan branch holds the installer and the patch; the game itself
ships as a release asset, never in git. `ENDINGS` as a global constant is gone —
`achievements::boss(id).endings` replaces it.

**Addendum (2026-08-18).** Two things the live fight taught us the same evening.

*Non-version releases are not JII releases.* `flowey-game` is the **fourth** release in this repo
that carries a game instead of a build. Both update paths took "the newest non-draft release" as
the newest JII — so a bundle published after the last `v*` tag would have been installed as JII
and failed looking for a tarball that isn't in it. `install.sh` and `selfupdate::pick_release`
now require a `v` tag; the shell parse also switched to `grep -o … | head -1`, because the
previous greedy `sed` would have returned the *oldest* release had GitHub ever answered with
single-line JSON instead of the pretty-printed body it sends curl.

*The fight must not depend on the keyboard layout.* Scratch matches on the character a key
produces, not on the key: on a Cyrillic layout `z` arrives as `я`, so the menu moves on the
arrows and then nothing ever confirms — a dead end at the first prompt. `jii-marker.js` now also
feeds the runtime the Latin letter the physical key stands for (`event.code` → `KeyZ` → `z`),
alongside whatever the layout produced. Telling the player to go change their system settings
would have been exactly the hand-off to the user that ADR-0080 exists to stop.

**Addendum (2026-08-21) — a published tag is never moved.** The release-picking fix above landed
*after* `v0.1.16-beta` was cut, so no shipped binary contained it. The tempting repair was to
force the tag onto the newer commit: `softprops/action-gh-release@v2` updates the existing
release in place, the assets would have been replaced, and the version number would have stayed
put. **Rejected.** A version number must name one immutable set of bytes — otherwise "I'm on
0.1.16" stops being a fact anyone can act on, a checksum a user recorded silently stops
matching, and a bug report can no longer be tied to a build. The cost of the alternative is one
extra patch number, which is free. `v0.1.17-beta` was cut instead, carrying that one fix.

*Consequence:* the fix a release forgets is a reason to release again, never a reason to edit
history. The same rule does **not** apply to the game-bundle releases (`chaos-game`,
`spamton-game`, `flowey-game`): they are not versioned, nothing resolves them by number, and
their assets are re-uploaded with `--clobber` as the fights are fixed.

## ADR-0082 — A miss must speak, `how` must answer for the whole machine, and a failed run must exit non-zero

**Status:** accepted (2026-08-21)

**Context.** A tester round on Arch (`jii yes-I-am-dev-and-want-to-test`, 10 pass / 2 fail) surfaced
three defects, two of them dead ends of exactly the kind ADR-0080 exists to prevent.

1. `jii totally-nonexistent-xyz321` printed **nothing whatsoever** and exited 0. The "not found"
   message, the library explanation and the browse links were all still in the code, at step 2 of
   the install path. ADR-0065 later inserted the manager-bootstrap step *before* it, ending in
   `if chosen.is_empty() { return Ok(()) }` — a guard written for "every candidate was dropped
   because its manager was declined". When nothing resolves at all, `chosen` is empty on the way
   in, so that guard fired immediately and swallowed the report. A silent success is the worst
   possible answer: the user cannot tell a typo from a crash.
2. `jii how htop` answered `'htop' was not installed by jii (no record)` for a package that was
   installed, in the distro's own repository, and which `jii remove htop` then removed without
   complaint. The command knew only JII's ledger, so it was both a dead end and a contradiction
   of what the rest of JII could plainly do.
3. `jii doctor`'s advice lines appeared under the wrong checks in the tester's log. Nothing was
   mis-indexed: `warn`/`error` write to stderr and everything else to stdout, and the moment
   either is redirected Rust line-buffers stdout while stderr stays unbuffered.

**Decision.**

- **The misses report is not behind any early return.** The bootstrap guard is removed; the single
  `chosen.is_empty()` check that ends the run now sits *after* step 2. A report that tells the user
  their input matched nothing is the most important thing such a run has to say, and must not be
  reachable only by luck of ordering.
- **`how` answers in three cases, in order:** JII's ledger (with the install date), else the
  system's own owner via `resolve_all_installed` (source, version, trust, and an explicit "jii has
  no record of it, so there is no date — but remove and update still work"), else, for something
  not installed anywhere, the source JII *would* use. The command's help always promised "how a
  package was **or would be** installed"; only the first half existed.
- **A run that printed a red ✗ exits non-zero.** New `JiiError::AlreadyReported`: an empty error
  that carries status and no message, suppressed by `main::report` the way `StepFailed` is. Used
  for a total miss and for a rejected spec (`@ref`, an unknown `:source`). Reporting success for a
  run that refused to do what was asked breaks every script that wraps JII.
- **The renderer flushes stdout before writing to stderr.** Warnings keep their own stream (scripts
  separate them), and keep their place in the output.

**Alternatives rejected.**

- *Move the not-found report into the resolution loop, per package.* Rejected: a batch would then
  interleave misses with the search of the next name, and the "install the rest anyway?" question
  needs the complete set.
- *Send warnings to stdout.* Rejected: `jii ... 2>/dev/null` losing its warnings is a real use, and
  the ordering problem is solved without giving that up.
- *Give `AlreadyReported` a message.* Rejected: the point is that the command already said it, in
  the user's language and with its own context. A second, English, generic line under it is the
  duplicate `StepFailed` was suppressed to avoid.
- *Leave `how` registry-only and say "not by jii, try `jii list`".* Rejected: a truthful refusal is
  still a refusal. The information was one `resolve_all_installed` away — the same call `remove`
  has always made.

**Consequences.** `how` is now async and does a provider fan-out on a ledger miss, so it is slower
in that case (it was instant because it answered nothing). Exit codes change for two paths that
previously returned 0 while printing an error; this is a fix, but it is a behaviour change for
anything that scripted around it. The devtest checklist's expectations for `jii list` and `jii how`
were themselves wrong and were corrected in the same pass — a checklist that describes the bug as
the expected result is worse than no checklist.

---

## ADR-0083 — A token belongs in a file JII reads, not in a shell profile everything inherits

**Status.** Accepted (2026-08-22).

**Context.** The first-run wizard and `jii doctor` both told the user, in step 2 of three, to add

```
export GITHUB_TOKEN="ghp_…"
```

to `~/.bashrc` (or `~/.zshrc`). That is the advice most of the internet gives, and it is the wrong
advice coming from **this** program in particular, for two independent reasons:

- An exported variable is inherited by the environment of **every process the user starts** from
  that point on. JII's whole job is installing third-party software, some of it from sources it
  itself labels `untrusted`. Handing a credential to every one of those binaries — readable from
  `/proc/<pid>/environ` by anything running as that user — to save an HTTP rate limit is a bad
  trade, and a strange one for a tool that ranks sources by trust and refuses to run as root.
- `~/.bashrc` is mode 0644 on a default Fedora account. Any other user on the machine can read it.

The report came from an outside contributor reviewing the repo, and it is correct. The token is
optional and scopeless in our own instructions, so the blast radius is small — but the advice is
what normalises the habit, and people reuse tokens they already have.

**Decision.** JII resolves a token from three places, first hit wins, and recommends the last one
least:

1. **the configured environment variable** — kept, and still first. It is what CI sets from its
   secret store, and what `GITHUB_TOKEN=… jii install owner/repo` sets for exactly one process.
   The variable was never the problem; *persisting* it in a shell profile was.
2. **`$XDG_CONFIG_HOME/jii/<var lowercased>`** — `GITHUB_TOKEN` → `~/.config/jii/github_token`, a
   one-line file beside `config.toml` that only JII reads. Nothing exports it, so no child process
   inherits it.
3. **the forge's own credential helper** — `gh auth token`. Someone who has already run
   `gh auth login` needs to do nothing at all, and the secret stays in whatever store `gh` chose.

The helper is a property of the **forge** (`Forge::token_command`), not of the core, so "GitHub has
a `gh` CLI" never becomes an `if source == "github"` (ADR-0004). Provenance is exposed through a new
default-`None` `Provider::credential_origin`, so `jii doctor` reports *where* a token came from
across any number of forges without naming one.

**Alternatives rejected.**

- *Keep the `~/.bashrc` advice and just document the risk.* Rejected: the tool's own setup flow is
  where the habit is created. A warning under bad instructions is still bad instructions.
- *Refuse to read a token file that is group/world-readable.* Rejected: it is the user's file and
  their machine, and silently ignoring a token they deliberately placed is a dead end (a
  standing UX rule here). `doctor` flags the mode instead, with a `chmod 600` it offers to run.
- *Encrypt the token file, or reach for a keyring.* Rejected as overengineering for an optional,
  scopeless, read-public-repos token. `gh auth login` already covers anyone who wants a real
  credential store, and route 3 defers to it.
- *Let JII write the token file itself (a `jii token` command).* Rejected for now: it is a new
  command, new locale keys, and a new way to get a secret onto disk, to replace a two-token
  shell snippet. The wizard prints `(umask 077; cat > …)` instead — 0600 by construction, and a
  here-doc keeps the token out of shell history, which `echo … > file` would not.

**Consequences.** `setup.gh_step_export` / `gh_step_reload` are gone from both locales, replaced by
three labelled routes plus an explicit line saying why `~/.bashrc` is not among them.
`check.token_ok` now names the provenance instead of asserting "the variable is set", which also
fixes a real confusion: someone with a stale export *and* a fresh token file could not tell which
one was in play. `doctor` gains one check (an exposed token file). The forge caches its resolution
in a `OnceLock`, so the credential helper is spawned at most once per process rather than once per
request. Anyone relying on the old advice keeps working unchanged — route 1 still reads the
variable, wherever it was set.

---

## ADR-0084 — The declared MSRV was wrong; the lockfile stays; rustfmt gets a config but no gate

**Status.** Accepted (2026-08-22).

**Context.** An outside contributor's PR ("Cleaning up the repository", #12) bundled five separate
changes: SPDX/REUSE licensing, a whole-tree `cargo fmt`, a `rust-toolchain.toml`, a `.gitignore`
rewrite, and the deletion of `Cargo.lock`. Reviewing it forced decisions on four of them.

**Decision.**

- **`rust-version` is 1.88, not 1.85.** `src/` uses let-chains (`… && let Some(x) = …`) in 21
  places; those stabilized in 1.88. Edition 2024 alone would only require 1.85, which is where the
  wrong number came from. The declaration was a promise the crate could not keep: anyone on 1.85
  got a wall of syntax errors instead of Cargo's "requires rustc 1.88". Correcting it also
  un-suppressed a `collapsible_if` lint that clippy had been holding back because the suggested fix
  needed an MSRV we claimed not to have — fixed in the same pass.
- **`rust-toolchain.toml` is added, pinned to `stable`.** The contributor's argument is right:
  without it, a contributor on rustup builds with whatever their default toolchain is. The channel
  is `stable` rather than the 1.88 floor deliberately — pinning here pins CI too, and CI should keep
  exercising current stable and current clippy. Holding the floor honest is `rust-version`'s job.
- **`Cargo.lock` stays tracked.** JII is a binary crate distributed as prebuilt artifacts, `ci.yml`
  and `release.yml` both build with `--locked` (they fail outright without the file), and ADR-0081
  says a released tag names one immutable set of bytes — which is not true of a build that resolves
  dependencies afresh. A package installer that silently picks up new transitive versions at build
  time is also the wrong shape of program to be casual about. `.gitignore` carries a comment saying
  so, because "why isn't this ignored" is a reasonable question to ask twice.
- **`rustfmt.toml` is added; `cargo fmt` is still not run and still not gated.** ADR-0013 keeps
  rustfmt out of the Definition of Done and that does not change here. The config exists so that
  anyone who does run it produces the house style: `use_small_heuristics = "Max"` is the load-
  bearing line — without it rustfmt explodes every compact struct literal, turning each of the 34
  entries in `achievements.rs` from one line into five. Reformatting the tree, if it happens, is
  its own commit with its own CI gate, not a rider on a licensing PR.

**Dependencies.** Six majors were behind and were raised: `toml` 0.8→1, `directories` 5→6,
`indicatif` 0.17→0.18, `sha2` 0.10→0.11, `zip` 2→8, `clap_mangen` 0.2→0.3, plus every in-range
update. **`reqwest` 0.12→0.13 was deliberately deferred**: 0.13 renamed `rustls-tls` to `rustls` and
split the feature set, which changes how TLS roots are selected — and JII's release binaries are
static musl builds where getting that wrong breaks every download silently. That migration needs to
be verified against an actual release build, so it gets its own change.

**Consequences.** CI now resolves dependencies under a 1.88 floor, so `cargo add` picks
MSRV-compatible versions on its own. `zip` 2→8 and `sha2` 0.10→0.11 are on the GitHub-release
install path (extract, then verify); both are covered by existing unit tests that exercise the real
round trip rather than only compiling. The rest of PR #12 — SPDX headers, `LICENSES/`, `REUSE.toml`
— is good work and is left to land as its own PR.

---

## ADR-0085 — Elevation is two questions, not one: are we root, and what is installed here

**Status.** Accepted (2026-09-05).

**Context.** The owner ran the tester checklist on five distros in phone containers. On Arch the
REAL-install step failed outright:

    Install htop (3.5.3-1) via pacman  [needs sudo]
    x failed to run sudo: No such file or directory (os error 2)

The session was `root` and the image had no `sudo` at all. `Platform::elevation_kind` answered from
exactly one fact — is there a TTY — and returned `Sudo` or `Pkexec`; neither our own uid nor the
presence of the helper entered into it. So JII prefixed `sudo` onto a command it could have run
directly, and reported the failure as a spawn error two layers below the decision that caused it.

**Decision.** `ElevationKind` answers from two facts: the effective uid, and which helper actually
exists on this host. It gains three variants:

- **`AlreadyRoot`** — uid 0. The command runs bare. Being root is the *easy* case; requiring a
  helper there is the bug.
- **`Doas`** — `sudo` absent but OpenBSD-style `doas` present (Void, Alpine, Artix).
- **`Missing`** — not root, and none of sudo/doas/pkexec. Refused **in words**, before the first
  privileged step runs, via a new `JiiError::NoElevation` with a remedy — rather than as a spawn
  failure halfway through a run that already printed "Installing…".

Order: root first; then, on a TTY, `sudo` → `doas` → `pkexec` (ask in the terminal the user is
already looking at); with no TTY, `pkexec` first (it can raise its own prompt) and the terminal
helpers after. The chooser is a pure function of `(euid, is_tty, has_binary)`, so the whole matrix
is unit-tested without a container per case. The uid is read from `/proc/self/status` — Linux-only,
like JII, and it avoids taking a `libc` dependency for one number.

The plan preview stops saying `[needs sudo]` unconditionally. It names what will really be used:
`[needs doas]`, `[as root]`, or a bare `[needs root]` where nothing can grant it.

**Alternatives.** *Probe `sudo` and fall back on failure* — the failure arrives mid-install, after
output has been printed and possibly after other steps have run; and it cannot distinguish "no
sudo" from "wrong password". *Require root for the whole process* — squarely against JII's rule
that it is never fully run as root.

**Consequences.** Root containers — the common case for testing and for CI images — install without
a helper. Void and Alpine work through `doas` without configuration. A host that genuinely cannot
elevate says so in one sentence with a way forward, instead of an errno.

---

## ADR-0086 — Unbounded work, done in silence, is indistinguishable from a hang

**Status.** Accepted (2026-09-05).

**Context.** Three symptoms from the same tester round, which turned out to be one idea:

1. **Gentoo hung** on the total-miss step. No output, no spinner; the tester pressed Ctrl+C. It was
   never reached the state where anything gets printed.
2. **A Fedora install on a phone printed several hundred progress lines**, each cut mid-escape,
   instead of one bar being redrawn.
3. Even on a healthy Fedora desktop, a name that resolves nowhere took **21 seconds**, most of it
   with nothing on screen.

**Decision.** Three bounds, and one rule about the terminal.

- **`is_available()` gets a timeout, a PATH pre-check, and a memo.** Thirteen of its fourteen call
  sites ran the tool with no timeout at all, and `emerge --version` loads the whole Portage stack —
  on a cold tree, longer than anyone waits. `which` now looks on `PATH` first (most managers are
  absent on any given host, and that costs no process), bounds the run at 3s with kill-on-drop, and
  memoizes per process. **A tool that is on `PATH` but too slow to answer counts as present**:
  calling it absent would silently drop the host's own package manager out of every search. The T6
  bootstrap path invalidates the memo after installing a manager, so "is it here *now*?" stays live
  where that matters.
- **Broadening gets a wall-clock budget.** A total miss ran one prefix round, two stem rounds and
  up to sixteen typo variants — nineteen full fan-outs over every source, sequentially. The stages
  are ordered best-guess-first, so a budget of `2 × network.timeout_secs` drops the tail nobody was
  going to wait for, not the useful part. Checked *between* rounds only; a round already under way
  keeps its own per-source timeout.
- **The progress line may never exceed the terminal width.** `\r\x1b[2K` erases only the row the
  cursor is on. A line that wraps leaves its earlier rows on screen, and at ~11 repaints a second
  the debris is immediate — which is exactly symptom 2, from a ~100-character package list on a
  ~40-column phone terminal. The spinner reserves what the bar needs and trims its *label*: the bar
  is the thing being watched, so the label is what gives. `render_bar` owns the invariant "never
  wider than the budget given" and degrades — full tail with the step counter, then the percentage
  alone, then the number with no bar — instead of overflowing.
- **And the terminal keeps talking.** The "Searching…" spinner stopped before the broaden pass
  began, so the slowest half of a miss was also the silent half. It now says it is looking wider.

**Alternatives.** *Kill the slow tool outright* — a manager that is merely heavy is not broken, and
dropping it makes JII wrong rather than slow. *A global deadline on the whole command* — too blunt:
a real install of a large package legitimately takes minutes.

**Consequences.** A total miss is bounded at roughly `prefix round + 2 × timeout + one round in
flight` — about 15s on the default 5s timeout, where it was unbounded. Repeated availability
questions within a run are free. And the standing UX rule gets its sharpest statement yet: **a
quiet terminal must never look hung** — not because silence is unfriendly, but because the tester
could not tell the difference between "working" and "wedged", and chose Ctrl+C.

---

## ADR-0087 — Achievements are earned quietly

**Status.** Accepted (2026-09-05).

**Context.** Every command that granted a badge printed `✓ Achievement unlocked  🩺  House Call`
inline. The owner's verdict: "убрать эти сообщения… Выглядит слишком нейросетно и детски. Пусть
ачивки будут только внутри команды JII achievements."

**Decision.** `grant_achievement` no longer prints anything. Badges are still awarded and persisted;
`jii achievements` is where they are seen. The boss fights keep their toast (`grant_boss`): there
the badge *is* the payoff of something the user deliberately went looking for, not an interruption
of work they came to do.

**Alternatives.** *A `--no-achievements` flag* — a setting to fix output nobody asked for is worse
than not printing it. *Print at the end of a run* — still noise in the middle of an answer.

**Consequences.** `doctor`, `search` and `how` output is that much shorter, which is the direction
the owner wants overall ("минималистичнее"). The one cost is discoverability, covered by the
command's presence in `--help` and by the badges being there whenever the user does look.

---

## ADR-0088 — A manager that can't be set up must not take the package down with it

**Status.** Accepted (2026-09-05).

**Context.** On openSUSE and Gentoo the tester's install step ended:

    htop is available via Snap, which isn't set up on this system yet.
    ✗ Couldn't find a package to set up Snap on this system — skipping Snap.
    Skipped htop.
    (exit: 0)

His note: "Я нихуя не понял что тут произошло." Three faults. The message passed `app = manager`,
so it named Snap where the app belonged, then contradicted itself one line down. Nothing said what
to do next. And a run that installed nothing reported success.

**Decision.** The runner-up candidates are kept alongside the winner (index-aligned, best first)
and handed to the bootstrap step. When a manager cannot be set up — or the user declines — JII
walks down the ranking for a source that works *on this machine* and says plainly that it is doing
so. **Unverified sources are never eligible for this fall-back**: automatic promotion of a
name-squat is precisely what the trust barrier exists to prevent, and on the tester's own box the
unverified `htop` on crates.io is an HTML-to-PDF converter. Where nothing qualifies, the app is
still skipped — but with the command that would set the manager up by hand and a pointer at
`jii search`.

Exit status distinguishes intent from failure: "JII offered and could not" is a failure and exits
non-zero; the user answering "no" stays a plain zero, because a decision is not a failure.

**Alternatives.** *Re-run the whole search restricted to installed sources* — a second fan-out for
information already ranked and in hand. *Fall back to anything at all, including unverified* —
faster to write and wrong; see the crates.io `htop`.

**Consequences.** The install flow carries one more parallel vector through one function. In
exchange, the most confusing outcome in the whole checklist becomes a sentence that says what
happened, what JII did instead, and what the user can do — and a run that installed nothing can no
longer claim success.

---

## ADR-0089 — JII speaks in sentences; the machinery goes underneath

**Status.** Accepted (2026-09-06).

**Context.** The owner's complaint about the product, in his own words: it must not look "как
типичный пакетный менеджер который пишет сухо и прячет важную информацию среди тонны текста
лишнего." Two screens proved him right. `jii search htop` printed six aligned rows in which the
official Fedora package and an HTML-to-PDF converter that merely shares the name looked equally
plausible. `jii doctor`'s suggestions printed one line per entry, each carrying title, rationale,
a `·`, the command, and — wrapped below — a caveat; at 200 columns the command that actually does
the thing sat past the right edge of most terminals.

Four candidate voices were mocked up and put to him. He rejected the framing of the prose one —
"говори тут не как человек к человеку а как просто программа. Не нашёл например а найдено" — and
then added the constraint that decided the design: "И должен быть ВСЕГДА ВЫБОР."

**Decision.** Five rules, and `src/ui/story.rs` is the only place that knows how to obey them.

1. **Impersonal.** "Found six", "Will install", "Done" — never "I found", "I suggest". A program
   claiming a first person is the tell of something pretending to be a person.
2. **There is always a choice.** Wherever JII decided something on the user's behalf, the
   alternatives are numbered on screen and the prompt takes a number (`prompt::decide` → `Pick`).
   This replaced the arrow-key chooser: a menu answers *which line*, prose answers *why*.
3. **What matters is prose; the rest is dim.** The reason a source won is a sentence, wrapped to
   the terminal. Versions, ids and trust words are indented and quiet.
4. **Quiet by default.** One live line while working, one line of outcome after. Nothing is said
   twice: when the offer already told the story, the friendly preview is suppressed.
5. **Never a dead end.** The next command is always on screen.

The reason a source wins or loses is asked of the source: `Provider::nature() -> SourceNature`
(system-native, community-repo, sandboxed, self-contained, built-from-source, language-registry,
upstream-binary), and presentation maps the *character* onto words. So JII can say "snap has the
newer version but carries its own runtime" while the core still never writes `if source == "snap"`
(ADR-0004). The trait method has **no default**: a new provider must decide what it is, and the
compiler asks.

Counted phrases are three keys (`.one`/`.few`/`.many`) resolved by the active language's plural
rule (`tn!`). "Найдено 3 вариантов" is the single most obvious sign of a translated program.

**Alternatives.** *A verdict card in box-drawing characters* — striking at 80 columns, broken at
40, and it hides the alternatives it is supposed to offer. *A denser table with a visual trust
weight* — still a table; the reader still has to know what "community" costs them. *Three-line
minimalism* — respects the expert and abandons everyone else, and it takes the choice away.

**Consequences.** `candidate_line`, `show_alternatives`, `Palette::mark_star` and the arrow-key
chooser for install are gone; `search`, `install`, `info` and `doctor` render through one module,
so a source reads identically wherever it appears. `jii search` now offers to install what it
found — a search that answers the question should not make the user retype it — and is asked only
on a real terminal, so a piped `jii search` stays a question. Every new provider costs one more
decision, which is the point.

---

## ADR-0090 — The recommendation catalog carries its own translations

**Status.** Accepted (2026-09-06).

**Context.** User-facing strings live in `locales/*.toml` (ADR-0050) and a test enforces en/ru key
parity. `data/recommend/catalog.toml` predates that rule and is *data*: eighteen entries whose
`title`, `why` and `note` are prose. Nothing translated them, so a Russian session printed a
Russian program listing English advice — "Play the video and audio formats Fedora omits by
default" under the heading «Звук и видео».

**Decision.** The prose travels with the entry: `title_ru`, `why_ru`, `note_ru` alongside the
English fields, and `Recommendation::title()/why()/note()` resolve against the active language
with English as the fallback. A test asserts every entry carries the Russian rendering, and that a
note exists in both languages or neither — the catalog's half of the parity guarantee.

**Alternatives.** *Move the prose into `locales/*.toml` keyed by entry id* — splits one entry
across three files, and the catalog stops being readable as the curated document it is; a
contributor adding a distro's entries would have to touch two locale files to say one thing.
*Leave it English* — the shape was fixed and the words were still foreign.

**Consequences.** Adding a catalog entry now means writing it twice. That is the price of the
guarantee, and it is the same price `locales/*.toml` already charges.

---

## ADR-0091 — Topics: answering the concept, not the string

**Status.** Accepted (2026-09-06).

**Context.** The owner's ask: `jii search "markdown"` should suggest Obsidian. It did not — it
answered with an npm library literally named `markdown`, because every provider searches by name
and the query happened to be one. `jii search браузер` was worse: no source anywhere carries a
Russian word as a package name, so the answer was "No candidates found" for a question with an
obvious answer.

**Decision.** `data/topics.toml` maps a concept to the programs that answer it: a list of terms
someone might type (in every shipped language) and a short, curated `picks` list. `jii search`
consults it *after* the literal search and answers with the topic when one matches. The literal
hits are never discarded silently — a line names them and `--exact` skips the layer entirely. An
install that resolves nothing points at the topic search instead of dead-ending on two
search-engine URLs.

The gate is one rule: **the topic answers unless the query is itself one of its picks.** `docker`
and `steam` are terms of the container and gaming topics, but someone typing them named a program
and gets that program. Everything else — including a package that merely carries the word, like
npm's `markdown` — is exactly the collision this exists to route around.

`picks` order is the curator's order and is *not* re-ranked: a topic is a curated reply ("for
markdown, people install Obsidian"), and re-ranking it by source priority turns it back into
whatever the registries happen to hold. Within one program the usual ranking still decides which
source wins, and the resolution is bounded by the same budget as `broaden_search` (ADR-0086).

Term matching is whole-query equality, not substring: a substring rule makes `jii search vim`
answer "virtual machines" (`vm` is a term). When JII cannot be sure what someone meant, saying
nothing is honest — the literal results are already on screen.

**Alternatives.** *Semantic search over package descriptions* (embeddings, or `dnf search --all`)
— the general answer, and a bad first one: it needs a model or a per-source description index, it
answers differently on every distro, and its failures are unexplainable. A curated table is
inspectable, reviewable in a pull request, and translatable. *Only description search* — `dnf
search --all markdown` returns forty libraries before it returns an editor. The two can coexist
later; the curated layer is what makes the common questions right today.

**Consequences.** A hand-maintained list that will always have gaps, and every topic must be
written in both languages. In exchange the questions people actually ask — "браузер", "заметки",
"запись экрана", "markdown" — get the answer a knowledgeable friend would give. Reverse-DNS app
ids are shown by their last segment (`md.obsidian.Obsidian` → `Obsidian`) in display only, since
a column of addresses is not a list of names. And identical offers from one source are now
collapsed in ranking — Fedora's `steam` appeared five times, once per repository carrying it.
