#!/bin/sh
# JII — the *secret* Chaos Simulator installer (Jevil edition).
#
#   curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/chaos/chaos_install.sh | sh
#
# Downloads a small, self-hosted fork of the "Chaos Simulator" (a Jevil battle,
# TurboWarp/Scratch packaged as an Electron app), launches it as a real desktop
# window, and installs JII **only once you win the fight** — whether you spare or
# kill Jevil. The modified game drops a `chaos-install` sentinel that JII reads on
# its next run to unlock the secret 🃏 `jevil` achievement (with the ending you got).
#
# Honest fallbacks (never a dead end): no interactive terminal, no graphical
# session, not x86_64 Linux, or the download fails → a normal, fight-free install.
#
# Nothing here runs as root. See docs/DECISIONS.md ADR-0076.
set -eu

# --- knobs (env overrides) -------------------------------------------------
# Where the game bundle tarball lives (a GitHub Release asset — ~98 MB packed).
CHAOS_SRC="${JII_CHAOS_SRC:-https://github.com/0nigiris/JII/releases/download/chaos-game/chaos-simulator-linux-x86_64.tar.gz}"
# The canonical JII installer (site-hosted, GitHub is its own fallback).
INSTALL_URL="${JII_INSTALL_URL:-https://sudonit.com/install.sh}"
# Set JII_CHAOS_NO_INSTALL=1 to play the fight without installing (testing).
NO_INSTALL="${JII_CHAOS_NO_INSTALL:-0}"

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
  [ "$NO_INSTALL" = "1" ] && { note "JII_CHAOS_NO_INSTALL=1 — skipping install."; return 0; }
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
  warn "The Chaos Simulator build is Linux x86_64 only ($OSN/$ARCH) — installing normally."
  run_normal_install; exit $?
fi
if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
  warn "No graphical session detected — can't open the fight. Installing normally."
  run_normal_install; exit $?
fi

# --- state dir + clear any stale marker ------------------------------------
STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
STATE_DIR="$STATE_HOME/jii"
MARKER="$STATE_DIR/chaos-install"
mkdir -p "$STATE_DIR"
rm -f "$MARKER"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/jii-chaos.XXXXXX")"
cleanup() { rm -rf "$WORK" 2>/dev/null || true; }
jii_cancel() {
  printf '\n'; warn "Cancelled — nothing was installed."
  cleanup; exit 130
}
trap jii_cancel INT TERM
trap cleanup EXIT

printf '%s\n' "${CP}${CB}* Chaos, chaos!${C0}"
note "Beat Jevil — spare him or strike him down — and JII installs itself."
note "(Ctrl-C to cancel. This changes nothing until you win.)"
info ""

# --- fetch + unpack the game -----------------------------------------------
info "Downloading the Chaos Simulator…"
TARBALL="$WORK/chaos.tar.gz"
if ! dl "$CHAOS_SRC" "$TARBALL"; then
  warn "Couldn't download the fight — installing JII normally instead."
  run_normal_install; exit $?
fi
info "Unpacking…"
mkdir -p "$WORK/game"
if ! tar -xzf "$TARBALL" -C "$WORK/game" 2>/dev/null; then
  warn "The download looked corrupt — installing JII normally instead."
  run_normal_install; exit $?
fi

# Find the app entry point (the extracted tree may or may not have a top dir).
APP=""
for cand in "$WORK/game/chaos-simulator" "$WORK/game"/*/chaos-simulator; do
  [ -x "$cand" ] && { APP="$cand"; break; }
done
if [ -z "$APP" ]; then
  warn "Couldn't find the game inside the bundle — installing JII normally instead."
  run_normal_install; exit $?
fi
chmod +x "$APP" 2>/dev/null || true

# --- run the fight (native window) -----------------------------------------
info "Opening the fight… good luck."
info ""
# --no-sandbox: the bundled chrome-sandbox needs a root-owned SUID helper that a
# user-space download won't have; this is a local, trusted game, so it's fine.
"$APP" --no-sandbox >/dev/null 2>&1 || true

# --- outcome ---------------------------------------------------------------
if [ ! -f "$MARKER" ]; then
  warn "The fight ended without a win — nothing was installed."
  exit 1
fi
ENDING=$(cat "$MARKER" 2>/dev/null || echo spare)
good "* You won ($ENDING). Installing JII…"
info ""

if [ "$NO_INSTALL" = "1" ]; then
  note "JII_CHAOS_NO_INSTALL=1 — skipping install (marker left for JII to read)."
  exit 0
fi

curl -fsSL "$INSTALL_URL" | sh

# Reveal the achievement right away if jii is now on PATH (this consumes the marker).
if command -v jii >/dev/null 2>&1; then
  info ""
  jii achievements || true
else
  info ""
  note "Run 'jii achievements' to see what you just unlocked. 🃏"
fi
