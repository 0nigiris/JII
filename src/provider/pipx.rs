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

use super::{Bootstrap, Ecosystem, FailureNote, Provider, command_plan, get_json_opt, which};
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

    /// PyPI lookups are network calls — no pipx CLI needed to find a match, so a host without
    /// pipx can still be offered "install pipx, then this application" (ADR-0061).
    fn can_search(&self) -> bool {
        true
    }

    fn web_url(&self, candidate: &PackageCandidate) -> Option<String> {
        Some(format!("https://pypi.org/project/{}/", candidate.name))
    }

    fn ecosystem(&self) -> Option<Ecosystem> {
        Some(Ecosystem {
            label: "Python (pipx)",
            bootstrap: Bootstrap::Packages(&["pipx"]),
        })
    }

    /// The other half of the ADR-0023 bargain: since pipx cannot pre-filter libraries out, it
    /// must at least explain the rejection it causes. `pipx install <lib>` fails with "No apps
    /// associated with package X", which is accurate but tells a user nothing — turn it into
    /// plain language plus the two real ways forward.
    fn explain_failure(&self, output: &str) -> Option<FailureNote> {
        let name = library_rejection(output)?;
        Some(FailureNote {
            message: crate::t!("library.pypi", name = name.clone()),
            hints: vec![
                crate::t!("library.pypi_pip", name = name.clone()),
                crate::t!("library.pypi_search", name = name),
            ],
        })
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        let url = format!("{API}/{}/json", query.raw.trim());
        let Some(body) = get_json_opt::<PypiResponse>(ID, &url).await? else {
            return Ok(Vec::new()); // no such package
        };
        Ok(vec![candidate(&body)])
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let mut reasons = vec![crate::t!("reason.pipx_pypi")];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }
        reasons.push(crate::t!("reason.pipx_installs", name = candidate.name.clone()));
        Ok(pipx_plan(&candidate.name, "install", reasons))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.remove_one", name = record.name.clone(), mgr = "pipx")];
        Ok(pipx_plan(&record.name, "uninstall", reasons))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // pipx has a first-class upgrade (unlike cargo/npm where update == reinstall).
        let reasons = vec![crate::t!("reason.update_one", name = record.name.clone(), mgr = "pipx")];
        Ok(pipx_plan(&record.name, "upgrade", reasons))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `pipx upgrade-all` upgrades every pipx-installed app (D10); user-space.
        let reasons = vec![crate::t!("reason.pipx_upgrade_all")];
        let argv = vec![BIN.to_string(), "upgrade-all".to_string()];
        Ok(Some(command_plan(ID, "all pipx apps", argv, false, reasons)))
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

/// PyPI `/<pkg>/json` response (only the `info` block + release files we use).
#[derive(Debug, Deserialize)]
struct PypiResponse {
    info: PypiInfo,
    /// The latest release's files — their upload times date the release.
    #[serde(default)]
    urls: Vec<PypiFile>,
}

#[derive(Debug, Deserialize)]
struct PypiInfo {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PypiFile {
    #[serde(default)]
    upload_time_iso_8601: Option<String>,
}

/// How old the newest release may be before the package reads as abandoned/squatting.
/// PyPI download counts are mirror-inflated (even junk gets ~1k/month) and the stats API
/// is rate-limited, so **staleness** is PyPI's honest junk marker: the `htop` squatter's
/// one release dates from 2016; real tools ship far more often than every 5 years.
const STALE_AFTER_YEARS: i64 = 5;

/// Whether the response's newest file upload is older than [`STALE_AFTER_YEARS`].
/// No parseable upload time → `false` (never guess someone into a warning).
fn is_stale(resp: &PypiResponse) -> bool {
    resp.urls
        .iter()
        .filter_map(|f| f.upload_time_iso_8601.as_deref())
        .filter_map(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .max()
        .is_some_and(|newest| {
            chrono::Utc::now().signed_duration_since(newest) > chrono::Duration::days(365 * STALE_AFTER_YEARS)
        })
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
        popularity: None,
        // A registry-specific junk marker (a years-stale sole release); the engine's
        // ranking heuristics turn this into the untrusted downgrade + loud warning.
        suspicious: is_stale(resp),
        raw: json!({}),
    }
}

/// The package name in pipx's "this is a library" rejection, if that is what `output` says.
/// pipx phrases it as `No apps associated with package <name>.` — the sole reliable marker;
/// the rest of its message (the `--include-deps` suggestion, the dependency app list) is noise
/// that would send the user off installing numpy's `f2py`.
fn library_rejection(output: &str) -> Option<String> {
    const MARKER: &str = "No apps associated with package ";
    let rest = output.split(MARKER).nth(1)?;
    let name = rest
        .split([' ', '.', '\n', '\r', ','])
        .next()
        .unwrap_or("")
        .trim();
    (!name.is_empty()).then(|| name.to_string())
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
    fn stale_sole_release_is_premarked_suspicious() {
        // The `htop` squatter class: one release uploaded many years ago.
        let stale = response(
            r#"{"info":{"name":"htop","version":"1.0","summary":"A training project"},
                "urls":[{"upload_time_iso_8601":"2016-03-30T14:57:57Z"}]}"#,
        );
        assert!(is_stale(&stale));
        assert!(candidate(&stale).suspicious);

        // A recent release is fine; so is a response with no parseable dates.
        let fresh_time = chrono::Utc::now().to_rfc3339();
        let fresh = response(&format!(
            r#"{{"info":{{"name":"black","version":"26.5.1"}},
                "urls":[{{"upload_time_iso_8601":"{fresh_time}"}}]}}"#,
        ));
        assert!(!is_stale(&fresh));
        let dateless =
            response(r#"{"info":{"name":"x","version":"1.0"},"urls":[{}]}"#);
        assert!(!is_stale(&dateless));
    }

    #[test]
    fn library_rejection_is_recognised_and_explained() {
        // The real pipx wording, dependency-app noise and all.
        let out = "creating virtual environment...\ninstalling affinity...\nNo apps associated with package affinity. Try again with '--include-deps' to\ninclude apps of dependent packages, which are listed above.\nNote: Dependent package 'numpy' contains 2 apps\n  - f2py\n";
        assert_eq!(library_rejection(out).as_deref(), Some("affinity"));

        let note = Pipx::new().explain_failure(out).expect("a library failure explains itself");
        assert!(note.message.contains("affinity"), "names the package: {}", note.message);
        // Never a dead end: an explanation always comes with somewhere to go.
        assert_eq!(note.hints.len(), 2);

        // An unrelated failure is left alone for the raw tail to show.
        assert!(library_rejection("error: network unreachable").is_none());
        assert!(Pipx::new().explain_failure("error: network unreachable").is_none());
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
