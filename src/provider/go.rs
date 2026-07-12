//! Go provider (`go install` — commands from the Go module ecosystem).
//!
//! `go install <module>@latest` builds a module's command into `$GOBIN` (or
//! `$GOPATH/bin`, default `~/go/bin`) — user-space, no root. Search resolves the latest
//! version via the Go module proxy. The query must be a full module path (e.g.
//! `github.com/junegunn/fzf`), like the github provider needs `owner/repo`.
//!
//! Go has no `uninstall`, so removal deletes the installed binary (an
//! [`Action::RemoveFile`], mirroring the github provider), and there is no cheap global
//! list mapping binaries back to modules, so `list_installed` is empty and the registry
//! (+ a file-existence `is_installed`) tracks what jii installed.
//!
//! **Program detection (ADR-0023):** only `main` packages are installable and the proxy
//! can't cheaply say which are `main`, so — like pipx — go does **not** pre-filter: it
//! offers the module and lets `go install` be the authority. Trust is `community` (a
//! language registry with checksum/transparency verification via `go.sum` /
//! sum.golang.org), matching cargo/npm/pipx.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

use super::{Bootstrap, Ecosystem, Provider, command_plan, get_json_opt, run_capture};
use crate::error::{JiiError, Result};
use crate::model::{
    Action, InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
};

const ID: &str = "go";
const BIN: &str = "go";
const PROXY: &str = "https://proxy.golang.org";

/// The Go (`go install`) installation source.
pub struct Go;

impl Go {
    pub fn new() -> Self {
        Go
    }
}

impl Default for Go {
    fn default() -> Self {
        Go::new()
    }
}

