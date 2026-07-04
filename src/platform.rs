//! Platform detection: distro, architecture, PATH, and session kind.
//!
//! Everything distro-specific is funneled through here so the rest of the code can
//! stay platform-agnostic. The MVP supports Fedora; other distros are detected but
//! not yet supported.

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::error::{JiiError, Result};

/// A Linux distribution family, parsed from `/etc/os-release`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Distro {
    Fedora,
    /// Any recognized `ID`/`ID_LIKE` we do not support yet.
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
    pub distro: Distro,
    /// Target arch as reported by the compiler (e.g. "x86_64", "aarch64").
    /// Consumed by GitHub release-asset filtering.
    pub arch: &'static str,
    /// Whether stdin/stdout is an interactive terminal.
    pub is_tty: bool,
    /// Directories on `PATH`. Used by the user-space PATH check in Phase 5.
    #[allow(dead_code)]
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

    /// True when JII can operate on this platform (MVP: Fedora only).
    pub fn is_supported(&self) -> bool {
        matches!(self.distro, Distro::Fedora)
    }

    /// Return the platform, or an error if it is unsupported.
    pub fn require_supported(&self) -> Result<()> {
        if self.is_supported() {
            Ok(())
        } else {
            Err(JiiError::UnsupportedPlatform(format!(
                "{:?} (MVP targets Fedora)",
                self.distro
            )))
        }
    }

    /// How to request elevation in the current session.
    pub fn elevation_kind(&self) -> ElevationKind {
        if self.is_tty {
            ElevationKind::Sudo
        } else {
            ElevationKind::Pkexec
        }
    }

    /// Whether a directory is on `PATH` (used to warn about `~/.local/bin` in Phase 5).
    #[allow(dead_code)]
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
