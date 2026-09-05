//! Platform detection: distro, architecture, PATH, and session kind.
//!
//! `Platform` is a **pure host-facts value object** — it answers only *"what is this
//! machine?"* (distro, arch, tty, PATH, elevation mechanism) and carries **no policy**.
//! Whether JII can act here is not a distro question but a *source* question ("is any
//! provider usable?"), so it lives in the engine, not here (ADR-0029). The core never
//! branches on `distro`; providers self-gate on their backing binary.

use std::path::PathBuf;
use std::sync::OnceLock;

/// How privilege elevation should be requested on this host.
///
/// Two facts decide it, in this order: **are we already root** (a container, a rescue
/// shell, `su -`), and **which helper actually exists here**. Assuming `sudo` cost a
/// tester every install on a root-only Arch container — JII ran `sudo pacman …` and
/// died with "failed to run sudo: No such file or directory". Being root is the easy
/// case, not an error; and a machine with no helper at all must say so in words, not
/// as a spawn failure (ADR-0085).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationKind {
    /// Already root — run the command directly, with no helper in front of it.
    AlreadyRoot,
    /// Interactive terminal present — use `sudo`.
    Sudo,
    /// `sudo` is absent but OpenBSD-style `doas` is here (Void, Alpine, Artix…).
    Doas,
    /// Graphical session / no controlling TTY — use `pkexec`.
    Pkexec,
    /// Root is needed and nothing on this machine can grant it.
    Missing,
}

impl ElevationKind {
    /// The helper binary this kind runs through, if any.
    pub fn helper(&self) -> Option<&'static str> {
        match self {
            ElevationKind::Sudo => Some("sudo"),
            ElevationKind::Doas => Some("doas"),
            ElevationKind::Pkexec => Some("pkexec"),
            ElevationKind::AlreadyRoot | ElevationKind::Missing => None,
        }
    }
}

/// Detected properties of the host, computed once and cached.
#[derive(Debug, Clone)]
pub struct Platform {
    /// Whether this is an Arch-family host (Arch, Manjaro, EndeavourOS, CachyOS, Artix…),
    /// by `ID`/`ID_LIKE`. A host fact, read by the AUR source/ecosystem so `jii yay`/`jii paru`
    /// and AUR search are offered **only** where they apply — never on Fedora/Debian/etc.
    pub arch_like: bool,
    /// Target arch as reported by the compiler (e.g. "x86_64", "aarch64").
    /// Consumed by GitHub release-asset filtering.
    pub arch: &'static str,
    /// Whether stdin/stdout is an interactive terminal.
    pub is_tty: bool,
    /// Whether the terminal can render non-ASCII glyphs (✓, ✗, ⚠…). The Linux text
    /// console (`TERM=linux`) and non-UTF-8 locales draw those as tofu boxes, so the
    /// UI falls back to ASCII markers there. A host fact only — set once, read by the UI.
    pub unicode: bool,
    /// Directories on `PATH`. Backs the user-space `~/.local/bin` check in `jii doctor`.
    pub path_dirs: Vec<PathBuf>,
    /// This host's distro *family*: the `ID` followed by every `ID_LIKE` token, most
    /// specific first. A host fact only — the core never branches on it. Read by the
    /// recommend-catalog, which used to match on `ID` alone: Linux Mint saw none of
    /// Debian's suggestions and Nobara none of Fedora's, though each names its parent
    /// right there in `/etc/os-release` (ADR-0029 stands — entries declare their distros,
    /// this only widens what counts as a match).
    pub distro_ids: Vec<String>,
    /// The process's effective user id. `0` means we are already root, and every
    /// elevation helper is then not just unnecessary but wrong to require.
    pub euid: u32,
}

