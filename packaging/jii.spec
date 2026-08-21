# RPM spec for JII — a *binary repack* of the official GitHub release tarballs.
#
# It ships the prebuilt static-musl binary (plus man page, completions and docs), so
# COPR / OBS / rpmbuild produce an installable .rpm with no compile. It is **multi-arch
# from one spec**: both the x86_64 and aarch64 release tarballs are Sources, and %prep
# unpacks the one matching the build's target arch. This lets COPR/OBS build *every*
# x86_64 **and** aarch64 chroot (Fedora, EPEL for RHEL/CentOS/Rocky/Alma, openSUSE) from
# a single SRPM. On each release, bump Version and %_tag. See packaging/README.md.
#
# (A from-source spec using the Fedora rust-packaging macros is a post-Beta option once
# the crate is submitted to Fedora proper.)

%global _tag v0.1.17-beta

Name:           jii
Version:        0.1.17~beta
Release:        1%{?dist}
Summary:        A smart universal package installer for Linux

License:        GPLv3+
URL:            https://github.com/0nigiris/JII
# One Source per published arch; %%prep picks the one matching the target CPU. A single
# SRPM then rebuilds correctly in both x86_64 and aarch64 chroots (COPR/OBS store sources
# per package, not per arch, so both tarballs must travel in the SRPM).
Source0:        %{url}/releases/download/%{_tag}/jii-%{_tag}-x86_64-linux.tar.gz
Source1:        %{url}/releases/download/%{_tag}/jii-%{_tag}-aarch64-linux.tar.gz

ExclusiveArch:  x86_64 aarch64
# Prebuilt binary: no debug symbols to extract, no ELF hardening checks to run.
%global debug_package %{nil}
%global __brp_strip %{nil}

%description
JII ("Just Install It") searches the sources you already have — DNF, COPR, Flatpak,
GitHub Releases, Cargo, npm, pipx, Go and more — ranks the results, installs the best
option, and explains why. It is not a package manager; it drives the ones you have,
and never runs fully as root: only the concrete steps that need it escalate, shown first.

%prep
# Unpack only the tarball matching the target arch (both are Sources in the SRPM).
%ifarch aarch64
%setup -q -T -b 1 -n jii-%{_tag}-aarch64-linux
%else
%setup -q -n jii-%{_tag}-x86_64-linux
%endif

%install
install -Dm0755 jii %{buildroot}%{_bindir}/jii
install -Dm0644 jii.1 %{buildroot}%{_mandir}/man1/jii.1
install -Dm0644 completions/jii.bash %{buildroot}%{_datadir}/bash-completion/completions/jii
install -Dm0644 completions/_jii %{buildroot}%{_datadir}/zsh/site-functions/_jii
install -Dm0644 completions/jii.fish %{buildroot}%{_datadir}/fish/vendor_completions.d/jii.fish

%files
%license LICENSE
%doc README.md
%{_bindir}/jii
%{_mandir}/man1/jii.1*
%{_datadir}/bash-completion/completions/jii
%{_datadir}/zsh/site-functions/_jii
%{_datadir}/fish/vendor_completions.d/jii.fish

%changelog
* Fri Aug 21 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.17~beta-1
- `jii update jii` and install.sh now require a `v*` tag when picking the newest
  release, so a boss-fight bundle is never mistaken for a JII release, and the
  shell one-liner no longer depends on the JSON body being pretty-printed
  (ADR-0081 addendum).
* Tue Aug 18 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.16~beta-1
- New `jii changelog`: JII's release notes in plain language, embedded in the binary
  and readable offline. Bare shows the running version, `jii changelog 0.1.12` any
  past release, `--all` the whole history, `--since <v>` everything newer.
- `jii update jii` no longer ends on "updated": it finishes by printing what the new
  version brought, by re-invoking the freshly installed binary (ADR-0079).
- Bootstrapping a manager now finishes it: snapd's socket and /snap link, Flatpak's
  remote on every path, and an offer to add Homebrew's line to your shell rc (brew is
  used by absolute path immediately). `jii doctor` checks brew has a compiler.
- A source can explain its own failure: `pipx install <library>` now reads as "that's
  a Python library, not a program" with real next steps instead of pipx's raw output
  (ADR-0080).
- `jii achievements` marks earned entries with ✓ (not colour alone), and `jii sources
  add <manager>` grants Bootstrapper. Trust level `untrusted` now displays as
  "unverified".
- A fourth secret install path and its badge, plus the boss table that made it a
  single row: a fight now declares its own endings and sentinel (ADR-0081).
