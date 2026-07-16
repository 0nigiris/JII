# Testing JII (for testers)

Thank you for testing! This guide is for people helping to verify JII on different
distributions. **Run everything in a VM or a disposable system** — the checklist performs
real installs and removals.

## The one command

```sh
jii yes-I-am-dev-and-want-to-test
```

That's the whole job. It is deliberately hidden from `--help` and the README — it exists
only for testers.

What it does:

1. Walks ~12 scripted steps covering the core flows: doctor, search (including the
   junk-package heuristics), info, a **real install** of `htop`, list/how, update,
   a **real removal**, dead-end handling for a nonexistent package, version-pin
   rejection, and the sources view.
2. Before each step it tells you **what to expect**; after each step it asks
   `Did this look right? [Y/n/s]`. Answer honestly — `n` prompts for a one-line note.
   You're the semantic check: the tool can't know that "installing npm *via npm*" is
   absurd, but you can.
3. Everything — commands, full output, exit codes, your verdicts — is duplicated into
   `jii-test-YYYYMMDD-HHMMSS.log` in the current directory. Your username and hostname
   are scrubbed from the log automatically.
4. At the end it offers to upload the log to a public paste service (0x0.st, with a
   fallback) in one keypress. The local file always stays.
5. It prints a **pre-filled GitHub issue link** carrying your distro/arch, the jii
   version, the log URL, and the per-step PASS/FAIL table. Reporting a broken run is
   one click.

The command exits non-zero if any step was judged FAIL, so scripted runs can detect a
bad round too.

## Recommended setup

- A fresh VM of the distro you're testing (Fedora, Ubuntu/Debian, Arch, openSUSE, Void…).
- Install JII the way a user would:
  `curl -fsSL https://raw.githubusercontent.com/0nigiris/JII/master/install.sh | sh`
- A user account with sudo (the install steps escalate through your system manager and
  will show the exact command first).
- Internet access (searches hit the live registries).

## What to look for beyond the checklist

- **Anything that looks hung.** A quiet terminal must never look frozen — there should
  always be a spinner or a line of progress.
- **Dead ends.** Every failure should tell you what to do next (a link, a command, a
  hint) — never a bare error.
- **Nonsense recommendations.** A package that merely shares a name with a well-known
  tool should be red/`untrusted` with a warning, not the top pick.
- **Language mixing.** With a Russian locale (`jii lang ru`) everything should be
  Russian; with English — English.

If you spot any of these outside the scripted steps, say `n` on the nearest step and
describe it, or open an issue directly — the pre-filled link at the end is the fastest
path.
