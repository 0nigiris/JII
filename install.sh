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

# --- Presentation -----------------------------------------------------------
# A friendly, branded install experience (the owner's ask: make `curl … | sh` look
# as polished as a good vendor installer). Everything degrades cleanly: colour only on a
# real terminal that isn't NO_COLOR/dumb, Unicode glyphs only under a UTF-8 locale (and
# never on the framebuffer console, whose font lacks them). None of this touches the
# install logic below — it only styles the output.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-}" != "dumb" ]; then
  _e=$(printf '\033')
  C0="${_e}[0m"; CB="${_e}[1m"; CD="${_e}[2m"; CG="${_e}[32m"; CP="${_e}[38;5;99m"; CY="${_e}[33m"
else
  C0=""; CB=""; CD=""; CG=""; CP=""; CY=""
fi
case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in
  *[Uu][Tt][Ff]8* | *[Uu][Tt][Ff]-8*) _uni=1 ;;
  *) _uni=0 ;;
esac
[ "${TERM:-}" = "linux" ] && _uni=0
if [ "$_uni" = 1 ]; then
  OK="✓"; DOT="•"; RL="─"; ARR="→"; BUL="·"
  BTL="╭"; BTR="╮"; BBL="╰"; BBR="╯"; BV="│"; BCN="╾─"
else
  OK="+"; DOT="="; RL="-"; ARR="->"; BUL="*"
  BTL="+"; BTR="+"; BBL="+"; BBR="+"; BV="|"; BCN="<-"
fi

