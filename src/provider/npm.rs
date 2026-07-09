//! npm provider (the npm registry — Node.js packages).
//!
//! Mirrors the cargo provider: search resolves an exact package via the npm registry
//! and only offers packages that install a **CLI** (a non-empty `bin`), so a
//! library-only package (e.g. `lodash`) yields no candidate — JII installs *programs*.
//!
//! Installs are user-space with **no root**: every command forces `--prefix
//! $HOME/.local`, so binaries land in `~/.local/bin` regardless of how npm's global
//! prefix is configured on the host (which may otherwise be a root-owned `/usr`). All
//! npm-specific detail (the prefix, the registry shape) lives here, never in the engine.
//! Trust is `community` (npm registry, verified by npm itself), so the plan is a single
//! unprivileged `npm` command with no separate download/verify step.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{Bootstrap, Ecosystem, Provider, command_plan, get_json_opt, which};
use crate::error::{JiiError, Result};
use crate::model::{
    InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
};

const ID: &str = "npm";
const BIN: &str = "npm";
const REGISTRY: &str = "https://registry.npmjs.org";

/// The npm installation source.
pub struct Npm;

impl Npm {
    pub fn new() -> Self {
        Npm
    }
}

impl Default for Npm {
    fn default() -> Self {
        Npm::new()
    }
}

