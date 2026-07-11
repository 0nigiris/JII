# Copyright 2026 0nigiris
# Distributed under the terms of the GNU General Public License v3
#
# Prebuilt-binary ebuild for JII. Install into an overlay:
#   <overlay>/app-admin/jii-bin/jii-bin-0.1.5_beta.ebuild
# then `ebuild jii-bin-0.1.5_beta.ebuild manifest` (generates the Manifest with hashes
# from the fetched tarballs) and `emerge app-admin/jii-bin`. The release binary is a
# static musl build, so RESTRICT=strip and no toolchain is needed.
EAPI=8

inherit

MY_TAG="v${PV/_/-}"

DESCRIPTION="Smart universal package installer for Linux"
HOMEPAGE="https://github.com/0nigiris/JII"
SRC_URI="
	amd64? ( ${HOMEPAGE}/releases/download/${MY_TAG}/jii-${MY_TAG}-x86_64-linux.tar.gz )
	arm64? ( ${HOMEPAGE}/releases/download/${MY_TAG}/jii-${MY_TAG}-aarch64-linux.tar.gz )
"

LICENSE="GPL-3+"
SLOT="0"
KEYWORDS="-* ~amd64 ~arm64"
RESTRICT="strip"  # prebuilt static binary
S="${WORKDIR}"

src_install() {
	local d
	if use amd64; then
		d="jii-${MY_TAG}-x86_64-linux"
	else
		d="jii-${MY_TAG}-aarch64-linux"
	fi

	dobin "${d}"/jii
	doman "${d}"/jii.1
	dodoc "${d}"/README.md

	newbashcomp "${d}"/completions/jii.bash jii
	insinto /usr/share/zsh/site-functions
	doins "${d}"/completions/_jii
	insinto /usr/share/fish/vendor_completions.d
	doins "${d}"/completions/jii.fish
}
