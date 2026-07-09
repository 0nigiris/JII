//! COPR provider (Fedora's community build service).
//!
//! COPR has no package search — only a *project* search — so this resolves a query
//! to a COPR project whose name matches exactly and that builds for the running
//! Fedora/arch, then plans the two privileged steps a user would run by hand:
//!
//! 1. `dnf5 -y copr enable <owner>/<project>`  (adds the third-party repo)
//! 2. `dnf5 -y install <name>`                 (installs the package)
//!
//! All network access is in `search` (like the github provider), so `plan_install`
//! is pure. Trust is `community`: COPR repos are user-provided add-ons, and the exact
//! `owner/project` being enabled is shown in the plan for confirmation.
//!
//! Choosing among several identically-named projects is inherently ambiguous (the
//! COPR name→project problem); this picks the first exact-name match that builds for
//! the host, and relies on the visible plan + confirmation as the safety net.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;

use super::{Probe, Provider, http_client, run_capture, which};
use crate::error::{JiiError, Result};
use crate::model::{
    Action, InstallPlan, InstalledRecord, PackageCandidate, Query, TrustLevel,
};

const ID: &str = "copr";
const BIN: &str = "dnf5";
const API: &str = "https://copr.fedorainfracloud.org/api_3";

/// The COPR installation source.
pub struct Copr {
    /// Host arch, used to keep only projects that build for it (e.g. "x86_64").
    arch: &'static str,
}

impl Copr {
    pub fn new(arch: &'static str) -> Self {
        Copr { arch }
    }
}

#[async_trait]
impl Provider for Copr {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // COPR repos are community add-ons — not distro-official.
        TrustLevel::Community
    }

    fn highlights(&self, _candidate: &PackageCandidate) -> Vec<String> {
        vec![crate::t!("reason.copr_community"), crate::t!("reason.copr_enables")]
    }

    async fn is_available(&self) -> bool {
        // Enabling/installing goes through dnf5; the API is reached at search time.
        which(BIN).await
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        let client = http_client()?;
        let url = format!("{API}/project/search");
        let resp = client
            .get(&url)
            .query(&[("query", &query.raw)])
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| JiiError::Other(anyhow::anyhow!("copr: {e}")))?;
        let search: SearchResponse = resp
            .json()
            .await
            .map_err(|e| JiiError::Other(anyhow::anyhow!("copr: malformed json: {e}")))?;

        Ok(select_project(&query.raw, &search.items, self.arch)
            .map(candidate)
            .into_iter()
            .collect())
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let project = candidate
            .raw
            .get("project")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JiiError::Other(anyhow::anyhow!("copr candidate missing 'project'")))?;
        Ok(build_install_plan(&candidate.name, project))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.remove_one", name = record.name.clone(), mgr = "copr")];
        Ok(root_plan(
            &record.name,
            vec![vec![BIN, "-y", "remove", &record.name]],
            reasons,
        ))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // One `dnf5 -y remove a b c` (COPR packages live in dnf once enabled).
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut argv = vec![BIN, "-y", "remove"];
        argv.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.remove_many", mgr = "copr/dnf", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), vec![argv], reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.update_one", name = record.name.clone(), mgr = "copr")];
        Ok(root_plan(
            &record.name,
            vec![vec![BIN, "-y", "upgrade", &record.name]],
            reasons,
        ))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // One `dnf5 -y upgrade a b c`.
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut argv = vec![BIN, "-y", "upgrade"];
        argv.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.update_many", mgr = "copr/dnf", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), vec![argv], reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // COPR packages are ordinary RPMs; they can't be told apart from other dnf
        // packages by rpm alone, so we don't enumerate them (the registry records
        // what jii installed). `is_installed` verifies a specific record via rpm.
        Ok(Vec::new())
    }

    async fn is_installed(&self, record: &InstalledRecord) -> bool {
        // A COPR install ends up in the rpm db; verify by querying it.
        run_capture(&[BIN, "repoquery", "-q", "--installed", &record.name])
            .await
            .is_ok_and(|out| out.lines().any(|l| !l.trim().is_empty()))
    }

    async fn probe(&self) -> Probe {
        // Any HTTP response from the API means it is reachable (search will work);
        // a lightweight query keeps it cheap. COPR has no client rate limit for us.
        let Ok(client) = http_client() else {
            return Probe::unreachable();
        };
        let url = format!("{API}/project/search");
        match client.get(&url).query(&[("query", "jii")]).send().await {
            Ok(_) => Probe {
                reachable: true,
                rate_limited: false,
                detail: None,
            },
            Err(_) => Probe::unreachable(),
        }
    }
}

/// COPR `project/search` response (only the fields we use).
#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    items: Vec<Project>,
}

/// One COPR project.
#[derive(Debug, Deserialize)]
struct Project {
    /// Project (and, by convention, package) name.
    name: String,
    /// `owner/project`, the spec passed to `dnf5 copr enable`.
    full_name: String,
    #[serde(default)]
    description: Option<String>,
    /// Built chroots, keyed like `fedora-44-x86_64` → results URL.
    #[serde(default)]
    chroot_repos: HashMap<String, serde_json::Value>,
}

/// Pick the COPR project to offer among those whose name equals the query (so the
/// package name is known) and that build for the host Fedora/arch. Among ties, prefer
/// the project that builds for the most Fedora releases — a rough "more maintained"
/// signal — since COPR gives us no popularity/quality metric here.
fn select_project<'a>(query: &str, projects: &'a [Project], arch: &str) -> Option<&'a Project> {
    let q = query.trim().to_ascii_lowercase();
    projects
        .iter()
        .filter(|p| p.name.to_ascii_lowercase() == q)
        .map(|p| (fedora_chroots(p, arch), p))
        .filter(|(n, _)| *n > 0)
        .max_by_key(|(n, _)| *n)
        .map(|(_, p)| p)
}