impl Platform {
    /// Detect the current platform (cached for the process lifetime).
    pub fn detect() -> &'static Platform {
        static PLATFORM: OnceLock<Platform> = OnceLock::new();
        PLATFORM.get_or_init(|| {
            let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
            Platform {
                arch_like: parse_arch_like(&os_release),
                arch: std::env::consts::ARCH,
                is_tty: detect_tty(),
                unicode: detect_unicode(),
                path_dirs: detect_path_dirs(),
                distro_ids: parse_distro_ids(&os_release),
                euid: detect_euid(),
            }
        })
    }

    /// How to request elevation in the current session.
    pub fn elevation_kind(&self) -> ElevationKind {
        choose_elevation(self.euid, self.is_tty, |bin| {
            self.path_dirs.iter().any(|d| d.join(bin).is_file())
        })
    }

    /// Whether a directory is on `PATH` (used to check `~/.local/bin` in `jii doctor`).
    pub fn is_on_path(&self, dir: &std::path::Path) -> bool {
        self.path_dirs.iter().any(|d| d == dir)
    }
}

/// `ID` first, then each `ID_LIKE` token — the host's distro family, most specific first.
///
/// Split out so it can be unit-tested without touching the filesystem. Values are
/// lowercased and de-duplicated (a few distros repeat their own id in `ID_LIKE`).
fn parse_distro_ids(os_release: &str) -> Vec<String> {
    let field = |key: &str| -> Option<String> {
        os_release.lines().find_map(|line| {
            let (k, v) = line.split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_ascii_lowercase())
        })
    };
    let mut out = Vec::new();
    if let Some(id) = field("ID").filter(|s| !s.is_empty()) {
        out.push(id);
    }
    for token in field("ID_LIKE").unwrap_or_default().split_whitespace() {
        if !out.iter().any(|seen| seen == token) {
            out.push(token.to_string());
        }
    }
    out
}

/// Whether `/etc/os-release` describes an Arch-family host. Arch itself sets `ID=arch` (and
/// no `ID_LIKE`); derivatives (Manjaro, EndeavourOS, CachyOS, Artix, Garuda…) set
/// `ID_LIKE=arch` (sometimes among others), so an `arch` token anywhere in the family is the
/// reliable, derivative-proof signal.
fn parse_arch_like(os_release: &str) -> bool {
    parse_distro_ids(os_release).iter().any(|id| id == "arch")
}

/// Pick the elevation mechanism from three facts: our own uid, whether a terminal is
/// there to type a password into, and which helpers exist. Pure, so the whole matrix is
/// unit-tested without a container per case.
///
/// Order matters. Root first: a root shell needs no helper, and demanding one there is
/// the bug this function was written for. Then, on a TTY, `sudo` → `doas` → `pkexec`
/// (ask in the terminal the user is already looking at); with no TTY, `pkexec` first
/// (it can raise its own graphical prompt) and the terminal helpers after.
fn choose_elevation(euid: u32, is_tty: bool, has: impl Fn(&str) -> bool) -> ElevationKind {
    if euid == 0 {
        return ElevationKind::AlreadyRoot;
    }
    let order: &[(&str, ElevationKind)] = if is_tty {
        &[
            ("sudo", ElevationKind::Sudo),
            ("doas", ElevationKind::Doas),
            ("pkexec", ElevationKind::Pkexec),
        ]
    } else {
        &[
            ("pkexec", ElevationKind::Pkexec),
            ("sudo", ElevationKind::Sudo),
            ("doas", ElevationKind::Doas),
        ]
    };
    order
        .iter()
        .find(|(bin, _)| has(bin))
        .map(|(_, kind)| *kind)
        .unwrap_or(ElevationKind::Missing)
}

/// Our effective uid, read from `/proc/self/status` — Linux-only, like JII, and it
/// avoids pulling `libc` in for one number. The `Uid:` line is
/// `real<TAB>effective<TAB>saved<TAB>fs`; the *effective* one is what the kernel
/// checks. Unreadable `/proc` degrades to "not root", the safe answer: JII then asks
/// for elevation it may not need, rather than skipping it when it does.
fn detect_euid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| parse_euid(&s))
        .unwrap_or(1)
}

