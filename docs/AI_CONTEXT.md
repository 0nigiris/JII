# JII — AI Context (Current State)

> **Purpose:** the single-page current state of the project, so any agent (AI or
> human) can pick up development in under five minutes. This file describes **only
> the present** — no history. History lives in git; decisions in
> [DECISIONS.md](DECISIONS.md); the plan in [TASKS.md](TASKS.md).
>
> **Keep this file current.** Updating it at the end of every session is mandatory
> (see the AI Handoff Policy in [CLAUDE.md](../CLAUDE.md)).

_Last updated: 2026-08-14_

---

## Most recent work (2026-08-14, batch 11) — read this first

**Owner feedback round #4 — RELEASED as `v0.1.11-beta` (tag pushed; CI builds the artifacts).**
Committed + pushed. The owner's live secret-installer run surfaced that the *released* binary must
carry the achievements code for `jii achievements` and the `sans` sentinel-unlock to work — hence this
release. Known follow-up the owner raised: the achievements ledger is trivially forgeable (plain JSON +
the sentinel file); add lightweight tamper-detection (an HMAC keyed in the binary → a snarky reset on a
mismatch) as deterrence, not real security (a local file can always be forged).

- **Installer header: bordered, centre-aligned tagline card + download spinner (owner: "A and B, text
  dead-centre").** `install.sh`'s `banner` now draws the ASCII JII cube beside a rounded box (`╭─╮`,
  `╾─` connector to the logo, ASCII `+ | <-` fallback) whose title + two tagline lines are exactly
  centred via `_center`/`_repeat` (ASCII text → `${#…}` is the true width). New `_spin_wait` +
  `dl_progress` wrap the *actual* package download in a braille spinner (`⠋⠙⠹…`, ASCII `|/-\`), so a
  slow network no longer looks hung; inert with no TTY, propagates the download's exit status. Install
  logic untouched; `sh -n` clean, rendered + status-propagation verified. **Still must be pushed to
  `master` to test the `curl … | sh` live.**
- **Achievements subsystem (ADR-0072) — first half of the owner's "secret Sans-fight installer".** New
  `src/achievements.rs`: a cosmetic JSON ledger (`$XDG_STATE_HOME/jii/achievements.json`) of
  `{id → unlocked_at}`, a static `CATALOG` (`first-install` 🌱, `doctor` 🩺, secret `sans` 💀), localized
  titles/descs (`achieve.<id>.title|desc`, en+ru). `jii achievements` (alias `achievement`) lists
  progress; secret+locked shows `???` (and `null` in `--json`) — never spoiled. `grant_achievement`
  is best-effort (swallows all errors, silent in JSON). Wired: `first-install` on a successful install,
  `doctor` on `jii doctor`. `sans` is granted via a **sentinel file** (`…/jii/secret-install`) the
  future secret installer drops, consumed once by `Achievements::take_sentinel` in `run()`. Verified
  end-to-end (sentinel → toast → `1/3`). 4 unit tests.
- **304 tests, clippy clean.** `cargo build`/`clippy`/`test` all green.

- **Secret Sans-fight installer (ADR-0073) — DONE, on the orphan `secret` branch.** `curl …/secret/
  secret_install.sh | sh` downloads a self-hosted fork of the Bad Time Simulator (`game.tar.gz`,
  2.8M incl. `media/*.ogg` audio), serves it via a python3 one-shot local server (token-gated
  `/claim`, self-terminating via `os._exit`), `xdg-open`s it, and on victory drops the
  `secret-install` sentinel (ADR-0072) then runs master's `install.sh`. The bundled `index.html` is
  a cleaned copy of the deployed page (Yandex SDK/ads/gtag/service-worker stripped, no-op stubs for
  `ShowAd()`/`ysdk.*`) + a poller that reads the C2 runtime's Text objects for Sans's "you win" line
  — **no game code patched**. Honest fallbacks (no TTY/DISPLAY/python3/browser → normal install).
  Verified headlessly (server/token/claim/shutdown, sentinel, no-TTY fallback) and in a real browser
  (boots to MainMenu, audio decodes clean, poller detects an injected win on the live runtime). The
  `secret` branch is **local only — not pushed yet** (needed for the live `curl` test). A real human
  playthrough is the owner's to run.

### Next (owner to steer)
Owner rounds #3/#4 items still open: the **codec re-offer bug** (needs a live Fedora-VM `jii doctor`
run to see the real failure) and refreshing `jii-test-guide.html`. Otherwise back to the beta-freeze
priorities below.

---

## Previous work (2026-07-26, batch 10)

**Post-`v0.1.10` polishing round (owner feedback, not yet released).**
Working tree is clean/green; **not tagged**. (The install.sh banner from this batch was superseded by
batch 11's boxed version above.)

- **Branded `install.sh` (owner: "copy Hydra's installer look for us").** The `curl … | sh` output
  now leads with an ASCII render of the JII cube logo + a tagline, section rules, a decorative
  progress bar on completed download steps (`ok_bar`), and a "JII is ready / Run / Uninstall /
  Docs·Issues" footer. Pure presentation — a `Presentation` block of shell helpers (`banner`, `ok`,
  `bullet`, `ok_bar`, `rule`, `warn`, `done_footer`) gated on `[ -t 1 ]` (colour) and a UTF-8 locale
  (glyphs), ASCII fallback otherwise. Install *logic* is untouched. `sh -n` clean; previewed.
  **To let the owner test it live it must be pushed to `master`** (install.sh is served from the raw
  master URL). Not pushed yet — awaiting the owner's go.
- **Progress bar fills the terminal width (ADR-0069 follow-up).** `render_bar` took a fixed 16 cells;
  it now takes a `budget` and stretches the bar to the live terminal width (`crossterm::terminal::size`
  re-read each frame, so it re-fits on resize) like dnf/pacman, reserving the `NN% [d/t]` suffix and a
  right margin, with a `MIN_BAR_CELLS` floor for narrow terminals. 5 `render_bar` tests.
- **Never present untrusted as "recommended" (ADR-0071).** New `ranking::recommended_index` = the
  first candidate that is not `Untrusted`/`suspicious`; the install chooser stars *that* (and defaults
  the cursor to it), and when nothing trusted matches it stars nothing and warns
  `install.no_trusted_match` instead of crowning a name-squat. Fixes the owner's `jii google` report
  (an untrusted `google` crate was starred "recommended"). New locale key (en/ru), 1 test.
- **Spinner on `doctor`'s silent `makecache` wait.** `refresh_repo_metadata` now animates a `Spinner`
  around a *captured* `dnf5 makecache` (was a bare, silent `run_plain_command` that looked like a
  hang). sudo/inherited-output steps keep their own output + intent line (a spinner would fight them).
- **300 tests, clippy clean.** `cargo build`/`clippy`/`test` all green.

### Owner items still OPEN this round (do next)

1. **Codec re-offer bug (`jii doctor`).** Owner: doctor offers Multimedia codecs a 3rd time and asks
   again after "installing" them — and "why didn't you install?". `doctor` treats a suggestion as done
   only when *all* its packages show in `dnf repoquery --installed`, so a re-offer means the codec
   install actually **failed** and the message was lost. Root cause needs the **live run output** from
   the owner's Fedora VM (dev host can't reproduce the failure). Also worth: make a failed
   `apply_suggestion` say so explicitly instead of propagating a bare error.
2. **`file:///home/oni/Downloads/jii-test-guide.html`** — owner wants the tester guide refreshed
   (deferred until the codec fix lands, to avoid rewriting it twice).

---

## Previous work (2026-07-25, batch 9) — released `v0.1.10-beta`

**Two changes, ADR-0069/0070.**

- **Live progress bars (ADR-0069).** Friendly-mode installs/updates/downloads now draw a real bar
  with a percentage read from the source's own output — no source-branching (ADR-0004).
  - New `src/progress.rs`: `parse_progress(line) -> Option<Progress>` reads two universal shapes only —
    a bracketed `[done/total]` counter (preferred, monotonic) or a bare `NN%`. Strict bracket parsing
    rejects dates/prose ratios. Unit-tested (dnf5 `[ 3/41]`, pip `(1/5)`, download `%`, negatives).
  - `Privilege::run_streamed` (replaces `run_captured`, removed): spawns piped stdout+stderr, reads
    both concurrently line by line, feeds each to a callback as it arrives, still returns
    `(success, combined)` for the error tail / update summary. stdin is null (plans pass `-y`).
  - `Spinner` grew `reporter() -> ProgressReporter` (a cloneable shared-reading handle) and draws
    `████░░░░  45%  [3/41]` when a reading exists, elapsed time otherwise. `render_bar` unit-tested.
  - Wiring: `exec::run_actions_quiet` streams `RunCommand`s and uses a chunk-streaming
    `download_reported` (exact byte % via `reqwest` `bytes_stream`; needed the new `stream` feature)
    for `Download`. `engine::run_plan_streamed` (replaces `run_plan_captured`) does the same for the
    whole-system update so `jii update` shows per-source bars too.
- **Flatpak update-all fix (ADR-0070).** `plan_update_all` dropped `--user`: `flatpak update -y`
  updates **every** scope, so `jii update` now refreshes the system-wide apps a desktop store
  installed under `/var/lib/flatpak`, not just per-user ones. Fixes the owner's report ("update said
  done, but KDE Discover still lists a pile"). Still root-free for JII — flatpak's own polkit handles
  the system portion. Install/uninstall/single-update stay `--user` (JII only tracks its own installs).
- **297 tests, clippy clean.** `cargo build`/`clippy`/`test` all green.

### Known bug found in batch 9, still NOT fixed (candidate for next release)

`jii install <name>` can **dead-end** when the top-ranked candidate comes from an ecosystem manager
that isn't installed (e.g. Snap) *and* bootstrapping that manager fails/declines: the app is dropped
with "Skipped <name>" even though working alternatives (cargo/npm/pipx/brew) were just listed as "Also
available". Root cause: the install loop keeps only the single `best` per package
(`cli/mod.rs` ~L654) and discards the ranked alternatives before `bootstrap_missing_managers` runs, so
a failed bootstrap has nothing to fall back to. Proper fix = thread the ranked alternatives through and
retry the package with the next candidate whose manager is available (respect "never dead-end"). Not
done here to keep the v0.1.10 release focused/stable; reproduced on the dev host with
`jii install ripgrep --dry-run`.

---

## Previous work (2026-07-16/17, batch 8)

**Full-project audit → fix-everything wave (owner: "Вообще всё"), plus three features (ADR-0067/0068).**

- **P1 fixes.** The github source now resolves **prerelease-only repos**: `latest_release` fetches
  `/releases?per_page=20` and `pick_release` prefers the newest non-draft release *with assets*
  (`/releases/latest` 404s when a repo has only prereleases). `jii update` returns an error naming
  every source whose bulk update failed (was: silent exit 0); self-update errors when the release
  check fails.
- **P2 fixes.** Remove-chooser and forge errors fully localized (new `remove.*` keys); `@ref` pins
  (`pkg@1.2`) recognised by `route_managers`; `jii sources --json` emits a stable schema (explicit
  nulls for disabled rows); Flatpak install plans idempotently prepend
  `flatpak remote-add --user --if-not-exists flathub …` so a fresh host works without setup.
- **P3 fixes.** Russian `д/н/да/нет` accepted everywhere `y/n` is; the interactive chooser scrolls
  on short terminals (window + ↑/↓ edge markers, mouse maps through the window); `jii how` prints
  **every** installed copy of a name via new `Registry::get_all`; `record_remove` matches
  name+source; the search cache prunes entries >30 days at save; self-update warns when the
  published tag parses **older** than the running version (`selfupdate::looks_like_downgrade`);
  `install.sh` no longer prints a spurious `curl: (23)` while resolving the tag.
- **Junk-package filter (ADR-0067).** `ranking::mark_suspicious` runs before sorting: a *community*
  candidate from a *network registry* (provider `can_search()`), non-path-style name, is demoted to
  **untrusted** when it looks like a name-squat — popularity below 1 000 (cargo `recent_downloads`,
  npm last-month downloads), or, with no popularity signal, no summary / `0.0.x` version, or a
  provider pre-mark (`suspicious`: pipx marks a sole release >5 years stale). Result: red
  `install.suspicious` warning, auto mode never picks it; a hard block was rejected — the owner
  wants a loud warning, not a dead end. `PackageCandidate` gained `popularity`/`suspicious`
  (serde-defaulted). Verified live: `jii search htop` shows pipx/cargo red; `htop:pipx --dry-run`
  warns.
- **Hidden tester checklist.** `jii yes-I-am-dev-and-want-to-test` (`src/devtest.rs`,
  `#[command(hide = true)]`, absent from README/--help, English-only by design): 12 scripted steps
  (doctor, search incl. junk heuristics, info, **real** htop install, list/how, update, **real**
  remove, dead-end on a nonexistent name, `npm@1.0` rejection, sources). Per-step expectation text +
  `[Y/n/s]` verdict; everything logged to `jii-test-YYYYMMDD-HHMMSS.log` with **username/hostname
  scrubbed**; one-key upload (0x0.st multipart → paste.c-net.org fallback); prints a **pre-filled
  GitHub issue link** with the PASS/FAIL table; exits non-zero if any step FAILed. Tester guide in
  **docs/TESTING.md**. reqwest gained the `multipart` feature for the upload.
- **Windows/macOS: plan only, NO code (ADR-0068).** Three waves, macOS-first (brew exists →
  cheapest credible port): A = macOS via `platform` abstraction + brew/cask; B = Windows via
  winget/scoop (needs privilege + path rework); C = parity polish. Roadmap also gained landing-page
  and launch-content bullets (Future).
- **Doc drift fixed:** CLAUDE.md now says the declarative-TOML source layer is *planned, not built*.

**285 tests, clippy clean.** Pushed to master and **released as `v0.1.9-beta`** (tag pushed
2026-07-17; `release.yml` builds the artifacts — refresh `packaging/aur/PKGBUILD` from them, same
debt as every tag). Afterwards (same day): **docs/JII_EXPLAINED.ru.md** — a single-file, owner-facing
full explanation of the project in Russian (every module, decision digest, event-ready FAQ; the
owner is presenting JII at Hack Club Star Dance / Beest), plus a README polish round (17-source
count fixed, stats strip under the badges, CI badge, bootstrap/junk/locale/--run bullets,
declarative-layer drift fixed, v0.1.9 tarball example, honest live-testing status).

**Post-release debt (carried):** refresh `packaging/aur/PKGBUILD` `pkgver` + `sha256sums` from the
published v0.1.8-beta tarballs; `graphify-out/` sits untracked (owner to choose: .gitignore or drop).

---

## Previous work (2026-07-15, batch 7)

**Owner testing round on v0.1.7-beta (Fedora + an apt host) — ten reports, all landed (ADR-0066).**
The owner is actively testing across Fedora/Ubuntu/Arch/openSUSE/Nix/Gentoo/Void and reporting; expect
more. Fixed this batch:

- **Bootstrap only via a source that works.** `jii htop:pipx` on a pipx-less box offered a chooser
  headed *"install pipx via pipx"* (same for npm). `Engine::first_available_package` →
  **`first_bootstrap_package`**, which resolves the package *and* its source, considers only sources
  that are `is_available()` right now, and pins it (`pipx:dnf`) via the ADR-0031 spec grammar. No
  chooser, no self-bootstrap.
- **brew/nix run their own script, with consent.** Was shown-never-run (a dead end: no distro package
  exists, so the script *is* the install path). Now shown in full + confirmed, **default yes**;
  `--auto`/`--yes` do **not** consent for it and a non-TTY only prints it (CLAUDE.md's untrusted-auto
  rule). `bash -c`, never elevated. Shared helper `Cli::offer_script_bootstrap`.
- **Progress you can see.** New **`ui::Spinner`** (stderr, erases itself, elapsed seconds past 3s;
  inert without a TTY / in `--json` / in Advanced). `exec::run_actions_quiet` gives install/remove/
  update captured-output-behind-a-spinner (what update-all already had); remove's preview is now one
  line per package like install's; Friendly's "Searching…" is a spinner. Failures still print the
  failing command + a tail of real output; `--dry-run`/Advanced unchanged.
- **`--run`** (global flag): start it after install, and on an already-installed package just start
  it. New **`Provider::launch_command`** — default is the package's own name, Flatpak overrides with
  `flatpak run <app-id>`; the core assembles nothing. Verified to exist before running (a font says
  "can't tell what to run" instead of guessing), then `exec`s.
- **`exec::changed_count` counted per line.** It searched the whole blob for the first "upgraded",
  landing on apt's *"The following packages will be upgraded:"* prose, so **every** apt update
  reported a bare "updated". dnf5's transaction summary counts too; apt's "N not upgraded" doesn't.
- **`jii sources` lists sources you disabled** (they're dropped from the provider registry, so the
  view could never show them) with each one's enable command + a footer naming disable/enable.
- **`jii man`** formats through `man(1)` at a terminal; still raw roff when redirected (packaging).
- **`jii providers` removed** — the deprecated hidden alias that duplicated `jii sources`.

**276 tests, clippy clean.** Verified live on Fedora: `sources` disable/enable view, `htop:pipx` and
`htop:brew` dry-runs, npm install/remove/`--run`, `--run` on a font and on a batch, `man -l` render.

**Release cut: `v0.1.8-beta` (2026-07-15).** Version bumped in `Cargo.toml`, `Cargo.lock`,
`packaging/jii.spec` (`%_tag` + `Version` + changelog); annotated tag `v0.1.8-beta` pushed →
`release.yml` builds/publishes the x86_64/aarch64 musl binaries + `.tar.gz`/`.deb`/`.rpm`.
**Post-release debt (same as every prior tag):** refresh `packaging/aur/PKGBUILD` `pkgver` +
`sha256sums` from the published v0.1.8 tarballs once CI uploads them.

**Known, not fixed (owner asked "why?", answered not coded):** registry sources return exact-name
junk — `htop` on PyPI is an unrelated "1st training project", so `jii htop:pipx` installs it and pip
fails. Ranking already puts dnf/apt first, so it only bites on an explicit `:pipx`/`:cargo` pin. A
relevance heuristic for registry name-squats is unscoped.

---

## Previous work (2026-07-13, batch 6)

**Two owner-reported Fedora bugs fixed (ADR-0064).**
- **`jii doctor` no longer lists foreign distros' native managers.** `Engine::diagnose` now applies
  the same relevance predicate `jii sources` already uses — factored into a shared
  `source_relevant(available, provider)` (`available || can_search() || ecosystem().is_some()`) in
  `engine/mod.rs`. A source that can neither run here nor be bootstrapped is **skipped**, so on Fedora
  `doctor` shows dnf/copr/flatpak/github + the cross-distro ecosystem managers — **never**
  apt/pacman/aur/zypper/void/gentoo (symmetric on other families). Pure capability, no source-id branch.
  `doctor` and `sources` now agree.
- **Codec setup no longer reports "not found" after enabling RPM Fusion.** Root cause: the just-added
  repo had no local metadata, so the dependent install queried a stale `dnf5 repoquery` cache. Fix:
  `doctor_offer` calls the new `refresh_repo_metadata` (best-effort non-root `dnf5 makecache`, guarded
  on dnf5, skipped in dry-run) right after enabling a prerequisite repo, before installing the
  dependent. New locale key `doctor.refreshing_meta` (en+ru).
- **Then landed #2/#5/#7 + closed #14 (2026-07-13).** #2 browse links (GitHub + Flathub search) on a
  total miss (`url_query_encode`, unit-tested); #5 real config path appended to `jii --help`
  (`main::parse_cli` dynamic `after_help`); #7 GitHub-token guidance in `jii doctor` when no token is
  configured; #14 closed by design — Flatpak is all `--user`, never `needs_root`, so no password
  detection is needed. **272 tests, clippy clean.** Brand: `assets/banner.png` + `assets/icon.png`
  added, banner shown in the README header.
- **#13 / T6 bootstrap landed (2026-07-13, ADR-0065).** `bootstrap_missing_managers` (`cli`) runs on the
  chosen set before planning: a candidate from an **uninstalled** ecosystem manager (Flatpak/Snap/cargo/
  npm/pipx/go — `can_search` without their CLI, so they outrank the last-resort GitHub binary) prompts
  "set up {manager} and install {app}?" (default yes, once per manager), installs the manager via the
  normal path, adds Flatpak's Flathub remote, confirms it landed (`Engine::source_available`), then the
  app installs through it. `Script` managers (brew/nix) stay show-only and skip the app. `--dry-run`
  previews both phases. 272 tests, clippy clean. Removed the dead `[bootstrap]` locale section.
  **T7:** live end-to-end on a manager-less host (this dev box has flatpak/cargo/npm/go installed).
- **Release cut: `v0.1.7-beta`.** Version bumped in `Cargo.toml`, `Cargo.lock`, `packaging/jii.spec`
  (`%_tag` + `Version` + changelog). Pushing the tag triggers `release.yml`. **Post-release debt (same
  as prior tags):** refresh `packaging/aur/PKGBUILD` `pkgver` + `sha256sums` from the published v0.1.7
  tarballs once CI uploads them.

---

## Previous work (2026-07-12, batch 5)

**AUR provider + `jii sources` redesign (ADR-0062).**
- **AUR provider** (`provider/aur.rs`, id `aur`, Community) — **Arch-family only.** New
  `Platform::arch_like` (from `/etc/os-release` `ID`/`ID_LIKE`, derivative-proof via the `arch`
  token) gates every entry point *plus* a helper (paru/yay) being present. Searches the AUR RPC v5;
  installs/removes/updates via the helper with `needs_root=false` (a helper escalates to pacman
  itself — the Flatpak-polkit precedent); `pacman -Qm` list. `search()`/`ecosystem()` both return
  empty/None off-Arch, so AUR never surfaces on Fedora/Debian. Ranked below Flatpak/Snap, above the
  language registries and github. Deliberately **not** `can_search`.
- **`jii providers` merged into `jii sources`** (providers is now a hidden alias). One view:
  ecosystem managers are annotated `[add: jii sources add <id>]` (missing) / `[remove: jii sources
  remove <id>]` (installed); system repos get no hint.
- **`jii sources add <id>`** = the old bootstrap (`yay`/`paru` show the manual `makepkg` install,
  shown-never-run). **`jii yay`/`jii paru`** likewise bootstrap a helper (routed in `route_managers`).
- **`jii sources remove <id>`** — uninstall an ecosystem manager. Reuses each ecosystem's
  `Bootstrap::Packages` as the OS package(s); detects the host system manager (`SysManager`:
  dnf/apt/pacman/zypper/xbps/portage), narrows to the package(s) actually installed (per-manager
  `pkg_installed` probe — never guess-removes a wrong name across distros), **shows the exact
  elevated command first**, confirms **default-no**, runs via `privilege.rs`. **System package
  managers are refused** (would break the OS); script-installed brew/nix → manual note; yay/paru →
  `pacman -Rs`. Removed the now-dead `Ecosystem.binary` field + `Palette::mark_bullet`.
- **271 tests, clippy clean.** Verified on Fedora: merged `jii sources` view; `remove dnf` refused;
  `-d` dry-run of `remove flatpak`→`dnf5 remove -y flatpak` and `remove go`→`golang`; `add yay`
  refused off-Arch. **T7:** live helper install/AUR search + real manager removal need an Arch host.

**Release cut: `v0.1.6-beta`** (2026-07-12). Version bumped in `Cargo.toml`/`Cargo.lock` and
`packaging/jii.spec` (`_tag`/`Version`/changelog); tag `v0.1.6-beta` pushed → `release.yml` builds and
publishes the x86_64/aarch64 musl binaries + `.tar.gz`/`.deb`/`.rpm` assets. **Post-release debt:**
refresh `packaging/aur/PKGBUILD` `pkgver` + sha256sums from the published tarballs (needs the release
assets to exist first — same two-phase flow as v0.1.5-beta's `dc3b73a`).

**`jii update` output polish (ADR-0063) — DONE.** Owner ran `jii update` and npm/flatpak flooded the
terminal. Now the whole-system update **captures** each bulk manager's output (`Privilege::run_captured`,
`Engine::run_plan_captured`) and renders one line per source — `  <source>  ✓ <headline>` + indented
notes — from `exec::summarize_update` (source-agnostic scanner: nothing-to-do / `changed N packages` /
`N upgraded` / `deprecated` count / `end-of-life` count; no source branch). Failures show `✗` + a short
output tail. The JII self-update GitHub check is now spawned **in parallel** with the system update
(`tokio::spawn(selfupdate::latest_release())`), so "Checking for a newer JII…" is near-instant. 271
tests (+3 summarize). **T7:** the live per-source summary needs a real update run to eyeball.

**Mid-word fuzzy + `-d`/`-n` — DONE.** `broaden_search` now has a 3rd stage that tries edit-distance-1
`typo_variants` (moved to `engine`, shared with the GitHub picker) as exact searches → `jii pipix`
resolves `pipx`. The `-d`/`-n` confusion is addressed by sharper help on all three flags + a `--preview`
alias for `--dry-run` (no behaviour change).

**Remaining owner backlog:** effectively cleared for the in-scope CLI items. Out of scope (owner):
Windows/macOS port and the GUI. Open T7 verification debts persist (live Arch AUR/helper install + real
manager removal + the live `jii update` per-source summary all need real non-Fedora hosts).

## Most recent work (2026-07-12, earlier) — read this too

**Cross-system testing round → a batch of fixes.** The owner personally tested every distro
except Gentoo/NixOS (report + screenshots in `~/Documents/suka/`) and filed ~26 issues (bugs,
UX, and architecture). Landed this session, most-critical first:

- **`install.sh` checksum bug (was breaking native install on Ubuntu + openSUSE).** nfpm writes the
  package's real version `0.1.5~beta` into the `.sha256` sidecar, but GitHub rewrites `~`→`.` in
  uploaded asset names, so `sha256sum -c` looked for a file not on disk and failed even though the
  bytes matched. Now verifies by **hash, not filename** (`verify_sha256` helper). Fix is live on
  `master` immediately (install.sh is served raw). Verified on the real released `.rpm` + `.deb`.
- **TTY/Unicode fallback.** `✓/✗/⚠/○` rendered as `▪` tofu on the Void live console (`TERM=linux`).
  `Platform::detect().unicode` (UTF-8 locale AND not a glyph-poor console) now drives `+/x/!/-`
  ASCII markers, centralised behind `Palette::mark_*`.
- **Prompt UX:** doctor's setup questionnaire now defaults to **yes** (`[Y/n]`, Enter accepts);
  destructive prompts stay default-no. `ask()` reads a **single keypress** (no Enter) via crossterm
  raw mode, with a line-input fallback.
- **`jii doctor` prints the config file path** (recurring "where's the config?" question).
- **`jii lang [en|ru|auto]`** — view/set the UI language from the CLI (writes `[ui] locale`);
  confirms in the newly chosen language via `i18n::tr_in`. `--lang` remains a per-run override.
- **First-run `jii doctor` now onboards.** A first-ever run that is `jii doctor` used to skip
  onboarding entirely (no mode choice, first-run left unmarked → next command re-onboarded). It now
  runs the wizard with `offer_doctor=false`, then doctor once. Other `setup` callers pass `true`.
- **GitHub ranked strictly last (pt.17 part A, ADR-0061).** `github` moved to the end of the
  default `priority` (below cargo/npm/pipx/go/brew/nix); every real package source is preferred.
- **pt.17 part B — bootstrap an uninstalled source before GitHub (ADR-0061, stages 1-2 DONE).**
  New `Provider::can_search()` (network search without the CLI): cargo/npm/pipx/go opt in (their
  search is already a registry API call), and **Flatpak searches Flathub's v2 API** when its CLI is
  absent. `Engine::search_one` now includes a `can_search` source even when uninstalled; on install,
  a chosen candidate whose manager isn't present prompts "install <manager> and get it there?" and
  bootstraps it first (reusing `bootstrap_ecosystem`). Verified end-to-end: `jii obsidian` on a box
  without Flatpak → offers Flathub `md.obsidian.Obsidian`, not a GitHub binary. Stage 3 (Snap/brew
  network search) is the only remainder. New `post_json_opt` helper.
- **Ranking: dotted app-ids match on their last segment.** `firefox`==`org.mozilla.firefox`,
  `obsidian`==`md.obsidian.Obsidian` are now *exact*, so the reverse-DNS name doesn't lose to an
  unrelated same-named crate/pypi package — fixes the openSUSE `jii firefox` "closest …" papercut,
  and the install "no exact match" note is suppressed on an app-id-tail hit. +2 ranking tests.
- **`apt` whole-system update runs `apt-get update` before `upgrade`** (was upgrade-only against a
  stale index). Two actions, one sudo (privilege layer batches them).
- **`jii cache [clear]`** — show / delete the on-disk search cache.
- **Install preview shows the candidate's web page** (`Provider::web_url`, cheap/sync): Flathub /
  crates.io / npm / PyPI / pkg.go.dev / snapcraft / GitHub-repo — "have a look first" before installing.
- **`install.sh` always speaks to PATH** after a portable install (confirm when already on PATH,
  else the add-to-PATH hint + full-path fallback); dropped its `⚠` glyph.
- **pt.17 part B stage 3 — Snap + Homebrew `can_search`** (api.snapcraft.io / formulae.brew.sh). Part B
  now covers every network-searchable source (cargo/npm/pipx/go/flatpak/snap/brew). `jii node:brew`,
  `jii hello:snap` verified bootstrap-first with neither installed.
- **`-s` short alias for `--source`.**
- **Flatpak installs are `--user`** (install/uninstall/update + the Flathub remote): never needs
  sudo/polkit, and no system-bus dependency — fixes the polkit prompt and the "Unable to connect to
  system bus" Flathub-remote failure on Void live.
- **Arch `jii doctor` suggestions** (VLC, GStreamer codecs+ffmpeg, Noto fonts, Steam-Flatpak) — the
  catalog was Fedora-only; title-uniqueness test is now per-distro.
- **`jii sources` hides other-distros' native managers by default** (`--all` to show). Pure-capability
  rule via new `SourceEntry.relevant` (usable | can_search | bootstrappable ecosystem) — no source-id
  branch; exactly pacman/apt/zypper/void/gentoo drop out on Fedora.
- **`jii sources disable|enable <id>`** — flip `[sources] disabled` (validated vs KNOWN_SOURCES); the
  provider registry already drops disabled sources, so JII stops considering it everywhere at once.
- **All remaining output glyphs are TTY-safe** (`⭐ ℹ ❯` → `* i >` via `palette.mark_*`), completing
  the Unicode fallback.

**Next open work (TASKS.md "remaining"):** finish the **`jii sources` redesign** — `remove --purge`
(deinstall the manager from the OS; **dangerous**, needs an ADR + per-manager removal commands) and the
full merge with `jii providers` into one view; `jii yay`/`jii paru` (an AUR-helper ecosystem —
Arch-specific, needs an AUR provider); richer fuzzy for mid-word typos (`pipix`→`pipx`; trailing typos
already work); `-d`/`-n` semantics unification. Windows/macOS + GUI remain explicitly out of scope.

<details><summary>Earlier the same day (2026-07-12) — "JII everywhere" packaging recipes (ADR-0060)</summary>

Prebuilt-binary recipes for every mainstream channel (all in `packaging/`, each a repack of the
release tarball, no compile): **`homebrew/jii.rb`**, **`alpine/APKBUILD`** (sha512 via `abuild
checksum`), **`void/template`**, **`gentoo/jii-bin-*.ebuild`**, **`nix/jii.nix`**. **crates.io
ready** (`Cargo.toml` metadata; `cargo publish --dry-run` clean → `cargo install jii`). Not
build-tested off-Fedora (dev host is Fedora-only) — validated-by-construction, each needs one real
build + the owner's account to publish. A shareable cross-system test guide was produced from
`docs/SUPPORTED_SYSTEMS.md`. No core Rust behaviour changed.

</details>

<details><summary>Previous session (2026-07-11) — install.sh native install + multi-arch spec</summary>

**"Install-easy" epic + a new steering directive.** Owner is running a cross-system testing round
(NixOS VM + friends on every distro) *before* any Windows/macOS port. Landed that session:

- **`install.sh` now does native installs (ADR-0059).** New `JII_METHOD=auto|native|portable`
  (default `auto`; also `--native`/`--portable`). `auto`: on an interactive terminal with a
  supported manager (`dnf`/`apt`/`zypper`) it **asks** (default native) whether to install the
  system `.rpm`/`.deb` via that manager or a portable binary to `~/.local/bin`; **no TTY (pipe/CI)
  → portable, never a surprise `sudo`.** Native asset URL is discovered from the release-by-tag JSON
  (arch+ext grep), `.sha256`-verified, and the exact `sudo … install` argv is printed before it runs.
  **Arch/`pacman` not wired yet** — its native path is the AUR (`jii-bin`), unpublished; pacman hosts
  get a note + portable fallback. Non-destructive paths verified on the Fedora dev host (syntax, URL
  discovery ×4, portable, auto-no-tty clean, auto-tty answered `n`); **live `sudo` install is T7 debt.**
- **Packaging bumped to v0.1.5-beta + made multi-arch/multi-distro.** `packaging/aur/PKGBUILD` (real
  sha256sums, pkgver `0.1.5_beta`). `packaging/jii.spec` is now **multi-arch** (both release tarballs as
  Source0/Source1, `%prep` `%ifarch` picks by target CPU) — one SRPM rebuilds correctly for **x86_64 and
  aarch64**, validated locally via `rpmbuild --target … --rebuild` + `file` on the packaged binary. This
  unblocks selecting **every** Fedora/EPEL(RHEL/CentOS/Rocky/Alma)/openSUSE chroot on COPR/OBS.
  `packaging/README.md` documents all channels: **AUR**, **COPR** (Fedora+EPEL+openSUSE chroots, `buildscm
  --method rpkg`), **OBS** (native openSUSE). Publishing still needs the owner's accounts (AUR SSH key /
  Fedora FAS / openSUSE). *Other CPU arches (ppc64le/s390x/…) need a musl binary built first — a CI
  cross-compile task in `release.yml`, tracked separately.* Vision: JII installable from every manager/repo.
- **`docs/SUPPORTED_SYSTEMS.md`** — cross-system test matrix + per-system smoke test (companion to
  `RELEASE_TESTPLAN.md`). **README** install section rewritten for the two methods; earlier README fix
  removed the `$ ` prompt from copy-paste blocks (a CachyOS tester copied `$ ` and hit `bash: $: …`).

**New directive (not yet implemented) — see [DECISIONS backlog / ROADMAP T6]:** (1) **Bootstrap a
missing manager** — if the best candidate's backend isn't installed (e.g. app on Flatpak but no
Flatpak), *offer to install the manager then the app*, never auto. This is **T6** (designed in
ROADMAP, currently FROZEN); the engine today *skips* `!is_available()` sources, so this needs T6 to
surface + prepend a bootstrap plan step. (2) **GitHub strictly last** — default `priority` in
`src/config.rs` currently ranks `github` *above* `cargo/npm/pipx/go/brew/nix`; owner wants it the
absolute last resort. Both are the agreed **next core work** after the install-easy epic. *Owner to
confirm github below cargo/npm/go/nix too.*

</details>

## Declarative Nix Etap C (2026-07-11)

**Declarative Nix Etap C: privileged auto-edit of the root-owned `configuration.nix` LANDED
(ADR-0058).** This closes the **last** declarative gap. Etap B auto-edited only a user-owned
`home.nix`; the NixOS system config stayed snippet-only because writing it needs root. Now
`strategy_for_target` (nix.rs) produces an `EditFile` for **any** readable/parseable config and tags
the system target with the new **`StrategyKind::EditFile { needs_root }`** flag (`needs_root == !home`;
unreadable/unparseable still falls back to the `Manual` snippet). The CLI's `apply_edit_file` branches
on that flag, *not* on the source: a user file writes directly (`write_nix_config`, unchanged); a
root file goes through **`write_nix_config_root`** — stage `new_content` in an `O_EXCL` temp, then run
two **explicit** elevated commands via `privilege.rs` (`cp -a -- <dest> <dest>.jii-bak`, then
`cp -- <tmp> <dest>`), `prime`d once. The exact `sudo`/`pkexec` argv is **printed before** anything
runs; `--dry-run` shows it and stages/writes nothing. JII still never runs fully as root — only those
two `cp` steps escalate, in the one escalation path. New `nix.edit_root_cmds` locale key (en+ru
parity). **259 tests green, clippy + build clean** (+4: `needs_root` classification, root dry-run
no-write/no-stage, exact elevated argv, unreadable→Manual). *Live escalated write still needs a real
NixOS host — extends the ADR-0056/0057 T7 verification debt.*

**Preceded (same day) by ADR-0057** — `[install] prefer_declarative = ask | always | never`
(`config::DeclarativePref`, default `ask`) + per-run flags `--nix-config` (→always) / `--nix-imperative`
(→never). `always` routes every resolved candidate offering an `EditFile` into the config edit —
single, batch *or* scripted — via the shared `apply_edit_file`; `ask` keeps the single-package
interactive menu (batch stays imperative, no prompt-storm); `never` is always imperative. Etap C plugs
straight into that routing: with `prefer_declarative = always`/`--nix-config` on a NixOS host,
`jii install <pkg>` now auto-edits `configuration.nix` (diff + sudo commands shown, backup, write).

---

## Most recent work (2026-07-10) — read this first

**doctor now enables a required repo before its dependents (ADR-0055).** Field report: a user opened
`jii doctor`, **skipped RPM Fusion**, accepted codecs/VLC (which live in RPM Fusion) → bare "not found"
+ an apparent VLC hang. Fix (data-driven, no core branch): `Recommendation` gains `requires` (+ reads
`id` again); codecs & VLC declare `requires = "rpmfusion"`; a **pure** `recommend::prerequisite(...)`
decides what to enable first; `doctor_offer` enables the prerequisite (its `manual` command **shown
before it runs**, `--dry-run` honoured, parent "yes" = consent) ahead of the dependent. Note: the
interactive doctor already *ran* `manual` repo-enables via `sh -c` (superseding the stale ADR-0035
"shown never run"); the missing piece was the dependency link + ordering. **233 tests** (pure
prerequisite logic + catalog wiring covered); read-only doctor render verified. Follow-ups: direct
`jii <pkg>` doesn't resolve prerequisites yet; `openh264` needs the Cisco repo (not RPM Fusion); the
VLC "hang" wasn't reproduced (diagnose separately on a clean `jii vlc`).

**CROSS-PLATFORM EXPANSION STARTED (ADR-0054).** The owner opened a multi-release program to grow
JII past Fedora-first toward a universal installer: **declarative Nix → Gentoo → Void → … →
Windows/macOS**. Sequencing was decided by risk, cheapest-first: the *declarative* Nix config-edit is
the one novel/high-risk piece (new kind of action — editing a hand-written config — plus setup
discovery), while emerge/xbps/winget/brew are imperative "just another `Provider`". So we prove the
platform seam with a cheap imperative provider first.
- **Gentoo (Portage/`emerge`) provider LANDED.** `src/provider/gentoo.rs`, id `gentoo`, **Official**
  (::gentoo tree is GPG-verified), self-gates on `emerge` (ADR-0029). Portage uses **atoms**
  `category/package`: exact-name search via `emerge --search "^name$"` (parses the `*  cat/pkg`
  blocks + `Latest version available:`/`Description:`; emits one candidate per `category/name`,
  keeping the atom in `raw`; a bare name in two categories → two candidates). Root plans `emerge
  --ask=n <atom>` / `emerge --unmerge` / `emerge --update` / `emerge -uDN @world`, full `_many`
  batching. `list_installed` reads `/var/db/pkg/<category>/<PF>` directly (no gentoolkit/portage-utils
  dep); pure `split_pf` (PF → name+version, revision-aware). Builds **from source** (slow — inherent
  to Gentoo, not hidden). Registered everywhere (after void). **No core source-branch. 243 tests.**
  Fixture-tested only — unverified on a live Gentoo host (T7). Reuses shared `[reason]` (`mgr="emerge"`)
  + new `gentoo_official`/`_many`.
- **Void (XBPS) provider LANDED.** `src/provider/void.rs`, id `void`, **Official** (Void repos are
  RSA-signed), self-gates on `xbps-install` (ADR-0029). Exact-name search via `xbps-query -R <name>`
  (property stanza, like `pacman -Si`, lax-captured; emits only on an exact `pkgname` match); plans
  `xbps-install -Sy` / `xbps-remove -Ry` / `xbps-install -Suy [pkg]` (root); `xbps-query -l` list;
  full `_many` batching + `plan_update_all`. Pure `split_pkgver` backs both parsers. Reuses shared
  `[reason]` keys (`mgr="xbps"`) + new `void_official`/`void_official_many`. Registered in
  provider/mod.rs + KNOWN_SOURCES + default priority (after zypper). **No core source-branch.**
  **228 tests green, clippy + build clean.** Verified on this Fedora host that `void` shows as
  *enabled-but-unavailable* and `pkg:void` misses honestly (xbps absent — same **T7 live-host debt**
  as apt/pacman/zypper/nix; parsers are fixture-tested only).
- **Declarative Nix — Etap A LANDED (snippet-first).** New optional `Provider::install_strategies`
  (default empty; ADR-0022 growth) + model `InstallStrategy`/`StrategyKind::{Imperative,Manual}`;
  engine `install_strategies(source_id, candidate)` (dispatch, no source-branch); CLI calls it **only
  for a single-package interactive install** and shows a chooser when non-empty. **Nix implements it:**
  detects which config files actually exist (NixOS `configuration.nix`→`environment.systemPackages`
  → `sudo nixos-rebuild switch`; standalone home-manager `home.nix`→`home.packages` →
  `home-manager switch`) and offers **only those** + the default imperative `nix profile install`,
  each with a hint. **No config detected → empty → no menu → plain imperative install (a Nix-on-Fedora
  user is never nagged).** A declarative pick **prints the exact snippet + file + apply command +
  backup note and installs nothing** ("show, never run", ADR-0048). Detection/snippet/guidance are
  pure + unit-tested; the menu→print→install-nothing path is **pty-verified** (stubbed nix + temp
  home.nix). New `[nix]` locale section (en+ru, parity green).
- **Declarative Nix — Etap B LANDED (parser-driven auto-edit; ADR-0056).** A home-manager `home.nix`
  the user owns is now **actually edited**, not just shown. `StrategyKind` gains
  `EditFile { path, new_content, diff, apply }`; the **provider** parses the file with **`rnix` 0.14**
  (lossless rowan CST), locates `home.packages` (unwrapping `with pkgs;`), and **splices the package
  into the original source bytes** (no reflow) — mirroring style (multi-line/inline/empty), preserving
  comments, detecting already-present, and returning `NotFound` → **Etap A snippet fallback** for
  anything it can't safely edit (attr absent / value not a plain list / unparseable). The root-owned
  NixOS `configuration.nix` is now **also auto-edited** via the privilege path — see Etap C (ADR-0058)
  at the top; in Etap B it was snippet-only. CLI: show diff → confirm (honours `--yes/--no/--auto`;
  `--dry-run` never writes as the menu is already gated off) → back up to `<path>.jii-bak` → write →
  print `home-manager switch`. `insert_package`/`find_list`/`line_diff`/`write_nix_config` unit-tested
  (multi-line, inline, empty, no-`with-pkgs`, comment-preserving, already-present, not-found,
  unparseable, backup+overwrite). **253 tests green, clippy + build clean.** New `[nix]` `edit_*`
  locale keys (en+ru parity). *Full menu→edit→apply flow not yet run on a live home-manager host — T7
  debt.*
- **Next in this program:** the declarative-Nix program is now **complete** through Etap C —
  `configuration.nix` auto-edit landed (ADR-0058). **Windows/macOS** is the remaining big piece — a
  **separate later epic** (breaks `privilege.rs`, paths, packaging, CI), explicitly deferred by the
  owner until the rest is done and bugs are minimal. Nearer-term is verification, not new features:
  the owner running the non-Fedora providers (apt/pacman/zypper/void/gentoo/nix) and the live Nix
  edit→apply (home-manager **and** NixOS) on real hosts (T7).

**#7 localization COMPLETE + first-run onboarding for any command.** The i18n migration
(ADR-0050) is finished: **zero user-facing string literals remain in Rust code** — every
string lives in `locales/en.toml` / `ru.toml`, keyed by namespace, looked up via `t!("key",
arg=…)`. English is source-of-truth + fallback; a parity test asserts en/ru have identical
key sets. Language resolves `--lang` › `[ui] locale` › `$LC_MESSAGES`/`$LANG` › `en`.
Migrated in staged commits (all green, pushed): plan preview + prompts; info/search rationale
+ `TrustLevel::display()`/`Health::display()` (JSON keeps the stable `label()`); every
provider reason string (shared `[reason]` table with `{mgr}`/`{name}` placeholders) incl. the
non-Fedora providers; setup wizard, list/history/audit, system-update, parse errors; self-plan
reasons; error remedies. The low-level `#[error]` Display prefixes stay English by design (the
technical cause line, not user guidance). **216 tests green, clippy clean.** Live-verified
Russian across install/search/info/list/history/doctor/preview.

**New behaviour — first-run setup before ANY command.** The onboarding wizard used to fire only
on a bare `jii`. Now the very first *interactive* use of JII for any task (`jii fastfetch`,
`jii search …`) announces up-front which command will run, runs the wizard, reloads the saved
config, then continues with the original invocation. setup/doctor/uninstall + hidden plumbing
are excluded. Verified under a pty (declining prompts → command still runs).

**Colour + mouse polish DONE (ADR-0052).** Owner's "простое" item, landed after i18n: a semantic
`Palette` (in `ui`, from `Renderer::palette()`) colours human output only when colour is enabled
(`--no-color`/`NO_COLOR`/`--json`/no-TTY stay plain) — source ids cyan, trust official=green/
community=yellow/untrusted=red, versions/secondary dimmed, `✓`/`→` green, headings + table headers
bold; pad-before-colour keeps alignment. The candidate chooser is reimplemented on **crossterm**
(dialoguer dropped): arrow keys **and mouse** (hover-highlight, click-to-pick, scroll), always
restores the terminal, non-TTY takes the default. 216 tests, pty- and pipe-verified.

**GitHub by-name search DONE (ADR-0053).** `jii exteragram` (a bare name only on GitHub) now, on a
normal-source miss in an interactive session, opens a **repo picker**: `Forge::search_repos`
(GitHub `/search/repositories`, relevance-ranked) → `model::RepoHit`s shown as
`owner/repo — description ★stars` with a "↓ Show more" entry that pages forever; picking resolves
the repo's latest release into the normal preview→confirm→install (untrusted → still confirmed).
Optional forge capability (`supports_repo_search`), no core source-branch. Owner/repo, pinned
source, intent flags, batch, `--json`, non-TTY all skip it. Crossterm menu now width-truncates each
item so long lines don't wrap. 219 tests, pty + pipe verified. **Also:** setup wizard
gained a full GitHub-token help step (benefit + create + export), which mitigates the tighter
search rate limit.

**Typo-tolerance DONE.** On top of GitHub's own fuzzy matching, when the verbatim term finds
nothing the picker retries cheap edit-distance-1 variants (`cli::typo_variants`: deletions then
adjacent transpositions, deduped/capped) and adopts the first that hits, paging that corrected term
and telling the user (`install.gh_corrected`). Live-verified: `jii exeteragram` → "showing results
for 'exteragram'" → the exteraSquad picker.

**Next up:** the cross-platform program (ADR-0054/0056/0057/0058) is active — Void ✅, declarative-Nix
Etap A ✅, Gentoo ✅, declarative-Nix Etap B ✅ (parser-driven auto-edit), declarative preference +
batch/scripted routing ✅ (ADR-0057), and declarative-Nix **Etap C ✅** (privileged `configuration.nix`
auto-edit, ADR-0058) done — the declarative-Nix program is now **complete**. **Windows/macOS** is the
remaining big piece (separate later epic, explicitly deferred until the rest is done). Other
candidates: richer info cards; more polish; the owner running the non-Fedora providers and the live Nix
edit→apply on real hosts (T7). GUI (Steam + KDE Discover + GNOME Software blend) stays **explicitly
parked** until the CLI is fully polished — do not start it.

---

## Prior work (2026-07-09) — read this first

Landed and pushed to `master` after the clean-VM UX waves:

- **Beta published.** Static musl binaries (x86_64 + aarch64), `.rpm`/`.deb`/AUR/COPR packaging,
  `install.sh` one-liner, completions + man page. CI release pipeline green; tags `v0.1.0-beta` and
  **`v0.1.1-beta`** published (12 assets each). Version in `Cargo.toml` = `0.1.1-beta`.
- **Self-update (ADR-0040, revised).** `jii update jii` self-updates method-aware (user-space →
  atomic binary swap via `Action::Replace`/`fs::rename`, no root; package → dnf/apt). `jii uninstall`
  self-removes. **Bare `jii update` updates everything** — system *then* JII itself (still prompts).
- **`jii doctor` is now an interactive setup questionnaire (ADR-0041).** Not advice anymore: each
  fixable check and each catalog suggestion (RPM Fusion, codecs, fonts, VLC, PATH…) is a yes/no
  question, applied on "yes". New `Fix::PathExport` edits the shell rc for `~/.local/bin` /
  `~/.cargo/bin`; `install`→`install_inner(assume_yes)` + `PromptFlags::with_yes` so the single
  question is the consent (trust barrier still gates untrusted). Read-only under `--json`/`-n`/no-TTY.
- **Smart search matching (ADR-0042).** Exact-first, broaden on a miss: `jii ayugram` →
  `ayugram-desktop`, trailing typo `ayugramm` still reaches it (prefix search + a ≤2-char
  trailing-trim fallback); shows "No exact match — closest: …" and confirms. `MatchMode` on `Query`
  (dnf appends `*`); name-aware `rank(config, query, cands)` with an exact>prefix>substring tier;
  `Engine::broaden_search`. Exact queries (`jii git`) stay noise-free. Verified live on Fedora.
- **Relicensed MIT → GPL-3.0-or-later** (LICENSE = canonical GPLv3; Cargo/README/packaging updated).
- **README** fully rewritten into a detailed, badged landing page.

**Post-testing UX wave (in progress, 4 slices landed — each an ADR, all green + pushed):**
- **#1 doctor analyses system state (ADR-0043):** `Engine::installed_index` gathers installed once;
  doctor skips already-done suggestions (no more offering an installed VLC). Catalog `check` field
  for non-obvious identities (Flatpak app-id, repo release pkg).
- **#2 search speed (ADR-0044):** per-source failure **circuit breaker** in the disk cache — a
  timing-out source (COPR) is skipped for `network.failure_cooldown_secs` (120). First search ~5s →
  repeats ~1.1s.
- **#6 info ≠ install (ADR-0045):** `Provider::reference`/`Engine::reference` + `Reference` model;
  `jii info lodash` shows a real card + a clear library note (shared `library_note` also fixes the
  install message, #5). Philosophy unchanged: programs, not libraries.
- **#4 bare manager name → bootstrap (ADR-0046):** `jii npm` installs/notes the npm *manager*, not a
  package called npm; `Engine::ecosystem_ids` (pure) + `install_inner` `route_managers` flag (loop
  guard); `jii npm:npm` still gets the registry package.

- **#3 nix `list_installed` (ADR-0047):** tolerant `nix profile list --json` parser (map + array
  schemas); Nix now feeds `installed_index` and owner resolution. *Fixture-tested only — needs a real
  Nix host to confirm the live JSON.*
- **#9 principle (ADR-0048):** "JII cooperates with the system; it is not the centre of the world" —
  recorded as the guiding ADR, cross-referencing the ADRs that embody it.
- **#10 (docs/RELEASE_TESTPLAN.md):** a manual per-release checklist across every command/area.
- **#8 forge abstraction (ADR-0049):** `Forge` trait + generic `ForgeProvider` (`provider/forge.rs`);
  GitHub is now `GithubForge`, one forge among peers. Adding Codeberg/Gitea/GitLab = implement `Forge`
  + register a source id. Behaviour identical (all tests moved + pass).

**#7 localization — DONE (2026-07-10).** See the 2026-07-10 "Most recent work" section at the top:
the whole post-testing UX wave is now complete. The forge abstraction (ADR-0049) means the next
task (GitHub by-name search) extends `GithubForge`/`ForgeProvider`, not a hardcoded exception.

**Still open (needs owner's hosts):** install/run the published `.rpm`/`.deb` on a live non-Fedora
box and on real aarch64; true end-to-end self-update fires only when the *next* tag is cut.

---

## What JII is

A smart universal package **installer** (not a manager) for Linux, in Rust,
Fedora-first. It searches multiple sources (DNF, Flatpak, and — soon — GitHub
Releases, COPR…), ranks them, installs the best, and explains why. Read
[CLAUDE.md](../CLAUDE.md) for binding constraints and
[ARCHITECTURE.md](ARCHITECTURE.md) for the canonical design.

## Current phase

**Terminal 1.0 (ADR-0026) — T1–T4 done; T5 candidate chooser landed; now pivoted to a UX-polish
pass.** After the first real dogfooding on a clean Fedora VM the user re-prioritised (2026-07-06):
**no new features/providers/GUI — polish the terminal experience.** T5's remaining **GitHub by-name
repo chooser is deferred** (ADR-0030 Proposed/deferred — a new feature, not a reported UX problem);
the version chooser is likewise paused. The current deliverable is **[docs/UX_EVALUATION.md](UX_EVALUATION.md)**:
16 UX problems classified (10 pure polish over existing seams, 6 need small optional-capability/UI
designs), a delivery order (U0–U8), and a NixOS architectural opinion. **Awaiting the user's one open
decision: doctor scope for 1.0 (actionable JII-diagnostics only vs the full codec/driver/font
recommend-catalog).** Cross-distro is real: JII runs on Debian/Ubuntu (apt), Arch (pacman),
openSUSE (zypper) and Nix, not just Fedora. Below is the pre-T4 Phase-5 context (still accurate).

**Phase 5 — user-space sources & update (done).** Phases 0–4 done and verified.
The pre-Phase-5 re-evaluation (ADR-0022) confirmed the model needs **no change** for
these providers. **`cargo`, `npm`, `pipx`, `go` are done** (pure `Provider`s, sharing
`get_json_opt`/`command_plan`); **`jii update` is done** (no per-source branching); the
post-8-provider **architecture review** is done (ADR-0024: architecture healthy, no code
change); and **batch install is done** (ADR-0025: `jii install a b c`, same-source merge
via optional `plan_install_many`, no model change). Next: **Homebrew** provider (ADR-0024).

## Last completed work

**U8 — first-run walkthrough polish (2026-07-06). The UX-polish pass (U0–U8) is now complete.**
Played the whole CLI as a new user and fixed the awkward edges (#15); no architectural change, no ADR.
Two small commits: (1) **aligned ledger tables** — `list`/`history`/`audit` printed ad-hoc
`{}  {}  {}` with no header/alignment (and `history` leaked the `Action` enum via `{:?}`); all three
now render through one `table_lines` helper (header row + data-driven column widths, so a long name
like `visual-studio-code` no longer breaks alignment the way audit's fixed `{:20}` did), plus
`Action::label()` for human past-tense history verbs (installed/removed/updated). (2) **update
message fix** — `jii update <not-installed>` printed the correct `✗ Not installed: X` then a
misleading `Nothing installed via jii yet.`; since bare `update` routes to the system update, an
empty named-path record set always means "the named ones aren't installed" (already stated), so the
follow-up is dropped (mirrors `remove`). Fedora-verified (list/history/audit short+long names; the
first-run wizard replayed via pty reads clean; friendly vs `-v` install preview). **180 tests green,
clippy clean.** Noted follow-up (not done, low value): in friendly single-install the "Also
available" block prints just before the recommendation line — mildly backwards, but reordering it
would restructure the preview flow, so left as-is.

**U7 — system-wide update (2026-07-06, D10, ADR-0034).** Bare `jii update` now updates the **whole
system**, not just JII's registry slice (#15, "the universal update command"). New **optional**
`Provider::plan_update_all() -> Result<Option<InstallPlan>>` (default `None`): "upgrade everything
this source owns". `Engine::plan_update_all` aggregates every available provider's `Some(plan)` into
a `SystemUpdate { plans, sources }`; `Engine::run_system_update` primes privilege **once** across the
mixed root/user plans and runs them — the engine never branches on the source id. **Non-regression:**
sources with no bulk path (github/cargo/go → `None`) still get their JII-installed packages updated
per-record, via a fallback batch appended to the same run (the version-refresh loop is extracted to
`refresh_for_update`, shared with the named path). Named `jii update <pkg>` is unchanged (registry
path; `:source` still pins). Implemented for **all** bulk managers: dnf `upgrade`, flatpak `update`,
apt `upgrade`, pacman `-Syu`, zypper `update`, snap `refresh`, nix `profile upgrade --all`, brew
`upgrade`, pipx `upgrade-all`, npm `update -g`. Bulk plans upgrade beyond JII's ledger so they are
**not** recorded (only the per-record fallbacks refresh the registry) → `jii list` may show a stale
version for a bulk-updated tracked package (documented debt). **177 tests green, clippy clean**;
verified on Fedora (bare `update --dry-run` = dnf + flatpak, friendly one-line preview, `-n` abort,
named path intact). Non-Fedora bulk impls unverified on a live host (T7 debt).

**U6 — helpful failure & doctor (2026-07-06).** All small commits, no architectural change to the
core; two ADRs (0032, 0033):
- **Actionable errors (D7, ADR-0032).** A pure, unit-tested `JiiError::remedy() -> Option<String>`
  maps a *typed* failure to a next step, rendered under the error (`  → …`) by `main.rs::report`
  (so a bad-config failure, before any `Renderer` exists, still gets its remedy). `UnknownSource`
  lists `KNOWN_SOURCES` + points at the config/`jii setup`; `Config`/`Io` (by `ErrorKind`) get
  specific advice; `Other(anyhow)` returns `None` on purpose — no string-sniffing opaque text into
  a misleading remedy.
- **doctor Tier 1 (D6).** `jii doctor` now prints, under the per-source health table, a "System
  checks:" section about JII itself working — is `~/.local/bin` on `PATH` (where cargo/npm/pipx/go/
  GitHub installs land), is `GITHUB_TOKEN` set. Read-only (reports + advises, no auto-apply). Pure
  `system_checks` decides; JSON stays the stable per-source array. Consumed the previously
  dead-coded `Platform::is_on_path`/`path_dirs`.
- **recommend-catalog Tier 2 (D6, ADR-0033).** A **data subsystem**, not code, not a `Provider`:
  `data/recommend/catalog.toml` embedded via `include_str!`, typed + loaded in `src/recommend.rs`,
  filtered by host distro via the new `Distro::id()` (the first real consumer of distro-awareness
  ADR-0029 deferred — entries *declare* their distros, no `if fedora` branch). `jii recommend`
  lists curated Fedora suggestions (RPM Fusion, codecs, VLC, fonts, Steam, Wine, tuned-ppd) grouped
  by category — each with why + the exact way to get it. `jii recommend <id>` **applies** one by
  routing its `packages` through the normal install path (preview → confirm → execute, so the U3
  pre-check + U5 preview come free); a `manual` repo-enable (RPM Fusion) is **shown, never run**
  (the trust boundary is called out). Analyze → Explain → Ask → Apply throughout. **175 tests
  green, clippy clean**; verified on Fedora (remedy, doctor checks, recommend list/apply/manual/
  unknown-id). **Debt:** Fedora catalog entries are hand-curated, unverified on a clean VM (T7).

**U5 — the Friendly/Advanced UX pass (2026-07-06).** A big verbosity + onboarding pass, all
landed as small commits, no architectural change:
- **Friendly/Advanced output modes (D8).** `config::OutputMode { Friendly (default), Advanced }`
  (serde lowercase, in `[ui] mode`). `Renderer` carries the mode; `is_friendly()` is `!json &&
  Friendly`. `-v`/`--verbose` forces Advanced for one run without touching the config. Friendly
  **hides secondary-source failure noise** (`report_source_failures` returns early — no more
  `⚠ copr: timeout` spam on a normal search) and **collapses the install preview** to one short
  scannable line per package (`Install <name> (<ver>) via <source> — <why>  [needs sudo]`);
  `--dry-run` and Advanced still print the full Plan block (the point of a dry-run is the detail).
- **First-run wizard + `jii setup` (DW).** `config::MetaConfig { first_run_completed }` +
  `Config::save()` (toml::to_string_pretty, `create_dir_all`) + `is_first_run()`. A bare `jii` in
  an interactive first-run session offers a 30-second setup (welcome → mode chooser → optional
  doctor → save); declining still marks it done so it never nags again. `jii setup` re-runs it on
  demand. Non-interactive/`--json`/piped sessions never trigger it.
- **A clap parse fix** discovered while testing: a global flag *before* a subcommand
  (`jii -v search git`, `jii --json search git`) used to misparse as `install ["search","git"]`
  because of `args_conflicts_with_subcommands = true` — removed; the full parse matrix re-verified.
- Neutral chooser prompt wording ("Your choice [N] (or 'n' to cancel):") so it reads the same for
  install/remove/setup. **165 tests green, clippy clean**, wizard + Friendly paths pty-verified in
  an isolated `XDG_CONFIG_HOME`.

**T5 (slice 1) — the interactive candidate chooser (`ui/prompt::choose`).** A single
interactive install that resolves to **more than one** candidate now shows a numbered source
menu — the recommendation pre-selected as the default (Enter installs it), each other source
selectable by number, `n` to cancel — instead of silently taking the top rank. The chooser
addresses the "never silently install the wrong thing" requirement. **Honest architectural
finding: no ADR and no engine/model change were needed.** The pre-declared "chooser/selection
model" growth turned out to already exist: `Provider::search` has returned `Vec<PackageCandidate>`
and the engine has ranked the whole set together since Phase 3, so the chooser is **pure
`cli`/`ui`** over the ranked list the install path already had. Design points: (1) picking a
source is itself the consent, so a **trusted** interactive pick skips the otherwise-redundant
`[Y/n]` (tracked by `chose_interactively`), while an **untrusted** pick still hits the trust
barrier (ADR-0006 preserved — `skip_confirm` is gated on `least_trusted <=
default_yes_max_trust`); (2) the chooser only fires for a **single**-package install with
`ranked.len() > 1` in an **interactive** session (`!--source && !effective_auto && !--yes &&
!--no && tty && !json`) — batch installs stay auto-picked to avoid a prompt storm, and every
non-interactive/intent-expressing path is unchanged. The pure `parse_choice` (empty→default,
`n`/`q`/`cancel`→cancel, in-range number→pick, else→re-ask) is unit-tested; the three live
paths (Enter→dnf, `2`→cargo, `n`→abort) plus the `--auto` bypass and the piped non-TTY
fallback were verified on a real pseudo-terminal. **150 tests.**

**T4 — cross-distro system providers + the platform-seam relaxation (ADR-0029, Accepted).**
The whole codebase coupled to the distro in exactly one place (`Platform::is_supported` →
`matches!(distro, Fedora)`); a full audit (real code) showed every provider already self-gates
on its **binary** via `which`, never the distro. So T4 was not an engine refactor — it removed
one artificial wall and de-privileged the `Distro` enum. Enacted: **removed**
`Platform::is_supported`/`require_supported` and `JiiError::UnsupportedPlatform`; `Platform` is
now a **pure host-facts value object** (`distro` kept as a fact, no reader until T6 config-seed/
bootstrap). "Supported" is redefined as **"≥1 usable install source"** (`Engine::any_source_available`,
the same `is_available` fan-out `source_catalog` uses), guarded at the 5 CLI entry points by a
shared `ensure_usable_source` (distinguishes "none enabled" from "none available" — clearer than
the distro wall even on Fedora). Then four providers, each a pure additive `Provider` that
self-gates on its binary, with `_many` batching:
- **apt** (Debian/Ubuntu): `apt-cache show` deb822 (pure `parse_show`, first stanza),
  `apt-get install/remove/install --only-upgrade` (root), `dpkg-query` list. Official.
- **pacman** (Arch): `pacman -Si` (pure `parse_si`), `pacman -S`/`-Rs` (root), `pacman -Q` list.
  Official; official repos only (AUR is a separate future source).
- **zypper** (openSUSE): `zypper --xmlout search` (dependency-free `<solvable>` attr parse),
  `zypper --non-interactive install/remove/update` (root), `rpm -qa` list. Official.
- **nix** (any distro): modern flakes CLI (`--extra-experimental-features`), `nix search --json`
  (exact `pname` decided in code), `nix profile install/remove/upgrade` — **user-space, no root**;
  empty list + `~/.nix-profile/bin/<name>` `is_installed` (go precedent). Community.
Shared **`provider::run_capture_lax`** (stdout even on non-zero exit) added beside `run_capture`:
apt-cache exits 100, `pacman -Si` 1, `zypper` 104 for "unknown package" = "no candidate", not a
source failure. No core branch on the source. Fedora behaviour verified unchanged (`jii sources`,
dnf dry-run). **150 tests.** **Debt:** nix `profile` CLI is version-fragile and was **not** verified
on a live Nix host (none here) — flagged for the T7 clean-VM pass; the `id`/`id_like` distro
predicate stays deferred to its first consumer (T6), per ADR-0029.

**Prior — Batch install — `jii install a b c …`.** Install many packages as one operation with no
change to `InstallPlan` or the Executor (ADR-0025). Each package runs the normal
search→rank→pick; the engine groups the chosen candidates by source and **merges
same-source installs into one command** where the source can (`dnf/cargo/npm/go install a b
c`) via a new **optional** `Provider::plan_install_many` (default `None` → per-candidate
fallback — the ADR-0022 growth pattern; the engine never branches on the source). One
grouped "Summary" preview + action preview, one confirmation governed by the **least-trusted
candidate** (`prompt::confirm_install_batch`; untrusted still always explicit, ADR-0006),
one root escalation (`exec::prime_for` once across all plans), one run
(`exec::run_actions`), and records written **as each plan succeeds** so a mid-batch failure
leaves the registry accurate. A not-found package is reported and does **not** cancel the
rest (offer to continue). A group of one keeps the richer single-package plan, so
`jii install <pkg>` output is byte-identical to before. Single install is now a batch of one
— the old `Engine::install` and `plan_install` wrapper were removed (one install
write-path, no duplicated recording to drift). Bootstrap-a-missing-manager is **deferred,
not faked** (needs the manager-install feature; the per-source grouping is its future
hook). Verified: dnf/cargo merges, mixed-source grouping, not-found continue, single-package
UX unchanged. **99 tests.**

**Prior — `jii update [<pkg>]`.** Wires the existing per-provider `plan_update` into a command,
with no per-source branching (ADR-0004 holds). For one named package (must be installed)
or every registry record, it re-searches the **owning** source via the normal search→rank
path (filtered by `source_id`) to get the latest version, **skips provably-current
packages** (exact version-string equality → an up-to-date system is a clean no-op, not a
reinstall), then runs each `plan_update` through the same preview → confirm (a single batch
prompt) → execute pipeline as install/remove. Engine gained `plan_update`/`update`; the
registry gained `record_update` (logs a history `Update`, refreshes the stored version),
sharing an `upsert` helper with `record_install` so the "replace + log + push" invariant
lives in one place. Version handling is honest: it records the just-installed latest from
the re-search, falling back to the prior version only when the source no longer reports one.
Verified end-to-end via `--dry-run` (a simulated go install showing `v0.60.0 → v0.73.1` +
the `go install …@latest` plan), the no-op path, and the missing-package error. 96 tests.

**Prior — `provider/go.rs` (Go modules, via `go install`)** + the pre-`go` helper refactor
(commit `f2e8377`). go is the 4th user-space provider, mirroring cargo/pipx: `search`
resolves a module path via the Go module proxy (`{proxy}/<mod>/@latest`, uppercase → `!x`
escaping), `plan_install`/`plan_update` = one unprivileged `go install <mod>@latest` into
`$GOBIN`/`$GOPATH/bin`/`~/go/bin` (PATH-warn), `plan_remove` deletes the installed binary
(Go has no uninstall — an `Action::RemoveFile`, like github), `list_installed` is empty
(no cheap global module→binary list; the registry + a file-existence `is_installed` track
it). **No app-filter (ADR-0023):** the proxy can't cheaply say which modules are `main`
(installable), so — like pipx — go offers the module and lets `go install` be the
authority. Community trust (go verifies checksums via `go.sum`/sum.golang.org).
`is_available` overrides the shared `which` because go uses `go version`, not `--version`
(the latter exits non-zero). Verified: real proxy search through JII (fzf→v0.73.1 offered,
BurntSushi/toml resolves with `!burnt!sushi` escaping), dry-run (single unprivileged
command). **Pre-`go` refactor:** the search 404-dance and single-command `InstallPlan`
construction had each reached 3× across cargo/npm/pipx (→ 4× with go), so extracted
`provider::get_json_opt` (GET → `Ok(None)` on 404, else typed JSON) and
`provider::command_plan` (one-`RunCommand` plan). Deliberately did **not** extract
`PackageCandidate` construction (per-provider, would leak trust/arch_ok) or the tolerant
stdout read (only 2×) — reducing maintenance cost, not line count.

**Prior — `provider/pipx.rs` (PyPI, via pipx).** Third Phase 5 provider, mirrors cargo:
`pipx install/uninstall`, first-class `pipx upgrade`, `pipx list --json`, installs to
`~/.local/bin` (no root), community trust. **Key decision — ADR-0023:** PyPI's API exposes
no reliable program-vs-library signal (the `Environment :: Console` classifier is ~40%
unreliable — measured on 10 popular CLIs), so pipx does **not** pre-filter (unlike cargo's
`bin_names` / npm's `bin`); it offers the package and lets `pipx install` reject non-apps.
Principle: a visible false positive beats silently hiding a real app. No core change, no
engine special-case. Verified: real PyPI search through JII (black + requests both offered),
dry-run (single unprivileged command), via a stubbed `pipx` on PATH (pipx not installed
here). Before writing pipx: assessed duplication — nothing hit the 3× threshold beyond the
already-extracted `http_client`, so no pre-pipx refactor (the `command_plan` extraction is
scheduled for `go`, the 4th user-space provider).

**Prior — `provider/npm.rs` (npm registry)** + a shared-`http_client()` refactor. npm mirrors
cargo: `search` hits the npm registry `/<pkg>/latest` and **only offers packages that
install a CLI** (non-empty `bin`), so a library like `lodash` yields no candidate.
Installs are unprivileged and forced into `$HOME/.local` via `--prefix` (binaries →
`~/.local/bin`, never root, regardless of npm's host prefix). `list_installed` reads
`npm ls -g --json` tolerantly. Community trust; no core change, no engine special-case.
Verified: real registry search through JII (prettier→v3.9.4 offered, lodash rejected),
dry-run (single unprivileged command), multi-source ranking. Also **extracted
`provider::http_client()`** (the reqwest builder + User-Agent was copied 3× in
copr/github/cargo; npm would have been the 4th) — pure refactor, `jii doctor` verified.

**Prior — `provider/cargo.rs` (crates.io).** First Phase 5 provider. `cargo install <crate>`
builds executables into `~/.cargo/bin` — user-space, no root. `search` hits the
crates.io `crates/{name}` API and **only offers crates that ship a binary** (checks
`bin_names` on the newest version), so a library-only crate (`serde`) yields no
candidate — JII installs *programs*, not libraries. Community trust (crates.io registry;
cargo verifies checksums itself, so the plan is one unprivileged `RunCommand`, no
separate Download/verify). `list_installed` parses `cargo install --list`. Registered in
`provider/mod.rs` like the others — **no engine special-case, no model change** (ADR-0022
holds). Verified: real crates.io search through JII (ripgrep→v15.1.0 offered, serde
rejected), dry-run (single unprivileged command), multi-source ranking (dnf recommended,
cargo listed as alternative), 5 unit tests. From-source compile not run (COPR precedent).

**Prior — architecture re-evaluation before Phase 5 (docs only).** Checked the live code against
the design. Verdict: load-bearing structure is sound (`Provider` seam, plan-as-`Action`,
trust threshold, registry-as-hint); **Phase 5 needs no model change**. Recorded **ADR-0022**
with three forward rules — (1) new capabilities (version mgmt, metadata, manager bootstrap)
are **optional `Provider` methods with safe defaults**, following the `probe`/`is_installed`
precedent, never a fat trait or core branch; (2) keep the **engine UI-free** — the
`&Renderer` in `Engine::install`/`remove` is the one `ui` coupling, to be decoupled via a
progress-event trait **before** a second frontend (not now, YAGNI); (3) versions/metadata/
rollback live in the provider/registry, not the core (reaffirms ADR-0009). Also **synced
`ARCHITECTURE.md`** §5/§9/§11/§15 to the evolved execution model (`Action`+`exec.rs`,
verification on `InstalledRecord`) — a stale canonical doc was an active hazard.

**Prior — GitHub `.zip` release assets** — `exec::extract` now dispatches on the archive's
file-name extension into `read_tar_gz` / `read_zip` (both decode to the same
`ArchiveFile` list, so member selection + writing stay format-agnostic — the seam
ADR-0016 predicted). github's `classify` gained `AssetKind::Zip` (ranked below `TarGz`,
which preserves unix modes) and now rejects delta-patch assets
(`.bsdiff`/`.patch`/`.delta`/`.zsync`) that used to masquerade as raw binaries —
surfaced by `denoland/deno`, which ships a `*.bsdiff` next to its Linux `.zip`. Verified:
real-release dry-run selects `deno-…-linux-gnu.zip` → Extract; zip round-trip
(create→extract→assert bytes+mode) unit-tested; the untrusted trust barrier correctly
refused a non-interactive real install (ADR-0006). Added the `zip` crate
(`default-features=false`, `deflate`). See ADR-0016 (2026-07-04 update).

Also this session (docs only): **ADR-0020** (JII is a universal layer, not another
package manager) and **ADR-0021** (integrate external backends like UPAC only via their
stable public API, as another `Provider`; implement nothing until that API exists), plus
new ROADMAP Future ideas (more managers, bootstrapping a missing manager, provider-supplied
metadata).

Prior Phase 4 slices, all verified end-to-end: `jii doctor` health/rate-limit (ADR-0019);
`jii audit` (ADR-0018); COPR provider (ADR-0017); `Action::Extract` + `.tar.gz` (ADR-0016);
github `jii remove` (`Provider::is_installed`); GitHub Releases provider (ADR-0014); the
execution model (`Action` enum + `exec.rs`, ADR-0007).

## Current task

**UX-WAVE 2 — real-use feedback from a clean Fedora VM (2026-07-06, owner-set).** The owner ran the
pushed build on a VM and filed 15 UX points; priority is now **product/UX polish, not architecture**.
Agreed decisions: **command order 1→2→3→4** = ① arrow-key TUI choosers → ② doctor becomes a real
*system helper* (PATH, ~/.cargo/bin, internet, missing managers, flathub, permissions, broken repos,
updates — Analyze→Explain→Ask→Apply, previewable fix plans) → ③ providers/marketplace (manage the
ecosystems themselves: install/remove/update npm/cargo/brew/snap/nix + bootstrap a missing manager)
→ ④ `info` becomes an app *card* (description/GitHub/site/license/author). Also decided: **recommend
folds into the new doctor and the standalone `jii recommend` command is removed** (owner disliked it);
**`list` and `audit` merge** into one (`jii list`, security via `jii list --audit`).
**Done this session (pushed):** #3 setup stops advertising next commands; #10 crisp "already
installed" (no "Nothing to do"); #12 `jii why`→`jii how` (`why` hidden alias); #13 crisp
Installed/Removed/Updated confirmed; **#1 arrow-key TUI choosers via `dialoguer` Select** (↑↓/Enter/
Esc, upgrades setup + source chooser + multi-owner remove at once; pty-verified); #6 `-d` alias for
`--dry-run`. **Diagnosed #9** (npm `lodash` finds nothing) = **by design**, not a bug (npm/cargo only
offer packages with a CLI `bin`; libraries aren't "programs"); a helpful "it's a library" message
needs a small provider signal — noted follow-up. **#11 already done in U7** (bare `jii update` =
whole system).

**② doctor-as-system-helper — slice 1 (read-only diagnostics) landed.** `jii doctor` now probes the
host environment beyond the two Tier-1 checks: **internet reachability** (a fast HTTPS HEAD; a
failure reads red/critical), **git** and **curl** presence (advice points at `jii git`/`jii curl`),
**~/.cargo/bin on PATH** (only when cargo is present or the dir exists), and **Flathub remote**
configured (only when Flatpak is installed). Facts are gathered concurrently (`tokio::join!`) in
`gather_system_facts`; the verdict/wording logic stays a pure, unit-tested `system_checks(&SystemFacts)`.
A closing summary line reports how many things need attention. 180 tests, clippy clean; verified live
(caught `~/.cargo/bin` missing from PATH on the dev host).
**② doctor-as-system-helper — slice 2 (`--fix`) landed.** `jii doctor --fix` offers the fixable
checks: git/curl route through the normal install path (which previews + confirms itself); the
Flathub remote is a plain command shown before it runs (`run_plain_command`; Flatpak elevates via
its own polkit, so JII wraps no sudo/pkexec). Each `Fix` is data on the `SystemCheck`
(`Fix::Install(pkg)` / `Fix::Command{argv,show}`), kept pure and unit-tested. `--dry-run` previews
every fix without asking or changing anything; a plain `jii doctor` nudges "run --fix" only when
something is fixable. PATH/token/internet stay manual-only (JII won't edit your shell rc or invent a
token). 183 tests, clippy clean; live-verified (nothing-fixable path on the dev host).
**② doctor-as-system-helper — slice 3 (fold recommend) landed (ADR-0035).** The recommend catalog
now surfaces at `doctor`'s tail as a compact "Suggestions for your system" section (title — why · the
exact command to run; `note:` caveats shown). The **standalone `jii recommend` command and its
apply-by-id path are removed** — applying is now just running the shown command (`jii vlc` / the
`manual` command), more transparent than `recommend <id>`. `Recommendation.id` is no longer read at
runtime (uniqueness invariant moved to `title`; slug kept in the TOML as an authoring anchor).
Catalog data subsystem (ADR-0033) untouched; only its presentation moved. 183 tests, clippy clean,
live-verified. **② doctor is now complete.**

**③ providers/marketplace landed (ADR-0036).** New read-only **`jii providers`** lists the installable
*ecosystem* managers (npm, cargo, brew, Flatpak, snap, pipx, go, nix) with their presence on this host
(installed vs available); base repos (dnf/apt) and non-managers (github) are absent — you don't install
those. Ecosystem-ness is **provider metadata**: an optional `Provider::ecosystem() -> Option<Ecosystem>`
(default `None`, ADR-0022 growth) declaring a `label`, `binary`, and a `Bootstrap`. **`jii providers add
<name>`** bootstraps a missing manager: `Bootstrap::Packages(&[…])` is an **ordered cross-distro
candidate list** (`nodejs-npm`→`npm`; `golang`→`go`→`golang-go`) resolved by `Engine::first_available_package`
(first that searches non-empty wins — JII's own search does the per-distro work, no source branch) then
handed to the **normal install path** (preview→confirm→execute→record, the `doctor --fix` reuse pattern);
`Bootstrap::Script(cmd)` (brew, nix) is **shown, never run** (trust boundary, ADR-0005/0006). Already-
installed / unknown-ecosystem answer clearly. 184 tests, clippy clean; live-verified on Fedora (providers
list + JSON; add already-installed/unknown/script/packages-dry-run → pipx resolved to dnf `pipx` with full
preview). **Debt:** the `Packages` candidate lists are hand-curated, unverified on clean non-Fedora VMs (T7).

**④ info app-card landed (ADR-0037).** `jii info` is now an app **card**: name → description → an aligned
metadata block (Source, Version, License, Homepage, Repository, Author — present fields only) → the source
list + recommendation. Rich metadata is an optional **`async Provider::describe(&candidate) -> Option<PackageInfo>`**
(default `None`, ADR-0022 growth) called only for the recommended candidate on `info` (never on the search
path). **dnf implements it fully** (one `dnf5 info` call, pure tested `parse_info`: Description/URL/License/
Vendor, first stanza wins, folds continuation lines); **github gives a cheap card** (repo URL + owner as
author from the `owner/repo` already in `raw`, no extra call); every other source inherits `None` and shows
the basic card (name/summary/version/trust/source degrade gracefully). `--json` now returns
`{candidates, recommended, info}` (was a bare array). 185 tests, clippy clean; live-verified (firefox full
dnf card, jqlang/jq github card, ripgrep:cargo sparse card, JSON). **Debt:** dnf License/Vendor shown
verbatim (RPM's SPDX-ish strings); cargo/npm/flatpak richer cards + the GitHub repo-metadata fetch are
follow-ups.
**list+audit merged (ADR-0038).** `jii list` gained a `--audit` flag: bare = the plain NAME/SOURCE/VERSION
table; `--audit` = the security view (trust/verification/concerns + "N need attention"). The **standalone
`jii audit` command is removed** (rendering moved to a private `audit_view` helper; the engine `audit()`
computation + `AuditEntry` model untouched). Same fold-a-command-into-a-flag pattern as ADR-0035. 185 tests,
clippy clean; live-verified (`list`, `list --audit`, and that `jii audit` now falls through to install).

**✅ UX-WAVE 2 COMPLETE** — all agreed items landed and pushed: ① arrow-key TUI choosers, ② doctor-as-
system-helper (+`--fix`, +folded recommend, ADR-0035), ③ providers/marketplace (ADR-0036), ④ info app-card
(ADR-0037), and the list+audit merge (ADR-0038), plus the earlier small fixes (#3/#6/#10/#12/#13).

**#9 follow-up landed (the last loose thread).** A library name (`serde`, `lodash`) used to read as a bare
"not found"; now an optional `Provider::explain_miss` (default `None`, ADR-0022; recorded under ADR-0023)
lets cargo/npm explain "'X' is a library — nothing to install as a program." The engine asks only on a total
miss, gated on `is_available`; the message renders under the miss in install/info/search. 185 tests (signal
already covered by the `library_only_*_yields_no_candidate` tests), clippy clean, live-verified (lodash/serde
explained; a truly-unknown name stays a plain miss; real programs unaffected). **Next:
Beta prep** (see BETA_ROADMAP.md).

**BETA PREP — packaging/distribution landed (ADR-0039).** Owner asked for "convenient install for every
distro without building" + a full pre-release code audit, then cut Beta. Packaging done: **static musl
binaries for x86_64 + aarch64** (one file, runs on every distro incl. ARM; built in CI via `cross`), native
**.deb/.rpm via nfpm** (one `packaging/nfpm.yaml`, both formats/arches, bundling man page + bash/zsh/fish
completions), an **`install.sh`** (`curl|sh`, arch-detect, sha256-verified, → ~/.local/bin), and **hidden
`jii completions <shell>` / `jii man`** (clap_complete/clap_mangen, no build.rs — single-crate-safe).
`release.yml` reworked into a build-matrix + aggregate-publish job attaching all assets on a `v*` tag.
Official-repo scaffolding prepared but not published (needs owner accounts): `packaging/jii.spec` (COPR
binary-repack) + `packaging/aur/PKGBUILD` (`jii-bin`) + `packaging/README.md` turnkey steps. `[profile.release]`
gained lto/codegen-units=1/strip (behavior unchanged). README Install section rewritten. **186 tests, clippy
clean, release binary builds (LTO ~58s); locally validated: tarball layout + install.sh extraction/checksum,
completions/man non-empty, spec parses.** The full release workflow (musl cross-build, nfpm, publish) runs
only on a real tag push — verified by construction + local checks; first run is the owner's `git tag v0.1.0-beta`.
**Pre-release code audit DONE (conservative, behavior-preserving).** Combed the codebase per the owner's
"comb every line, dedup, remove unneeded, optimize — but don't remove useful functions or break it": (1)
**narrowed the model.rs `#![allow(dead_code)]`** module-wide silencer to three *targeted*, documented
`#[allow(dead_code)]` (Query.kind + QueryKind::Description reserved for Phase 6 semantic search;
Verification::Gpg/Sigstore reserved, verifier stubs them fail-closed per ADR-0016) — so future *accidental*
dead code in model.rs is now caught (BETA_ROADMAP debt "narrow or remove it" addressed conservatively, no API
removed); (2) **idiomatic cleanups** at 10 sites (`map(f).unwrap_or(false)`→`is_ok_and`/`is_some_and`,
`map(f).unwrap_or(x)`→`map_or`) across provider/mod, copr, github, go, nix, snap, cli — behavior identical.
Audit findings: **no** TODO/FIXME/unimplemented, **no** panic risks outside idiomatic `Mutex::lock().unwrap()`
(poison-only), **no** renderer-bypass printing (all stdout is in main's error-reporter, the Renderer itself,
the prompt, and the deliberate completions/man output), no further worthwhile dedup (providers already share
helpers, ADR-0027). Standard clippy stays clean; the 241 pedantic/nursery hits are style-only (`Self::`,
const-fn, doc backticks) and deliberately not chased (churn without value, against "don't break it").
Behavior re-verified live (providers/info/sources/library-miss unchanged). 186 tests, clippy clean, release
builds. **Next: owner cuts Beta** (`git tag v0.1.0-beta`); optional remaining agent work: integration tests
(BETA item 2) + CONTRIBUTING/SECURITY docs (item 4). clean-VM verification (Arch/Ubuntu/Debian/openSUSE) needs
the owner's hosts. `cli/mod.rs` split stays deferred (owner chose conservative cleanup).

**FIRST BETA PUBLISHED (2026-07-09).** `v0.1.0-beta` released end-to-end after two CI fixes (nfpm from the
goreleaser apt repo — install-action can't; render nfpm.yaml via `envsubst` — nfpm left `${BIN}` unexpanded).
Release has 12 assets: tarball/.deb/.rpm for x86_64 + aarch64 (+ sha256). **Not yet installed/run on a live
host** (risk #1 open — cross-distro + arm64 unproven).

**Self-update/uninstall landed (ADR-0040).** Owner-requested: `jii` is now a reserved name meaning the tool
itself. `jii update jii` self-updates from the newest GitHub release the right way for how it was installed —
**user-space binary** (install.sh/tarball/cargo) is atomically swapped in place (new `Action::Replace` =
`fs::rename`, no `ETXTBSY`, no root); **package** (.rpm/.deb) is upgraded via dnf/apt as a previewable root
step (never clobbers the package db). `jii uninstall` / `jii remove jii` self-remove. Bare `jii update` updates
everything — the system and then JII itself (self-update runs last, still prompts). All previewable (`--dry-run`); version compare is opaque "different tag → offer"
(ADR-0009). New `src/selfupdate.rs` (detection + release fetch + asset selection + pure plan builders,
unit-tested); `Engine::run_self_plan` executes via the existing executor+privilege. `Cargo.toml` version
aligned to `0.1.0-beta`. **Verify once Bash classifier is back: clippy + full test suite (expected ~192).**
The self-update fetch+swap can't be exercised until the owner cuts the *next* tag — pure parts tested, network
path verified by construction + `--dry-run`.

**Superseded — BETA-READINESS FEATURE FREEZE (2026-07-06).** Was: freeze features, drive to Beta
(CI ✓ already present; release workflow + install docs landed — see [BETA_ROADMAP.md](BETA_ROADMAP.md)).
The VM run reprioritised to UX-wave 2 *before* cutting Beta; the Beta plan still stands and resumes
after this polish wave. Release infra is ready (owner cuts it by pushing a `v*` tag). The UX-polish pass (U0–U8) is complete
and the CLI is functionally done. The owner has **frozen new large features** and set the drive to the
**first public Beta**, priority order: **(1) CI → (2) integration tests → (3) clean-VM verification on
Arch/Ubuntu/Debian/openSUSE → (4) README/logo/screenshots/asciinema/docs → (5) public release.** The
plan and the parked backlog (undo, bootstrap, version chooser, doctor --fix, declarative providers,
etc.) live in **[docs/BETA_ROADMAP.md](BETA_ROADMAP.md)** — its "Frozen" section must NOT be started
without an explicit post-Beta go-ahead. Bug fixes / hardening / tests / docs / packaging stay in
scope; new user-facing capabilities do not. **#3 (clean-VM) is the one Beta blocker an agent can't
close alone** — it needs the owner's real non-Fedora hosts (agent can script the smoke test). Next
recommended action: **#1 CI** (GitHub Actions: build + clippy -D warnings + test + fmt --check).

**Prior phase — Terminal 1.0 (ADR-0026), UX-polish pass (2026-07-06, DONE).** After dogfooding on a
clean Fedora VM the owner re-prioritised to **UX polish, no new features**; the remaining T5 feature
slices (GitHub by-name repo chooser, version chooser) are **deferred** (now parked in BETA_ROADMAP).
Plan + classification live in **[docs/UX_EVALUATION.md](UX_EVALUATION.md)** (U0–U8, "Progress" is the
live status).
Doctor scope decided: **Tier 1 + the recommend-catalog, both in 1.0** (own catalog ADR; ROADMAP
"Analyze→Explain→Ask→Apply" holds). **Landed so far (all [A], no ADR):** U0 measured (startup ~0ms
fine; cold search was 8s because one straggler — copr, ~9s API — burned the timeout, not a
parallelism problem); U1 killed unavailable-provider spam + de-duped the single-package preview; U2
lowered the search timeout 8→5s (search 8.05→5.08s); U3 added an already-installed pre-check
(targeted `installed_lookup`, in-place update offer) and multi-owner `remove` (`resolve_all_installed`
+ chooser with "all"). U4 landed the `PackageSpec` grammar (ADR-0031) across install/remove/update/
info; **U5** added Friendly/Advanced modes + the first-run wizard/`jii setup`; **U6** added
actionable errors (ADR-0032), doctor Tier 1 system checks, and the recommend-catalog (ADR-0033:
`jii recommend` list + apply); **U7** made bare `jii update` a system-wide upgrade (ADR-0034:
`plan_update_all` across all bulk managers + per-record fallback); **U8** was the final walkthrough
polish — aligned, headered tables for `list`/`history`/`audit` (one data-driven `table_lines` helper,
`Action::label()` for human history verbs) and a fix for `jii update <not-installed>` no longer
claiming the ledger is empty. **The UX-polish pass (U0–U8) is complete.** 180 tests green throughout.

**CLI grammar LOCKED — ADR-0031.** After a first-principles pass (UX_EVALUATION §E/§E.1) the package
spec **`name[:source][@ref]`** is now the *language of JII*: source/version/channel belong to the
**spec**, not flags; `@ref` is **source-interpreted** (core never parses it, ADR-0004/0009); the spec
is universal across install/remove/update/info and an explicit `:source` suppresses the chooser.
Durable binding principle: *"does this belong to the package or the command?"* — package attributes
extend `PackageSpec`, never a new flag. `--auto`→`-y`, `--profile`→config/wizard, `--source` demoted
to whole-command synonym. **Syntax is settled — do not re-open it.**

**U4 landed (ADR-0031 + #4 + D5).** `PackageSpec::parse` (pure, `model.rs`, 11 tests) for
`name[:source][@ref]`; wired into **install** — `:source` pins the provider and suppresses the chooser,
`@ref` parsed but explicitly rejected until the version chooser lands, unknown source → did-you-mean,
explicit source with no match → honest miss (no silent substitution); clap untouched, backwards
compatible. **D5**: optional `Provider::highlights` (dnf/copr/flatpak/github/cargo) → `jii info` reads
like the README; UI still never branches on source id. **Chooser (#4):** clearer header + "⭐
recommended" tag. **162 tests green, clippy clean**, verified on Fedora (pty chooser, info, spec paths).

**ADR-0031 tail done:** the spec is now universal — `remove`/`update`/`info` parse it too (same
`parse_specs`). `jii remove firefox:flatpak` pins the copy (the non-interactive answer to the
multi-owner chooser); `update node:brew` picks the copy to update; `info firefox:flatpak` narrows
(`ranked_for` gained a `source` override). `@ref` rejected everywhere; `search` stays free-text.
**U4 complete** — 162 tests green, clippy clean.

**U5 landed (D8 + DW).** Friendly/Advanced output modes + first-run wizard/`jii setup` + a clap fix.
**U6 landed (D7 + D6, ADR-0032/0033).** Actionable errors (`JiiError::remedy`), doctor Tier 1
system checks, recommend-catalog. **U7 landed (D10, ADR-0034).** System-wide `jii update`
(`plan_update_all` across all bulk managers + per-record fallback). Both detailed under "Last
completed work". **177 tests green.**

**Next: U8** — first-run walkthrough polish (the last UX track). Then the UX pass is complete.
Streaming/progressive search (UX_EVALUATION §A, own ADR) is the real speed fix and is on the list.
`--auto`→`-y`, `--profile`→config, `--no-color`→NO_COLOR are the flag-shed follow-ups from ADR-0031.
Structural cleanup queued: **split `cli/mod.rs`** (~1700 lines) into `cli/commands/*`. Recommend
follow-ups: interactive multi-pick, skip already-satisfied entries, a real repo-enable capability (so
RPM Fusion becomes a previewable plan, not a shown command). Update debt: a bulk-updated tracked
package can show a stale version in `jii list`.

<details><summary>Earlier T1–T3 detail (all landed)</summary>

**Terminal 1.0 (ADR-0026) — T1 & T2 done; T3 next.** Priority changed (ADR-0026): finish the
*whole* terminal version ("CLI 1.0") before the first public Beta, instead of going straight to
Homebrew. The full ordered plan is T1–T8 in [ROADMAP.md](ROADMAP.md) / [TASKS.md](TASKS.md); the
scope + the three pre-declared architecture growths (platform-seam relax, provider-ordered
versions, bootstrap-as-plan) are in **ADR-0026**.

**T1 (read-only honesty layer) landed:** `jii search` (ranked candidates, top `→`), `jii info`
(sources + recommendation with a **source-agnostic** rationale — no branching on the source id),
`jii sources` (active vs enabled-but-unavailable). Pure rendering over `search`/`rank`; engine
gained `source_catalog()`. Old `search`/`info` stubs + `not_yet` gone; README de-lied.

**T2 (batch update/remove) landed:** `jii update a b c` / `jii remove a b c` (and `jii update` =
all). Exactly the ADR-0025 machinery — **no new architecture**. Optional
`plan_remove_many`/`plan_update_many` (dnf/copr/flatpak/cargo/npm + go-update; the rest inherit
`None` → per-record fallback). Engine gained generic `group_by_source`, `RecordOp`,
`plan_record_batch` (→ `RecordBatch { plans, unplannable }`: an un-updatable package like a
github install is reported, never fatal), and `remove_batch`/`update_batch` mirroring
`install_batch`. Single = batch of one; the old single `Engine::remove`/`update`/`plan_remove`/
`plan_update` and `exec::run_plan` were removed (one write-path). Update carries the post-update
record (version = refreshed target); engine stamps installed_at/verification. Verified via
isolated `XDG_STATE_HOME` dry-runs (merged `dnf5 remove/upgrade`, mixed dnf+cargo grouping,
version transitions, single-package richer plan).

**T3 (provider breadth) landed — Homebrew, Snap, AppImage:**

**Homebrew (`brew`):** `provider/homebrew.rs`, same proven shape as cargo/npm/pipx/go —
formula API (`formulae.brew.sh/api/formula/<name>.json`) via `get_json_opt`, unprivileged `brew
install/uninstall/upgrade` (+ `_many`), `brew list --versions`, community trust, no library filter
(ADR-0023). Registered in config (`KNOWN_SOURCES` + default priority; `is_available` gates it off
where `brew` is absent). **Empirical scaffold verdict — ADR-0027: NO shared `RegistryProvider`.**
After 5 providers the only identical code is ~8 lines of boilerplate; `search`/plans/`list_installed`
are irreducibly per-provider; the genuine sharing already lives in the free-function helpers
(`get_json_opt`/`command_plan`/`run_capture`/`which`/…). Verified: real formula API shape matches
the structs (curl), 404→empty, `jii sources` lists brew.

**Snap (`snap`):** `provider/snap.rs` — first **system** provider in the breadth track (root;
`sudo snap install`). Store info API (`api.snapcraft.io/v2/snaps/info/<name>?fields=…`, needs the
`Snap-Device-Series` header → `http_client` directly, like github/copr). `snap install/remove/
refresh` (+ `_many`). **Classic confinement** handled: verifying the live API showed `confinement`
is only returned as an explicit `fields` item (and `fields` restricts the response), so the query
lists `version,confinement,summary,title`; classic snaps get `--classic`, and a classic snap in a
batch declines the merge (`--classic` can't apply selectively) → per-snap fallback. `snap list`
parsed. Community trust; registered near flatpak in priority.

**AppImage (ADR-0028): not a standalone provider.** It has no manager/API and its catalog
(`appimage.github.io/feed.json`) has no download URLs — it is a *delivery format over GitHub
releases*. So `github::classify` now accepts `.AppImage` assets as raw binaries **without** the
`linux` token (AppImages are Linux-only; arch still required; `.AppImage.zsync` rejected).
`jii owner/repo` installs an AppImage today; by-name discovery folds into T5 (repo chooser). The
reserved `"appimage"` id was removed from `KNOWN_SOURCES`.

<details><summary>Homebrew reference (from ADR-0024, the original T3 pick — now landed)</summary>

Same shape as cargo/npm/pipx/go: `is_available` (`brew`), `search` via the formula API
(`https://formulae.brew.sh/api/formula/<name>.json`) with `get_json_opt`, `plan_install`/
`update`/`remove` = single unprivileged `brew install`/`upgrade`/`uninstall` via
`command_plan` (no root; brew is user-owned), `list_installed` (`brew list --versions` or
`--json`), community trust. Handle formula-vs-cask (casks are GUI apps; on Linux brew is
formula-only, so start formula-only). **Empirical check while doing it:** this is the 5th
registry-user-space provider — evaluate (do not assume) whether a thin shared
`RegistryProvider` scaffold now pays off (resolved: ADR-0027, no scaffold).

</details>

Recorded non-blocking debts to respect (ADR-0024): version comparison (add a
provider-computed normalized key beside `PkgVersion`'s raw string only when version-aware
work is next needed), and splitting `cli/mod.rs` (~615 lines) into `cli/commands/*` when it
next grows.

Polish/hardening deferred (not blocking Phase 5; several are now **future features**, do
not implement as silent heuristics):
- **GitHub repository selection** — interactive, "never silently install the wrong repo".
- `.tar.xz` archives (needs an xz decoder dep); better COPR disambiguation; real
  GPG/sigstore verification in `exec.rs::verify_bytes` (currently fail-closed).
- **Engine UI-free seam** (ADR-0022): decouple `&Renderer` from `Engine::install/remove`
  — do this **before** any GUI/second frontend, not now.

Full list in [TASKS.md](TASKS.md) Phase 5.

</details>

## Next recommended task

**T5 (remaining) — the GitHub by-name repo chooser, then the version chooser.** The generic
candidate chooser is done (pure `cli`/`ui`, no ADR); what's left needs real new **provider
capabilities** and so each gets its own ADR:
- **GitHub by-name repo discovery** — github currently answers only explicit `owner/repo`; add a
  bare-name path (github `/search/repositories`) that returns the top few repos (with an
  installable Linux release) as candidates, which then flow into the existing chooser so the user
  disambiguates ("never silently install the wrong repo"). ADR: name→repo policy — ranking
  (stars? exact-name?), filtering to repos that actually publish a usable release, and how many to
  surface. This is the noisier/riskier piece; keep it conservative.
- **Version chooser** — `--version <v>` + an optional `Provider::available_versions` (provider-
  ordered, ADR-0022 growth pattern) so a source can offer real version choices. ADR for the
  version growth (pre-declared in ADR-0026); note the per-source pinning-syntax divergence
  (dnf `pkg-1.2.3`, cargo `--version`, github a release tag).

After T5: T6 (bootstrap a missing manager — where the `id`/`id_like` distro predicate finally
gets built, ADR-0029), T7 (hardening + **clean-VM testing on Fedora/Arch/Ubuntu/Debian/openSUSE**,
incl. verifying the nix provider **and the chooser interactively** on a live host), T8 (public
polish). Then the first Beta.

## Current blockers

None.

## Build status

`cargo build` — clean, no warnings. `cargo clippy` — clean.

## Test status

`cargo test` — **297 passing, 0 failing** (see the batch sections above for what the newer tests
cover; the notes below describe the older baseline). Packaging coverage: `cli::cli_definition_is_valid`
(`clap` validates the whole command tree, incl. the new hidden `completions`/`man`). ④ coverage: `dnf::parse_info_takes_first_stanza_and_folds_continuations`
(folded description, URL/Vendor, first stanza wins over a later one). ③ coverage: `provider::ecosystems_declare_bootstrap_and_base_repos_do_not`
(every ecosystem manager declares a non-empty binary + a usable `Bootstrap`; dnf/github declare none). U7 coverage: dnf/flatpak `plan_update_all` (whole-system
upgrade, root vs user). U6 coverage: `error::remedy` (unknown-source lists the
known ones, Io branches on `ErrorKind`, opaque errors invent nothing), `cli::system_checks` (PATH +
token pass/fail + advice + env-name), `recommend` (embedded catalog parses, ids unique, empty-distros
applies everywhere, distro filter selects matching). U5 coverage: `config` mode/first-run
(`mode_defaults_to_friendly_and_first_run_is_true`, TOML round-trip, partial-TOML mode parse). U4
coverage: `PackageSpec::parse` (11 cases — plain/source/ref combos, npm scope safety, last-colon/
last-at split, trimming, structural errors). T5 coverage: `prompt::parse_choice` (empty→default,
`n`/`no`/`q`/`quit`/`cancel`→cancel, in-range number→zero-based pick, out-of-range/garbage→invalid).
T4 coverage: apt (`parse_show` first-stanza deb822,
Description-md5/folded-body excluded, batch install/remove/update-only-upgrade), pacman (`parse_si`
first stanza with URL-in-value intact, `parse_query`, `-Rs` remove, batch), zypper (`parse_search_xml`
skips `<solvable-list>` container, dep-free `attr`, non-interactive root plans), nix (`parse_search`
exact-`pname` over near-names, unprivileged flake install/upgrade). Earlier coverage: homebrew
(formula→candidate, unprivileged
plan, `brew list --versions`, batch), snap (candidate + classic detection, root plan, `--classic`,
batch merge-vs-decline, `snap list`), github `.AppImage` acceptance (no-`linux` token, wrong-arch/
`.zsync` rejection), `info`/`search` rendering helpers
(`recommendation_reasons` source-agnostic rationale, `one_line`, `candidate_line`),
`group_by_source` (first-seen order), batch remove/update merges (dnf root remove+upgrade,
cargo uninstall, flatpak update), dnf/flatpak parsers, ranking,
registry (incl. `record_update` version refresh + `Update` history), cache, privilege
elevation prefixing, the executor (sha256 digest,
verification accept/reject/case-insensitive/fail-closed, place+mode+remove, tar.gz **and
zip** extract + member selection, unknown-format rejection, run_action), github
(owner/repo, release JSON, asset selection incl. `.zip`/tar.gz preference, checksums,
plan shapes), copr (search parsing, exact-name + fedora/arch chroot selection, two-step
root plan), cargo (binary-crate vs library-only candidate filtering, unprivileged plan
shape, `cargo install --list` parsing), npm (CLI vs library-only filter incl. bin-as-
string, user-prefixed plan shape, `npm ls -g --json` parsing), pipx (candidate shape,
install/upgrade plans, `pipx list --json` parsing), go (candidate shape, unprivileged
`go install @latest` plan, binary-name derivation incl. `/v2` major-version skip, proxy
uppercase→`!x` escaping), **batch merge** (dnf/cargo/go `plan_install_many` collapse a
group into one command), audit (verification resolution +
concern logic), and doctor health mapping (`health_from` precedence).

## Environment & commands

- **Target/dev OS:** Fedora (dnf5). Rust edition 2024.
- **Build:** `cargo build` (must be warning-clean).
- **Lint:** `cargo clippy` (installed; must be clean).
- **Test:** `cargo test`.
- **Preview a plan:** `cargo run -- install <pkg> --dry-run` (no side effects).
- **`cargo fmt` / `rustfmt` are NOT installed** on this dev host — match the
  surrounding code style by hand; do not rely on `cargo fmt`.
- External tools invoked at runtime: `dnf5`, `flatpak`, `sudo`/`pkexec`. GitHub
  provider (next) will use HTTPS via `reqwest` and optionally `GITHUB_TOKEN`.
- **CI** (`.github/workflows/ci.yml`) runs clippy (`-D warnings`) + tests on every
  push/PR — the automated Definition of Done (ADR-0013). The runner has no
  dnf5/flatpak, so end-to-end `--dry-run` checks stay manual on Fedora.

## Important architectural decisions (quick reference)

Full rationale in [DECISIONS.md](DECISIONS.md). The load-bearing ones:

- **Core never branches on source** — everything behind the `Provider` trait (ADR-0004).
- **`Plan` is first-class** — declarative `Action`s, always previewable via `--dry-run`
  (ADR-0003), executed by `exec.rs` (ADR-0007).
- **JII never fully root** — only concrete steps escalate, via `privilege.rs` (ADR-0005).
- **Trust threshold, not global yes** — `untrusted` always confirmed (ADR-0006).
- **Single crate**, **JSON registry** (not SQLite), **`PkgVersion(String)`** not semver
  (ADR-0001/0002/0009).

## Known technical debt

- **COPR project ambiguity** — several projects can share a package name; we pick the
  exact-name match building for the most Fedora chroots, but that is a weak signal (a
  fork may build widely). The visible `owner/project` in the plan + confirmation is the
  safety net. A real popularity/quality metric isn't in the search API (ADR-0017).
- **GitHub archives: `.tar.gz`/`.tgz` + `.zip`** — `.tar.xz`-only releases still yield
  no candidate (ADR-0016); adding it means an xz decoder dependency. `.zip` entries
  authored on non-unix systems carry no mode, so the sole-executable fallback can't fire
  — the exact-basename match still resolves the common single-binary case.
- **GitHub binary named after the repo** — the placed file is `~/.local/bin/<repo>`;
  when the archive's binary basename differs (e.g. ripgrep's `rg`), it's still
  installed as `<repo>`. Fine for now (repo==binary in the common case).
- **Flatpak identified by appid** (`org.gimp.GIMP`): `jii remove gimp` may not resolve
  a Flatpak by friendly name. Revisit with a name/id split if it becomes painful.
- **`latest`/`minimal` profiles + freshness/health ranking tie-breakers** are reserved
  — they need comparable versions / dependency-footprint data not yet collected.
- **GPG / sigstore verification** are stubbed to fail closed in `exec.rs::verify_bytes`
  — implement when a source needs them (GitHub).
- **`cli/mod.rs`** (~1700 lines after U4–U7 — spec parsing, the wizard, Friendly preview,
  doctor Tier 1, `recommend`, system update) holds every command handler inline. It has now well crossed the
  "unwieldy" line flagged in ADR-0024; splitting into `cli/commands/*` (one module per subcommand
  + a shared helpers module) is the next structural cleanup, best done between UX slices so it
  doesn't collide with in-flight feature work.
- **recommend-catalog is hand-curated + Fedora-only (ADR-0033).** The `data/recommend/catalog.toml`
  entries (package names, the RPM Fusion command) are authored by hand and **not verified on a
  clean VM**; verify in the T7 clean-VM pass. Non-Fedora entries are deliberately empty until
  verified on a real host. `manual` (repo-enable) entries are shown, never run — a real repo-enable
  capability (previewable plan) is a follow-up.
- **System update doesn't refresh the registry (ADR-0034).** Bare `jii update` runs each manager's
  bulk upgrade (`dnf upgrade`, `flatpak update`, …), which upgrades packages beyond JII's ledger, so
  those plans are **not** recorded — only the per-record fallbacks (github/cargo/go) refresh the
  registry. Consequence: after a system update, `jii list` may show a stale version for a
  bulk-updated *tracked* dnf/flatpak package. Re-querying every tracked version per update is
  expensive; accepted for MVP. The non-Fedora `plan_update_all` impls are unverified on a live host.
- **pipx/go offer libraries (ADR-0023, by design):** PyPI/Go expose no reliable
  program-vs-library signal, so `pipx`/`go` don't pre-filter (cargo/npm do). They offer
  the package; the tool rejects a non-app at install. Accepted — a visible false positive
  beats silently hiding a real app. Add a filter only if reliable metadata appears.
- **Engine↔UI seam (ADR-0022):** `Engine::install`/`remove` take `&crate::ui::Renderer`
  so the executor can print progress — the one `ui` type reaching into the engine. Fine
  now (single CLI frontend), but it must be decoupled (a progress-event/`ProgressSink`
  trait) **before** a GUI/second frontend or a workspace split. Meanwhile: **do not add
  new `ui` types to engine signatures.**
- **nix provider is untested on a live host + version-fragile (T4).** Implemented against the
  modern flakes CLI; no Nix host was available here. `nix profile` remove/list schemas have
  shifted across Nix versions — `list_installed` returns empty and `is_installed` checks
  `~/.nix-profile/bin/<name>` (go-style; name==binary caveat). Verify search/install/remove/
  upgrade on a real Nix/NixOS box in the T7 clean-VM pass.
- **apt/pacman version = first search stanza (T4).** `apt-cache show`/`pacman -Si` list versions
  highest-first, so the first stanza is taken as the candidate version. It is informational; the
  actual `apt-get`/`pacman` install resolves the real candidate regardless. Fine for MVP.
- **apt non-interactive relies on `-y` only.** No `DEBIAN_FRONTEND=noninteractive` is set (the
  `Action` model runs argv without env); revisit if a package's postinst prompts.

## Where things live

```
src/
  model.rs       core types (Action, InstallPlan, PackageCandidate, TrustLevel…)
  provider/      Provider trait + http_client/get_json_opt/command_plan/run_capture[_lax] +
                 dnf, copr, apt, pacman, zypper, nix, flatpak, snap, github, cargo, npm,
                 pipx, go, homebrew
  engine/        orchestration (search→rank→plan→execute) + ranking.rs;
                 any_source_available() = source-based "supported" (ADR-0029)
  exec.rs        plan executor (the one place that runs a plan's actions)
  privilege.rs   sudo/pkexec elevation (prime + run)
  cache.rs       on-disk TTL search cache (stale-on-error)
  registry.rs    JSON install registry
  recommend.rs   recommend-catalog: typed model + embedded-TOML loader + distro filter
  cli/, ui/, config.rs, platform.rs, error.rs
data/            recommend/catalog.toml — the curated recommend-catalog (embedded at build)
docs/            ARCHITECTURE (canonical) · ROADMAP · TASKS · DECISIONS · this file
AGENTS.md        tool-neutral onboarding entry (read first); CLAUDE.md = Claude's copy
LICENSE          MIT
```

To add a source: implement `Provider` (or a declarative TOML later) — never edit the
core. Use the `/new-provider` skill.
