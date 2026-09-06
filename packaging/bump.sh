#!/bin/sh
# SPDX-FileCopyrightText: 2026 0nigiris
#
# SPDX-License-Identifier: GPL-3.0-or-later
# Bump every downstream packaging recipe to a released version.
#
#   packaging/bump.sh 0.1.20-beta
#
# The recipes here are prebuilt-binary recipes: each one names a release tag and
# carries the checksums of that release's tarballs. Six files drifting apart by hand
# is how they ended up three months stale, so this script is the only way they move.
# It downloads the release's small `.sha256` sidecars — never the tarballs — and
# rewrites the version and digests in place.
#
# Alpine is the exception: aports wants sha512, which cannot be derived from sha256,
# so its `sha512sums` stays empty for `abuild checksum` to fill on the build host.
# Gentoo carries no digests either (`ebuild … manifest` generates them), only a
# version in its filename.
set -eu

VERSION="${1:-}"
if [ -z "$VERSION" ]; then
	echo "usage: packaging/bump.sh <version>   e.g. 0.1.20-beta" >&2
	exit 2
fi

TAG="v${VERSION}"
BASE="https://github.com/0nigiris/JII/releases/download/${TAG}"
DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

# Underscore form for package systems that forbid a hyphen in the version.
US=$(printf '%s' "$VERSION" | tr '-' '_')
# Void forbids both hyphen and underscore.
VOID=$(printf '%s' "$VERSION" | tr -d '-')

fetch_sha() {
	curl -fsSL "${BASE}/jii-${TAG}-$1-linux.tar.gz.sha256" | cut -d' ' -f1
}

echo "Fetching checksums for ${TAG}…"
SHA_X=$(fetch_sha x86_64)
SHA_A=$(fetch_sha aarch64)
[ ${#SHA_X} -eq 64 ] && [ ${#SHA_A} -eq 64 ] || { echo "bad checksums — is ${TAG} released?" >&2; exit 1; }

# Nix wants SRI (base64), which is the same 32 bytes in another alphabet.
# hex → raw bytes → base64, without xxd (absent on plenty of minimal hosts).
sri() { printf "$(printf '%s' "$1" | sed 's/../\\x&/g')" | base64 | tr -d '\n'; }
SRI_X="sha256-$(sri "$SHA_X")"
SRI_A="sha256-$(sri "$SHA_A")"

edit() { # edit <file> <sed-expr>...
	f="$1"; shift
	tmp="${f}.bump"
	cp "$f" "$tmp"
	for e in "$@"; do sed -i "$e" "$tmp"; done
	mv "$tmp" "$f"
	echo "  bumped ${f#"$DIR"/}"
}

edit "$DIR/aur/PKGBUILD" \
	"s/^pkgver=.*/pkgver=${US}/" \
	"s/^sha256sums_x86_64=.*/sha256sums_x86_64=('${SHA_X}')/" \
	"s/^sha256sums_aarch64=.*/sha256sums_aarch64=('${SHA_A}')/" \
	"s|Digests of the v[0-9][^ ]* release|Digests of the ${TAG} release|"

edit "$DIR/alpine/APKBUILD" \
	"s/^pkgver=.*/pkgver=${US}/" \
	"s|reconstruct the real tag (v[0-9][^)]*)|reconstruct the real tag (${TAG})|"

edit "$DIR/void/template" \
	"s/^version=.*/version=${VOID}/" \
	"s/^_tag=.*/_tag=\"${TAG}\"/" \
	"s|x86_64)  _arch=x86_64;  checksum=[0-9a-f]*|x86_64)  _arch=x86_64;  checksum=${SHA_X}|" \
	"s|aarch64) _arch=aarch64; checksum=[0-9a-f]*|aarch64) _arch=aarch64; checksum=${SHA_A}|"

edit "$DIR/nix/jii.nix" \
	"s|^  version = \".*\";|  version = \"${VERSION}\";|" \
	"/\"x86_64-linux\"/,/};/ s|hash = \"sha256-[^\"]*\";|hash = \"${SRI_X}\";|" \
	"/\"aarch64-linux\"/,/};/ s|hash = \"sha256-[^\"]*\";|hash = \"${SRI_A}\";|"

edit "$DIR/homebrew/jii.rb" \
	"s|^  version \".*\"|  version \"${VERSION}\"|" \
	"s|releases/download/v[^/]*/jii-v[^-]*-beta-|releases/download/${TAG}/jii-${TAG}-|g" \
	"/on_intel do/,/end/ s|sha256 \"[0-9a-f]*\"|sha256 \"${SHA_X}\"|" \
	"/on_arm do/,/end/ s|sha256 \"[0-9a-f]*\"|sha256 \"${SHA_A}\"|"

# Gentoo carries its version in the filename and nothing else.
old=$(ls "$DIR"/gentoo/jii-bin-*.ebuild 2>/dev/null | head -1 || true)
new="$DIR/gentoo/jii-bin-${US}.ebuild"
if [ -n "$old" ] && [ "$old" != "$new" ]; then
	if git -C "$DIR/.." ls-files --error-unmatch "$old" >/dev/null 2>&1; then
		git -C "$DIR/.." mv "$old" "$new"
	else
		mv "$old" "$new"
	fi
	sed -i "s/jii-bin-[0-9][0-9a-z._]*\.ebuild/jii-bin-${US}.ebuild/g" "$new"
	echo "  renamed gentoo/$(basename "$old") → jii-bin-${US}.ebuild"
fi

sed -i "s|download/v[0-9][0-9a-z.-]*/jii-v[0-9][0-9a-z.-]*-\$a|download/${TAG}/jii-${TAG}-\$a|; \
	s|jii [0-9][0-9a-z.-]*\"|jii ${VERSION}\"|; \
	s|jii-bin [0-9][0-9a-z._]*\"|jii-bin ${US}\"|" "$DIR/README.md"
echo "  bumped packaging/README.md"

echo
echo "All recipes now point at ${TAG}."
echo "Alpine's sha512sums stays empty — run \`abuild checksum\` on the build host."
