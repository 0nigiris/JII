<div align="center">

# JII — Just Install It

**A smart universal package installer for Linux.**
You think about *software*. JII figures out *how* to install it — and explains *why*.

</div>

---

JII is **not** a package manager. It sits on top of the ones you already have
(DNF, COPR, Flatpak, GitHub Releases, Cargo, npm, pipx, Go…), searches all of them
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
jii <name>          search → rank → recommend → install
jii remove <name>   remove using the source that installed it
jii update [<name>] update one/all with the correct manager
jii search <query>  show options without installing
jii info <name>     versions, sources, trust, size
jii why <name>      explain the how & why
jii doctor          source health, latency, rate limits
jii audit           verify signatures & trust
jii history         installation history
jii list            what JII installed
jii config          manage configuration
```

Global flags: `--auto`, `--source <id>`, `--profile <stable|latest|sandbox|minimal>`,
`--dry-run`, `-y/--yes`, `-n/--no`, `--json`, `--no-color`, `-v/--verbose`.

## Configuration

Optional TOML at `~/.config/jii/config.toml` (sane defaults if absent). Precedence:
CLI flag > env > config > default.

```toml
[sources]
priority = ["dnf", "copr", "flatpak", "github", "cargo", "npm", "pipx", "go"]

[install]
profile = "stable"
default_yes = true
default_yes_max_trust = "community"   # below this trust level, JII still asks

[trust]
allow_untrusted_auto = false
```

## Status

🚧 **Early development.** MVP targets **Fedora** (dnf5, COPR, Flatpak, GitHub).
Cross-distro support (apt, pacman, zypper, nix, AUR) is planned.

## Documentation

- [Architecture](docs/ARCHITECTURE.md) — the source of truth for design.
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

TBD.
