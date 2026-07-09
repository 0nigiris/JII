# JII — Release Test Plan (manual)

> Run this checklist before **every** release (tag). It is a *manual* pass over the whole
> tool on a real machine — the automated `cargo test` suite covers units; this covers
> behaviour, wording, and the cross-source flows that only a live system exercises.
>
> **How to use:** copy the checklist into the release issue/PR (or tick in a scratch copy),
> run each command, and confirm the **Expect** column. Prefer `--dry-run`/`-n` where a real
> install would be disruptive. Note the environment you ran on (distro, arch, which managers
> are present) — coverage depends on it.
>
> Legend: ✅ pass · ⚠️ pass-with-note · ❌ fail (file an issue). `-n` answers "no" to prompts;
> `-d`/`--dry-run` previews without changing anything.

## 0. Pre-flight

- [ ] `cargo build --release` clean; `cargo clippy --all-targets` clean; `cargo test` green.
- [ ] `jii --version` prints the tag you're about to cut (Cargo version == tag).
- [ ] Record env: `uname -m`, distro, and `jii sources` output (which managers are usable).

## 1. Search & matching

| Command | Expect |
|---|---|
| `jii search htop` | Ranked candidates; best marked `→`. |
| `jii search "markdown editor"` | Free-text search returns candidates (no spec parsing). |
| `jii firefox -n` | Exact match recommended; **no** "no exact match" note. |
| `jii ayugram -n` | "No exact match … Closest: ayugram-desktop" then offer (prefix broadening). |
| `jii ayugramm -n` | Trailing-typo still resolves to the closest; note shown. |
| `jii zzzznope -n` | Clean "not found", no crash, no giant list. |
| `jii FastFetch -n` | Case-insensitive match. |
| `time jii search htop` ×2 | 1st may be slow (a source times out once); **2nd is fast** (circuit breaker). |

## 2. Install — single, sources, dry-run

| Command | Expect |
|---|---|
| `jii fastfetch -d` | Full plan previewed; nothing changes. |
| `jii <a-real-uninstalled-pkg>` then `y` | Installs; `jii list` shows it afterwards. |
| `jii htop --auto` | Installs the recommended within trust limits, no prompt. |
| `jii neovim:flatpak -d` | Source pinned to flatpak in the plan. |
| `jii bat:dnf -d` | Source pinned to dnf. |
| `jii <pkg> --source flatpak -d` | Global `--source` honoured. |
| `jii <installed-pkg>` | "already installed via <src>", no pointless reinstall (cooperate). |

## 3. Batch install

| Command | Expect |
|---|---|
| `jii ripgrep bat fd-find -d` | One grouped plan; per-source batching. |
| `jii htop mpv:flatpak -d` | Mixed sources in one run; summary lists each. |
| `jii git curl wget --auto -d` | Batch preview; single confirmation path. |

## 4. Package managers (bootstrap) — #4/#9

| Command | Expect |
|---|---|
| `jii npm` (npm present) | "Node.js (npm) is already installed — a package manager JII drives." |
| `jii cargo` (present) | Same "already installed" note; no reinstall, **no** "npm via npm" loop. |
| `jii pipx` (absent) | Resolves a distro package for pipx and offers it (e.g. via dnf); **no loop**. |
| `jii nix` (absent) | Shows the install script, **does not run it** (trust boundary). |
| `jii npm:npm -n` | Pinned → treats it as the *package* named npm, not the manager. |
| `jii providers` | Lists ecosystem managers + installed/available. |
| `jii providers add npm` | Same bootstrap path as `jii npm`. |

## 5. npm / Cargo / language sources — #5

| Command | Expect |
|---|---|
| `jii prettier:npm -d` | npm CLI package resolves and plans a user-prefix install (no root). |
| `jii lodash -n` | "npm library … nothing to install; run `npm install lodash`" (clear, actionable). |
| `jii ripgrep:cargo -d` | crates.io binary crate; user-space plan. |
| `jii <a-cargo-lib>` | Library refused with a clear message (programs, not libraries). |

## 6. GitHub / forge sources

| Command | Expect |
|---|---|
| `jii jqlang/jq -n` | GitHub release resolved; **untrusted → always confirmed**, even with `--auto`. |
| `jii jqlang/jq --auto -n` | Still asks (trust barrier), not silently installed. |
| `jii owner/doesnotexist -n` | Clean "not found", no crash. |

## 7. Flatpak / Nix

