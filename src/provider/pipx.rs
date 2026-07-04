//! pipx provider (PyPI — Python applications in isolated venvs).
//!
//! Mirrors cargo/npm: search resolves an exact package via the PyPI JSON API and plans
//! `pipx install <pkg>` into `~/.local/bin` — user-space, no root.
//!
//! **Program-vs-library detection (ADR-0023):** unlike crates.io (`bin_names`) and npm
//! (`bin`), the PyPI JSON API exposes **no** reliable "does this install a CLI" signal —
//! there is no entry-points field, and the `Environment :: Console` classifier is ~40%
//! unreliable (poetry/twine/awscli are real pipx apps that omit it). So pipx does **not**
//! pre-filter: it offers the package and lets `pipx install` reject non-apps at execution
//! (a clear message, and the plan is fully previewable). A false positive (offering a
//! library that pipx then rejects) is safer than a false negative (silently hiding a real
//! app). Trust is `community`; pip/pipx verify PyPI hashes, so the plan is a single
//! unprivileged command with no separate download/verify step.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{Provider, command_plan, get_json_opt, which};
use crate::error::{JiiError, Result};
use crate::model::{
    InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
};

const ID: &str = "pipx";
const BIN: &str = "pipx";
const API: &str = "https://pypi.org/pypi";

/// The pipx (PyPI) installation source.
pub struct Pipx;

impl Pipx {
    pub fn new() -> Self {
        Pipx
    }
}

impl Default for Pipx {
    fn default() -> Self {
        Pipx::new()
    }
}

#[async_trait]
impl Provider for Pipx {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // PyPI is a community registry (not distro-official).
        TrustLevel::Community
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        let url = format!("{API}/{}/json", query.raw.trim());
        let Some(body) = get_json_opt::<PypiResponse>(ID, &url).await? else {
            return Ok(Vec::new()); // no such package
        };
        Ok(vec![candidate(&body)])
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let mut reasons = vec!["PyPI (Python application, via pipx)".to_string()];
        if let Some(v) = &candidate.version {
            reasons.push(format!("Version {v}"));
        }
        reasons.push(format!(
            "Installs {} into ~/.local/bin (no root; ensure it is on PATH)",
            candidate.name
        ));
        Ok(pipx_plan(&candidate.name, "install", reasons))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![format!("Remove {} (installed via pipx)", record.name)];
        Ok(pipx_plan(&record.name, "uninstall", reasons))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // pipx has a first-class upgrade (unlike cargo/npm where update == reinstall).
        let reasons = vec![format!("Update {} via pipx", record.name)];
        Ok(pipx_plan(&record.name, "upgrade", reasons))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // `pipx list --json` may exit non-zero in benign states while still printing
        // JSON, so read stdout regardless of exit status (only a spawn failure is fatal).
        let out = tokio::process::Command::new(BIN)
            .args(["list", "--json"])
            .output()
            .await
            .map_err(|e| JiiError::spawn(BIN, e))?;
        Ok(parse_pipx_list(&String::from_utf8_lossy(&out.stdout), ID))
    }
}

/// PyPI `/<pkg>/json` response (only the `info` block we use).
#[derive(Debug, Deserialize)]
struct PypiResponse {
    info: PypiInfo,
}

#[derive(Debug, Deserialize)]
struct PypiInfo {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    summary: Option<String>,
}

/// `pipx list --json` document (only the fields we use).
#[derive(Debug, Deserialize)]
struct PipxList {
    #[serde(default)]
    venvs: std::collections::HashMap<String, PipxVenv>,
}

#[derive(Debug, Deserialize)]
struct PipxVenv {
    #[serde(default)]
    metadata: Option<PipxMeta>,
}

#[derive(Debug, Deserialize)]
struct PipxMeta {
    #[serde(default)]
    main_package: Option<PipxMain>,
}

#[derive(Debug, Deserialize)]
struct PipxMain {
    #[serde(default)]
    package: Option<String>,
    #[serde(default)]
    package_version: Option<String>,
}

