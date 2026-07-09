# Packaging JII

JII ships prebuilt so nobody has to compile it. This directory holds everything the
release pipeline and the optional distro repos need.

## What's automated (no action needed)

Pushing a `v*` tag runs [`.github/workflows/release.yml`](../.github/workflows/release.yml),
which builds **static musl** binaries for `x86_64` and `aarch64` and publishes, on the
GitHub Release:

| Asset | Install |
|-------|---------|
| `jii-<tag>-<arch>-linux.tar.gz` (+ `.sha256`) | extract, drop `jii` on your `PATH` |
| `jii_<ver>_<arch>.deb` | `sudo apt install ./jii_*.deb` |
| `jii-<ver>.<arch>.rpm` | `sudo dnf install ./jii-*.rpm` |
| `install.sh` (in the repo root) | `curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/master/install.sh \| sh` |

The `.deb`/`.rpm` are assembled with [nfpm](https://nfpm.goreleaser.com) from
[`nfpm.yaml`](nfpm.yaml) — one spec, both formats, both arches, no target host required.
They bundle the binary, a man page (`jii.1`) and bash/zsh/fish completions.

That already delivers "download and install on any distro, no building." The two options
below add *native repositories* (`dnf copr enable …`, `yay -S jii-bin`) and need **your**
accounts — everything is prepared so each is a couple of commands.

## Publish to COPR (Fedora / RHEL / openSUSE) — needs a Fedora account

[`jii.spec`](jii.spec) is a **binary repack** of the release tarball, so a COPR build Just
Works (no compile, no build-time network beyond fetching the tarball).

1. Create a COPR project at <https://copr.fedorainfracloud.org> (e.g. `jii`).
2. Bump `Version`/`%_tag` in `jii.spec` to the release you cut.
3. Build it:
   ```console
   $ copr-cli build <your-user>/jii packaging/jii.spec
   ```
   (or upload the spec in the web UI). COPR builds `x86_64` and `aarch64`.
4. Users then:
   ```console
   $ sudo dnf copr enable <your-user>/jii
   $ sudo dnf install jii
   ```

## Publish to the AUR (Arch) — needs an AUR account + SSH key

[`aur/PKGBUILD`](aur/PKGBUILD) is a prebuilt-binary package (`jii-bin`).

1. On each release, bump `pkgver` (use `_` for the `-`, e.g. `0.1.0_beta`) and refresh
   checksums:
   ```console
   $ cd packaging/aur
   $ updpkgsums          # fills sha256sums_* from the release tarballs
   $ makepkg --printsrcinfo > .SRCINFO
   ```
2. Push to the AUR git remote (`ssh://aur@aur.archlinux.org/jii-bin.git`).
3. Users then: `yay -S jii-bin` (or any AUR helper).

## Bumping the version

Per release, update the version in three hand-maintained places (the tarball/deb/rpm
versions come from the git tag automatically):

- `Cargo.toml` `version`
- `packaging/jii.spec` — `Version` and `%_tag`
- `packaging/aur/PKGBUILD` — `pkgver` (then `updpkgsums` + `.SRCINFO`)
