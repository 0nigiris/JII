//! Cargo provider (crates.io — Rust's package registry).
//!
//! `cargo install <crate>` builds a crate's executables from source into
//! `~/.cargo/bin` — a **user-space** install, no root (like a future npm/pipx/go).
//! Search resolves an exact crate name via the crates.io API and, crucially, only
//! offers crates that actually ship a binary: a library-only crate (e.g. `serde`) is
//! not an installable *program*, so it yields no candidate.
//!
//! All network access is in `search` (like github/copr); `plan_install` is pure. Trust
//! is `community`: crates.io is a community registry, and cargo verifies each crate's
//! checksum against the index itself, so the plan is a single unprivileged `cargo`
//! command with no separate download/verify step.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{Provider, run_capture, which};
use crate::error::{JiiError, Result};
use crate::model::{
    Action, InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
};

const ID: &str = "cargo";
const BIN: &str = "cargo";
const API: &str = "https://crates.io/api/v1";

/// The Cargo (crates.io) installation source.
pub struct Cargo;

impl Cargo {
    pub fn new() -> Self {
        Cargo
    }

    fn client(&self) -> Result<reqwest::Client> {
        // crates.io asks clients to send a User-Agent that identifies the app.
        reqwest::Client::builder()
            .user_agent(concat!("jii/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| JiiError::Other(anyhow::anyhow!("http client: {e}")))
    }
}

impl Default for Cargo {
    fn default() -> Self {
        Cargo::new()
    }
}

#[async_trait]
impl Provider for Cargo {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // crates.io is a community registry (not distro-official).
        TrustLevel::Community
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        let client = self.client()?;
        let url = format!("{API}/crates/{}", query.raw.trim());
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| JiiError::Other(anyhow::anyhow!("cargo: {e}")))?;
        // A 404 means "no such crate" — not an error, just nothing to offer.
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        let resp = resp
            .error_for_status()
            .map_err(|e| JiiError::Other(anyhow::anyhow!("cargo: {e}")))?;
        let body: CrateResponse = resp
            .json()
            .await
            .map_err(|e| JiiError::Other(anyhow::anyhow!("cargo: malformed json: {e}")))?;

        Ok(candidate(&body).into_iter().collect())
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let mut reasons = vec!["crates.io (Rust community registry)".to_string()];
        if let Some(v) = &candidate.version {
            reasons.push(format!("Version {v}"));
        }
        reasons.push(format!(
            "Builds {} into ~/.cargo/bin (no root; ensure it is on PATH)",
            candidate.name
        ));
        Ok(user_plan(&candidate.name, &["install", &candidate.name], reasons))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![format!("Remove {} (installed via cargo)", record.name)];
        Ok(user_plan(&record.name, &["uninstall", &record.name], reasons))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // `cargo install` reinstalls the newest published version if one exists.
        let reasons = vec![format!("Update {} via cargo (reinstall newest)", record.name)];
        Ok(user_plan(&record.name, &["install", &record.name], reasons))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        let out = run_capture(&[BIN, "install", "--list"]).await?;
        Ok(parse_installed_list(&out, ID))
    }
}

/// crates.io `crates/{name}` response (only the fields we use).
#[derive(Debug, Deserialize)]
struct CrateResponse {
    #[serde(rename = "crate")]
    krate: CrateInfo,
    /// Versions, newest first. The newest tells us whether the crate ships a binary.
    #[serde(default)]
    versions: Vec<VersionInfo>,
}

