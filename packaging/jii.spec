# RPM spec for JII — a *binary repack* of the official GitHub release tarball.
#
# It downloads the prebuilt static-musl binary (plus man page, completions and docs)
# and installs them, so COPR/rpmbuild produce an installable .rpm with no compile and
# no build-time network beyond fetching Source0. Ideal for a `copr build` that Just Works.
# On each release, bump Version and %_tag. See packaging/README.md for the COPR steps.
#
# (A from-source spec using the Fedora rust-packaging macros is a post-Beta option once
# the crate is submitted to Fedora proper.)

%global _tag v0.1.5-beta

Name:           jii
Version:        0.1.5~beta
Release:        1%{?dist}
Summary:        A smart universal package installer for Linux

License:        GPLv3+
URL:            https://github.com/0nigiris/JII
Source0:        %{url}/releases/download/%{_tag}/jii-%{_tag}-%{_arch}-linux.tar.gz

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
%autosetup -n jii-%{_tag}-%{_arch}-linux

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
* Sat Jul 11 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.5~beta-1
- Rebuild against the v0.1.5-beta release (declarative Nix Etaps A/B/C, Gentoo
  provider, doctor repo-enable ordering, install preference).
* Thu Jul 09 2026 0nigiris <0nigiris@users.noreply.github.com> - 0.1.0~beta-1
- First public Beta.
