#!/bin/sh
# JII installer.
#
#   curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/master/install.sh | sh
#
# Two ways to install (see JII_METHOD below):
#   • native   — the system package (.rpm/.deb) via dnf/apt/zypper. Integrated: man page,
#                shell completions, removable with your package manager. Needs sudo.
#   • portable — the static musl binary dropped in ~/.local/bin. No root, any distro.
#
# By default (JII_METHOD=auto): if a supported package manager is present and this is an
# interactive terminal, JII asks which you want (default: native). In a pipe / CI (no
# terminal) it installs portable and never runs sudo unprompted — escalation only ever
# happens after you say yes.
#
# Options via env (or args --native / --portable):
#   JII_METHOD=auto|native|portable  install method (default: auto)
#   JII_VERSION=v0.1.5-beta          install a specific tag (default: latest release)
#   JII_BIN_DIR=/usr/local/bin       portable install dir (default: ~/.local/bin)
set -eu

REPO="0nigiris/JII"
BIN_DIR="${JII_BIN_DIR:-$HOME/.local/bin}"
METHOD="${JII_METHOD:-auto}"

err() { printf 'jii-install: %s\n' "$1" >&2; exit 1; }
info() { printf '%s\n' "$1"; }

# --- 0. Parse args ----------------------------------------------------------
for arg in "$@"; do
  case "$arg" in
    --native) METHOD="native" ;;
    --portable) METHOD="portable" ;;
    --method=*) METHOD="${arg#--method=}" ;;
    -h | --help)
      info "Usage: install.sh [--native|--portable]   (or set JII_METHOD)"
      exit 0
      ;;
    *) err "unknown option: $arg (try --native or --portable)" ;;
  esac
done
case "$METHOD" in
  auto | native | portable) : ;;
  *) err "JII_METHOD must be auto, native or portable (got '$METHOD')." ;;
esac

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

# Verify FILE against a sha256 SIDECAR by comparing digests only — never by the
# filename recorded inside the sidecar. nfpm writes the package's real version into
# the sidecar (jii-0.1.5~beta-…), but GitHub rewrites '~' to '.' in uploaded asset
# names (jii-0.1.5.beta-…), so `sha256sum -c` looks for a file that isn't on disk and
# fails spuriously even though the bytes are correct. Compare the hash, ignore the name.
verify_sha256() {
  _want=$(awk '{print $1; exit}' "$2" 2>/dev/null)
  _got=$(sha256sum "$1" 2>/dev/null | awk '{print $1; exit}')
  [ -n "$_want" ] && [ "$_want" = "$_got" ]
}

# --- 2. Detect architecture -------------------------------------------------
case "$(uname -m)" in
  x86_64 | amd64) ARCH="x86_64"; RPMARCH="x86_64"; DEBARCH="amd64" ;;
  aarch64 | arm64) ARCH="aarch64"; RPMARCH="aarch64"; DEBARCH="arm64" ;;
  *) err "unsupported CPU architecture: $(uname -m) (x86_64 and aarch64 are published)." ;;
esac

# --- 3. Detect the native package manager + escalation ----------------------
# NATIVE_KIND: rpm | deb (a package we publish and can install here) | aur | "" (none).
if command -v dnf >/dev/null 2>&1; then
  NATIVE_MGR="dnf"; NATIVE_KIND="rpm"
elif command -v apt-get >/dev/null 2>&1; then
  NATIVE_MGR="apt"; NATIVE_KIND="deb"
elif command -v zypper >/dev/null 2>&1; then
  NATIVE_MGR="zypper"; NATIVE_KIND="rpm"
elif command -v pacman >/dev/null 2>&1; then
  NATIVE_MGR="pacman"; NATIVE_KIND="aur"
else
  NATIVE_MGR=""; NATIVE_KIND=""
fi

if [ "$(id -u)" -eq 0 ]; then
  ESC=""; CAN_ESC=1
elif command -v sudo >/dev/null 2>&1; then
  ESC="sudo"; CAN_ESC=1
else
  ESC=""; CAN_ESC=0
fi

# Can we do a real native install here (have a publishable package + a way to escalate)?
NATIVE_OK=0
case "$NATIVE_KIND" in
  rpm | deb) [ "$CAN_ESC" -eq 1 ] && NATIVE_OK=1 ;;
esac

# --- 4. Resolve the version -------------------------------------------------
TAG="${JII_VERSION:-}"
if [ -z "$TAG" ]; then
  info "Finding the latest release…"
  # Use the /releases *list*, not /releases/latest: the latter 404s on a repo whose
  # only releases are pre-releases (every JII beta tag is one). The list is newest-first,
  # so the first tag_name is the newest published release. Parsed without needing jq.
  # Buffer the whole response first (into a variable), *then* filter — piping curl
  # straight into `grep -m1` closes the pipe early and makes curl print a spurious
  # "curl: (23)" write error.
  RELEASES=$(fetch "https://api.github.com/repos/$REPO/releases?per_page=20") \
    || err "could not reach GitHub to find the latest release; set JII_VERSION=<tag>."
  TAG=$(printf '%s\n' "$RELEASES" | grep -m1 '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
  [ -n "$TAG" ] || err "could not determine the latest release; set JII_VERSION=<tag>."
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT INT TERM

# --- 5. Ask a yes/no question on the controlling terminal -------------------
# Returns 0 for yes (default on empty input), 1 for no *or* when there is no terminal
# to ask on (a pipe / CI). Never blocks a non-interactive run.
ask_default_yes() {
  # Prompt and read inside a group whose stderr is silenced, so a failed open of
  # /dev/tty (no controlling terminal) produces no noise — the group just exits
  # non-zero and we fall back. On success the typed answer is echoed back to us.
  _ans=$(
    { printf '%s' "$1" >/dev/tty && IFS= read -r _r </dev/tty && printf '%s' "$_r"; } 2>/dev/null
  ) || return 1
  case "$_ans" in
    [Nn] | [Nn][Oo]) return 1 ;;
    *) return 0 ;;
  esac
}