| Command | Expect |
|---|---|
| `jii <flatpak-app>:flatpak -d` | Flatpak plan; Flatpak's own polkit, not jii sudo. |
| `jii <nixpkg>:nix -d` (Nix host) | `nix profile install nixpkgs#…`, no root. |
| `jii list` (Nix host) | Nix packages installed outside jii appear (schema-tolerant list). |

## 8. info — shows, never installs — #6

| Command | Expect |
|---|---|
| `jii info fastfetch` | App card: description, source, version, links, recommendation. |
| `jii info vlc` | Card with metadata block. |
| `jii info lodash` | **Info card** (description/homepage/repo) + library note — **not** "nothing to install". |
| `jii info node:npm` | Pinned source card. |
| `jii info nonexistent` | "No information found …" (info-framed, not install-framed). |

## 9. how / why

| Command | Expect |
|---|---|
| `jii how vlc` | Explains how it would install (or did). |
| `jii why fastfetch` | Alias of `how`. |
| `jii how <not-installed>` | Honest "not installed by jii" hint. |

## 10. update / self-update — #9

| Command | Expect |
|---|---|
| `jii update -d` | Whole-system upgrade plan (each manager's bulk upgrade). |
| `jii update` | System upgrade **then** self-update prompt (updates everything). |
| `jii update <pkg> -d` | Updates just that package via its owning source. |
| `jii update jii` | Checks GitHub (list endpoint), reports newest or "already up to date". |

## 11. remove — #9

| Command | Expect |
|---|---|
| `jii remove <pkg> -d` | Removes via the owning source; default prompt **no** (destructive). |
| `jii remove <multi-owner> ` | Chooser: which copy / all. |
| `jii remove <flatpak>:flatpak -d` | Pinned removal. |
| `jii uninstall -d` | Self-remove preview (method-aware); does not run under `-d`. |
| `jii remove <not-installed>` | "Not installed", no crash. |

## 12. doctor — analyses the system — #1

| Command | Expect |
|---|---|
| `jii doctor` | Source health + system checks + **only missing** setup items offered. |
| `jii doctor` (with VLC/codecs already installed) | Does **not** offer to install them. |
| answer `y` to "Add ~/.cargo/bin to PATH?" | Appends the right line to the shell rc (idempotent). |
| `jii doctor -n` | Read-only; lists suggestions, changes nothing. |
| `jii doctor --json` | Machine-readable source array; no questionnaire. |

## 13. list / audit / history / sources

| Command | Expect |
|---|---|
| `jii list` | What JII installed. |
| `jii list --audit` | Security view: source, trust, verification, concerns. |
| `jii history` | Newest-first install history. |
| `jii sources` | Providers + usable-here status. |

## 14. Flags & output modes

| Command | Expect |
|---|---|
| `jii <pkg> -v -d` | Advanced: per-source failures + full plan. |
| `jii <pkg> --json -d` | JSON plan output. |
| `jii <pkg> --profile latest -d` | Ranking preset applied. |
| `jii <pkg> --profile sandbox -d` | Flatpak floated to the top. |
| `jii --no-color htop -n` | No ANSI colour codes. |
| `jii <pkg> -y` / `-n` | Prompt defaults forced yes / no. |

## 15. Edge cases

| Command | Expect |
|---|---|
| `jii ""` | Rejected/handled cleanly, no panic. |
| `jii vlc vlc -d` | Duplicate name handled (no double plan). |
| `jii firefox@123 -n` | `@ref` reserved → clear error, not silent latest. |
| `jii foo:nonexistentsource` | Unknown source → clear error (with did-you-mean if available). |
| No network | Sources degrade to stale cache / skip; no hang beyond timeout. |
| Fresh box (no managers) | Runs, finds nothing to install, `jii doctor` still helps. |

## 16. Localization (once #7 lands)

| Command | Expect |
|---|---|
| `jii --lang ru vlc -n` | Russian UI strings. |
| `jii --lang en vlc -n` | English UI strings. |
| `LC_MESSAGES=ru_RU.UTF-8 jii doctor` | Auto-detected Russian. |
| config `[ui] lang` | Honoured when no flag/env. |

## 17. Packaging / install (per release artifacts)

- [ ] `curl … install.sh | sh` installs the newest tag to `~/.local/bin`, verifies sha256, clean output.
- [ ] `.rpm` installs on Fedora; `.deb` installs on Debian/Ubuntu (incl. man page + completions).
- [ ] aarch64 artifacts install/run on real ARM hardware. *(open risk until tested on a host)*
- [ ] `jii update jii` on the previous release upgrades to this one (user-space swap or dnf/apt).

---

_Keep this file current: when a command's behaviour changes, update the Expect column in the
same PR (AI Handoff Policy)._