fn parse_euid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn detect_tty() -> bool {
    // Key interactivity off stdin: prompts read from it, so if it is not a real
    // terminal we must fall back to defaults instead of pretending to ask.
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// Whether the terminal can be trusted to render glyphs like ✓/✗/⚠. Two things break
/// them: a non-UTF-8 locale (the bytes aren't even encodable), and the Linux text
/// console `TERM=linux`, whose built-in font has no glyph for them even under UTF-8 —
/// which is exactly what showed up as `▪` boxes on a Void live console. Conservative:
/// require a UTF-8 locale AND not a known glyph-poor console.
fn detect_unicode() -> bool {
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let utf8 = locale.contains("utf-8") || locale.contains("utf8");
    let poor_console = std::env::var("TERM")
        .map(|t| t == "linux" || t == "dumb")
        .unwrap_or(false);
    utf8 && !poor_console
}

fn detect_path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_derivative_carries_its_parent_in_the_family() {
        assert_eq!(parse_distro_ids("ID=fedora\n"), vec!["fedora"]);
        assert_eq!(
            parse_distro_ids("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"),
            vec!["linuxmint", "ubuntu", "debian"]
        );
        // A derivative that repeats its own id in ID_LIKE must not list it twice.
        assert_eq!(parse_distro_ids("ID=arch\nID_LIKE=arch\n"), vec!["arch"]);
        assert!(parse_distro_ids("").is_empty());
    }

    #[test]
    fn arch_like_by_id() {
        assert!(parse_arch_like("ID=arch\n"));
    }

    #[test]
    fn arch_like_derivatives_by_id_like() {
        // Manjaro/EndeavourOS/CachyOS all set ID_LIKE=arch (sometimes among other tokens).
        assert!(parse_arch_like("ID=manjaro\nID_LIKE=arch\n"));
        assert!(parse_arch_like("ID=endeavouros\nID_LIKE=\"arch\"\n"));
        assert!(parse_arch_like("ID=cachyos\nID_LIKE=arch\n"));
    }

    #[test]
    fn arch_like_false_elsewhere() {
        assert!(!parse_arch_like("ID=fedora\n"));
        assert!(!parse_arch_like("ID=ubuntu\nID_LIKE=debian\n"));
        assert!(!parse_arch_like(""));
    }

    /// The tester's Arch container: uid 0 and no `sudo` anywhere. JII used to ask for
    /// sudo regardless and die on the spawn.
    #[test]
    fn root_needs_no_helper_even_with_none_installed() {
        assert_eq!(choose_elevation(0, true, |_| false), ElevationKind::AlreadyRoot);
        assert_eq!(choose_elevation(0, false, |_| true), ElevationKind::AlreadyRoot);
    }

    #[test]
    fn a_terminal_prefers_sudo_then_doas_then_pkexec() {
        assert_eq!(choose_elevation(1000, true, |_| true), ElevationKind::Sudo);
        assert_eq!(choose_elevation(1000, true, |b| b != "sudo"), ElevationKind::Doas);
        assert_eq!(choose_elevation(1000, true, |b| b == "pkexec"), ElevationKind::Pkexec);
    }

    #[test]
    fn without_a_terminal_pkexec_leads_but_the_others_still_count() {
        assert_eq!(choose_elevation(1000, false, |_| true), ElevationKind::Pkexec);
        assert_eq!(choose_elevation(1000, false, |b| b == "sudo"), ElevationKind::Sudo);
    }

    #[test]
    fn a_user_with_no_helper_at_all_is_named_not_a_spawn_error() {
        assert_eq!(choose_elevation(1000, true, |_| false), ElevationKind::Missing);
        assert_eq!(choose_elevation(1000, false, |_| false), ElevationKind::Missing);
    }

    #[test]
    fn euid_is_the_second_field_of_the_uid_line() {
        let status = "Name:\tjii\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\n";
        assert_eq!(parse_euid(status), Some(1000));
        // A setuid process: real 1000, effective 0 — the effective one is what counts.
        assert_eq!(parse_euid("Uid:\t1000\t0\t0\t0\n"), Some(0));
        assert_eq!(parse_euid("Name:\tjii\n"), None);
    }
}
