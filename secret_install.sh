#!/bin/sh
# JII — the secret installer. 🦴
#
#   curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/secret/secret_install.sh | sh
#
# There is no button. You beat Sans, JII installs itself, and a hidden achievement unlocks.
# Everything degrades honestly: no browser / no python3 / no graphical session / a pipe → we
# quietly fall back to the normal installer (no fight, no achievement). Nothing here ever runs
# as root; the actual install is delegated to the canonical install.sh, which asks as usual.
#
# Env knobs (mostly for testing):
#   JII_SECRET_SRC        game bundle URL or local path (default: the secret branch's game.tar.gz)
#   JII_SECRET_NO_INSTALL=1  after a win, drop the achievement sentinel but DON'T install (dry test)
#   JII_INSTALL_URL       canonical installer URL (default: master's install.sh)
set -eu

REPO="0nigiris/JII"
GAME_SRC="${JII_SECRET_SRC:-https://raw.githubusercontent.com/$REPO/secret/game.tar.gz}"
INSTALL_URL="${JII_INSTALL_URL:-https://raw.githubusercontent.com/$REPO/master/install.sh}"

# --- Presentation (colour on a real terminal only; ASCII-safe) ---------------
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-}" != "dumb" ]; then
  _e=$(printf '\033'); C0="${_e}[0m"; CB="${_e}[1m"; CD="${_e}[2m"; CG="${_e}[32m"; CP="${_e}[38;5;99m"; CY="${_e}[33m"
else
  C0=""; CB=""; CD=""; CG=""; CP=""; CY=""
fi
say()  { printf '%s\n' "$1"; }
ok()   { printf '%s✓%s %s\n' "$CG" "$C0" "$1"; }
info() { printf '%s·%s %s\n' "$CD" "$C0" "$1"; }
warn() { printf '%s!%s %s\n' "$CY" "$C0" "$1"; }
err()  { printf 'jii-secret: %s\n' "$1" >&2; exit 1; }

banner() {
  printf '\n'
  printf '%s   *  *   %s  %sBad Time Simulator%s\n' "$CP" "$C0" "$CB" "$C0"
  printf '%s  ******  %s  Beat Sans and JII installs itself.\n' "$CP" "$C0"
  printf '%s ** ** ** %s  %sLose… and you'"'"'ll have a bad time.%s\n' "$CP" "$C0" "$CD" "$C0"
  printf '%s  ******  %s\n' "$CP" "$C0"
  printf '\n'
}

# --- Downloader --------------------------------------------------------------
if command -v curl >/dev/null 2>&1; then
  dl() { curl -fsSL --retry 3 --retry-delay 1 "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  dl() { wget -q --tries=3 --waitretry=1 -O "$2" "$1"; }
else
  dl() { return 1; }
fi

# The normal path, used whenever the fight can't run (and after a win). Never a dead end.
run_normal_install() {
  info "Installing JII the normal way…"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 --retry-delay 1 "$INSTALL_URL" | sh
  elif command -v wget >/dev/null 2>&1; then
    wget -q --tries=3 --waitretry=1 -O- "$INSTALL_URL" | sh
  else
    err "need curl or wget to install."
  fi
}

# Drop the sentinel JII picks up on its next run to grant the secret 'sans' achievement.
drop_achievement_sentinel() {
  _state="${XDG_STATE_HOME:-$HOME/.local/state}/jii"
  mkdir -p "$_state" 2>/dev/null || return 0
  : > "$_state/secret-install" 2>/dev/null || return 0
}

banner

# --- Preconditions for the fight; any miss → honest fallback -----------------
GRAPHICAL=0
if [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then GRAPHICAL=1; fi

PYOK=0
if command -v python3 >/dev/null 2>&1 \
  && python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 7) else 1)' >/dev/null 2>&1; then
  PYOK=1
fi

# Note: with `curl … | sh` stdin is the script pipe, not a terminal — that's the *normal* way
# to run this, so we must NOT require a TTY on stdin. We only need stdout to be a real terminal
# (so this isn't a `> log` / CI capture); the fight itself lives in the browser, and the fallback
# installer reads its own prompts from /dev/tty.
if [ ! -t 1 ]; then
  warn "Not an interactive terminal — skipping the fight."
  run_normal_install; exit 0
fi
if [ "$GRAPHICAL" -ne 1 ]; then
  warn "No graphical session detected — the Sans fight needs a browser."
  run_normal_install; exit 0
fi
if [ "$PYOK" -ne 1 ]; then
  warn "python3 (3.7+) not found — it's needed to host the fight locally."
  run_normal_install; exit 0
fi

# --- Fetch and unpack the game ----------------------------------------------
WORK=$(mktemp -d)
# Cleanup always. A Ctrl-C cancels outright — the user deliberately chose the secret path,
# so we stop rather than silently doing a plain install they didn't ask for.
jii_cancel() {
  kill "${SRV:-}" 2>/dev/null
  printf '\n'
  warn "Cancelled — nothing was installed."
  exit 130
}
trap 'kill "${SRV:-}" 2>/dev/null; rm -rf "$WORK"' EXIT
trap jii_cancel INT TERM
GAME="$WORK/game"
mkdir -p "$GAME"