/// Build a candidate from a PyPI lookup. pipx cannot be pre-filtered to apps-only (the
/// API exposes no entry points — ADR-0023), so every existing package yields a candidate
/// and `pipx install` is the authority on whether it actually installs a CLI.
fn candidate(resp: &PypiResponse) -> PackageCandidate {
    PackageCandidate {
        name: resp.info.name.clone(),
        source_id: ID.to_string(),
        version: resp
            .info
            .version
            .clone()
            .filter(|s| !s.is_empty())
            .map(PkgVersion::new),
        trust: TrustLevel::Community,
        // Python apps are largely platform-independent; pip/pipx verify PyPI hashes.
        arch_ok: true,
        signed: true,
        summary: resp.info.summary.clone().filter(|s| !s.is_empty()),
        raw: json!({}),
    }
}

/// A single unprivileged `pipx <verb> <name>` plan (install/uninstall/upgrade).
fn pipx_plan(name: &str, verb: &str, reasons: Vec<String>) -> InstallPlan {
    let argv = vec![BIN.to_string(), verb.to_string(), name.to_string()];
    command_plan(ID, name, argv, false, reasons)
}

/// Parse `pipx list --json` into installed records. Tolerant: malformed JSON yields no
/// records rather than an error.
fn parse_pipx_list(stdout: &str, source_id: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    let Ok(doc) = serde_json::from_str::<PipxList>(stdout) else {
        return Vec::new();
    };
    doc.venvs
        .into_iter()
        .filter_map(|(app, venv)| {
            let main = venv.metadata.and_then(|m| m.main_package);
            let (name, version) = match main {
                // Prefer the recorded package name; fall back to the venv key.
                Some(m) => (
                    m.package.filter(|s| !s.is_empty()).unwrap_or(app),
                    m.package_version,
                ),
                None => (app, None),
            };
            if name.is_empty() {
                return None;
            }
            Some(InstalledRecord {
                name,
                source_id: source_id.to_string(),
                version: version.filter(|v| !v.is_empty()).map(PkgVersion::new),
                installed_at: now,
                // pipx/pip verify PyPI hashes — a self-verifying manager.
                verification: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(json: &str) -> PypiResponse {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn pypi_package_becomes_a_community_candidate() {
        let c = candidate(&response(
            r#"{"info":{"name":"black","version":"24.1.0","summary":"code formatter"}}"#,
        ));
        assert_eq!(c.name, "black");
        assert_eq!(c.source_id, "pipx");
        assert_eq!(c.version.as_ref().unwrap().0, "24.1.0");
        assert_eq!(c.trust, TrustLevel::Community);
        assert!(c.arch_ok && c.signed);
        assert_eq!(c.summary.as_deref(), Some("code formatter"));
    }

    #[test]
    fn install_and_upgrade_are_unprivileged_pipx_commands() {
        let install = pipx_plan("black", "install", vec![]);
        assert_eq!(install.source_id, "pipx");
        assert!(!install.needs_root());
        match &install.actions[0] {
            crate::model::Action::RunCommand { argv, needs_root } => {
                assert_eq!(argv, &["pipx", "install", "black"]);
                assert!(!needs_root);
            }
            other => panic!("expected run, got {other:?}"),
        }
        // Update uses pipx's first-class upgrade.
        let upgrade = pipx_plan("black", "upgrade", vec![]);
        match &upgrade.actions[0] {
            crate::model::Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["pipx", "upgrade", "black"])
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[test]
    fn parses_pipx_list() {
        let sample = r#"{
            "pipx_spec_version": "0.1",
            "venvs": {
                "black": {"metadata": {"main_package": {"package": "black", "package_version": "24.1.0"}}},
                "httpie": {"metadata": {"main_package": {"package": "httpie", "package_version": "3.2.2"}}}
            }
        }"#;
        let mut recs = parse_pipx_list(sample, "pipx");
        recs.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "black");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "24.1.0");
        assert_eq!(recs[0].source_id, "pipx");
        assert_eq!(recs[1].name, "httpie");
    }

    #[test]
    fn pipx_list_tolerates_empty_and_garbage() {
        assert!(parse_pipx_list(r#"{"venvs":{}}"#, "pipx").is_empty());
        assert!(parse_pipx_list("not json", "pipx").is_empty());
    }
}
