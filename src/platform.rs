//! Platform detection: distro, architecture, PATH, and session kind.
//!
//! `Platform` is a **pure host-facts value object** — it answers only *"what is this
//! machine?"* (distro, arch, tty, PATH, elevation mechanism) and carries **no policy**.
//! Whether JII can act here is not a distro question but a *source* question ("is any
//! provider usable?"), so it lives in the engine, not here (ADR-0029). The core never
//! branches on `distro`; providers self-gate on their backing binary.

use std::path::PathBuf;
use std::sync::OnceLock;

/// A Linux distribution family, parsed from `/etc/os-release`.
///
/// A detected host fact, not a support gate. No distro is privileged over another;
/// the durable `id`/`id_like` family predicate is introduced when a real consumer
/// needs it (T6 bootstrap), not speculatively (ADR-0029).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Distro {
    Fedora,
    /// Any other recognized `ID`.
    Other(String),
    Unknown,
}

/// How privilege elevation should be requested, based on the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationKind {
    /// Interactive terminal present — use `sudo`.
    Sudo,
    /// Graphical session / no controlling TTY — use `pkexec`.
    Pkexec,
}

/// Detected properties of the host, computed once and cached.
#[derive(Debug, Clone)]
pub struct Platform {
    /// Detected distro family. A host fact only — the core never branches on it; it is
    /// there for config-seeding / bootstrap (T6), which are its first real consumers.
    #[allow(dead_code)]
    pub distro: Distro,
    /// Target arch as reported by the compiler (e.g. "x86_64", "aarch64").
    /// Consumed by GitHub release-asset filtering.
    pub arch: &'static str,
    /// Whether stdin/stdout is an interactive terminal.
    pub is_tty: bool,
    /// Directories on `PATH`. Backs the user-space `~/.local/bin` check in `jii doctor`.
    pub path_dirs: Vec<PathBuf>,
}

impl Platform {
    /// Detect the current platform (cached for the process lifetime).
    pub fn detect() -> &'static Platform {
        static PLATFORM: OnceLock<Platform> = OnceLock::new();
        PLATFORM.get_or_init(|| Platform {
            distro: detect_distro(),
            arch: std::env::consts::ARCH,
            is_tty: detect_tty(),
            path_dirs: detect_path_dirs(),
        })
    }

    /// How to request elevation in the current session.
    pub fn elevation_kind(&self) -> ElevationKind {
        if self.is_tty {
            ElevationKind::Sudo
        } else {
            ElevationKind::Pkexec
        }
    }

    /// Whether a directory is on `PATH` (used to check `~/.local/bin` in `jii doctor`).
    pub fn is_on_path(&self, dir: &std::path::Path) -> bool {
        self.path_dirs.iter().any(|d| d == dir)
    }
}

fn detect_distro() -> Distro {
    let content = match std::fs::read_to_string("/etc/os-release") {
        Ok(c) => c,
        Err(_) => return Distro::Unknown,
    };
    parse_distro(&content)
}

/// Parse a distro family from the contents of `/etc/os-release`.
///
/// Split out so it can be unit-tested without touching the filesystem.
fn parse_distro(os_release: &str) -> Distro {
    let field = |key: &str| -> Option<String> {
        os_release.lines().find_map(|line| {
            let (k, v) = line.split_once('=')?;
            if k.trim() == key {
                Some(v.trim().trim_matches('"').to_string())
            } else {
                None
            }
        })
    };

    let id = field("ID").unwrap_or_default();
    let id_like = field("ID_LIKE").unwrap_or_default();

    if id == "fedora" || id_like.split_whitespace().any(|t| t == "fedora") {
        Distro::Fedora
    } else if id.is_empty() {
        Distro::Unknown
    } else {
        Distro::Other(id)
    }
}

fn detect_tty() -> bool {
    // Key interactivity off stdin: prompts read from it, so if it is not a real
    // terminal we must fall back to defaults instead of pretending to ask.
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
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
    fn parses_fedora_by_id() {
        let os = r#"NAME="Fedora Linux"
ID=fedora
VERSION_ID=44
"#;
        assert_eq!(parse_distro(os), Distro::Fedora);
    }

    #[test]
    fn parses_fedora_by_id_like() {
        let os = r#"ID=nobara
ID_LIKE="fedora"
"#;
        assert_eq!(parse_distro(os), Distro::Fedora);
    }

    #[test]
    fn parses_other_distro() {
        let os = "ID=ubuntu\nID_LIKE=debian\n";
        assert_eq!(parse_distro(os), Distro::Other("ubuntu".to_string()));
    }

    #[test]
    fn unknown_when_empty() {
        assert_eq!(parse_distro(""), Distro::Unknown);
    }
}