#[async_trait]
impl Provider for Npm {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // The npm registry is a community registry (not distro-official).
        TrustLevel::Community
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    fn ecosystem(&self) -> Option<Ecosystem> {
        Some(Ecosystem {
            label: "Node.js (npm)",
            binary: BIN,
            bootstrap: Bootstrap::Packages(&["nodejs-npm", "npm"]),
        })
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // `/<pkg>/latest` returns just the latest manifest — small, with bin/version.
        let url = format!("{REGISTRY}/{}/latest", query.raw.trim());
        let Some(manifest) = get_json_opt::<Manifest>(ID, &url).await? else {
            return Ok(Vec::new()); // no such package
        };
        Ok(candidate(&manifest).into_iter().collect())
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let prefix = user_prefix()?;
        let mut reasons = vec!["npm registry (community)".to_string()];
        if let Some(v) = &candidate.version {
            reasons.push(format!("Version {v}"));
        }
        reasons.push(format!(
            "Installs {} into {prefix}/bin (no root; ensure it is on PATH)",
            candidate.name
        ));
        Ok(npm_plan(&candidate.name, "install", &prefix, reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // One `npm install --global --prefix $HOME/.local a b c` (still no root).
        let prefix = user_prefix()?;
        let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
        let mut argv = vec![
            BIN.to_string(),
            "install".to_string(),
            "--global".to_string(),
            "--prefix".to_string(),
            prefix,
        ];
        argv.extend(names.iter().cloned());
        let reasons = vec![format!("npm registry (community): {}", names.join(", "))];
        Ok(Some(command_plan(ID, &names.join(", "), argv, false, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let prefix = user_prefix()?;
        let reasons = vec![format!("Remove {} (installed via npm)", record.name)];
        Ok(npm_plan(&record.name, "uninstall", &prefix, reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        Ok(Some(npm_many("uninstall", records, "Remove (via npm)")?))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // Reinstalling the package pulls the newest published version.
        let prefix = user_prefix()?;
        let reasons = vec![format!("Update {} via npm (reinstall newest)", record.name)];
        Ok(npm_plan(&record.name, "install", &prefix, reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        Ok(Some(npm_many("install", records, "Update (via npm, reinstall newest)")?))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `npm update -g` updates every globally-installed package in our user prefix (D10).
        let prefix = user_prefix()?;
        let reasons = vec!["Update all global npm packages".to_string()];
        let argv = vec![
            BIN.to_string(),
            "update".to_string(),
            "--global".to_string(),
            "--prefix".to_string(),
            prefix,
        ];
        Ok(Some(command_plan(ID, "all global npm packages", argv, false, reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        let prefix = user_prefix()?;
        // `npm ls` can exit non-zero on benign warnings (extraneous/peer deps) while
        // still printing valid JSON, so we read stdout regardless of exit status — only
        // a spawn failure is fatal. (This tolerance is an npm quirk; it stays here.)
        let out = tokio::process::Command::new(BIN)
            .args(["ls", "-g", "--prefix", &prefix, "--json", "--depth=0"])
            .output()
            .await
            .map_err(|e| JiiError::spawn(BIN, e))?;
        Ok(parse_ls_json(&String::from_utf8_lossy(&out.stdout), ID))
    }
}

/// npm registry `/<pkg>/latest` manifest (only the fields we use).
#[derive(Debug, Deserialize)]
struct Manifest {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    /// `bin` may be an object `{ "cmd": "path" }` or a bare string; absent for a library.
    #[serde(default)]
    bin: Option<serde_json::Value>,
}

/// `npm ls -g --json` document (only the top-level dependency map).
#[derive(Debug, Deserialize)]
struct LsDoc {
    #[serde(default)]
    dependencies: std::collections::HashMap<String, LsDep>,
}

#[derive(Debug, Deserialize)]
struct LsDep {
    #[serde(default)]
    version: Option<String>,
}

/// Build a candidate from a manifest, or `None` if the package installs no CLI (a
/// library is not something `npm install -g` gives a user to run).
fn candidate(manifest: &Manifest) -> Option<PackageCandidate> {
    if !has_bin(manifest.bin.as_ref()) {
        return None;
    }
    Some(PackageCandidate {
        name: manifest.name.clone(),
        source_id: ID.to_string(),
        version: manifest
            .version
            .clone()
            .filter(|s| !s.is_empty())
            .map(PkgVersion::new),
        trust: TrustLevel::Community,
        // npm packages are platform-independent JS; npm verifies registry integrity.
        arch_ok: true,
        signed: true,
        summary: manifest.description.clone().filter(|d| !d.is_empty()),
        raw: json!({}),
    })
}

/// Whether a manifest's `bin` field declares at least one executable.
fn has_bin(bin: Option<&serde_json::Value>) -> bool {
    match bin {
        Some(serde_json::Value::String(s)) => !s.is_empty(),
        Some(serde_json::Value::Object(map)) => !map.is_empty(),
        _ => false,
    }
}

/// The user-space prefix all npm commands target (`$HOME/.local`), so installs never
/// need root and binaries land in `~/.local/bin`.
fn user_prefix() -> Result<String> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".local").to_string_lossy().into_owned())
        .ok_or_else(|| JiiError::Other(anyhow::anyhow!("cannot resolve home directory for npm --prefix")))
}

/// A single unprivileged `npm <verb> -g --prefix <prefix> <name>` plan. The forced
/// user prefix is npm's quirk (keeps installs out of root-owned dirs); the plan shape
/// itself is the shared [`command_plan`].
fn npm_plan(name: &str, verb: &str, prefix: &str, reasons: Vec<String>) -> InstallPlan {
    let argv = vec![
        BIN.to_string(),
        verb.to_string(),
        "--global".to_string(),
        "--prefix".to_string(),
        prefix.to_string(),
        name.to_string(),
    ];
    command_plan(ID, name, argv, false, reasons)
}

/// One `npm <verb> --global --prefix $HOME/.local a b c` for a whole group (no root).
fn npm_many(verb: &str, records: &[&InstalledRecord], label: &str) -> Result<InstallPlan> {
    let prefix = user_prefix()?;
    let names: Vec<String> = records.iter().map(|r| r.name.clone()).collect();
    let mut argv = vec![
        BIN.to_string(),
        verb.to_string(),
        "--global".to_string(),
        "--prefix".to_string(),
        prefix,
    ];
    argv.extend(names.iter().cloned());
    let reasons = vec![format!("{label}: {}", names.join(", "))];
    Ok(command_plan(ID, &names.join(", "), argv, false, reasons))
}

/// Parse `npm ls -g --json` output into installed records (top-level deps only).
/// Tolerant: malformed JSON yields no records rather than an error.
fn parse_ls_json(stdout: &str, source_id: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    let Ok(doc) = serde_json::from_str::<LsDoc>(stdout) else {
        return Vec::new();
    };
    doc.dependencies
        .into_iter()
        .filter_map(|(name, dep)| {
            if name.is_empty() {
                return None;
            }
            Some(InstalledRecord {
                name,
                source_id: source_id.to_string(),
                version: dep.version.filter(|v| !v.is_empty()).map(PkgVersion::new),
                installed_at: now,
                // npm verifies registry integrity itself — a self-verifying manager.
                verification: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: &str) -> Manifest {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn cli_package_becomes_a_community_candidate() {
        let m = manifest(
            r#"{"name":"prettier","version":"3.9.4","description":"code formatter","bin":{"prettier":"bin/prettier.cjs"}}"#,
        );
        let c = candidate(&m).unwrap();
        assert_eq!(c.name, "prettier");
        assert_eq!(c.source_id, "npm");
        assert_eq!(c.version.as_ref().unwrap().0, "3.9.4");
        assert_eq!(c.trust, TrustLevel::Community);
        assert!(c.arch_ok && c.signed);
        assert_eq!(c.summary.as_deref(), Some("code formatter"));
    }

    #[test]
    fn bin_as_string_is_a_cli() {
        // Some packages declare `bin` as a bare string (cmd name == package name).
        let m = manifest(r#"{"name":"serve","version":"14.0.0","bin":"build/main.js"}"#);
        assert!(candidate(&m).is_some());
    }

    #[test]
    fn library_only_package_yields_no_candidate() {
        // lodash ships no bin — JII must not offer it.
        let m = manifest(r#"{"name":"lodash","version":"4.17.21","description":"utils"}"#);
        assert!(candidate(&m).is_none());
        // An empty bin object is also "no CLI".
        assert!(candidate(&manifest(r#"{"name":"x","bin":{}}"#)).is_none());
    }

    #[test]
    fn install_plan_is_one_unprivileged_user_prefixed_command() {
        let plan = npm_plan("prettier", "install", "/home/u/.local", vec![]);
        assert_eq!(plan.source_id, "npm");
        assert!(!plan.needs_root());
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, needs_root } => {
                assert_eq!(
                    argv,
                    &["npm", "install", "--global", "--prefix", "/home/u/.local", "prettier"]
                );
                assert!(!needs_root);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[test]
    fn parses_ls_json() {
        let sample = r#"{
            "name": "root",
            "dependencies": {
                "prettier": {"version": "3.9.4", "overridden": false},
                "typescript": {"version": "5.6.2"}
            }
        }"#;
        let mut recs = parse_ls_json(sample, "npm");
        recs.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "prettier");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "3.9.4");
        assert_eq!(recs[0].source_id, "npm");
        assert_eq!(recs[1].name, "typescript");
    }

    #[test]
    fn ls_json_tolerates_empty_and_garbage() {
        assert!(parse_ls_json("{}", "npm").is_empty());
        assert!(parse_ls_json("not json", "npm").is_empty());
    }
}
