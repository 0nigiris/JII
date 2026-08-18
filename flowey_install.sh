#!/bin/sh
# JII — the *secret* Omega Flowey installer.
#
#   curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/flowey/flowey_install.sh | sh
#
# Downloads "Omega Flowey Simulator" — Undertale's final neutral-route boss, rebuilt as a
# Scratch project and packaged for the desktop — opens it as a real window, and installs JII
# **once you win**. Normal or hard mode, both count. The game drops a `flowey-install` marker
# naming which one; JII reads it on its next run and unlocks the secret 🌻 achievement.
#
# Honest fallbacks (never a dead end): no interactive terminal, no graphical session, not
# x86_64 Linux, no room on disk, or the download fails → a normal, fight-free install. Lose the
# fight and it *offers* one, because losing to Omega Flowey is the expected outcome.
#
# Nothing here runs as root. See docs/DECISIONS.md ADR-0081.
set -eu

# --- knobs (env overrides) -------------------------------------------------
# The packaged game (Electron + the Scratch project, ~121 MB compressed).
GAME_SRC="${JII_FLOWEY_SRC:-https://github.com/0nigiris/JII/releases/download/flowey-game/omega-flowey-linux-x86_64.tar.gz}"
# The canonical JII installer (site-hosted, GitHub is its own fallback).
INSTALL_URL="${JII_INSTALL_URL:-https://sudonit.com/install.sh}"
# Set JII_FLOWEY_NO_INSTALL=1 to play the fight without installing (testing).
NO_INSTALL="${JII_FLOWEY_NO_INSTALL:-0}"
MARKER_NAME="flowey-install"
# The tarball plus what it unpacks to, with a little room to spare.
NEED_KB=420000

# --- pretty output (only on a TTY) -----------------------------------------
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  C0=$(printf '\033[0m'); CB=$(printf '\033[1m'); CD=$(printf '\033[2m')
  CG=$(printf '\033[32m'); CY=$(printf '\033[33m')
else
  C0=''; CB=''; CD=''; CG=''; CY=''
fi
info()  { printf '%s\n' "$*"; }
note()  { printf '%s%s%s\n' "$CD" "$*" "$C0"; }
good()  { printf '%s%s%s\n' "$CG" "$*" "$C0"; }
warn()  { printf '%s%s%s\n' "$CY" "$*" "$C0" >&2; }

# --- normal (fight-free) install -------------------------------------------
run_normal_install() {
  [ "$NO_INSTALL" = "1" ] && { note "JII_FLOWEY_NO_INSTALL=1 — skipping install."; return 0; }
  info "Installing JII the normal way…"
  curl -fsSL "$INSTALL_URL" | sh
}

# --- gates: fall back to a normal install when the fight can't run ----------
if [ ! -t 1 ]; then
  warn "Not an interactive terminal — no fight this time."
  run_normal_install; exit $?
fi
if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required." >&2; exit 1
fi
if ! command -v tar >/dev/null 2>&1; then
  warn "tar is required to unpack the fight — installing normally."
  run_normal_install; exit $?
fi
ARCH=$(uname -m 2>/dev/null || echo unknown)
OSN=$(uname -s 2>/dev/null || echo unknown)
if [ "$OSN" != "Linux" ] || { [ "$ARCH" != "x86_64" ] && [ "$ARCH" != "amd64" ]; }; then
  warn "This build is Linux x86_64 only ($OSN/$ARCH) — installing normally."
  run_normal_install; exit $?
fi
if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
  warn "No graphical session detected — can't open the fight. Installing normally."
  run_normal_install; exit $?
fi

