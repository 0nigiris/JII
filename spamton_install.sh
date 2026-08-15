#!/bin/sh
# JII — the *secret* Spamton NEO installer.
#
#   curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/spamton/spamton_install.sh | sh
#
# Downloads CherrySodaPop's "Spamton-NEO-VGB" — Deltarune's Spamton NEO fight recreated in
# the style of a retro handheld console (Godot, GPL-3.0) — launches it as a real desktop
# window, and installs JII **only once you win**: blow him apart, or cut his strings and set
# him free. The game drops a `spamton-install` marker with the ending; JII reads it on its
# next run and unlocks the secret 🎭 achievement.
#
# Honest fallbacks (never a dead end): no interactive terminal, no graphical session, not
# x86_64 Linux, or the download fails → a normal, fight-free install.
#
# Nothing here runs as root. See docs/DECISIONS.md ADR-0077.
set -eu

# --- knobs (env overrides) -------------------------------------------------
# The exported game (a single self-contained Godot binary, ~42 MB).
GAME_SRC="${JII_SPAMTON_SRC:-https://github.com/0nigiris/JII/releases/download/spamton-game/spamton-neo-vgb.x86_64}"
# The canonical JII installer (site-hosted, GitHub is its own fallback).
INSTALL_URL="${JII_INSTALL_URL:-https://sudonit.com/install.sh}"
# Set JII_SPAMTON_NO_INSTALL=1 to play the fight without installing (testing).
NO_INSTALL="${JII_SPAMTON_NO_INSTALL:-0}"
MARKER_NAME="spamton-install"

# --- pretty output (only on a TTY) -----------------------------------------
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  C0=$(printf '\033[0m'); CB=$(printf '\033[1m'); CD=$(printf '\033[2m')
  CP=$(printf '\033[35m'); CG=$(printf '\033[32m'); CY=$(printf '\033[33m')
else
  C0=''; CB=''; CD=''; CP=''; CG=''; CY=''
fi
info()  { printf '%s\n' "$*"; }
note()  { printf '%s%s%s\n' "$CD" "$*" "$C0"; }
good()  { printf '%s%s%s\n' "$CG" "$*" "$C0"; }
warn()  { printf '%s%s%s\n' "$CY" "$*" "$C0" >&2; }

dl() { curl -fsSL --retry 3 --retry-delay 1 "$1" -o "$2"; }

# --- normal (fight-free) install -------------------------------------------
run_normal_install() {
  [ "$NO_INSTALL" = "1" ] && { note "JII_SPAMTON_NO_INSTALL=1 — skipping install."; return 0; }
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

# --- state dir + clear any stale marker ------------------------------------
STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
STATE_DIR="$STATE_HOME/jii"
MARKER="$STATE_DIR/$MARKER_NAME"
mkdir -p "$STATE_DIR"
rm -f "$MARKER"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/jii-spamton.XXXXXX")"
cleanup() { rm -rf "$WORK" 2>/dev/null || true; }
jii_cancel() {
  printf '\n'; warn "Cancelled — nothing was installed."
  cleanup; exit 130
}
trap jii_cancel INT TERM
trap cleanup EXIT

printf '%s\n' "${CP}${CB}* [BIG SHOT] INCOMING${C0}"
note "A tiny handheld Spamton NEO. Blow him apart or cut his strings — either wins."
note "(Ctrl-C to cancel. This changes nothing until you win.)"
info ""

# --- fetch the game --------------------------------------------------------
info "Downloading the fight…"
APP="$WORK/spamton-neo-vgb"
if ! dl "$GAME_SRC" "$APP"; then
  warn "Couldn't download the fight — installing JII normally instead."
  run_normal_install; exit $?
fi
chmod +x "$APP" 2>/dev/null || true

# --- run the fight (native window) -----------------------------------------
info "Opening the fight… good luck."
note "Arrows to choose, Z/Enter to confirm. Close the window when you're done."
info ""
"$APP" >/dev/null 2>&1 || true

# --- outcome ---------------------------------------------------------------
if [ ! -f "$MARKER" ]; then
  warn "The fight ended without a win — nothing was installed."
  exit 1
fi
ENDING=$(cat "$MARKER" 2>/dev/null || echo spare)
good "* You won ($ENDING). Installing JII…"
info ""

if [ "$NO_INSTALL" = "1" ]; then
  note "JII_SPAMTON_NO_INSTALL=1 — skipping install (marker left for JII to read)."
  exit 0
fi

curl -fsSL "$INSTALL_URL" | sh

# Reveal the achievement right away if jii is now on PATH (this consumes the marker).
if command -v jii >/dev/null 2>&1; then
  info ""
  jii achievements || true
else
  info ""
  note "Run 'jii achievements' to see what you just unlocked. 🎭"
fi
