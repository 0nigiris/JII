# JII — the `spamton` branch 🎭

> *"[BIG SHOT]"*

This orphan branch exists for exactly one thing: the **secret Spamton NEO installer**.

```sh
curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/spamton/spamton_install.sh | sh
```

It downloads **Spamton-NEO-VGB** — Deltarune's Spamton NEO fight recreated in the style of a
retro handheld console — opens it as a real desktop window, and installs JII **only once you
win**. Both endings count: blow him apart, or cut his strings and let him be a real boy. JII
then unlocks the secret 🎭 achievement, remembering which one you chose.

Nothing runs as root. Ctrl-C changes nothing. If the fight can't run (no terminal, no
graphical session, not Linux x86_64, download failed) the script falls back to a plain,
fight-free install — it never dead-ends.

For the sibling fights: 🦴 Sans lives on [`secret`](../../tree/secret), 🃏 Jevil (twice) on
[`chaos`](../../tree/chaos). The real project lives on [`master`](../../tree/master).

## Contents

| File | What it is |
| --- | --- |
| `spamton_install.sh` | The installer. Downloads the game, runs it, installs on a win. |
| `patches/jii_marker.gd` | The 24-line Godot helper that writes the win marker. |
| `patches/spamton-neo-vgb.patch` | Every change JII makes to the game, against upstream. |

The game itself is **not** in git — it ships as the `spamton-neo-vgb.x86_64` asset on the
`spamton-game` release.

## Credits & rights

**Spamton, Deltarune and Undertale are © Toby Fox.** The game is
[**Spamton-NEO-VGB**](https://github.com/CherrySodaPop/Spamton-NEO-VGB) by CherrySodaPop,
**GPL-3.0**, redistributed only so the installer has something to launch. JII claims no
ownership of any of it and makes no money from it.

JII's only changes are `jii_marker.gd` and two one-line calls at the win branches the fight
already has (`health <= 0` and `wireHealth <= 0`); it is then built with Godot 3.6. The
complete modified source ships as `spamton-neo-vgb-source.tar.gz` on the `spamton-game`
release, as GPL-3.0 requires; the patch is also in `patches/` here.

If you are a rights holder and want this taken down, open an issue — it will be removed.