# Repeat $1 exactly $2 times (small counts only — the banner and rules).
_repeat() { _n=$2; _o=""; _i=0; while [ "$_i" -lt "$_n" ]; do _o="$_o$1"; _i=$((_i + 1)); done; printf '%s' "$_o"; }
# Centre ASCII text $1 inside a field of width $2 with spaces (exact — ASCII only, so
# ${#…} is the display width). Longer-than-field text is returned unchanged.
_center() {
  _t="$1"; _w="$2"; _l=${#_t}
  [ "$_l" -ge "$_w" ] && { printf '%s' "$_t"; return; }
  _pad=$((_w - _l)); _lft=$((_pad / 2)); _rgt=$((_pad - _lft))
  printf '%s%s%s' "$(_repeat ' ' "$_lft")" "$_t" "$(_repeat ' ' "$_rgt")"
}

# Terminal width, clamped to a comfortable range (falls back to 60 with no `tput`).
_cols() {
  c=$(tput cols 2>/dev/null || echo 0)
  case "$c" in *[!0-9]* | "") c=60 ;; esac
  [ "$c" -lt 24 ] && c=24
  [ "$c" -gt 72 ] && c=72
  printf '%s' "$c"
}
# A dim horizontal rule across the width (a section separator, like the vendor mockup).
rule() {
  w=$(_cols); i=0; s=""
  while [ "$i" -lt "$w" ]; do s="$s$RL"; i=$((i + 1)); done
  printf '%s%s%s\n' "$CD" "$s" "$C0"
}
ok() { printf '%s%s%s %s\n' "$CG" "$OK" "$C0" "$1"; }
bullet() { printf '%s%s%s %s\n' "$CD" "$BUL" "$C0" "$1"; }
warn() { printf '%s%s %s%s\n' "$CY" "!" "$1" "$C0"; }
# A completed step with a decorative full progress bar, e.g. `✓ Downloaded jii… [•••••] 100%`.
ok_bar() {
  w=$(_cols)
  cells=$((w - ${#1} - 12))
  [ "$cells" -lt 6 ] && cells=6
  [ "$cells" -gt 32 ] && cells=32
  bar=""; i=0
  while [ "$i" -lt "$cells" ]; do bar="$bar$DOT"; i=$((i + 1)); done
  printf '%s%s%s %s  %s[%s%s%s]%s %s100%%%s\n' \
    "$CG" "$OK" "$C0" "$1" "$CD" "$CP" "$bar" "$CD" "$C0" "$CB" "$C0"
}
# A spinner shown while a background job (a download) runs, so a slow network never
# looks like a hang. Inert when there's no terminal to animate (a pipe / CI).
_spin_frames() {
  if [ "$_uni" = 1 ]; then printf '⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏'; else printf '| / - \\'; fi
}
_spin_wait() {
  _p="$1"; _lbl="$2"
  if [ ! -t 1 ]; then wait "$_p" 2>/dev/null; return "$?"; fi
  while kill -0 "$_p" 2>/dev/null; do
    for _f in $(_spin_frames); do
      kill -0 "$_p" 2>/dev/null || break
      printf '\r  %s%s%s %s' "$CP" "$_f" "$C0" "$_lbl"
      sleep 0.08 2>/dev/null || sleep 1
    done
  done
  printf '\r%s\r' "$(_repeat ' ' $((${#_lbl} + 4)))"
  wait "$_p" 2>/dev/null
}
# Download $1 → $2 while showing a spinner labelled $3; propagates the download's status.
dl_progress() {
  if [ -t 1 ]; then
    dl "$1" "$2" &
    _spin_wait "$!" "$3"
  else
    dl "$1" "$2"
  fi
}

# The JII cube (its logo, in ASCII) beside a bordered, centre-aligned tagline card.
banner() {
  _t1="One installer for every source"
  _t2="you have. It explains its picks."
  _bar="$(_repeat "$RL" 36)"
  # Box rows (border dim, title bold); content is centred inside a 36-wide field.
  BXa="${CD}${BTL}${_bar}${BTR}${C0}"
  BXb="${CD}${BV}${C0}$(_repeat ' ' 36)${CD}${BV}${C0}"
  BXc="${CD}${BV}${C0}${CB}$(_center 'Just Install It.' 36)${C0}${CD}${BV}${C0}"
  BXe="${CD}${BV}${C0}$(_center "$_t1" 36)${CD}${BV}${C0}"
  BXf="${CD}${BV}${C0}$(_center "$_t2" 36)${CD}${BV}${C0}"
  BXh="${CD}${BBL}${_bar}${BBR}${C0}"
  GAP="   "; GAPC=" ${CD}${BCN}${C0}"
  printf '\n'
  printf '%s%s%s%s%s\n' "$CP" "            " "$C0" "$GAP"  "$BXa"
  printf '%s%s%s%s%s\n' "$CP" "   ________ " "$C0" "$GAP"  "$BXb"
  printf '%s%s%s%s%s\n' "$CP" "  /       /|" "$C0" "$GAP"  "$BXc"
  printf '%s%s%s%s%s\n' "$CP" " /_______/ |" "$C0" "$GAP"  "$BXb"
  printf '%s%s%s%s%s\n' "$CP" " | J I I | |" "$C0" "$GAPC" "$BXe"
  printf '%s%s%s%s%s\n' "$CP" " |       | /" "$C0" "$GAP"  "$BXf"
  printf '%s%s%s%s%s\n' "$CP" " |_______|/ " "$C0" "$GAP"  "$BXb"
  printf '%s%s%s%s%s\n' "$CP" "            " "$C0" "$GAP"  "$BXh"
  printf '\n'
}
# The closing "you're all set" block: what to run, how to remove, where the docs are.
done_footer() {
  _run="$1"; _uninstall="$2"
  printf '\n'
  rule
  ok "${CB}JII is ready.${C0}"
  [ -n "$_run" ] && bullet "Run:        $_run"
  bullet "Uninstall:  $_uninstall"
  printf '\n'
  printf '%sDocs %s github.com/%s   %s   Issues %s github.com/%s/issues%s\n' \
    "$CD" "$ARR" "$REPO" "$BUL" "$ARR" "$REPO" "$C0"
}

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
banner
TAG="${JII_VERSION:-}"
if [ -z "$TAG" ]; then
  bullet "Finding the latest release…"
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
ok "JII $TAG is available"

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
    warn "Could not fetch release metadata; falling back to a portable install."
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
    warn "No native $NATIVE_KIND package for $ARCH in $TAG; falling back to a portable install."
    return 1
  }

  ASSET=$(basename "$URL")
  dl_progress "$URL" "$TMP/$ASSET" "Downloading $ASSET…" || {
    warn "Download failed; falling back to a portable install."
    return 1
  }
  ok_bar "Downloaded $ASSET"
  if dl "$URL.sha256" "$TMP/$ASSET.sha256" 2>/dev/null; then
    verify_sha256 "$TMP/$ASSET" "$TMP/$ASSET.sha256" \
      || err "checksum verification failed — refusing to install."
    ok "Checksum verified"
  fi

  case "$NATIVE_MGR" in
    dnf) set -- dnf install -y "$TMP/$ASSET" ;;
    zypper) set -- zypper --non-interactive install --allow-unsigned-rpm "$TMP/$ASSET" ;;
    apt) set -- apt-get install -y "$TMP/$ASSET" ;;
    *) return 1 ;;
  esac
  rule
  bullet "Installing via $NATIVE_MGR (sudo may ask for your password):"
  bullet "  ${ESC:+$ESC }$*"
  # shellcheck disable=SC2086
  $ESC "$@" || {
    warn "Native install failed; falling back to a portable install."
    return 1
  }
  return 0
}

