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

%global _tag v0.1.6-beta

Name:           jii
Version:        0.1.6~beta
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
