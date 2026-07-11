# Packaging JII

JII ships prebuilt so nobody has to compile it. This directory holds everything the
release pipeline and the optional distro repos need.

## What's automated (no action needed)

Pushing a `v*` tag runs [`.github/workflows/release.yml`](../.github/workflows/release.yml),
which builds **static musl** binaries for `x86_64` and `aarch64` and publishes, on the
GitHub Release:

| Asset | Install |
|-------|---------|
| `jii-<tag>-<arch>-linux.tar.gz` (+ `.sha256`) | extract, drop `jii` on your `PATH` (or `install.sh`) |
| `jii_<ver>_<arch>.deb` | `sudo apt install ./jii_*.deb` |
| `jii-<ver>.<arch>.rpm` | `sudo dnf install ./jii-*.rpm` **or** `sudo zypper install ./jii-*.rpm` |
| `install.sh` (in the repo root) | `curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/master/install.sh \| sh` |

The `.deb`/`.rpm` are assembled with [nfpm](https://nfpm.goreleaser.com) from
[`nfpm.yaml`](nfpm.yaml) — one spec, both formats, both arches, no target host required.
They bundle the binary, a man page (`jii.1`) and bash/zsh/fish completions. The `.rpm`
installs on **Fedora/RHEL and openSUSE** alike, and `install.sh`'s `JII_METHOD=native`
already drives `dnf`/`apt`/`zypper` to install it (ADR-0059).

That already delivers "download and install on any distro, no building." The sections
below add *native repositories* (so users get updates) and need **your** accounts —
everything is prepared so each is a few commands.

> **Which channel serves whom**
> | Distro | Native repo channel |
> |---|---|
> | Fedora / RHEL / CentOS / Rocky / Alma | **COPR** |
> | openSUSE Leap / Tumbleweed | **OBS** (native) or COPR openSUSE chroots |
> | Arch / CachyOS / Manjaro / … | **AUR** (`jii-bin`) |
> | Debian / Ubuntu | the `.deb` on Releases (a hosted apt repo is future work) |

---

## Publish to COPR (Fedora / RHEL / openSUSE) — needs a Fedora account

[`jii.spec`](jii.spec) is a **binary repack** of the release tarball (no compile). COPR
can build Fedora, EPEL (RHEL/CentOS/Rocky/Alma) **and** openSUSE chroots from the same
spec.

1. Log in at <https://copr.fedorainfracloud.org> (Fedora Account / FAS).
2. **New Project** (e.g. `jii`). Under **Chroots** tick **everything you want to serve, in
   both x86_64 and aarch64** — the spec is multi-arch (see below), so select freely:
   - Fedora — every `fedora-*-x86_64` / `fedora-*-aarch64` (and `fedora-rawhide-*`);
   - **EPEL** (RHEL / CentOS Stream / Rocky / Alma servers) — `epel-8-*`, `epel-9-*`, `epel-10-*`;
   - openSUSE — `opensuse-leap-15.6-*`, `opensuse-tumbleweed-*`.
3. Make sure `jii.spec`'s `Version`/`%_tag` match the release you cut (see "Bumping").
4. Build it — **web UI** (easiest): project → *Builds* → *New Build* → **SCM** tab:
   - Clone url `https://github.com/0nigiris/JII.git`, Committish `master`,
     Spec File `packaging/jii.spec`, SRPM method **`rpkg`**.

   or **CLI** (needs `copr-cli`, token in `~/.config/copr`):
   ```sh
   copr-cli buildscm <your-user>/jii \
     --clone-url https://github.com/0nigiris/JII.git \
     --commit master --spec packaging/jii.spec --method rpkg
   ```
   (`copr-cli build` alone expects an SRPM, not a spec — use `buildscm`, or build an SRPM
   first with `spectool -g` + `rpmbuild -bs`.)
5. Users then:
   ```sh
   # Fedora / RHEL:
   sudo dnf copr enable <your-user>/jii && sudo dnf install jii
   # openSUSE (COPR shows the exact .repo URL on the project page):
   sudo zypper addrepo <copr-project-page>/repo/opensuse-tumbleweed/…jii…​.repo
   sudo zypper install jii
   ```

> **Both arches work.** `jii.spec` is multi-arch (both release tarballs are Sources; `%prep`
> picks by target CPU), so one SRPM rebuilds correctly in **x86_64 and aarch64** chroots —
> validated by building both from a single SRPM. Pick every chroot you want.

---

## Publish to OBS (openSUSE — native) — needs an openSUSE account

The idiomatic home for openSUSE packages is the [openSUSE Build Service](https://build.opensuse.org):
native `zypper` integration and a **1-Click Install** button. The same [`jii.spec`](jii.spec)
works.

1. Account at <https://build.opensuse.org>; install the client: `sudo zypper install osc`
   (openSUSE) or `sudo dnf install osc` (Fedora). Configure once: `osc` (prompts for login).
2. Create a package in your home project and add the spec + release tarball:
   ```sh
   osc checkout home:<your-user>
   cd home:<your-user>
   osc mkpac jii && cd jii
   cp ~/JII/packaging/jii.spec .
   # the spec is multi-arch → commit BOTH release tarballs (Source0 + Source1):
   for a in x86_64 aarch64; do
     curl -fLO https://github.com/0nigiris/JII/releases/download/v0.1.5-beta/jii-v0.1.5-beta-$a-linux.tar.gz
   done
   osc add jii.spec *.tar.gz
   osc commit -m "jii 0.1.5-beta"
   ```
   (Alternatively add a `_service` with the `download_url` service so OBS fetches the
   tarball itself instead of committing it.)
3. In the project's **Repositories**, enable `openSUSE_Tumbleweed` and/or `openSUSE_Leap_15.6`.
4. Users then: the package page's **1-Click Install**, or
   ```sh
   sudo zypper addrepo https://download.opensuse.org/repositories/home:<your-user>/openSUSE_Tumbleweed/home:<your-user>.repo
   sudo zypper refresh && sudo zypper install jii
   ```

---

## Publish to the AUR (Arch) — needs an AUR account + SSH key

[`aur/PKGBUILD`](aur/PKGBUILD) is a prebuilt-binary package (`jii-bin`). Do this on an
Arch/CachyOS box (`sudo pacman -S --needed base-devel git pacman-contrib`).

1. Clone the (empty, for a new package) AUR repo and drop the prepared PKGBUILD in:
   ```sh
   git clone ssh://aur@aur.archlinux.org/jii-bin.git ~/aur-jii-bin
   cp ~/JII/packaging/aur/PKGBUILD ~/aur-jii-bin/ && cd ~/aur-jii-bin
   ```
2. Refresh checksums, build-test, and generate `.SRCINFO`:
   ```sh
   updpkgsums                              # fills sha256sums_* from the release tarballs
   makepkg -si                             # builds + installs; check `jii --version`
   makepkg --printsrcinfo > .SRCINFO
   ```
3. Commit and push (AUR's branch is `master`):
   ```sh
   git add PKGBUILD .SRCINFO && git commit -m "jii-bin 0.1.5_beta" && git push origin master
   ```
4. Users then: `yay -S jii-bin` (or any AUR helper).

---

## Multi-arch spec (how it works)

`jii.spec` is already multi-arch, so **one SRPM serves every x86_64 and aarch64 chroot**.
It carries both release tarballs as Sources and `%prep` unpacks the one matching the target
CPU (COPR/OBS store sources per-package, not per-arch, so both must travel in the SRPM):

```spec
Source0: %{url}/releases/download/%{_tag}/jii-%{_tag}-x86_64-linux.tar.gz
Source1: %{url}/releases/download/%{_tag}/jii-%{_tag}-aarch64-linux.tar.gz
%prep
%ifarch aarch64
%setup -q -T -b 1 -n jii-%{_tag}-aarch64-linux
%else
%setup -q -n jii-%{_tag}-x86_64-linux
%endif
```

Validated locally by rebuilding the one SRPM for both targets
(`rpmbuild --target x86_64|aarch64 --rebuild`): each produces a correct-arch `.rpm` (verified
with `file` on the packaged `/usr/bin/jii`).

> **Other CPU arches** (ppc64le, s390x, armhfp, i686…) that COPR/OBS *offer* need a JII
> **binary** for that arch first — today the release only builds `x86_64` + `aarch64` musl
> statics. Adding more is a CI cross-compilation task (`release.yml`), tracked separately.

---

## Bumping the version

Per release, update the version in three hand-maintained places (the tarball/deb/rpm
versions come from the git tag automatically):

- `Cargo.toml` `version`
- `packaging/jii.spec` — `Version` and `%_tag`
- `packaging/aur/PKGBUILD` — `pkgver` (then `updpkgsums` + `.SRCINFO`)