#[derive(Debug, Deserialize)]
struct CrateInfo {
    name: String,
    #[serde(default)]
    max_stable_version: Option<String>,
    #[serde(default)]
    max_version: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VersionInfo {
    /// Names of the binaries this version installs (empty for a library-only crate).
    #[serde(default)]
    bin_names: Vec<String>,
}

/// Build a candidate from a crate lookup, or `None` if the crate ships no executable
/// (a library is not something `cargo install` — or JII — can install as a program).
fn candidate(resp: &CrateResponse) -> Option<PackageCandidate> {
    let has_binary = resp
        .versions
        .first()
        .is_some_and(|v| v.bin_names.iter().any(|b| !b.is_empty()));
    if !has_binary {
        return None;
    }
    let version = resp
        .krate
        .max_stable_version
        .clone()
        .or_else(|| resp.krate.max_version.clone())
        .filter(|s| !s.is_empty());
    Some(PackageCandidate {
        name: resp.krate.name.clone(),
        source_id: ID.to_string(),
        version: version.map(PkgVersion::new),
        trust: TrustLevel::Community,
        // Cargo builds from source for the host, so arch is always compatible; the
        // registry checksum is verified by cargo itself during install.
        arch_ok: true,
        signed: true,
        summary: resp.krate.description.clone().filter(|d| !d.is_empty()),
        raw: json!({}),
    })
}

/// Assemble a single unprivileged `cargo <args>` plan. Shared by install/remove/update.
fn user_plan(name: &str, args: &[&str], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![BIN.to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    InstallPlan {
        candidate_ref: name.to_string(),
        source_id: ID.to_string(),
        actions: vec![Action::RunCommand {
            argv,
            needs_root: false,
        }],
        download_size: None,
        reasons,
    }
}

/// Parse `cargo install --list` into installed records. A crate header is at column 0
/// (`name vX.Y.Z:`, possibly with a ` (path/url)` suffix); the binaries under it are
/// indented and skipped.
fn parse_installed_list(stdout: &str, source_id: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    stdout
        .lines()
        // Indented lines list a crate's binaries; only column-0 lines are headers.
        .filter(|l| !l.is_empty() && !l.starts_with(char::is_whitespace))
        .filter_map(|line| {
            let header = line.trim_end().strip_suffix(':')?;
            let (name, rest) = header.split_once(' ')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            // rest is like "v15.1.0" or "v0.1.0 (/some/path)"; take the version token.
            let ver = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_start_matches('v');
            Some(InstalledRecord {
                name: name.to_string(),
                source_id: source_id.to_string(),
                version: (!ver.is_empty()).then(|| PkgVersion::new(ver)),
                installed_at: now,
                // cargo verifies the crate checksum itself — a self-verifying manager.
                verification: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BINARY_CRATE: &str = r#"{
        "crate": {
            "name": "ripgrep",
            "max_stable_version": "15.1.0",
            "max_version": "15.1.0",
            "description": "recursively search directories for a regex pattern"
        },
        "versions": [{"bin_names": ["rg"]}]
    }"#;

    const LIBRARY_CRATE: &str = r#"{
        "crate": {"name": "serde", "max_stable_version": "1.0.0", "description": "serde"},
        "versions": [{"bin_names": []}]
    }"#;

    fn parse(sample: &str) -> CrateResponse {
        serde_json::from_str(sample).unwrap()
    }

    #[test]
    fn binary_crate_becomes_a_community_candidate() {
        let c = candidate(&parse(BINARY_CRATE)).unwrap();
        assert_eq!(c.name, "ripgrep");
        assert_eq!(c.source_id, "cargo");
        assert_eq!(c.version.as_ref().unwrap().0, "15.1.0");
        assert_eq!(c.trust, TrustLevel::Community);
        assert!(c.arch_ok && c.signed);
        assert_eq!(
            c.summary.as_deref(),
            Some("recursively search directories for a regex pattern")
        );
    }

    #[test]
    fn library_only_crate_yields_no_candidate() {
        // `cargo install serde` has nothing to install — JII must not offer it.
        assert!(candidate(&parse(LIBRARY_CRATE)).is_none());
    }

    #[test]
    fn install_plan_is_one_unprivileged_cargo_command() {
        let c = candidate(&parse(BINARY_CRATE)).unwrap();
        let plan = user_plan(&c.name, &["install", &c.name], vec![]);
        assert_eq!(plan.source_id, "cargo");
        assert!(!plan.needs_root());
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            Action::RunCommand { argv, needs_root } => {
                assert_eq!(argv, &["cargo", "install", "ripgrep"]);
                assert!(!needs_root);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[test]
    fn parses_install_list() {
        let sample = "\
ripgrep v15.1.0:
    rg
bat v0.24.0:
    bat
cargo-edit v0.13.6 (/home/u/src/cargo-edit):
    cargo-add
";
        let recs = parse_installed_list(sample, "cargo");
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].name, "ripgrep");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "15.1.0");
        assert_eq!(recs[0].source_id, "cargo");
        assert_eq!(recs[1].name, "bat");
        assert_eq!(recs[1].version.as_ref().unwrap().0, "0.24.0");
        // A path/git-installed crate keeps just the version token, not the source.
        assert_eq!(recs[2].name, "cargo-edit");
        assert_eq!(recs[2].version.as_ref().unwrap().0, "0.13.6");
    }

    #[test]
    fn install_list_skips_blank_and_binary_lines() {
        let recs = parse_installed_list("\n    orphan-bin\n", "cargo");
        assert!(recs.is_empty());
    }
}