#[async_trait]
impl Provider for Go {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // A language registry with checksum/transparency verification — community tier.
        TrustLevel::Community
    }

    async fn is_available(&self) -> bool {
        // `go` uses `go version` (not `--version`), so the shared `which` doesn't fit.
        run_capture(&[BIN, "version"]).await.is_ok()
    }

    /// The Go module proxy is queried over the network — no go toolchain needed to find a
    /// match, so a host without go can still be offered "install go, then this" (ADR-0061).
    fn can_search(&self) -> bool {
        true
    }

    fn web_url(&self, candidate: &PackageCandidate) -> Option<String> {
        // The candidate name is the module path (e.g. github.com/junegunn/fzf).
        Some(format!("https://pkg.go.dev/{}", candidate.name))
    }

    fn ecosystem(&self) -> Option<Ecosystem> {
        Some(Ecosystem {
            label: "Go",
            bootstrap: Bootstrap::Packages(&["golang", "go", "golang-go"]),
        })
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        let module = query.raw.trim();
        // Proxy paths lowercase uppercase letters as `!x` (e.g. BurntSushi → !burnt!sushi).
        let url = format!("{PROXY}/{}/@latest", escape_module(module));
        let Some(latest) = get_json_opt::<GoLatest>(ID, &url).await? else {
            return Ok(Vec::new()); // no such module
        };
        Ok(vec![candidate(module, &latest)])
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let mut reasons = vec![crate::t!("reason.go_module")];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }
        reasons.push(crate::t!(
            "reason.go_builds",
            name = binary_name(&candidate.name),
            dir = go_bin_display()
        ));
        Ok(go_install_plan(&candidate.name, reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // `go install a@latest b@latest` builds each module's command in one run.
        let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
        let mut argv = vec![BIN.to_string(), "install".to_string()];
        argv.extend(names.iter().map(|n| format!("{n}@latest")));
        let reasons = vec![crate::t!("reason.go_modules_many", names = names.join(", "))];
        Ok(Some(command_plan(ID, &names.join(", "), argv, false, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // Go has no uninstall; remove the installed binary (like the github provider).
        let bin = go_bin_dir()
            .ok_or_else(|| JiiError::Other(anyhow::anyhow!("cannot resolve Go bin dir")))?
            .join(binary_name(&record.name));
        let reasons = vec![crate::t!("reason.go_remove", name = record.name.clone(), path = bin.display().to_string())];
        Ok(InstallPlan {
            candidate_ref: record.name.clone(),
            source_id: ID.to_string(),
            actions: vec![Action::RemoveFile { path: bin }],
            download_size: None,
            reasons,
        })
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // Reinstalling @latest pulls the newest published version.
        let reasons = vec![crate::t!("reason.go_update_one", name = record.name.clone())];
        Ok(go_install_plan(&record.name, reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // `go install a@latest b@latest` rebuilds each module's binary in one run.
        // (No `plan_remove_many`: removal deletes individual binaries — `RemoveFile`
        // per record — so there is no single command to merge into.)
        let names: Vec<String> = records.iter().map(|r| r.name.clone()).collect();
        let mut argv = vec![BIN.to_string(), "install".to_string()];
        argv.extend(names.iter().map(|n| format!("{n}@latest")));
        let reasons = vec![crate::t!("reason.go_update_many", names = names.join(", "))];
        Ok(Some(command_plan(ID, &names.join(", "), argv, false, reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // Go has no global list mapping binaries back to modules cheaply; the registry
        // records what jii installed, and `is_installed` verifies a record via the file.
        Ok(Vec::new())
    }

    async fn is_installed(&self, record: &InstalledRecord) -> bool {
        go_bin_dir().is_some_and(|d| d.join(binary_name(&record.name)).exists())
    }
}

/// Go module proxy `/@latest` response (only the version we use).
#[derive(Debug, Deserialize)]
struct GoLatest {
    #[serde(rename = "Version")]
    version: String,
}

/// Build a candidate from a module path + its latest version. No app-filter (ADR-0023):
/// the proxy can't say whether the module is a `main` (installable) package, so `go
/// install` is the authority.
fn candidate(module: &str, latest: &GoLatest) -> PackageCandidate {
    PackageCandidate {
        name: module.to_string(),
        source_id: ID.to_string(),
        version: (!latest.version.is_empty()).then(|| PkgVersion::new(&latest.version)),
        trust: TrustLevel::Community,
        arch_ok: true, // built from source for the host
        signed: true,  // go verifies module checksums (go.sum / sum.golang.org)
        summary: None, // the proxy `@latest` carries no description
        raw: json!({}),
    }
}

/// A `go install <module>@latest` plan (single unprivileged command).
fn go_install_plan(module: &str, reasons: Vec<String>) -> InstallPlan {
    let argv = vec![
        BIN.to_string(),
        "install".to_string(),
        format!("{module}@latest"),
    ];
    command_plan(ID, module, argv, false, reasons)
}

/// Where `go install` places binaries: `$GOBIN`, else `$GOPATH/bin`, else `~/go/bin`.
fn go_bin_dir() -> Option<PathBuf> {
    if let Ok(gobin) = std::env::var("GOBIN")
        && !gobin.is_empty()
    {
        return Some(PathBuf::from(gobin));
    }
    let gopath = std::env::var("GOPATH")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().join("go")))?;
    Some(gopath.join("bin"))
}

/// A short human label for the install dir shown in plan reasons.
fn go_bin_display() -> String {
    go_bin_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/go/bin".to_string())
}

/// The installed binary's name: the last path element, skipping a Go major-version
/// suffix (`/v2`, `/v3`…). E.g. `github.com/foo/bar/v2` → `bar`, `.../cmd/tool` → `tool`.
fn binary_name(module: &str) -> &str {
    let trimmed = module.trim_end_matches('/');
    let mut parts = trimmed.rsplit('/');
    let last = parts.next().unwrap_or(trimmed);
    if is_major_version(last) {
        parts.next().unwrap_or(last)
    } else {
        last
    }
}

/// Whether a path element is a Go major-version suffix like `v2`.
fn is_major_version(seg: &str) -> bool {
    seg.len() >= 2 && seg.starts_with('v') && seg[1..].bytes().all(|b| b.is_ascii_digit())
}

/// Escape a module path for the proxy: uppercase `X` → `!x` (proxy paths are lowercase).
fn escape_module(module: &str) -> String {
    let mut out = String::with_capacity(module.len());
    for c in module.chars() {
        if c.is_ascii_uppercase() {
            out.push('!');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_becomes_a_community_candidate() {
        let c = candidate(
            "github.com/junegunn/fzf",
            &GoLatest {
                version: "v0.73.1".into(),
            },
        );
        assert_eq!(c.name, "github.com/junegunn/fzf");
        assert_eq!(c.source_id, "go");
        assert_eq!(c.version.as_ref().unwrap().0, "v0.73.1");
        assert_eq!(c.trust, TrustLevel::Community);
        assert!(c.arch_ok && c.signed);
    }

    #[test]
    fn install_plan_is_one_unprivileged_go_command() {
        let plan = go_install_plan("github.com/junegunn/fzf", vec![]);
        assert_eq!(plan.source_id, "go");
        assert!(!plan.needs_root());
        match &plan.actions[0] {
            Action::RunCommand { argv, needs_root } => {
                assert_eq!(argv, &["go", "install", "github.com/junegunn/fzf@latest"]);
                assert!(!needs_root);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[test]
    fn binary_name_takes_last_segment_skipping_major_version() {
        assert_eq!(binary_name("github.com/junegunn/fzf"), "fzf");
        assert_eq!(binary_name("github.com/foo/bar/cmd/tool"), "tool");
        assert_eq!(binary_name("github.com/foo/bar/v2"), "bar");
        assert_eq!(binary_name("github.com/foo/bar/v10"), "bar");
        assert_eq!(binary_name("github.com/foo/bar/"), "bar");
        // `v` alone or `vword` is a real segment, not a version suffix.
        assert_eq!(binary_name("example.com/vanity"), "vanity");
    }

    #[test]
    fn escape_module_lowercases_uppercase_with_bang() {
        assert_eq!(
            escape_module("github.com/BurntSushi/toml"),
            "github.com/!burnt!sushi/toml"
        );
        assert_eq!(
            escape_module("github.com/junegunn/fzf"),
            "github.com/junegunn/fzf"
        );
    }

    #[tokio::test]
    async fn batch_merges_into_one_go_install_with_latest_suffixes() {
        let a = candidate("github.com/junegunn/fzf", &GoLatest { version: "v1".into() });
        let b = candidate("github.com/x/y", &GoLatest { version: "v1".into() });
        let plan = Go::new()
            .plan_install_many(&[&a, &b])
            .await
            .unwrap()
            .expect("go batches");
        assert_eq!(plan.actions.len(), 1);
        assert!(!plan.needs_root());
        match &plan.actions[0] {
            Action::RunCommand { argv, .. } => {
                assert_eq!(
                    argv,
                    &[
                        "go",
                        "install",
                        "github.com/junegunn/fzf@latest",
                        "github.com/x/y@latest"
                    ]
                );
            }
            other => panic!("expected run, got {other:?}"),
        }
    }
}
