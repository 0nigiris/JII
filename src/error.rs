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
    /// The current platform is not supported (MVP targets Fedora).
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),

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
}
