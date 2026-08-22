// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Error types for JII.
//!
//! The core uses a single [`JiiError`] enum so failures carry a clear, typed cause
//! and the UI layer can turn them into actionable messages.

use std::path::PathBuf;

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, JiiError>;

/// All errors JII can produce.
#[derive(Debug, thiserror::Error)]
pub enum JiiError {
    /// Failed to read or parse the configuration file.
    #[error("config error: {0}")]
    Config(String),

    /// A referenced source id is unknown (e.g. a typo in `priority`).
    #[error("unknown source: {0}")]
    UnknownSource(String),

    /// An I/O failure, annotated with the path when available.
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A plan step (an external command) exited non-zero. Carries the command and the output
    /// it produced so the caller can let the *source* explain it in human terms
    /// ([`crate::provider::Provider::explain_failure`]) before falling back to a raw tail.
    #[error("`{command}` failed")]
    StepFailed { command: String, output: String },

    /// The command failed and has **already told the user why**, in its own words and in
    /// their language. Carries no message of its own: it exists so a path that printed a
    /// red ✗ can still exit non-zero, instead of reporting success for a run that plainly
    /// did not succeed. `main::report` prints nothing for it.
    #[error("")]
    AlreadyReported,

    /// Any other error, wrapped for convenience.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl JiiError {
    /// Build an [`JiiError::Io`] with the offending path attached.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        JiiError::Io {
            path: path.into(),
            source,
        }
    }

    /// Error for an external command that could not be spawned.
    pub fn spawn(cmd: &str, source: std::io::Error) -> Self {
        JiiError::Other(anyhow::anyhow!("failed to run {cmd}: {source}"))
    }

    /// A concrete next step for the user, if this error has one (D7 — actionable errors).
    ///
    /// Pure (no I/O) so it is unit-testable: it maps *what happened* to *what to do next*,
    /// which the UI renders under the error line. Returns `None` when there is no honest,
    /// specific remedy — better to stay silent than invent one.
    pub fn remedy(&self) -> Option<String> {
        match self {
            JiiError::UnknownSource(id) => Some(crate::t!(
                "remedy.unknown_source",
                id = id.clone(),
                known = crate::config::KNOWN_SOURCES.join(", ")
            )),
            JiiError::Config(_) => Some(crate::t!("remedy.config")),
            JiiError::Io { path, source } => match source.kind() {
                std::io::ErrorKind::NotFound => {
                    Some(crate::t!("remedy.io_not_found", path = path.display().to_string()))
                }
                std::io::ErrorKind::PermissionDenied => {
                    Some(crate::t!("remedy.io_denied", path = path.display().to_string()))
                }
                _ => None,
            },
            // A failed step is already explained where it happens (the source's own note, or
            // a tail of its real output), so a generic remedy here would only add noise.
            JiiError::StepFailed { .. } => None,
            // Already explained by the command that raised it, remedy included.
            JiiError::AlreadyReported => None,
            // `Other` wraps opaque text from many call sites; string-sniffing it would be
            // fragile and often wrong, so we don't guess a remedy here.
            JiiError::Other(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_reported_carries_a_status_and_nothing_else() {
        // It exists so a command that printed its own red ✗ can still exit non-zero. If it ever
        // grew a message or a remedy, the user would see the same failure explained twice — the
        // exact thing `StepFailed` is suppressed for.
        let e = JiiError::AlreadyReported;
        assert_eq!(e.to_string(), "");
        assert!(e.remedy().is_none());
    }

    #[test]
    fn unknown_source_lists_the_known_ones() {
        let r = JiiError::UnknownSource("dfn".to_string()).remedy().unwrap();
        assert!(r.contains("dnf"), "should list real sources: {r}");
        assert!(r.contains("'dfn'"));
    }

    #[test]
    fn io_remedy_depends_on_kind() {
        let not_found = JiiError::io("/no/such", std::io::Error::from(std::io::ErrorKind::NotFound));
        assert!(not_found.remedy().unwrap().contains("does not exist"));

        let denied = JiiError::io(
            "/root/secret",
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        assert!(denied.remedy().unwrap().contains("Permission denied"));

        // An unremarkable I/O kind has no specific remedy.
        let other = JiiError::io("/x", std::io::Error::from(std::io::ErrorKind::Other));
        assert!(other.remedy().is_none());
    }

    #[test]
    fn opaque_errors_have_no_invented_remedy() {
        assert!(JiiError::Other(anyhow::anyhow!("boom")).remedy().is_none());
    }
}
