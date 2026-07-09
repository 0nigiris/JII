<div align="center">

# JII — Just Install It

**A smart universal package installer for Linux.**
You think about *software*. JII figures out *how* to install it — and explains *why*.

</div>

---

JII is **not** a package manager. It sits on top of the ones you already have
(DNF, COPR, apt, pacman, zypper, Nix, Flatpak, Snap, GitHub Releases — incl. AppImage assets —
Cargo, npm, pipx, Go, Homebrew…), searches all of them
at once, ranks the results, and installs the best option — transparently.

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

Install without prompts (within your trust threshold):

```console
$ jii fastfetch --auto
```

## Why JII

- **One command for everything** — no need to know whether software lives in DNF,
  Flatpak, COPR, or a GitHub release.
- **No `sudo` needed** — JII asks for elevation only when a step actually requires
  it, and shows you the exact command first. It never runs fully as root.
- **Explains every decision** — `jii why <name>` tells you how something was
  installed and why that source was chosen.
- **Safe by default** — sources carry trust levels; **auto mode never installs an
  untrusted source automatically**, and artifacts are verified (GPG / sha256 /
  sigstore) where possible.
- **Previewable** — `--dry-run` shows the full plan before anything happens.
- **Remembers** — `jii remove discord` uses whatever installed it; `jii update`
  updates each package with the correct manager.

## Install

JII ships prebuilt for **x86_64** and **aarch64** as a static [musl] binary — one file, no
runtime deps, runs on every Linux distro (glibc or musl, old or new). No compiling required.

**One-liner** (installs to `~/.local/bin`, no root):

```console
$ curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/master/install.sh | sh
```

It auto-detects your CPU, downloads the matching binary from the latest release, verifies its
sha256, and installs it. Then run `jii doctor` to confirm it sees your sources.