* Sat Aug 16 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.15~beta-1
- Achievements: 15 → 30. Every boss ending is now its own badge (spare and kill
  each count, plus a "both ways" badge for seeing them all, plus one for beating
  every boss). Ending badges stay out of the list entirely until you've won that
  fight, then appear as named goals instead of another `???`.
- Eight new everyday ones: finish the setup wizard, ask `jii how`, preview with
  `--dry-run`, run `jii list --audit`, pin a source (`htop:flatpak`), install five
  packages at once, switch language, and install between 5am and 8am (ADR-0078).
* Fri Aug 15 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.14~beta-1
- A third secret achievement: 🎭 `spamton`, earned by beating Spamton NEO in the
  matching install path (the `spamton` branch). Like 🃏 it remembers the ending —
  blown apart, or freed by cutting his strings. The boss sentinels are now a
  single generic mechanism, so a new fight needs no new plumbing (ADR-0077).
* Fri Aug 15 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.13~beta-1
- A second secret achievement: 🃏 `jevil`, earned by beating Jevil in the
  Chaos-Simulator install path (the `chaos` branch, coming next). It remembers
  whether you spared or struck him down and shows that ending. This release
  carries the in-binary half (the `chaos-install` sentinel + the achievement);
  the fight installer follows (ADR-0076).
* Fri Aug 15 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.12~beta-1
- `jii achievements` grows from 3 badges to 13 — everyday ones you stumble into,
  several to hunt for (install from five sources, the night shift, self-update,
  bootstrap a manager), two extreme grinds (100 and 500 installs), a completionist
  crown, and the secret. The ledger is now signed (HMAC bound to this machine): a
  hand-edited achievements file is caught, mocked and reset — earn them for real
  (ADR-0074). Honestly-earned badges from older versions carry over untouched.
* Thu Aug 14 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.11~beta-1
- New `jii achievements` command — a small, playful badge ledger (ADR-0072). The
  `curl … | sh` installer got a bordered, centre-aligned tagline card and a download
  spinner; the search chooser never stars an untrusted match as "recommended"
  (ADR-0071); and the live progress bar now stretches to the terminal width.
* Fri Jul 25 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.10~beta-1
- Rebuild against the v0.1.10-beta release: installs, updates and downloads now show a
  live progress bar with a real percentage read from the source's own output (dnf/apt's
  [3/41] step counter, a download's byte percentage) instead of a bare spinner; and
  `jii update` now updates *every* Flatpak — the system-wide apps a desktop store
  installed under /var/lib/flatpak, not just per-user ones (fixes "update said done but
  Discover still lists updates").
* Fri Jul 17 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.9~beta-1
- Rebuild against the v0.1.9-beta release: full-project audit round — GitHub source
  resolves prerelease-only repos; update/self-update fail loudly; name-squatting
  registry packages (npm/crates/PyPI) are demoted to untrusted with a warning;
  self-update warns before an apparent downgrade; Flatpak plans add the user-scope
  Flathub remote; Russian y/n keys; scrolling chooser on short terminals; `jii how`
  lists every copy of a name; search-cache pruning; localized remove/forge errors.
* Wed Jul 15 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.8~beta-1
- Rebuild against the v0.1.8-beta release: a missing manager is now set up only
  through a source that works here (no more "install pipx via pipx"); Homebrew/Nix
  offer to run their own installer script instead of refusing; live progress while
  installing/removing/updating, with the number of packages actually updated; new
  --run to start a package once installed; `jii sources` lists sources you disabled
  and how to re-enable them; `jii man` formats through man(1); `jii providers`
  removed (use `jii sources`).
* Mon Jul 13 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.7~beta-1
- Rebuild against the v0.1.7-beta release: `jii doctor` shows only host-relevant
  sources (no foreign distro managers) + refreshes metadata after enabling a repo
  (fixes the codec "not found"); browse links on a total miss; config path in
  `--help`; GitHub-token hint in `doctor`; and T6 — bootstrap an uninstalled
  manager (Flatpak/Snap/cargo/…) before its app instead of falling to GitHub.
* Sun Jul 12 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.6~beta-1
- Rebuild against the v0.1.6-beta release: AUR provider + yay/paru (Arch-family
  only), `jii sources` merges providers with add/remove of ecosystem managers,
  quiet per-source `jii update` summary + parallel self-check, mid-word typo
  recovery, and -d/--dry-run vs -n/--no disambiguation.
* Sat Jul 11 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.5~beta-1
- Rebuild against the v0.1.5-beta release (declarative Nix Etaps A/B/C, Gentoo
  provider, doctor repo-enable ordering, install preference).
* Thu Jul 09 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.0~beta-1
- First public Beta.
