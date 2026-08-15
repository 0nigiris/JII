# JII — Supported systems & per-system smoke test

> Companion to [`RELEASE_TESTPLAN.md`](RELEASE_TESTPLAN.md). That file is the deep,
> behaviour-by-behaviour pass; **this** file answers two questions for a cross-system
> testing round: **which systems do we support**, and **what do I run on each one every
> time**. Hand it to a friend on any distro — they can follow §3 top to bottom.
>
> JII is **Linux-only** today (x86_64 and aarch64). Windows/macOS are future work.

## 1. Supported systems

JII never branches on the distro — it drives whatever **sources** (package managers)
are present. So "supported system" = "a system whose native manager JII has a provider
for", plus the cross-distro managers that work anywhere they're installed.

### Native package managers (one per distro family)

| Distro family | Native source id | Tier |
|---|---|---|
| **Fedora**, RHEL, CentOS Stream, Rocky, AlmaLinux | `dnf` (+ `copr`) | **1 — primary** (Fedora-first) |
| Debian, Ubuntu, Linux Mint, Pop!_OS, elementary, Zorin | `apt` | 2 — implemented, live-verify |
| Arch, **CachyOS**, Manjaro, EndeavourOS, Garuda | `pacman` | 2 — implemented, live-verify |
| openSUSE Leap / Tumbleweed | `zypper` | 2 — implemented, live-verify |
| Void Linux | `void` (xbps) | 2 — implemented, live-verify |
| Gentoo | `gentoo` (portage/emerge) | 2 — implemented, live-verify |
| NixOS | `nix` (+ declarative config edit) | 2 — implemented, live-verify |

> **Tier 2** = the provider is built and unit-tested, but has **not** been verified on a
> live host yet. Confirming these is the whole point of this testing round.

### Cross-distro sources (work on ANY of the above, wherever the tool is installed)

| Source id | What it is |
|---|---|
| `flatpak` | Flatpak / Flathub apps |
| `snap` | Snap packages |
| `brew` | Homebrew / Linuxbrew |
| `cargo` | crates.io binary crates |
| `npm` | npm CLI packages |
| `pipx` | PyPI applications |
| `go` | `go install` binaries |
| `github` | GitHub Releases (forge) — the **last-resort** fallback |

### CPU architectures

`x86_64` and `aarch64` (arm64). The binary is a static musl build, so glibc/musl and
old/new distros all work.

## 2. Install JII (pick one per system)

```sh
# A. Portable one-liner — no root, any distro (installs to ~/.local/bin)
curl -fsSL https://sudonit.com/install.sh | sh   # fallback: raw.githubusercontent.com/0nigiris/JII/master/install.sh

# B. Native package (system-integrated: man page, completions, removable via the pkg mgr)
sudo dnf install ./jii-*.rpm          # Fedora / RHEL / openSUSE-rpm
sudo apt install ./jii_*.deb          # Debian / Ubuntu
yay -S jii-bin                        # Arch / CachyOS  (once published to the AUR)
#   .rpm/.deb are on the GitHub Release; grab the one for your arch.

# C. Once the native repos are published (recipes ready in packaging/):
apk add jii                           # Alpine
sudo xbps-install jii                 # Void
sudo emerge jii-bin                   # Gentoo
nix-build packaging/nix/jii.nix       # Nix / NixOS
brew install 0nigiris/jii/jii         # Homebrew on Linux
cargo install jii                     # any OS with a Rust toolchain
```

Run `jii --version` right after — it should print the current tag.

## 3. The smoke test — run this on EVERY system, every time

Copy-paste block. Replace `<pkg>` with a small program that exists in **this** distro's
repos (suggestions in §4), and `<native>` with this system's native source id from §1.
`-d`/`--dry-run` previews without changing anything; `-n` answers "no" to prompts.

