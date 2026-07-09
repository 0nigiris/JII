//! Nix provider (the Nix package manager — Nixpkgs, on any distro incl. NixOS).
//!
//! Unlike apt/pacman/zypper, Nix is **not** distro-bound and installs into the user's Nix
//! **profile with no root** (like cargo/go). It self-gates on the `nix` binary (ADR-0029).
//! Every invocation passes `--extra-experimental-features "nix-command flakes"` so the
//! modern `nix` CLI works regardless of the host's global config. Search reads
//! `nix search --json` (a stable JSON schema); install/remove/upgrade use `nix profile`.
//!
//! Like go, there is no cheap list mapping profile entries back to names, so
//! `list_installed` is empty and a file-existence `is_installed` verifies a record via the
//! profile's `bin/<name>` symlink (same name==binary caveat go carries). Trust is
//! `community` (Nixpkgs is a curated community collection with reproducible builds).

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::{Bootstrap, Ecosystem, Provider, command_plan, run_capture_lax, which};
use crate::error::Result;
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel};

const ID: &str = "nix";
const BIN: &str = "nix";

/// The Nix (Nixpkgs, `nix profile`) installation source.
pub struct Nix;

impl Nix {
    pub fn new() -> Self {
        Nix
    }
}

impl Default for Nix {
    fn default() -> Self {
        Nix::new()
    }
}