/// How many Fedora chroots the project builds for `arch` (0 = not installable here).
fn fedora_chroots(project: &Project, arch: &str) -> usize {
    let suffix = format!("-{arch}");
    project
        .chroot_repos
        .keys()
        .filter(|c| c.starts_with("fedora-") && c.ends_with(&suffix))
        .count()
}

/// Build a candidate from a matched project. Name = project name (the package to
/// install); `owner/project` is stashed in `raw` for the enable step.
fn candidate(project: &Project) -> PackageCandidate {
    PackageCandidate {
        name: project.name.clone(),
        source_id: ID.to_string(),
        version: None,
        trust: TrustLevel::Community,
        arch_ok: true,
        signed: true,
        summary: project
            .description
            .as_ref()
            .filter(|d| !d.is_empty())
            .cloned(),
        raw: json!({ "project": project.full_name }),
    }
}

/// The two privileged steps of a COPR install: enable the repo, then install.
fn build_install_plan(name: &str, project: &str) -> InstallPlan {
    let reasons = vec![
        crate::t!("reason.copr_repo"),
        crate::t!("reason.repository", repo = project),
        crate::t!("reason.copr_enables_then", name = name),
    ];
    root_plan(
        name,
        vec![
            vec![BIN, "-y", "copr", "enable", project],
            vec![BIN, "-y", "install", name],
        ],
        reasons,
    )
}

/// Assemble a plan of root-requiring `dnf5` commands.
fn root_plan(name: &str, commands: Vec<Vec<&str>>, reasons: Vec<String>) -> InstallPlan {
    let actions = commands
        .into_iter()
        .map(|argv| Action::RunCommand {
            argv: argv.into_iter().map(String::from).collect(),
            needs_root: true,
        })
        .collect();
    InstallPlan {
        candidate_ref: name.to_string(),
        source_id: ID.to_string(),
        actions,
        download_size: None,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "items": [
            {"name": "toolbox", "full_name": "sim0n/toolbox",
             "chroot_repos": {"fedora-44-x86_64": "u"}},
            {"name": "starship", "full_name": "atim/starship", "description": "Cross-shell prompt",
             "chroot_repos": {"fedora-44-x86_64": "u", "fedora-43-x86_64": "u"}},
            {"name": "starship", "full_name": "rhelonly/starship",
             "chroot_repos": {"rhel-10-x86_64": "u"}},
            {"name": "starship", "full_name": "armperson/starship",
             "chroot_repos": {"fedora-44-aarch64": "u"}}
        ]
    }"#;

    fn projects() -> Vec<Project> {
        serde_json::from_str::<SearchResponse>(SAMPLE).unwrap().items
    }

    #[test]
    fn parses_search_response() {
        let items = projects();
        assert_eq!(items.len(), 4);
        assert_eq!(items[1].full_name, "atim/starship");
    }

    #[test]
    fn selects_most_maintained_exact_name_for_host() {
        // Among x86_64 fedora builds, atim/starship has the most chroots (fedora-44
        // + fedora-43); rhelonly has no fedora, armperson is aarch64-only.
        let items = projects();
        let p = select_project("starship", &items, "x86_64").unwrap();
        assert_eq!(p.full_name, "atim/starship");
    }

    #[test]
    fn respects_arch_when_selecting() {
        // Only armperson/starship builds for aarch64.
        let items = projects();
        let p = select_project("starship", &items, "aarch64").unwrap();
        assert_eq!(p.full_name, "armperson/starship");
    }

    #[test]
    fn no_match_when_name_differs_or_no_fedora_chroot() {
        assert!(select_project("nonexistent", &projects(), "x86_64").is_none());
        // Name matches exist, but none build a fedora chroot for riscv64.
        assert!(select_project("starship", &projects(), "riscv64").is_none());
    }

    #[test]
    fn fedora_chroots_counts_matching_only() {
        let items = projects();
        assert_eq!(fedora_chroots(&items[1], "x86_64"), 2); // fedora-44 + fedora-43
        assert_eq!(fedora_chroots(&items[2], "x86_64"), 0); // rhel-10 (not fedora)
        assert_eq!(fedora_chroots(&items[1], "aarch64"), 0); // no aarch64 chroot
    }

    #[test]
    fn candidate_is_community_with_project_in_raw() {
        let c = candidate(select_project("starship", &projects(), "x86_64").unwrap());
        assert_eq!(c.name, "starship");
        assert_eq!(c.source_id, "copr");
        assert_eq!(c.trust, TrustLevel::Community);
        assert_eq!(c.raw.get("project").unwrap().as_str(), Some("atim/starship"));
        assert_eq!(c.summary.as_deref(), Some("Cross-shell prompt"));
    }

    #[test]
    fn install_plan_is_enable_then_install_both_root() {
        let plan = build_install_plan("starship", "atim/starship");
        assert_eq!(plan.source_id, "copr");
        assert!(plan.needs_root());
        assert_eq!(plan.actions.len(), 2);
        match &plan.actions[0] {
            Action::RunCommand { argv, needs_root } => {
                assert_eq!(argv, &["dnf5", "-y", "copr", "enable", "atim/starship"]);
                assert!(needs_root);
            }
            other => panic!("expected run, got {other:?}"),
        }
        match &plan.actions[1] {
            Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["dnf5", "-y", "install", "starship"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }
}