```sh
# 0. Record the environment (paste this into your report)
uname -m ; (. /etc/os-release && echo "$PRETTY_NAME") ; jii --version

# 1. Health & discovery — does JII see this system's sources?
jii doctor            # source health + system checks (answer prompts, or add -n to preview)
jii sources           # every provider + whether it's usable HERE
jii providers         # ecosystem managers present / bootstrappable

# 2. Search & matching (read-only, changes nothing)
jii search htop
jii firefox -n        # exact match; must NOT show a false "closest match" note
jii zzzznope -n       # clean "not found" — no crash, no giant list

# 3. Plan preview (dry-run — nothing is installed)
jii fastfetch -d              # full install plan
jii htop --source <native> -d # force this system's native manager into the plan

# 4. Real install → verify → remove (pick <pkg> from §4)
jii <pkg>             # confirm 'y'; then actually RUN <pkg> to confirm it works
jii list             # <pkg> appears, tagged with the source it came from
jii how <pkg>        # explains how it was installed
jii remove <pkg> -d  # preview removal (default answer is "no" — destructive)
#   jii remove <pkg>  # do the real removal if you want to test it

# 5. Update path (preview only)
jii update -d        # whole-system upgrade plan
jii update jii       # self-update check against GitHub

# 6. Localisation
jii --lang ru doctor -n   # Russian UI
jii --lang en doctor -n   # English UI
```

**What "pass" looks like:** every command runs without a panic/traceback; `jii sources`
lists this distro's native manager as usable; the dry-run plans read sensibly; the real
install puts a working binary on PATH and `jii list` remembers it; wording is clean in
both languages.

## 4. Per-system specifics

Pick a tiny, uncontroversial CLI as `<pkg>` — install/remove is cheap and safe.

| System | `<native>` | Good `<pkg>` to try | Notes |
|---|---|---|---|
| Fedora / RHEL | `dnf` | `htop`, `fastfetch`, `ripgrep` | Also test `jii bat:copr -d` (COPR). |
| Debian / Ubuntu | `apt` | `htop`, `neofetch`, `jq` | `fd` is `fd-find` on Debian. |
| Arch / CachyOS | `pacman` | `htop`, `fastfetch`, `ripgrep` | Native install path is the AUR (`yay -S jii-bin`). |
| openSUSE | `zypper` | `htop`, `jq` | |
| Void | `void` | `htop`, `jq` | xbps naming can differ. |
| Gentoo | `gentoo` | `app-misc/jq` | emerge is slow — prefer `-d` previews. |
| NixOS | `nix` | `hello`, `ripgrep` | See §5 (declarative). |
| Any (Flatpak) | `flatpak` | `jii <app>:flatpak -d` | Flatpak uses its own polkit, not jii sudo. |

## 5. NixOS — declarative install (extra checks)

JII can edit your Nix config instead of doing an imperative `nix profile install`
(ADR-0054/0056/0058). On a NixOS box also run:

```sh
jii ripgrep -d                 # default plan (imperative or declarative per config)
jii ripgrep --nix-config -d    # force editing the Nix config; shows a diff, changes nothing
jii ripgrep --nix-imperative -d # force imperative for this run
```

- **home-manager** (`~/.config/home-manager/home.nix` or `~/.nixpkgs/…`): the edit goes to
  `home.packages`, applied with `home-manager switch`. Written directly (you own the file).
- **System** (`/etc/nixos/configuration.nix`): the edit goes to `environment.systemPackages`,
  applied with `sudo nixos-rebuild switch`. Because it's root-owned, JII **prints the exact
  `sudo cp` commands first**, backs the file up to `configuration.nix.jii-bak`, then writes.
  Under `-d` it shows the commands but writes/stages **nothing**. ← verify this carefully.

## 6. What to report back (per system)

1. The line 0 output: `uname -m`, `PRETTY_NAME`, `jii --version`.
2. `jii sources` output (so we know which managers were live).
3. Any command that **panicked, errored, hung, or looked wrong** — paste the command and
   the output/screenshot.
4. Anything confusing in the wording (either language).

> Deep-dive: the full behaviour matrix (batch installs, forge/`owner/repo`, trust barriers,
> audit view, edge cases) is in [`RELEASE_TESTPLAN.md`](RELEASE_TESTPLAN.md) — do that pass
> at least once on Fedora.