# --- 8. Portable install ----------------------------------------------------
portable_install() {
  ASSET="jii-${TAG}-${ARCH}-linux.tar.gz"
  BASE="https://github.com/$REPO/releases/download/$TAG"

  dl_progress "$BASE/$ASSET" "$TMP/$ASSET" "Downloading $ASSET…" || err "download failed: $BASE/$ASSET"
  ok_bar "Downloaded $ASSET"

  if dl "$BASE/$ASSET.sha256" "$TMP/$ASSET.sha256" 2>/dev/null; then
    verify_sha256 "$TMP/$ASSET" "$TMP/$ASSET.sha256" \
      || err "checksum verification failed — refusing to install."
    ok "Checksum verified"
  else
    warn "No checksum published for this asset; skipping verification."
  fi

  tar -xzf "$TMP/$ASSET" -C "$TMP"
  # The tarball holds a top-level dir jii-<tag>-<arch>-linux/ with the binary inside.
  SRC=$(find "$TMP" -type f -name jii -perm -u+x | head -n1)
  [ -n "$SRC" ] || err "the archive did not contain a 'jii' binary."

  mkdir -p "$BIN_DIR"
  install -m 0755 "$SRC" "$BIN_DIR/jii"
  ok "Installed to $BIN_DIR/jii"

  # PATH hint (portable only — the native path lands in /usr/bin, already on PATH).
  # Either way say something, so the user always knows whether `jii` is runnable now
  # (some distros, e.g. openSUSE, already put ~/.local/bin on PATH — then just confirm it).
  case ":$PATH:" in
    *":$BIN_DIR:"*)
      PORTABLE_RUN="jii doctor"
      ;;
    *)
      PORTABLE_RUN="$BIN_DIR/jii doctor"
      bullet "$BIN_DIR is not on your PATH yet — add it to run \`jii\` by name:"
      bullet "  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
      ;;
  esac
}

# --- 9. Do it ---------------------------------------------------------------
if [ "$METHOD" = "native" ]; then
  if [ "$NATIVE_KIND" = "aur" ]; then
    warn "On Arch, the native package is the AUR 'jii-bin' (\`yay -S jii-bin\`) — not published yet."
    bullet "Installing the portable binary for now."
    METHOD="portable"
  elif [ "$NATIVE_OK" -eq 1 ]; then
    if native_install; then
      ok "Installed via $NATIVE_MGR"
      done_footer "jii doctor" "${ESC:+$ESC }$NATIVE_MGR remove jii"
      exit 0
    fi
    METHOD="portable"
  else
    warn "No supported native package manager with escalation here; installing portable."
    METHOD="portable"
  fi
fi

portable_install
done_footer "${PORTABLE_RUN:-jii doctor}" "rm $BIN_DIR/jii"