**Native package** — grab the `.rpm` or `.deb` for your arch from
[Releases](https://github.com/0nigiris/JII/releases) and install it with your package manager
(it also drops a man page + shell completions):

```console
$ sudo dnf install ./jii-*.rpm      # Fedora / RHEL / openSUSE
$ sudo apt install ./jii_*.deb      # Debian / Ubuntu
```

**Arch (AUR):** `yay -S jii-bin` — once the package is published (see
[`packaging/`](packaging/README.md)).

**Manual tarball** — if you prefer to place it yourself:

```console
$ tar -xzf jii-v0.1.0-beta-x86_64-linux.tar.gz
$ sha256sum -c jii-v0.1.0-beta-x86_64-linux.tar.gz.sha256   # optional integrity check
$ install -Dm755 jii-v0.1.0-beta-x86_64-linux/jii ~/.local/bin/jii
```

**Build from source** (needs a recent Rust toolchain):

```console
$ git clone https://github.com/0nigiris/JII && cd JII
$ cargo install --path .    # then add ~/.cargo/bin to your PATH
```

JII drives the package managers you already have — it doesn't bundle any. On a machine with none
of its supported sources (dnf5, Flatpak, apt, …) it will run but find nothing to install.

[musl]: https://musl.libc.org/

## Commands

```
jii <name…>         search → rank → recommend → install (one or many packages)
jii remove <name>   remove using the source that installed it
jii update [<name>] named: update that package; bare: update the whole system
jii update jii      update JII itself from the latest GitHub release (self-update)
jii search <query>  show ranked candidates without installing
jii info <name>     app card: description, homepage, license, author + all sources & why
jii sources         list providers and whether each is usable here
jii providers       show ecosystem managers (npm, cargo, brew, Flatpak…) + what's installed
jii providers add   bootstrap a missing manager (e.g. `jii providers add npm`)
jii how <name>      explain how JII would install (or did install) it
jii doctor          source health + system checks + curated suggestions
jii doctor --fix    offer to fix what it can (install git/curl, add Flathub)
jii history         installation history
jii list            what JII installed (add --audit for signatures, trust & concerns)
jii setup           re-run the first-run wizard (output mode, optional doctor)
jii uninstall       remove JII itself (same as `jii remove jii`)
```

**Updating JII itself.** `jii update jii` checks GitHub for the newest release and updates
this binary the right way for how you installed it: a user-space install (install.sh / tarball
/ `cargo`) is swapped in place with no root; a `.rpm`/`.deb` install is upgraded through
`dnf`/`apt` (shown first). A bare `jii update` also mentions when a newer JII is out. Remove
JII with `jii uninstall`. Everything is previewable with `--dry-run`.

**Package spec — `name[:source]`.** Pin a source right on the name to skip the chooser:
`jii firefox:flatpak`, `jii remove firefox:flatpak`, `jii info node:npm`. The same spec
works across install/remove/update/info; `search` stays free-text. (A `@version`/`@channel`
suffix is reserved — parsed but not yet honored, so it errors with a clear message.)

**Output modes.** JII defaults to **Friendly** — short, jargon-free output that hides
secondary-source noise and shows a one-line install preview. Pass `-v`/`--verbose` (or set
`[ui] mode = "advanced"`) for full detail: per-source failures and the complete plan. The
first bare `jii` run offers a 30-second setup to pick your mode; `jii setup` re-runs it.

Configuration is via `~/.config/jii/config.toml` (see below), not a `jii config`
command yet.

Global flags: `--auto`, `--source <id>`, `--profile <stable|latest|sandbox|minimal>`,
`-d/--dry-run`, `-y/--yes`, `-n/--no`, `--json`, `--no-color`, `-v/--verbose`.

**GitHub releases:** name the repo as `jii <owner>/<repo>` (e.g. `jii jqlang/jq`). JII
picks the release binary for your architecture, verifies its sha256 when the release
publishes one, and installs it to `~/.local/bin` without root. GitHub binaries are
`untrusted`, so they are always confirmed explicitly — even with `--auto`.

## Configuration

Optional TOML at `~/.config/jii/config.toml` (sane defaults if absent). Precedence:
CLI flag > env > config > default.

```toml
[sources]
priority = ["dnf", "copr", "apt", "pacman", "zypper", "flatpak", "snap", "github", "cargo", "npm", "pipx", "go", "brew", "nix"]

[install]
profile = "stable"
default_yes = true
default_yes_max_trust = "community"   # below this trust level, JII still asks

[ui]
mode = "friendly"                     # "friendly" (default) or "advanced"

[trust]
allow_untrusted_auto = false
```

## Status

🧪 **Beta.** The terminal CLI is feature-complete and used daily on **Fedora** (dnf5, COPR).
It also runs cross-distro — **apt** (Debian/Ubuntu), **pacman** (Arch), **zypper** (openSUSE),
**Nix** — alongside Flatpak, Snap, GitHub Releases and the language managers; each source
self-gates on its tool, so JII uses whatever is present on your machine. This is a Beta to
gather real-world feedback: the **Fedora path is well-exercised**, while the non-Fedora backends
are implemented but not yet validated on clean VMs of every distro. Bug reports and rough edges
are exactly what we're looking for — please open an issue.

## Documentation

- [AGENTS.md](AGENTS.md) — start here if you are an AI or a new contributor. If you are just a user, nvm
- [AI context](docs/AI_CONTEXT.md) — current state (phase, next task, build/test).
- [Architecture](docs/ARCHITECTURE.md) — the source of truth for design.
- [Decisions](docs/DECISIONS.md) — ADRs: why the architecture is the way it is.
- [Roadmap](docs/ROADMAP.md) — phased delivery plan.
- [Tasks](docs/TASKS.md) — actionable checklist.

## Tech

Rust · async (`tokio`) · single crate (modular) · JSON state (SQLite later) ·
provider-trait architecture with declarative data-driven sources.

## Philosophy

> The user should never need to think about package managers.
> The user only thinks about software.
> *"I want Docker."* — JII decides **how** and explains **why**.

## License

[MIT](LICENSE).
