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

use super::{Provider, command_plan, get_json_opt, run_capture, which};
use crate::error::Result;
use crate::model::{
    InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
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
        let url = format!("{API}/crates/{}", query.raw.trim());
        let Some(body) = get_json_opt::<CrateResponse>(ID, &url).await? else {
            return Ok(Vec::new()); // no such crate
        };
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
        Ok(cargo_plan(&candidate.name, "install", reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // `cargo install a b c` builds them all into ~/.cargo/bin in one unprivileged run.
        let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
        let mut argv = vec![BIN.to_string(), "install".to_string()];
        argv.extend(names.iter().cloned());
        let reasons = vec![format!(
            "crates.io (Rust community registry): {}",
            names.join(", ")
        )];
        Ok(Some(command_plan(ID, &names.join(", "), argv, false, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![format!("Remove {} (installed via cargo)", record.name)];
        Ok(cargo_plan(&record.name, "uninstall", reasons))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // `cargo install` reinstalls the newest published version if one exists.
        let reasons = vec![format!("Update {} via cargo (reinstall newest)", record.name)];
        Ok(cargo_plan(&record.name, "install", reasons))
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

/// A single unprivileged `cargo <verb> <name>` plan (install/uninstall).
fn cargo_plan(name: &str, verb: &str, reasons: Vec<String>) -> InstallPlan {
    let argv = vec![BIN.to_string(), verb.to_string(), name.to_string()];
    command_plan(ID, name, argv, false, reasons)
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
        let plan = cargo_plan(&c.name, "install", vec![]);
        assert_eq!(plan.source_id, "cargo");
        assert!(!plan.needs_root());
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, needs_root } => {
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

    #[tokio::test]
    async fn batch_merges_into_one_unprivileged_cargo_command() {
        let mut a = candidate(&parse(BINARY_CRATE)).unwrap(); // ripgrep
        a.name = "ripgrep".into();
        let mut b = a.clone();
        b.name = "bat".into();
        let plan = Cargo::new()
            .plan_install_many(&[&a, &b])
            .await
            .unwrap()
            .expect("cargo batches");
        assert_eq!(plan.actions.len(), 1);
        assert!(!plan.needs_root());
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["cargo", "install", "ripgrep", "bat"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }
}
