<div align="center">

# 🚀 JII — Just Install It

**A smart, universal package _installer_ for Linux.**

You think about *software*. JII figures out *how* to install it — from every source on your
machine — picks the best option, and explains *why*.

<br>

[![Release](https://img.shields.io/github/v/release/0nigiris/JII?include_prereleases&sort=semver&label=release&color=blue)](https://github.com/0nigiris/JII/releases)
[![Build](https://img.shields.io/github/actions/workflow/status/0nigiris/JII/release.yml?label=release%20build)](https://github.com/0nigiris/JII/actions)
[![License: GPL v3](https://img.shields.io/github/license/0nigiris/JII?color=green)](LICENSE)
[![Made with Rust](https://img.shields.io/badge/Rust-edition%202024-orange?logo=rust)](https://www.rust-lang.org/)
[![Platform: Linux](https://img.shields.io/badge/platform-Linux%20(x86__64%20%7C%20aarch64)-lightgrey?logo=linux)](#install)
[![Status: Beta](https://img.shields.io/badge/status-beta-yellow)](#-status)

</div>

---

```console
$ jii fastfetch

Searching...
  ✓ DNF
  ✓ COPR
  ✓ GitHub Releases
  ✓ Flatpak

Recommended: DNF — Official Fedora package, v2.21 (latest)
  ✓ Highest trust        ✓ Automatic updates
  ✓ Official package     ✓ Version matches upstream

Install? [Y/n]
```

One command. Every source. The best pick, explained — no `sudo`, no guessing which manager
has it.

<div align="center">

### [Install](#install) · [Quick start](#quick-start) · [Commands](#command-reference) · [How it works](#how-it-works) · [Sources](#sources--providers) · [Config](#configuration) · [FAQ](#faq)

</div>

---

## Table of contents

- [What JII is (and isn't)](#what-jii-is-and-isnt)
- [Why JII](#why-jii)
- [Install](#install)
- [Quick start](#quick-start)
- [Command reference](#command-reference)
- [How it works](#how-it-works)
- [Sources & providers](#sources--providers)
- [Trust & safety](#trust--safety)
- [Updating JII itself](#updating-jii-itself)
- [Package spec syntax](#package-spec-syntax)
- [Output modes](#output-modes)
- [Configuration](#configuration)
- [Status](#-status)
- [FAQ](#faq)
- [Architecture & docs](#architecture--docs)
- [Contributing](#contributing)
- [License](#license)

---

## What JII is (and isn't)

JII is **not** a package manager. It sits *on top of* the ones you already have and
orchestrates them:

> **DNF · COPR · apt · pacman · zypper · XBPS · Nix · Flatpak · Snap · GitHub Releases (incl.
> AppImage assets) · Cargo · npm · pipx · Go · Homebrew**

It searches all of them at once, ranks the results by trust, freshness and your chosen profile,
recommends the single best option, and installs it — transparently, and without ever running
fully as root.

|  | JII | A package manager (dnf/apt/…) |
|---|---|---|
| **Scope** | Every source at once | One ecosystem |
| **Job** | *Choose* the best way to install | *Execute* one install |
| **You need to know** | The software's name | Which manager has it, and its exact package id |
| **Root** | Only the concrete step that needs it | Usually the whole run |
| **Explains its choice** | ✓ `jii why <name>` | ✗ |

---

## Why JII

- **🎯 One command for everything** — no need to know whether software lives in DNF, Flatpak,
  COPR, or a GitHub release. Just `jii <name>`.
- **🔒 No blanket `sudo`** — JII asks for elevation only when a step actually requires it, batches
  those steps, and shows you the **exact command first**. It never runs fully as root.
- **💡 Explains every decision** — `jii why <name>` tells you *how* something was installed and
  *why* that source won.
- **🛡️ Safe by default** — every source carries a trust level; **auto mode never installs an
  untrusted source automatically**, and artifacts are verified (GPG / sha256 / sigstore) where the
  source provides it.
- **👀 Previewable** — `--dry-run` prints the full plan before anything happens. Nothing is a
  surprise.
- **🧠 Remembers** — `jii remove discord` uses whatever installed it; `jii update` updates each
  package with the correct manager, and can upgrade your whole system in one go.
- **♻️ Updates itself** — `jii update jii` (or a bare `jii update`) pulls the newest release from
  GitHub and swaps itself in place — no root for a user-space install.

---

## Install

JII ships prebuilt for **x86_64** and **aarch64** as a static [musl] binary — **one file, no
runtime deps**, runs on every Linux distro (glibc or musl, old or new). No compiling required.

### One-liner (recommended)

Installs to `~/.local/bin`, **no root**:

```console
$ curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/master/install.sh | sh
```

It auto-detects your CPU, downloads the matching binary from the latest release, verifies its
sha256, and installs it. Then run `jii doctor` to confirm it sees your sources.

### Native package (`.rpm` / `.deb`)

Grab the file for your arch from [Releases](https://github.com/0nigiris/JII/releases) — it also
drops a man page and shell completions:

```console
$ sudo dnf install ./jii-*.rpm      # Fedora / RHEL / openSUSE
$ sudo apt install ./jii_*.deb      # Debian / Ubuntu
```

### Arch (AUR)

```console
$ yay -S jii-bin        # once published — see packaging/README.md
```

### Manual tarball

If you prefer to place it yourself:

```console
$ tar -xzf jii-v0.1.3-beta-x86_64-linux.tar.gz
$ sha256sum -c jii-v0.1.3-beta-x86_64-linux.tar.gz.sha256   # optional integrity check
$ install -Dm755 jii-v0.1.3-beta-x86_64-linux/jii ~/.local/bin/jii
```

### Build from source

Needs a recent Rust toolchain (edition 2024):

```console
$ git clone https://github.com/0nigiris/JII && cd JII
$ cargo install --path .    # then make sure ~/.cargo/bin is on your PATH
```

> **Note.** JII drives the package managers you already have — it doesn't bundle any. On a machine
> with none of its supported sources (dnf5, Flatpak, apt, …) it runs fine but finds nothing to
> install. Run `jii doctor` any time to see what it detects.

[musl]: https://musl.libc.org/

---

## Quick start

```console
# Install something — JII searches every source, ranks, recommends, installs
$ jii fastfetch

# Install several at once
$ jii ripgrep bat fd-find

# Install without prompts (only within your trust threshold)
$ jii fastfetch --auto

# Just look — see the ranked candidates, install nothing
$ jii search markdown editor

# Preview the exact plan without touching anything
$ jii docker --dry-run

# Pin the source right on the name
$ jii firefox:flatpak

# A GitHub release by repo
$ jii jqlang/jq

# The full app card: description, homepage, license, every source + why
$ jii info neovim

# Why did that get installed the way it did?
$ jii why neovim

# Update everything — the whole system *and* JII itself
$ jii update

# Remove it — using whatever installed it
$ jii remove discord

# Is my machine healthy? What sources can JII see?
$ jii doctor
```

---

## Command reference

```
jii <name…>          search → rank → recommend → install (one or many packages)
jii remove <name…>   remove using the source that installed each package
jii update [<name>]  named: update those packages · bare: update the whole system + JII itself
jii update jii       update JII itself from the latest GitHub release (self-update)
jii search <query>   show ranked candidates without installing
jii info <name>      app card: description, homepage, license, author + all sources & why
jii how <name>       explain how JII would install (or did install) it   (alias: jii why)
jii sources          list installation sources and whether each is usable here
jii providers        show ecosystem managers (npm, cargo, brew, Flatpak…) + what's installed
jii providers add    bootstrap a missing manager (e.g. jii providers add npm)
jii doctor           diagnose sources + host, then interactively set up what's missing
                     (git/curl, PATH, Flathub, RPM Fusion, codecs, fonts…) — one y/n per item
jii list             what JII installed  (add --audit for signatures, trust & concerns)
jii history          installation history
jii setup            re-run the first-run wizard (output mode, optional doctor)
jii uninstall        remove JII itself (same as jii remove jii)
```

### Global flags

| Flag | Meaning |
|---|---|
| `--auto` | Install the recommended option without confirmation (still within trust limits) |
| `--source <id>` | Force a specific source (e.g. `--source flatpak`) |
| `--profile <p>` | Ranking preset: `stable` · `latest` · `sandbox` · `minimal` |
| `-d`, `--dry-run` | Show the full plan, change nothing |
| `-y`, `--yes` | Assume "yes" to prompts |
| `-n`, `--no` | Assume "no" to prompts |
| `--json` | Emit machine-readable JSON |
| `-v`, `--verbose` | Full detail: per-source failures, the complete plan (repeatable) |
| `--no-color` | Disable ANSI colors |
| `--lang <l>` | UI language (`en`, `ru`); overrides config + `$LC_MESSAGES` |

---

## How it works

Every JII action is built as an **`InstallPlan`** *before* anything executes — which is why
`--dry-run` can show you exactly what would happen. The pipeline:

```
       ┌──────────┐     ┌────────┐     ┌───────────┐     ┌──────────┐     ┌─────────┐
name → │  Search  │ →   │  Rank  │  →  │ Recommend │  →  │   Plan   │  →  │ Execute │
       │ all srcs │     │ score  │     │  explain  │     │ preview  │     │ w/ sudo │
       └──────────┘     └────────┘     └───────────┘     └──────────┘     └─────────┘
       fan-out, in      trust ×         the single       InstallPlan      only the
       parallel; each   freshness ×     best pick,       (previewable     steps that
       source gates     profile ×       with reasons     via --dry-run)   need it, exact
       on its tool      priority                                          command shown
```

1. **Search** — JII queries every usable source in parallel. Each source self-gates: if its tool
   isn't installed, it quietly sits out (no errors, no noise in Friendly mode). Matching is
   **exact-first, then it broadens on a miss**: `jii ayugram` finds `ayugram-desktop`, and even a
   trailing typo like `jii ayugramm` still gets there — JII shows *"No exact match — closest:
   ayugram-desktop"* and lets you confirm or decline. Common exact queries (`jii git`) stay
   noise-free and fast.
2. **Rank** — candidates are scored by **name-match closeness** (an exact name beats a prefix beats
   a substring), then **trust level**, your **profile**, and source **priority**. The core never
   branches on the source name — it only reasons over the `Provider` trait and a uniform
   `PackageCandidate` model.
3. **Recommend** — the top candidate is presented with plain-language reasons ("Official package",
   "Highest trust", "Version matches upstream").
4. **Plan** — the chosen install becomes an `InstallPlan` of concrete actions (download, verify,
   extract, run command, replace…). `--dry-run` stops here and prints it.
5. **Execute** — actions run through the executor; only the steps that truly need root escalate via
   `sudo`/`pkexec`, batched, with the exact command shown first.

### Ranking profiles

| Profile | Prefers | Use it when |
|---|---|---|
| `stable` *(default)* | Distro repositories | You want the well-tested, auto-updated option |
| `latest` | Freshness over priority | You want the newest version, wherever it lives |
| `sandbox` | Flatpak | You prefer sandboxed, self-contained apps |
| `minimal` | Smallest dependency footprint | You want the leanest install |

Set a default in config (`[install] profile = "stable"`) or override per-run with `--profile`.

---

## Sources & providers

JII understands **14 sources** today. Each is a `Provider`; the core adds a new one just by
implementing the trait (simple sources are declarative). Every source self-gates on its tool, so
JII uses whatever is present on your machine — mix and match freely.

| Source | Ecosystem / what it is | Typical trust |
|---|---|---|
| **DNF** | Fedora / RHEL official repositories | 🟢 Official |
| **COPR** | Fedora community build service | 🟡 Community |
| **apt** | Debian / Ubuntu repositories | 🟢 Official |
| **pacman** | Arch Linux repositories | 🟢 Official |
| **zypper** | openSUSE repositories | 🟢 Official |
| **XBPS** | Void Linux repositories | 🟢 Official |
| **Nix** | nixpkgs | 🟡 Community |
| **Flatpak** | Flathub apps (sandboxed) | 🟢/🟡 Official-verified · Community |
| **Snap** | Snap Store | 🟡 Community |
| **GitHub Releases** | Upstream release binaries (incl. AppImage) | 🔴 Untrusted |
| **Cargo** | crates.io (Rust) | 🟡 Community |
| **npm** | npm registry (Node) | 🟡 Community |
| **pipx** | PyPI apps (Python) | 🟡 Community |
| **Go** | Go modules | 🟡 Community |
| **Homebrew** | Linuxbrew | 🟡 Community |

Run **`jii sources`** to see which are usable on *your* box, and **`jii providers`** to see the
ecosystem managers and what's installed. Missing one? `jii providers add npm` bootstraps it.

---

## Trust & safety

Safety is a first-class design constraint, not an afterthought.

- **Three trust levels** — `official` › `community` › `untrusted`. They drive both ranking and what
  auto mode is allowed to do.
- **Auto mode never installs `untrusted` automatically.** A GitHub release binary is always
  confirmed explicitly — even with `--auto`.
- **`default_yes` is a *threshold*, not a switch.** `default_yes_max_trust` sets the floor: at or
  above it prompts default to "yes"; below it, JII still asks. You choose where the line is.
- **JII is never fully run as root.** Providers *plan* privileged steps but never execute them;
  escalation is isolated, batched, and the exact command is shown before it runs.
- **Artifacts are verified where possible** — sha256 for release tarballs, GPG / sigstore where the
  source offers it.
- **Everything is previewable.** `--dry-run` shows the full plan, root steps included, before a
  single byte changes.

---

## Updating JII itself

JII treats *itself* like any other piece of software — the right way for how it was installed:

| You installed via | `jii update jii` does |
|---|---|
| `install.sh` / tarball / `cargo` (user-space) | Downloads the newest musl tarball, verifies sha256, and **atomically swaps** the binary in place — **no root** |
| `.rpm` / `.deb` (packaged) | Downloads the matching package and upgrades it through `dnf` / `apt` (a previewable root step — never clobbers the package database) |

- **`jii update`** (bare) updates **everything** — your whole system *and* then JII itself.
- **`jii update jii`** updates only JII.
- **`jii uninstall`** (or `jii remove jii`) removes it.

Version comparison is deliberately honest — a *different published tag* means "an update is
available", so you decide (no fragile semver guessing). All of it is previewable with `--dry-run`.

---

## Package spec syntax

Pin a source right on the name to skip the chooser — **`name[:source]`**:

```console
$ jii firefox:flatpak            # install the Flatpak, no prompt
$ jii remove firefox:flatpak     # remove that specific copy
$ jii info node:npm              # the npm card
```

The same spec works across `install` / `remove` / `update` / `info`; `search` stays free-text.

> A `@version` / `@channel` suffix is **reserved** — it's parsed but not yet honored, so it errors
> with a clear message rather than silently ignoring you.

**GitHub releases:** name the repo as `jii <owner>/<repo>` (e.g. `jii jqlang/jq`). JII picks the
release binary for your architecture, verifies its sha256 when the release publishes one, and
installs it to `~/.local/bin` without root.

---

## Output modes

JII defaults to **Friendly** — short, jargon-free output that hides secondary-source noise and
shows a one-line install preview.

- **Friendly** *(default)* — the essentials, nothing else.
- **Advanced** — pass `-v`/`--verbose` (or set `[ui] mode = "advanced"`) for full detail:
  per-source failures and the complete plan.

The first bare `jii` run offers a 30-second setup to pick your mode; `jii setup` re-runs it.

---

## Configuration

Optional TOML at `~/.config/jii/config.toml` (sane defaults if absent).
**Precedence:** CLI flag › env › config › default.

```toml
[sources]
# Tie-breaker order when candidates rank equally.
priority = ["dnf", "copr", "apt", "pacman", "zypper", "flatpak", "snap",
            "github", "cargo", "npm", "pipx", "go", "brew", "nix"]

[install]
profile = "stable"                    # stable | latest | sandbox | minimal
default_yes = true                    # prompts default to "yes"…
default_yes_max_trust = "community"   # …but only at/above this trust; below it JII still asks

[ui]
mode = "friendly"                     # friendly (default) | advanced
locale = "auto"                       # "auto" (detect from $LANG/$LC_MESSAGES) | "en" | "ru"

[trust]
allow_untrusted_auto = false          # auto mode never auto-installs untrusted (keep false)
```

> There's no `jii config` command yet — edit the file directly.

---

## 🧪 Status

**Beta.** The terminal CLI is feature-complete and used daily on **Fedora** (dnf5, COPR).

It also runs cross-distro — **apt** (Debian/Ubuntu), **pacman** (Arch), **zypper** (openSUSE),
**Nix** — alongside Flatpak, Snap, GitHub Releases and the language managers; each source
self-gates on its tool, so JII uses whatever is present.

This is a Beta to gather real-world feedback:

- ✅ **Fedora path is well-exercised.**
- 🧪 **Non-Fedora backends** are implemented but not yet validated on clean VMs of every distro.
- 🧪 **aarch64 packages** are built and published but not yet installed on live ARM hardware.

Bug reports and rough edges are exactly what we're looking for — please
[open an issue](https://github.com/0nigiris/JII/issues).

---

## FAQ

<details>
<summary><b>Is JII a replacement for dnf / apt / pacman?</b></summary>

No. JII drives the managers you already have — it never replaces them. Think of it as the layer
that *decides* which one to use for a given piece of software and then delegates to it.
</details>

<details>
<summary><b>Does it need root / sudo?</b></summary>

Not globally. JII is never run fully as root. A user-space install (GitHub binary, cargo, the
one-liner) needs no root at all. When a step genuinely requires elevation (a distro package),
JII shows you the exact command and escalates just that step.
</details>

<details>
<summary><b>How does it decide which source "wins"?</b></summary>

Candidates are scored by trust level, freshness (does the version match upstream?), your active
profile, and source priority. `jii why <name>` explains the winner in plain language; `-v` shows
the full ranking.
</details>

<details>
<summary><b>Will it ever install something sketchy automatically?</b></summary>

No. Untrusted sources (e.g. arbitrary GitHub binaries) are always confirmed explicitly — even with
`--auto`. The `default_yes_max_trust` threshold controls where prompts stop defaulting to "yes".
</details>

<details>
<summary><b>Can I see what it will do before it does it?</b></summary>

Yes — `--dry-run` on any command prints the full `InstallPlan`, including any root steps, and
changes nothing.
</details>

<details>
<summary><b>How do I add a new source?</b></summary>

Implement the `Provider` trait — the core never branches on the source name, so nothing else has to
change. Simple sources are declarative. See <a href="docs/ARCHITECTURE.md">ARCHITECTURE.md</a>.
</details>

<details>
<summary><b>Which architectures are supported?</b></summary>

x86_64 and aarch64, as static musl binaries — one file, no runtime deps, on any Linux distro.
</details>

---

## Architecture & docs

- [**AGENTS.md**](AGENTS.md) — start here if you're an AI or a new contributor. *(Just a user? Skip it.)*
- [**docs/AI_CONTEXT.md**](docs/AI_CONTEXT.md) — current state: phase, next task, build/test status.
- [**docs/ARCHITECTURE.md**](docs/ARCHITECTURE.md) — the source of truth for design.
- [**docs/DECISIONS.md**](docs/DECISIONS.md) — ADRs: *why* the architecture is the way it is.
- [**docs/ROADMAP.md**](docs/ROADMAP.md) — phased delivery plan.
- [**docs/TASKS.md**](docs/TASKS.md) — actionable checklist.
- [**docs/RELEASE_TESTPLAN.md**](docs/RELEASE_TESTPLAN.md) — manual pre-release checklist across every command.
- [**packaging/**](packaging/README.md) — how the `.rpm` / `.deb` / AUR / COPR artifacts are built.

### Tech

Rust · async (`tokio`) · single crate (modular, not a workspace) · JSON state (SQLite later) ·
provider-trait architecture with declarative, data-driven sources · static musl binaries built in
CI for x86_64 + aarch64.

---

## Contributing

Bug reports and real-world feedback from non-Fedora distros are the most valuable thing right now.

1. Read [AGENTS.md](AGENTS.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — the design is
   fixed unless implementation reveals a concrete problem.
2. Keep commits small and focused; `cargo build`, `cargo clippy` and `cargo test` must stay clean.
3. Adding a source? Implement `Provider` — don't branch the core on a source name.

---

## Philosophy

> The user should never need to think about package managers.
> The user only thinks about software.
> *"I want Docker."* — JII decides **how** and explains **why**.

---

## License

[GNU General Public License v3.0 or later](LICENSE) © JII contributors.

<div align="center">
<br>
<sub>Built with 🦀 Rust · <a href="https://github.com/0nigiris/JII">github.com/0nigiris/JII</a></sub>
</div>