# --- somewhere with room to unpack -----------------------------------------
# The game is big. Prefer $TMPDIR, but fall back to the home cache rather than filling a
# small /tmp (often a RAM-backed tmpfs) and failing halfway through.
free_kb() {
  kb=$(df -Pk "$1" 2>/dev/null | awk 'NR==2 {print $4}')
  case "$kb" in ''|*[!0-9]*) echo 0 ;; *) echo "$kb" ;; esac
}
BASE="${TMPDIR:-/tmp}"
if [ "$(free_kb "$BASE")" -lt "$NEED_KB" ]; then
  BASE="${XDG_CACHE_HOME:-$HOME/.cache}"
  mkdir -p "$BASE" 2>/dev/null || true
fi
if [ "$(free_kb "$BASE")" -lt "$NEED_KB" ]; then
  warn "Not enough free space for the fight (~400 MB) — installing normally."
  run_normal_install; exit $?
fi

# --- state dir + clear any stale marker ------------------------------------
STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
STATE_DIR="$STATE_HOME/jii"
MARKER="$STATE_DIR/$MARKER_NAME"
mkdir -p "$STATE_DIR"
rm -f "$MARKER"

WORK="$(mktemp -d "$BASE/jii-flowey.XXXXXX")"
cleanup() { rm -rf "$WORK" 2>/dev/null || true; }
jii_cancel() {
  printf '\n'; warn "Cancelled — nothing was installed."
  cleanup; exit 130
}
trap jii_cancel INT TERM
trap cleanup EXIT

printf '%s\n' "${CB}* You IDIOT.${C0}"
note "Omega Flowey. Normal or hard — either one counts, if you can actually win."
note "(Ctrl-C to cancel. This changes nothing until you win.)"
info ""

# --- fetch the game --------------------------------------------------------
info "Downloading the fight (~121 MB)…"
TARBALL="$WORK/omega-flowey.tar.gz"
if ! curl -f -L --retry 3 --retry-delay 1 --progress-bar "$GAME_SRC" -o "$TARBALL"; then
  warn "Couldn't download the fight — installing JII normally instead."
  run_normal_install; exit $?
fi
info "Unpacking…"
if ! tar -xzf "$TARBALL" -C "$WORK"; then
  warn "Couldn't unpack the fight — installing JII normally instead."
  run_normal_install; exit $?
fi
rm -f "$TARBALL"
APP="$WORK/omega-flowey/omega-flowey"
if [ ! -f "$APP" ]; then
  warn "The download didn't contain the game — installing JII normally instead."
  run_normal_install; exit $?
fi
chmod +x "$APP" "$WORK/omega-flowey/chrome_crashpad_handler" 2>/dev/null || true

# --- run the fight (native window) -----------------------------------------
info "Opening the fight… good luck. You'll need it."
note "Arrows to move, Z/Enter to confirm. Close the window when you're done."
info ""
"$APP" >/dev/null 2>&1 || true

# --- outcome ---------------------------------------------------------------
if [ ! -f "$MARKER" ]; then
  warn "He won. Of course he did."
  info ""
  # Losing to Omega Flowey is the normal outcome, so this is never where it ends: offer the
  # plain install instead of dropping the user on the floor with nothing.
  printf '%sInstall JII the normal way anyway? [Y/n] %s' "$CB" "$C0"
  read -r ANSWER </dev/tty || ANSWER=""
  case "$ANSWER" in
    [nN]*) note "Fine. Run this again when you want another round."; exit 1 ;;
    *) info ""; run_normal_install; exit $? ;;
  esac
fi
ENDING=$(cat "$MARKER" 2>/dev/null || echo normal)
good "* You won ($ENDING). Installing JII…"
info ""

if [ "$NO_INSTALL" = "1" ]; then
  note "JII_FLOWEY_NO_INSTALL=1 — skipping install (marker left for JII to read)."
  exit 0
fi

curl -fsSL "$INSTALL_URL" | sh

# Reveal the achievement right away if jii is now on PATH (this consumes the marker).
if command -v jii >/dev/null 2>&1; then
  info ""
  jii achievements || true
else
  info ""
  note "Run 'jii achievements' to see what you just unlocked. 🌻"
fi
