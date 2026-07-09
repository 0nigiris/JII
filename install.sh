#!/bin/sh
# JII installer — download a prebuilt binary and drop it on your PATH. No build, no root.
#
#   curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/master/install.sh | sh
#
# What it does: detects your CPU (x86_64 / arm64), downloads the matching static
# musl binary from the latest GitHub release, verifies its sha256, and installs it
# to ~/.local/bin (override with JII_BIN_DIR). It never needs sudo.
#
# Options via env:
#   JII_VERSION=v0.1.0-beta   install a specific tag (default: latest release)
#   JII_BIN_DIR=/usr/local/bin install elsewhere (may need write access)
set -eu

REPO="0nigiris/JII"
BIN_DIR="${JII_BIN_DIR:-$HOME/.local/bin}"

err() { printf 'jii-install: %s\n' "$1" >&2; exit 1; }
info() { printf '%s\n' "$1"; }

# --- 1. Preconditions -------------------------------------------------------
[ "$(uname -s)" = "Linux" ] || err "JII is Linux-only (found $(uname -s))."

if command -v curl >/dev/null 2>&1; then
  dl() { curl -fsSL "$1" -o "$2"; }
  fetch() { curl -fsSL "$1"; }
elif command -v wget >/dev/null 2>&1; then
  dl() { wget -qO "$2" "$1"; }
  fetch() { wget -qO- "$1"; }
else
  err "need curl or wget to download."
fi

# --- 2. Detect architecture -------------------------------------------------
case "$(uname -m)" in
  x86_64 | amd64) ARCH="x86_64" ;;
  aarch64 | arm64) ARCH="aarch64" ;;
  *) err "unsupported CPU architecture: $(uname -m) (x86_64 and aarch64 are published)." ;;
esac

# --- 3. Resolve the version -------------------------------------------------
TAG="${JII_VERSION:-}"
if [ -z "$TAG" ]; then
  info "Finding the latest release…"
  # Use the /releases *list*, not /releases/latest: the latter 404s on a repo whose
  # only releases are pre-releases (every JII beta tag is one). The list is newest-first,
  # so the first tag_name is the newest published release. Parsed without needing jq.
  TAG=$(fetch "https://api.github.com/repos/$REPO/releases?per_page=20" \
    | grep -m1 '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
  [ -n "$TAG" ] || err "could not determine the latest release; set JII_VERSION=<tag>."
fi

ASSET="jii-${TAG}-${ARCH}-linux.tar.gz"
BASE="https://github.com/$REPO/releases/download/$TAG"

# --- 4. Download + verify + install ----------------------------------------
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM

info "Downloading $ASSET …"
dl "$BASE/$ASSET" "$TMP/$ASSET" || err "download failed: $BASE/$ASSET"

if dl "$BASE/$ASSET.sha256" "$TMP/$ASSET.sha256" 2>/dev/null; then
  info "Verifying checksum…"
  # The .sha256 file records the asset's basename; verify from inside $TMP.
  ( cd "$TMP" && sha256sum -c "$ASSET.sha256" >/dev/null 2>&1 ) \
    || err "checksum verification failed — refusing to install."
else
  info "No checksum published for this asset; skipping verification."
fi

tar -xzf "$TMP/$ASSET" -C "$TMP"
# The tarball holds a top-level dir jii-<tag>-<arch>-linux/ with the binary inside.
SRC=$(find "$TMP" -type f -name jii -perm -u+x | head -n1)
[ -n "$SRC" ] || err "the archive did not contain a 'jii' binary."

mkdir -p "$BIN_DIR"
install -m 0755 "$SRC" "$BIN_DIR/jii"
info "Installed jii $TAG → $BIN_DIR/jii"

# --- 5. PATH hint -----------------------------------------------------------
case ":$PATH:" in
  *":$BIN_DIR:"*) : ;;
  *)
    info ""
    info "⚠ $BIN_DIR is not on your PATH. Add it, e.g.:"
    info "    echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
    ;;
esac

info ""
info "Done. Try:  jii doctor"
