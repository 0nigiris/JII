# JII — the `chaos` branch 🃏

> *"Chaos, chaos!"*

This orphan branch exists for exactly one thing: the **secret Jevil installers**. There are
two of them, they are two different games, and they unlock the same 🃏 achievement.

**The Chaos Simulator** — the big one, a full Jevil battle in a native window:

```sh
curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/chaos/chaos_install.sh | sh
```

**Jevil-VGB** — the same fight shrunk into a retro handheld, 43 MB, one file:

```sh
curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/chaos/vgb_install.sh | sh
```

Either one downloads the game, opens it as a real desktop window, and installs JII **only
once you win** — whether you spare Jevil (put him to sleep) or strike him down. JII then
unlocks the secret 🃏 achievement, remembering which ending you got.

Nothing runs as root. Ctrl-C changes nothing. If the fight can't run (no terminal, no
graphical session, not Linux x86_64, download failed) the script falls back to a plain,
fight-free install — it never dead-ends.

For the sibling fights: 🦴 Sans lives on [`secret`](../../tree/secret), 🎭 Spamton NEO on
[`spamton`](../../tree/spamton). The real project lives on [`master`](../../tree/master).

## Contents

| File | What it is |
| --- | --- |
| `chaos_install.sh` | Chaos Simulator installer. Downloads, runs, installs on a win. |
| `vgb_install.sh` | Jevil-VGB installer. Same idea, one small binary. |
| `patches/jii_marker.gd` | The 24-line Godot helper that writes the win marker. |
| `patches/jevil-vgb.patch` | Every change JII makes to Jevil-VGB, against upstream. |

The games themselves are **not** in git — they ship as assets on the `chaos-game` release
(`chaos-simulator-linux-x86_64.tar.gz`, `jevil-vgb.x86_64`).

## Credits & rights

**Jevil, Deltarune and Undertale are © Toby Fox.** Both games here are third-party fan
projects, redistributed only so the installers have something to launch. JII claims no
ownership of any of it and makes no money from it.

- **Chaos Simulator** — a Scratch/TurboWarp game packaged with the TurboWarp Packager;
  no licence is stated. JII's only additions are a victory detector
  (`resources/app/jii-detector.js`) and a one-way Electron bridge that writes a marker file
  when the fight ends. No game logic is altered.
- **Jevil-VGB** — by [CherrySodaPop](https://github.com/CherrySodaPop/Jevil-VGB), **GPL-3.0**.
  JII adds `jii_marker.gd` and two one-line calls at the win branches the game already has,
  then builds it with Godot 3.6. The complete modified source ships as
  `jevil-vgb-source.tar.gz` on the `chaos-game` release, as GPL-3.0 requires; the patch is
  also in `patches/` here.

If you are a rights holder and want this taken down, open an issue — it will be removed.