# --- 6. Choose method (auto → ask or fall back) -----------------------------
if [ "$METHOD" = "auto" ]; then
  if [ "$NATIVE_OK" -eq 1 ] \
    && ask_default_yes "Install jii system-wide with $NATIVE_MGR (needs sudo)?  [Y = system / n = portable in ~/.local/bin] "; then
    METHOD="native"
  else
    METHOD="portable"
  fi
fi

# --- 7. Native install (returns non-zero to request a portable fallback) ----
native_install() {
  REL_JSON=$(fetch "https://api.github.com/repos/$REPO/releases/tags/$TAG") || {
    info "Could not fetch release metadata; falling back to a portable install."
    return 1
  }
  case "$NATIVE_KIND" in
    rpm) PAT="\\.$RPMARCH\\.rpm" ;;
    deb) PAT="_$DEBARCH\\.deb" ;;
  esac
  # Pick the asset's download URL straight from the release JSON (robust to the
  # package's version/release-number naming), excluding the .sha256 sidecar.
  URL=$(printf '%s\n' "$REL_JSON" \
    | grep -o '"browser_download_url": *"[^"]*"' \
    | sed 's/.*"\(https[^"]*\)".*/\1/' \
    | grep -E "$PAT\$" | grep -v '\.sha256$' | head -n1)
  [ -n "$URL" ] || {
    info "No native $NATIVE_KIND package for $ARCH in $TAG; falling back to a portable install."
    return 1
  }

  ASSET=$(basename "$URL")
  info "Downloading $ASSET …"
  dl "$URL" "$TMP/$ASSET" || {
    info "Download failed; falling back to a portable install."
    return 1
  }
  if dl "$URL.sha256" "$TMP/$ASSET.sha256" 2>/dev/null; then
    info "Verifying checksum…"
    verify_sha256 "$TMP/$ASSET" "$TMP/$ASSET.sha256" \
      || err "checksum verification failed — refusing to install."
  fi

  case "$NATIVE_MGR" in
    dnf) set -- dnf install -y "$TMP/$ASSET" ;;
    zypper) set -- zypper --non-interactive install --allow-unsigned-rpm "$TMP/$ASSET" ;;
    apt) set -- apt-get install -y "$TMP/$ASSET" ;;
    *) return 1 ;;
  esac
  info ""
  info "JII will install the native package with this command (sudo may ask for your password):"
  info "    ${ESC:+$ESC }$*"
  # shellcheck disable=SC2086
  $ESC "$@" || {
    info "Native install failed; falling back to a portable install."
    return 1
  }
  return 0
}

# --- 8. Portable install ----------------------------------------------------
portable_install() {
  ASSET="jii-${TAG}-${ARCH}-linux.tar.gz"
  BASE="https://github.com/$REPO/releases/download/$TAG"

  info "Downloading $ASSET …"
  dl "$BASE/$ASSET" "$TMP/$ASSET" || err "download failed: $BASE/$ASSET"

  if dl "$BASE/$ASSET.sha256" "$TMP/$ASSET.sha256" 2>/dev/null; then
    info "Verifying checksum…"
    verify_sha256 "$TMP/$ASSET" "$TMP/$ASSET.sha256" \
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

  # PATH hint (portable only — the native path lands in /usr/bin, already on PATH).
  case ":$PATH:" in
    *":$BIN_DIR:"*) : ;;
    *)
      info ""
      info "⚠ $BIN_DIR is not on your PATH. Add it, e.g.:"
      info "    echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
      ;;
  esac
}

# --- 9. Do it ---------------------------------------------------------------
if [ "$METHOD" = "native" ]; then
  if [ "$NATIVE_KIND" = "aur" ]; then
    info "On Arch, the native package is the AUR 'jii-bin' (\`yay -S jii-bin\`) — not published yet."
    info "Installing the portable binary for now."
    METHOD="portable"
  elif [ "$NATIVE_OK" -eq 1 ]; then
    if native_install; then
      info "Installed jii $TAG (native $NATIVE_KIND via $NATIVE_MGR)."
      info ""
      info "Done. Try:  jii doctor"
      exit 0
    fi
    METHOD="portable"
  else
    info "No supported native package manager with escalation here; installing portable."
    METHOD="portable"
  fi
fi

portable_install
info ""
info "Done. Try:  jii doctor"
