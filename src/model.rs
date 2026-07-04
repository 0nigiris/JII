//! Core data model shared across the whole pipeline: `search → rank → plan → execute`.
//!
//! These types are source-agnostic on purpose — the engine and UI operate only on
//! them, never on provider-specific details (see `docs/ARCHITECTURE.md` §11).

// The full model is defined up front per the agreed architecture; some fields and
// variants are consumed only by later phases (trust/audit, health, registry). Allow
// dead code for the model module while those phases land (see docs/ROADMAP.md).
#![allow(dead_code)]

use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A package version string as reported by a source.
///
/// Sources are heterogeneous — RPM uses EVR (`2.63.1-1.fc44`, with epoch/release),
/// GitHub uses tags (`v2.63.1`), Flatpak uses its own scheme — so we keep the raw
/// string for faithful display. Cross-source version comparison (the freshness
/// tie-breaker) is added in Phase 3 where it is actually needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PkgVersion(pub String);

impl PkgVersion {
    pub fn new(s: impl Into<String>) -> Self {
        PkgVersion(s.into())
    }
}

impl fmt::Display for PkgVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A user request, either a package name or a free-text description.
#[derive(Debug, Clone)]
pub struct Query {
    pub raw: String,
    pub kind: QueryKind,
}

impl Query {
    /// Build a name query (the only kind used in the MVP).
    pub fn name(raw: impl Into<String>) -> Self {
        Query {
            raw: raw.into(),
            kind: QueryKind::Name,
        }
    }
}

/// How a [`Query`] should be interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryKind {
    /// Match against package names (MVP).
    Name,
    /// Match against descriptions/metadata (future).
    Description,
}

/// Trust attached to a source or repository. Drives the confirmation barrier and
/// whether `--auto` may proceed silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    /// Official distro repos, verified Flathub, etc.
    Official,
    /// COPR, known third-party repos, crates.io, etc.
    Community,
    /// Arbitrary binaries / unknown URLs — always confirmed, even in `--auto`.
    Untrusted,
}

impl TrustLevel {
    /// Lowercase human label, e.g. for prompts and `why`.
    pub fn label(&self) -> &'static str {
        match self {
            TrustLevel::Official => "official",
            TrustLevel::Community => "community",
            TrustLevel::Untrusted => "untrusted",
        }
    }
}

/// Reported health of a source, used by `doctor` and as a ranking tie-breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Slow,
    Offline,
    RateLimited,
}

impl Health {
    /// Lowercase human label for display.
    pub fn label(&self) -> &'static str {
        match self {
            Health::Healthy => "healthy",
            Health::Slow => "slow",
            Health::Offline => "offline",
            Health::RateLimited => "rate-limited",
        }
    }
}

/// How a downloaded artifact is verified before it is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// Expected lowercase hex SHA-256 digest.
    Sha256(String),
    Gpg,
    Sigstore,
    /// No verification available (the source provides none).
    None,
}

impl Verification {
    /// Short human label for previews and audit output.
    pub fn label(&self) -> &'static str {
        match self {
            Verification::Sha256(_) => "sha256",
            Verification::Gpg => "gpg",
            Verification::Sigstore => "sigstore",
            Verification::None => "unverified",
        }
    }
}

/// A single installation candidate produced by a provider's `search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageCandidate {
    pub name: String,
    pub source_id: String,
    pub version: Option<PkgVersion>,
    pub trust: TrustLevel,
    /// Whether this candidate is compatible with the current arch/libc.
    pub arch_ok: bool,
    /// Whether the artifact carries a signature/checksum we can verify.
    pub signed: bool,
    pub summary: Option<String>,
    /// Source-specific payload, opaque to the core, consumed by `plan_install`.
    pub raw: serde_json::Value,
}

/// One action in a plan. Each variant has a single, clear responsibility; the plan
/// executor dispatches to a focused handler per variant. Providers describe actions
/// but never execute them.
#[derive(Debug, Clone)]
pub enum Action {
    /// Run an external command; `needs_root` requests privilege elevation.
    RunCommand { argv: Vec<String>, needs_root: bool },
    /// Download `url` to `dest`, enforcing `verify` before the file is used.
    Download {
        url: String,
        dest: PathBuf,
        verify: Verification,
    },
    /// Place a file at `dest` with the given unix `mode` (e.g. into ~/.local/bin).
    Place { src: PathBuf, dest: PathBuf, mode: u32 },
    /// Extract the binary named `member` from a (already-verified) archive to `dest`
    /// with the given unix `mode`. The handler locates the member inside the archive.
    Extract {
        archive: PathBuf,
        member: String,
        dest: PathBuf,
        mode: u32,
    },
    /// Remove a file (uninstall for file-based sources).
    RemoveFile { path: PathBuf },
}

/// A previewable, executable plan. `Plan` is a first-class concept: every action
/// builds one before touching the system, and `--dry-run` renders it.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    /// Human-readable reference to the candidate this plan installs.
    pub candidate_ref: String,
    pub source_id: String,
    pub actions: Vec<Action>,
    pub download_size: Option<u64>,
    /// Why this candidate/source was recommended — shown to the user.
    pub reasons: Vec<String>,
}

impl InstallPlan {
    /// True if any action requires root (drives preview and elevation).
    pub fn needs_root(&self) -> bool {
        self.actions
            .iter()
            .any(|a| matches!(a, Action::RunCommand { needs_root: true, .. }))
    }
}

/// A record of software JII installed, persisted in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledRecord {
    pub name: String,
    pub source_id: String,
    pub version: Option<PkgVersion>,
    pub installed_at: DateTime<Utc>,
}