#[async_trait]
impl Provider for Nix {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // Nixpkgs: a curated community collection with reproducible builds.
        TrustLevel::Community
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    fn ecosystem(&self) -> Option<Ecosystem> {
        Some(Ecosystem {
            label: "Nix",
            binary: BIN,
            // Nix bootstraps via its own multi-user installer, not a distro package.
            bootstrap: Bootstrap::Script("sh <(curl -L https://nixos.org/nix/install) --daemon"),
        })
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // `nix search nixpkgs <name> --json` returns a JSON map keyed by attr path. The
        // regex is anchored to cut noise, but the authoritative match is an exact `pname`
        // done in code (so we never install the wrong near-name). A non-match yields `{}`.
        let name = query.raw.trim();
        let regex = format!("^{name}$");
        let out = run_capture_lax(&[
            BIN,
            "--extra-experimental-features",
            "nix-command flakes",
            "search",
            "nixpkgs",
            &regex,
            "--json",
        ])
        .await?;
        Ok(parse_search(&out, name, self.trust()))
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let mut reasons = vec!["Nixpkgs package (community)".to_string()];
        if let Some(v) = &candidate.version {
            reasons.push(format!("Version {v}"));
        }
        reasons.push("Installs into your Nix profile (no root)".to_string());
        let flake = flake_ref(&candidate.name);
        let argv = nix_argv(&["profile", "install", &flake]);
        Ok(command_plan(ID, &candidate.name, argv, false, reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // One `nix profile install nixpkgs#a nixpkgs#b` for the whole group (no root).
        let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
        let flakes: Vec<String> = names.iter().map(|n| flake_ref(n)).collect();
        let mut sub = vec!["profile", "install"];
        sub.extend(flakes.iter().map(|s| s.as_str()));
        let reasons = vec![format!("Nixpkgs packages (community): {}", names.join(", "))];
        Ok(Some(command_plan(ID, &names.join(", "), nix_argv(&sub), false, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // `nix profile remove <name>` drops the profile entry (no root).
        let reasons = vec![format!("Remove {} from your Nix profile", record.name)];
        let argv = nix_argv(&["profile", "remove", &record.name]);
        Ok(command_plan(ID, &record.name, argv, false, reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut sub = vec!["profile", "remove"];
        sub.extend_from_slice(&names);
        let reasons = vec![format!("Remove from Nix profile: {}", names.join(", "))];
        Ok(Some(command_plan(ID, &names.join(", "), nix_argv(&sub), false, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![format!("Upgrade {} in your Nix profile", record.name)];
        let argv = nix_argv(&["profile", "upgrade", &record.name]);
        Ok(command_plan(ID, &record.name, argv, false, reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut sub = vec!["profile", "upgrade"];
        sub.extend_from_slice(&names);
        let reasons = vec![format!("Upgrade in Nix profile: {}", names.join(", "))];
        Ok(Some(command_plan(ID, &names.join(", "), nix_argv(&sub), false, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `nix profile upgrade --all` upgrades every package in the user profile (D10);
        // user-space, no root (like the rest of the nix provider).
        let reasons = vec!["Upgrade all packages in your Nix profile".to_string()];
        let argv = nix_argv(&["profile", "upgrade", "--all"]);
        Ok(Some(command_plan(ID, "nix profile", argv, false, reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // `nix profile list --json` has changed schema across Nix versions; the registry
        // records what jii installed, and `is_installed` verifies via the profile symlink.
        Ok(Vec::new())
    }

    async fn is_installed(&self, record: &InstalledRecord) -> bool {
        nix_profile_bin(&record.name)
            .map(|p| p.exists())
            .unwrap_or(false)
    }
}

/// Nixpkgs flake reference for a package name (`nixpkgs#ripgrep`).
fn flake_ref(name: &str) -> String {
    format!("nixpkgs#{name}")
}

/// Build a `nix --extra-experimental-features "nix-command flakes" <sub…>` argv. Passing
/// the features per-invocation makes the modern CLI work whether or not the host enabled
/// flakes globally.
fn nix_argv(sub: &[&str]) -> Vec<String> {
    let mut argv = vec![
        BIN.to_string(),
        "--extra-experimental-features".to_string(),
        "nix-command flakes".to_string(),
    ];
    argv.extend(sub.iter().map(|s| s.to_string()));
    argv
}

/// The profile symlink a `nix profile install` exposes for `name`: `~/.nix-profile/bin/name`.
fn nix_profile_bin(name: &str) -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().join(".nix-profile/bin").join(name))
}

/// One `nix search --json` entry (only the fields we use).
#[derive(Debug, Deserialize)]
struct NixEntry {
    pname: String,
    version: String,
    description: Option<String>,
}

/// Parse `nix search --json` output (a map keyed by attr path) into at most one candidate:
/// the entry whose `pname` matches the query **exactly** (the regex can still return
/// near-names, so the exact match is decided here). Unparseable/empty output → no candidate.
fn parse_search(stdout: &str, name: &str, trust: TrustLevel) -> Vec<PackageCandidate> {
    let map: BTreeMap<String, NixEntry> =
        serde_json::from_str(stdout.trim()).unwrap_or_default();
    map.into_values()
        .find(|e| e.pname == name)
        .map(|e| {
            vec![PackageCandidate {
                name: name.to_string(),
                source_id: ID.to_string(),
                version: (!e.version.is_empty()).then(|| PkgVersion::new(&e.version)),
                trust,
                arch_ok: true,
                signed: true,
                summary: e.description.filter(|s| !s.is_empty()),
                raw: json!({}),
            }]
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "legacyPackages.x86_64-linux.ripgrep": {
        "pname": "ripgrep",
        "version": "14.1.1",
        "description": "A utility that combines the usability of The Silver Searcher with the raw speed of grep"
      },
      "legacyPackages.x86_64-linux.ripgrep-all": {
        "pname": "ripgrep-all",
        "version": "0.10.6",
        "description": "rga: ripgrep, but also search in PDFs, E-Books, ..."
      }
    }"#;

    #[test]
    fn exact_pname_wins_over_near_names() {
        let cands = parse_search(SAMPLE, "ripgrep", TrustLevel::Community);
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(c.name, "ripgrep");
        assert_eq!(c.version.as_ref().unwrap().0, "14.1.1");
        assert_eq!(c.source_id, "nix");
        assert_eq!(c.trust, TrustLevel::Community);
        assert!(c.summary.as_deref().unwrap().starts_with("A utility"));
    }

    #[test]
    fn no_exact_match_yields_nothing() {
        assert!(parse_search(SAMPLE, "ripgrepx", TrustLevel::Community).is_empty());
        assert!(parse_search("{}", "ripgrep", TrustLevel::Community).is_empty());
        assert!(parse_search("", "ripgrep", TrustLevel::Community).is_empty());
    }

    fn rec(name: &str) -> InstalledRecord {
        InstalledRecord {
            name: name.to_string(),
            source_id: ID.to_string(),
            version: None,
            installed_at: chrono::Utc::now(),
            verification: None,
        }
    }

    fn argv_of(plan: &InstallPlan) -> Vec<String> {
        assert!(!plan.needs_root(), "nix profile ops are user-space (no root)");
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, needs_root } => {
                assert!(!needs_root);
                argv.clone()
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn install_plan_is_one_unprivileged_flake_install() {
        let c = parse_search(SAMPLE, "ripgrep", TrustLevel::Community).remove(0);
        let plan = Nix::new().plan_install(&c).await.unwrap();
        assert_eq!(
            argv_of(&plan),
            &[
                "nix",
                "--extra-experimental-features",
                "nix-command flakes",
                "profile",
                "install",
                "nixpkgs#ripgrep"
            ]
        );
    }

    #[tokio::test]
    async fn batch_install_merges_flake_refs() {
        let a = parse_search(SAMPLE, "ripgrep", TrustLevel::Community).remove(0);
        let b = PackageCandidate { name: "fd".into(), ..a.clone() };
        let plan = Nix::new()
            .plan_install_many(&[&a, &b])
            .await
            .unwrap()
            .expect("nix batches");
        let argv = argv_of(&plan);
        assert_eq!(&argv[argv.len() - 2..], &["nixpkgs#ripgrep", "nixpkgs#fd"]);
    }

    #[tokio::test]
    async fn update_uses_profile_upgrade() {
        let plan = Nix::new().plan_update(&rec("ripgrep")).await.unwrap();
        let argv = argv_of(&plan);
        assert_eq!(&argv[argv.len() - 2..], &["upgrade", "ripgrep"]);
    }
}
