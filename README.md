# JII — the `flowey` branch 🌻

> *"You IDIOT."*

This orphan branch exists for exactly one thing: the **secret Omega Flowey installer**.

```sh
curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/flowey/flowey_install.sh | sh
```

It downloads **Omega Flowey Simulator** — Undertale's final neutral-route boss, rebuilt as a
Scratch project and packaged for the desktop — opens it as a real window, and installs JII
**only once you win**. Normal mode and hard mode both count; JII remembers which one, and
unlocks the secret 🌻 achievement for it.

Losing is the expected outcome, so losing is not a dead end: the script offers you a plain
install anyway. Nothing runs as root. Ctrl-C changes nothing. If the fight can't run (no
terminal, no graphical session, not Linux x86_64, no room on disk, download failed) it falls
back to a normal, fight-free install.

For the sibling fights: 🦴 Sans lives on [`secret`](../../tree/secret), 🃏 Jevil (twice) on
[`chaos`](../../tree/chaos), 🎭 Spamton NEO on [`spamton`](../../tree/spamton). The real
project lives on [`master`](../../tree/master).

## Contents

| File | What it is |
| --- | --- |
| `flowey_install.sh` | The installer. Downloads the game, runs it, installs on a win. |
| `patches/jii-marker.js` | The one file JII adds to the game: it writes the win marker. |
| `patches/omega-flowey.patch` | Every other change JII makes — four lines, against the original. |

The game itself is **not** in git — it ships as the `omega-flowey-linux-x86_64.tar.gz` asset on
the `flowey-game` release (~121 MB).

## How the win is detected

The fight already knows when it is over: the project sets the stage variable `flowey hp` to
9950 when the battle starts and broadcasts `flowey death` the moment it drops below 1.
`jii-marker.js` watches that same variable from Electron's main process — the page itself stays
sandboxed and unmodified — and writes `normal` or `hard` (the game's own menu toggle) into
`$XDG_STATE_HOME/jii/flowey-install`. JII picks the marker up on its next run.

It arms only after seeing a health bar above zero, so a game that was never started cannot look
like a win.

The only other change is that the packaged project's connection to TurboWarp's cloud-variable
servers is removed: this fight is offline, and an installer has no business phoning anywhere.

## Credits & rights

**Undertale, Flowey and Omega Flowey are © Toby Fox.** The game is a fan-made Scratch project
(*Omega Flowey Fight V1.2*), packaged for the desktop with the
[TurboWarp Packager](https://packager.turbowarp.org/). JII claims no ownership of any of it,
changes nothing about the fight itself, and makes no money from it — it is redistributed only
so the installer has something to launch.

If you are the author of the project (or any rights holder) and want this credited differently
or taken down, open an issue — it will be changed or removed.
