# JII — the `chaos` branch 🃏

> *"Chaos, chaos!"*

This orphan branch exists for exactly one thing: the **secret Jevil installer**.

```sh
curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/chaos/chaos_install.sh | sh
```

It downloads a copy of the **Chaos Simulator** (a Jevil battle), opens it as a real
desktop window, and installs JII **only once you win** — whether you spare Jevil or
strike him down. JII then unlocks the secret 🃏 achievement, remembering which ending
you got.

Nothing runs as root. Ctrl-C changes nothing. If the fight can't run (no terminal, no
graphical session, not Linux x86_64, download failed) the script falls back to a plain,
fight-free install — it never dead-ends.

For the sibling 🦴 Sans fight, see the [`secret`](../../tree/secret) branch. The real
project lives on [`master`](../../tree/master).

## Contents

| File | What it is |
| --- | --- |
| `chaos_install.sh` | The installer. Downloads the game, runs it, installs on a win. |

The game bundle itself is **not** in git — it is ~98 MB packed and ships as the
`chaos-simulator-linux-x86_64.tar.gz` asset on the `chaos-game` release.

## Credits & rights

The **Chaos Simulator** is a third-party fan project (a Scratch/TurboWarp game packaged
with the TurboWarp Packager); it is redistributed here only so the installer has
something to launch. **Jevil, Deltarune and Undertale are © Toby Fox.** JII claims no
ownership of any of it and makes no money from it.

The only JII-authored changes inside the bundle are a small victory detector
(`resources/app/jii-detector.js`) and a one-way Electron bridge that writes a marker
file when the fight ends. No game logic is altered.

If you are a rights holder and want this taken down, open an issue — it will be removed.