info "Fetching the arena…"
case "$GAME_SRC" in
  http://* | https://*)
    dl "$GAME_SRC" "$WORK/game.tar.gz" || { warn "Could not download the game bundle."; run_normal_install; exit 0; }
    ;;
  *)
    [ -f "$GAME_SRC" ] || { warn "Local game bundle not found: $GAME_SRC"; run_normal_install; exit 0; }
    cp "$GAME_SRC" "$WORK/game.tar.gz"
    ;;
esac
tar -xzf "$WORK/game.tar.gz" -C "$GAME" 2>/dev/null || { warn "Could not unpack the game."; run_normal_install; exit 0; }
# The tarball holds the game files at its top level (or under a single dir); find index.html.
INDEX=$(find "$GAME" -maxdepth 2 -name index.html | head -n1)
[ -n "$INDEX" ] || { warn "Game bundle looks wrong (no index.html)."; run_normal_install; exit 0; }
ROOT=$(dirname "$INDEX")

# --- The local one-shot server ----------------------------------------------
TOKEN=$(head -c 18 /dev/urandom 2>/dev/null | od -An -tx1 2>/dev/null | tr -d ' \n')
[ -n "$TOKEN" ] || TOKEN="jii$$$(date +%s)"
PORTFILE="$WORK/port"
CLAIMED="$WORK/claimed"
SERVER="$WORK/server.py"

cat > "$SERVER" <<'PY'
import http.server, socketserver, os, sys, threading, time

portfile, token, root, claimed = sys.argv[1:5]

class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k):
        super().__init__(*a, directory=root, **k)
    def log_message(self, *a):
        pass
    def _serve_index(self):
        try:
            with open(os.path.join(root, "index.html"), "r", encoding="utf-8") as f:
                html = f.read()
        except OSError:
            self.send_error(500); return
        body = html.replace("__JII_CLAIM_TOKEN__", token).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)
    def do_GET(self):
        parts = self.path.split("?", 1)
        path, query = parts[0], (parts[1] if len(parts) > 1 else "")
        if path == "/claim":
            ok = ("token=" + token) in query
            self.send_response(200 if ok else 403)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", "2")
            self.end_headers()
            try:
                self.wfile.write(b"ok" if ok else b"no")
                self.wfile.flush()
            except OSError:
                pass
            if ok:
                try:
                    open(claimed, "w").close()
                except OSError:
                    pass
                # Stop the one-shot server reliably. serve_forever()/shutdown() from a request
                # thread proved flaky here, so exit the whole process after a beat that lets the
                # response reach the browser (which has already shown "You win").
                def _bye():
                    time.sleep(0.3)
                    os._exit(0)
                threading.Thread(target=_bye, daemon=True).start()
            return
        if path in ("/", "/index.html"):
            self._serve_index(); return
        return super().do_GET()

class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True

httpd = Server(("127.0.0.1", 0), Handler)
with open(portfile, "w") as f:
    f.write(str(httpd.server_address[1]))
try:
    httpd.serve_forever()
except KeyboardInterrupt:
    pass
PY

python3 "$SERVER" "$PORTFILE" "$TOKEN" "$ROOT" "$CLAIMED" &
SRV=$!

# Wait briefly for the server to report its port.
PORT=""
i=0
while [ "$i" -lt 50 ]; do
  if [ -s "$PORTFILE" ]; then PORT=$(cat "$PORTFILE"); break; fi
  kill -0 "$SRV" 2>/dev/null || break
  sleep 0.1 2>/dev/null || sleep 1
  i=$((i + 1))
done
[ -n "$PORT" ] || { warn "Could not start the local server."; run_normal_install; exit 0; }

URL="http://127.0.0.1:$PORT/"
ok "Arena is live at $URL"
if command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$URL" >/dev/null 2>&1 || true
elif command -v gio >/dev/null 2>&1; then
  gio open "$URL" >/dev/null 2>&1 || true
else
  warn "Couldn't open a browser automatically — open this yourself:"
  say "    $URL"
fi

printf '\n%s* It'"'"'s a beautiful day outside. Beat Sans to install JII.%s\n' "$CB" "$C0"
info "Waiting for your victory…  (Ctrl-C to cancel)"

# Block until the server shuts down on a valid /claim. A Ctrl-C is handled by jii_cancel
# (which exits); reaching past this line means the server ended on its own.
wait "$SRV" 2>/dev/null || true

printf '\n'
if [ -f "$CLAIMED" ]; then
  ok "* You win."
  drop_achievement_sentinel
  if [ "${JII_SECRET_NO_INSTALL:-0}" = "1" ]; then
    info "(dry test: achievement sentinel dropped, skipping the real install)"
  else
    run_normal_install
  fi
else
  # Server ended without a win and without a Ctrl-C (unexpected). Don't spring a surprise
  # install on the user — just stop.
  warn "The fight ended without a win — nothing was installed."
  exit 1
fi
