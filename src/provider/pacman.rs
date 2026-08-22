// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pacman provider (Arch Linux and derivatives — Manjaro, EndeavourOS…).
//!
//! Self-gates on `pacman` (absent elsewhere → drops out; no distro check, ADR-0029).
//! Covers the **official** repositories only (core/extra); the AUR is a separate, lower-
//! trust source and is intentionally out of scope here. Search reads `pacman -Si`; plans
//! run `pacman`; the installed list comes from `pacman -Q`. Parsing is pure and unit-tested.

use async_trait::async_trait;
use serde_json::json;

use super::{Provider, command_plan, nonempty_lines, run_capture, run_capture_lax, which};
use crate::error::Result;
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel};

/// The pacman binary (drives search, plans and the installed list).
const BIN: &str = "pacman";

/// This provider's stable source id.
const ID: &str = "pacman";

/// The Pacman installation source.
pub struct Pacman;

impl Pacman {
    pub fn new() -> Self {
        Pacman
    }
}

impl Default for Pacman {
    fn default() -> Self {
        Pacman::new()
    }
}

#[async_trait]
impl Provider for Pacman {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // Arch official repos (core/extra) are signed. (AUR is not this provider.)
        TrustLevel::Official
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // `pacman -Si <name>` prints the sync-db info stanza for an exact package name; it
        // exits 1 for an unknown package, which is "no candidate", not a source failure —
        // so read stdout laxly (empty → no match).
        let out = run_capture_lax(&[BIN, "-Si", &query.raw]).await?;
        Ok(parse_si(&out, self.id(), self.trust()))
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let repo = candidate.raw.get("repo").and_then(|v| v.as_str()).unwrap_or("unknown");
        let mut reasons = vec![
            crate::t!("reason.pacman_official"),
            crate::t!("reason.repository", repo = repo),
        ];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }
        Ok(root_plan(
            &candidate.name,
            &["-S", "--noconfirm", &candidate.name],
            reasons,
        ))
    }

    async fn plan_install_many(&self, candidates: &[&PackageCandidate]) -> Result<Option<InstallPlan>> {
        // One `pacman -S --noconfirm a b c` for the whole group.
        let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
        let mut args = vec!["-S", "--noconfirm"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.pacman_official_many", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // -Rs also removes dependencies no longer required by anything else (the closest
        // analogue to dnf/apt cleanup); the full command is shown before it runs.
        let reasons = vec![crate::t!(
            "reason.remove_one",
            name = record.name.clone(),
            mgr = "pacman"
        )];
        Ok(root_plan(&record.name, &["-Rs", "--noconfirm", &record.name], reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["-Rs", "--noconfirm"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!(
            "reason.remove_many",
            mgr = "pacman",
            names = names.join(", ")
        )];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // pacman has no isolated single-package upgrade; installing the target pulls its
        // newest version from the current sync db. (A full `-Syu` is the user's own call —
        // JII updates the packages it tracks, one target at a time.)
        let reasons = vec![crate::t!(
            "reason.update_one",
            name = record.name.clone(),
            mgr = "pacman"
        )];
        Ok(root_plan(&record.name, &["-S", "--noconfirm", &record.name], reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["-S", "--noconfirm"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!(
            "reason.update_many",
            mgr = "pacman",
            names = names.join(", ")
        )];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `pacman -Syu --noconfirm` = sync the db and upgrade the whole system (D10).
        let reasons = vec![crate::t!("reason.upgrade_all_system", mgr = "pacman")];
        Ok(Some(root_plan("system", &["-Syu", "--noconfirm"], reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // `pacman -Q` prints `name version` (single space) per installed package.
        let out = run_capture(&[BIN, "-Q"]).await?;
        Ok(parse_query(&out, self.id()))
    }
}

/// Build a single-command, root-requiring `pacman <args>` plan.
fn root_plan(name: &str, args: &[&str], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![BIN.to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    command_plan(ID, name, argv, true, reasons)
}

/// Parse `pacman -Si` output into at most one candidate: the **first stanza** (blank-line
/// separated). Lines are `Key : Value` (the key padded for alignment); the first `:` is the
/// separator, so values containing `:` (URLs) survive. We read Name, Version, Repository and
/// Description.
fn parse_si(stdout: &str, source_id: &str, trust: TrustLevel) -> Vec<PackageCandidate> {
    let stanza = stdout.split("\n\n").next().unwrap_or("");

    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut repo: Option<String> = None;
    let mut summary: Option<String> = None;

    for line in stanza.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Name" => name = Some(value.to_string()),
            "Version" => version = Some(value.to_string()),
            "Repository" => repo = Some(value.to_string()),
            "Description" if !value.is_empty() => summary = Some(value.to_string()),
            _ => {}
        }
    }

    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return Vec::new();
    };
    vec![PackageCandidate {
        name,
        source_id: source_id.to_string(),
        version: version.filter(|v| !v.is_empty()).map(|v| PkgVersion::new(&v)),
        trust,
        arch_ok: true,
        signed: true,
        summary,
        popularity: None,
        suspicious: false,
        raw: json!({ "repo": repo.unwrap_or_default() }),
    }]
}

/// Parse `pacman -Q` output (`name version` per line) into installed records.
fn parse_query(stdout: &str, source_id: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    nonempty_lines(stdout)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let version = parts.next();
            Some(InstalledRecord {
                name: name.to_string(),
                source_id: source_id.to_string(),
                version: version.map(PkgVersion::new),
                installed_at: now,
                verification: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Repository      : extra
Name            : ripgrep
Version         : 14.1.1-1
Description     : A search tool that combines the usability of ag with the raw speed of grep
Architecture    : x86_64
URL             : https://github.com/BurntSushi/ripgrep
Licenses        : MIT
Depends On      : gcc-libs

Repository      : extra
Name            : ripgrep-debug
Version         : 14.1.1-1
Description     : Detached debugging symbols for ripgrep
";

    #[test]
    fn parses_first_stanza_with_url_value_intact() {
        let cands = parse_si(SAMPLE, "pacman", TrustLevel::Official);
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(c.name, "ripgrep");
        assert_eq!(c.version.as_ref().unwrap().0, "14.1.1-1");
        assert_eq!(c.source_id, "pacman");
        assert_eq!(c.trust, TrustLevel::Official);
        assert_eq!(c.raw.get("repo").unwrap().as_str(), Some("extra"));
        assert!(c.summary.as_deref().unwrap().starts_with("A search tool"));
    }

    #[test]
    fn unknown_package_yields_no_candidate() {
        assert!(parse_si("", "pacman", TrustLevel::Official).is_empty());
        assert!(parse_si("error: package 'nope' was not found\n", "pacman", TrustLevel::Official).is_empty());
    }

    #[test]
    fn parses_query_name_and_version() {
        let recs = parse_query("ripgrep 14.1.1-1\nhtop 3.4.0-1\n", "pacman");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "ripgrep");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "14.1.1-1");
        assert_eq!(recs[1].name, "htop");
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
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, needs_root } => {
                assert!(needs_root, "pacman transactions need root");
                argv.clone()
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn batch_install_merges_into_one_root_pacman_command() {
        let cands = parse_si(SAMPLE, "pacman", TrustLevel::Official);
        let extra = PackageCandidate {
            name: "htop".into(),
            ..cands[0].clone()
        };
        let refs = vec![&cands[0], &extra];
        let plan = Pacman::new()
            .plan_install_many(&refs)
            .await
            .unwrap()
            .expect("pacman batches");
        assert_eq!(argv_of(&plan), &["pacman", "-S", "--noconfirm", "ripgrep", "htop"]);
    }

    #[tokio::test]
    async fn batch_remove_uses_recursive_flag() {
        let (a, b) = (rec("git"), rec("htop"));
        let refs = vec![&a, &b];
        let plan = Pacman::new()
            .plan_remove_many(&refs)
            .await
            .unwrap()
            .expect("pacman batches");
        assert_eq!(argv_of(&plan), &["pacman", "-Rs", "--noconfirm", "git", "htop"]);
    }
}
