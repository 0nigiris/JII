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

## Commands

```
jii <name…>         search → rank → recommend → install (one or many packages)
jii remove <name>   remove using the source that installed it
jii update [<name>] update one/all with the correct manager
jii search <query>  show ranked candidates without installing
jii info <name>     sources, versions, trust, and the recommendation + why
jii sources         list providers and whether each is usable here
jii why <name>      explain the how & why
jii doctor          source health, latency, rate limits
jii audit           verify signatures & trust
jii history         installation history
jii list            what JII installed
```

Configuration is via `~/.config/jii/config.toml` (see below), not a `jii config`
command yet.

Global flags: `--auto`, `--source <id>`, `--profile <stable|latest|sandbox|minimal>`,
`--dry-run`, `-y/--yes`, `-n/--no`, `--json`, `--no-color`, `-v/--verbose`.

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

[trust]
allow_untrusted_auto = false
```

## Status

🚧 **Early development.** Best exercised on **Fedora** (dnf5, COPR), but JII now runs
cross-distro: **apt** (Debian/Ubuntu), **pacman** (Arch), **zypper** (openSUSE) and **Nix**,
alongside Flatpak, Snap, GitHub Releases and the language managers. Each source self-gates on
its tool, so JII uses whatever is present on your machine. (AUR and live clean-VM validation of
every backend are still to come.)

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
