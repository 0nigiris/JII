//! DNF provider (Fedora, dnf5).
//!
//! Uses `dnf5 repoquery` with an explicit `--queryformat` so we parse stable,
//! machine-readable output instead of human text. All parsing is done by pure
//! functions (`parse_candidates`, `parse_installed`) so they can be unit-tested on
//! fixed samples.

use async_trait::async_trait;
use serde_json::json;

use super::{Provider, command_plan, nonempty_lines, run_capture, which};
use crate::error::Result;
use crate::model::{
    InstallPlan, InstalledRecord, PackageCandidate, PackageInfo, PkgVersion, Query, TrustLevel,
};

/// Field separator embedded in the `--queryformat`. A real tab is sent to dnf5
/// (Rust interprets the escape); dnf5 does not itself expand `\t`.
const SEP: char = '\t';

/// The dnf5 binary name.
const BIN: &str = "dnf5";

/// This provider's stable source id.
const ID: &str = "dnf";

/// The DNF installation source.
pub struct Dnf;

impl Dnf {
    pub fn new() -> Self {
        Dnf
    }
}

impl Default for Dnf {
    fn default() -> Self {
        Dnf::new()
    }
}

#[async_trait]
impl Provider for Dnf {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // Fedora repositories are official and signed.
        TrustLevel::Official
    }

    fn highlights(&self, candidate: &PackageCandidate) -> Vec<String> {
        let mut h = vec![crate::t!("reason.dnf_official"), crate::t!("reason.dnf_native")];
        if let Some(repo) = candidate.raw.get("repoid").and_then(|v| v.as_str())
            && !repo.is_empty()
        {
            h.push(crate::t!("reason.repository", repo = repo));
        }
        h
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    async fn describe(&self, candidate: &PackageCandidate) -> Option<PackageInfo> {
        // `jii info` is an explicit "tell me more", so one extra `dnf5 info` call is fine
        // (never on the hot search path). Parsed by the pure, tested `parse_info`.
        let out = run_capture(&[BIN, "info", &candidate.name]).await.ok()?;
        let fields = parse_info(&out);
        let get = |k: &str| fields.get(k).cloned().filter(|s| !s.is_empty());
        let info = PackageInfo {
            description: get("Description").or_else(|| get("Summary")),
            homepage: get("URL"),
            repository: None,
            license: get("License"),
            author: get("Vendor"),
        };
        (!info.is_empty()).then_some(info)
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // Latest-version, available packages. Empty output = no match (dnf5 exits 0 even
        // when nothing matches). By default the term is an exact name; on the engine's
        // broaden-on-miss step (ADR-0042) it becomes a `<term>*` prefix glob, so
        // `ayugram` can resolve to `ayugram-desktop` — dnf5 repoquery matches names by
        // shell glob. (A `*term*` substring is deliberately avoided: `*git*` matches ~1300
        // packages; a prefix keeps the fallback focused.)
        let pattern = match query.match_mode {
            crate::model::MatchMode::Exact => query.raw.clone(),
            crate::model::MatchMode::Prefix => format!("{}*", query.raw),
        };
        let qf = format!("%{{name}}{SEP}%{{evr}}{SEP}%{{repoid}}{SEP}%{{summary}}\n");
        let out = run_capture(&[
            BIN,
            "repoquery",
            "-q",
            "--available",
            "--latest-limit=1",
            &pattern,
            "--qf",
            &qf,
        ])
        .await?;
        Ok(parse_candidates(&out, self.id(), self.trust()))
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let repoid = candidate
            .raw
            .get("repoid")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let mut reasons = vec![crate::t!("reason.dnf_official")];
        reasons.push(crate::t!("reason.repository", repo = repoid));
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }

        Ok(root_plan(&candidate.name, &["install", "-y", &candidate.name], reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // One `dnf5 install -y a b c` for the whole group (dnf resolves them together).
        let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
        let mut args = vec!["install", "-y"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.dnf_official_many", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    // plan_remove / plan_update / list_installed are part of the Provider contract,
    // implemented now and exercised from Phase 2+ (remove, update, list).
    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.remove_one", name = record.name.clone(), mgr = "dnf")];
        Ok(root_plan(&record.name, &["remove", "-y", &record.name], reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // One `dnf5 remove -y a b c` for the whole group (one escalation, one transaction).
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["remove", "-y"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.remove_many", mgr = "dnf", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.update_one", name = record.name.clone(), mgr = "dnf")];
        Ok(root_plan(&record.name, &["upgrade", "-y", &record.name], reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // One `dnf5 upgrade -y a b c` for the whole group.
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["upgrade", "-y"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.update_many", mgr = "dnf", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `dnf5 upgrade -y` with no names = upgrade every installed package (D10).
        let reasons = vec![crate::t!("reason.upgrade_all_system", mgr = "dnf")];
        Ok(Some(root_plan("system", &["upgrade", "-y"], reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        let qf = format!("%{{name}}{SEP}%{{evr}}\n");
        let out = run_capture(&[BIN, "repoquery", "-q", "--installed", "--qf", &qf]).await?;
        Ok(super::parse_installed_records(&out, self.id()))
    }
}

/// Build a single-command, root-requiring `dnf5 <args>` plan (the plan shape itself is
/// the shared [`command_plan`]; `dnf5` needs root).
fn root_plan(name: &str, args: &[&str], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![BIN.to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    command_plan(ID, name, argv, true, reasons)
}

/// Parse `repoquery` search output into candidates. Lines are
/// `name<TAB>evr<TAB>repoid<TAB>summary`; malformed lines are skipped.
fn parse_candidates(stdout: &str, source_id: &str, trust: TrustLevel) -> Vec<PackageCandidate> {
    nonempty_lines(stdout)
        .filter_map(|line| {
            let mut fields = line.splitn(4, SEP);
            let name = fields.next()?.trim();
            if name.is_empty() {
                return None;
            }
            let evr = fields.next().unwrap_or("").trim();
            let repoid = fields.next().unwrap_or("").trim();
            let summary = fields.next().unwrap_or("").trim();

            Some(PackageCandidate {
                name: name.to_string(),
                source_id: source_id.to_string(),
                version: (!evr.is_empty()).then(|| PkgVersion::new(evr)),
                trust,
                arch_ok: true,
                signed: true,
                summary: (!summary.is_empty()).then(|| summary.to_string()),
                raw: json!({ "repoid": repoid }),
            })
        })
        .collect()
}

/// Parse `dnf5 info <pkg>` (`Key : Value`, with `      : …` continuation lines) into a
/// field map, keeping the **first** stanza's value for each key (dnf lists the installed
/// copy before available ones; the first is what the card shows). Pure and unit-tested.
fn parse_info(stdout: &str) -> std::collections::HashMap<String, String> {
    let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut last_key: Option<String> = None;
    for line in stdout.lines() {
        let Some((left, right)) = line.split_once(':') else {
            last_key = None;
            continue;
        };
        let key = left.trim();
        let val = right.trim();
        if key.is_empty() {
            // Continuation of the previous field (folded description, etc.).
            if let Some(k) = &last_key
                && let Some(existing) = fields.get_mut(k)
            {
                existing.push(' ');
                existing.push_str(val);
            }
        } else if !fields.contains_key(key) {
            fields.insert(key.to_string(), val.to_string());
            last_key = Some(key.to_string());
        } else {
            // A repeated key = a later stanza; keep the first and stop folding into it.
            last_key = None;
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_info_takes_first_stanza_and_folds_continuations() {
        let sample = "\
Installed packages
Name            : firefox
Summary         : Mozilla Firefox Web browser
URL             : https://www.mozilla.org/firefox/
License         : MPL-2.0
Description     : Mozilla Firefox is an open-source web browser, designed for standards
                : compliance, performance and portability.
Vendor          : Fedora Project

Available packages
Name            : firefox
License         : SOMETHING-ELSE
";
        let f = parse_info(sample);
        assert_eq!(f.get("URL").unwrap(), "https://www.mozilla.org/firefox/");
        assert_eq!(f.get("Vendor").unwrap(), "Fedora Project");
        // The folded continuation line joins onto the description.
        assert_eq!(
            f.get("Description").unwrap(),
            "Mozilla Firefox is an open-source web browser, designed for standards compliance, performance and portability."
        );
        // The first stanza wins over the later "Available packages" one.
        assert_eq!(f.get("License").unwrap(), "MPL-2.0");
    }

    #[test]
    fn parses_search_line() {
        let sample = "fastfetch\t2.63.1-1.fc44\tupdates\tFast neofetch-like system information tool\n";
        let cands = parse_candidates(sample, "dnf", TrustLevel::Official);
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(c.name, "fastfetch");
        assert_eq!(c.version.as_ref().unwrap().0, "2.63.1-1.fc44");
        assert_eq!(c.source_id, "dnf");
        assert_eq!(c.trust, TrustLevel::Official);
        assert_eq!(c.summary.as_deref(), Some("Fast neofetch-like system information tool"));
        assert_eq!(c.raw.get("repoid").unwrap().as_str(), Some("updates"));
    }

    #[test]
    fn empty_output_yields_no_candidates() {
        assert!(parse_candidates("", "dnf", TrustLevel::Official).is_empty());
        assert!(parse_candidates("\n  \n", "dnf", TrustLevel::Official).is_empty());
    }

    #[test]
    fn summary_may_contain_tabs_but_is_preserved_as_remainder() {
        // splitn(4) keeps everything after the 3rd tab as the summary.
        let sample = "pkg\t1.0-1\trepo\ta summary\twith tab\n";
        let cands = parse_candidates(sample, "dnf", TrustLevel::Official);
        assert_eq!(cands[0].summary.as_deref(), Some("a summary\twith tab"));
    }

    #[test]
    fn missing_fields_are_tolerated() {
        let cands = parse_candidates("lonely\n", "dnf", TrustLevel::Official);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].name, "lonely");
        assert!(cands[0].version.is_none());
        assert!(cands[0].summary.is_none());
    }

    #[tokio::test]
    async fn batch_merges_into_one_root_dnf_command() {
        let cands = parse_candidates(
            "git\t2.55\trepo\tvcs\nhtop\t3.4\trepo\tviewer\n",
            "dnf",
            TrustLevel::Official,
        );
        let refs: Vec<&PackageCandidate> = cands.iter().collect();
        let plan = Dnf::new()
            .plan_install_many(&refs)
            .await
            .unwrap()
            .expect("dnf batches");
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.needs_root());
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, needs_root } => {
                assert_eq!(argv, &["dnf5", "install", "-y", "git", "htop"]);
                assert!(needs_root);
            }
            other => panic!("expected run, got {other:?}"),
        }
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

    #[tokio::test]
    async fn batch_remove_merges_into_one_root_dnf_command() {
        let (a, b) = (rec("git"), rec("htop"));
        let refs = vec![&a, &b];
        let plan = Dnf::new()
            .plan_remove_many(&refs)
            .await
            .unwrap()
            .expect("dnf batches");
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.needs_root());
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["dnf5", "remove", "-y", "git", "htop"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn batch_update_merges_into_one_root_dnf_command() {
        let (a, b) = (rec("git"), rec("htop"));
        let refs = vec![&a, &b];
        let plan = Dnf::new()
            .plan_update_many(&refs)
            .await
            .unwrap()
            .expect("dnf batches");
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["dnf5", "upgrade", "-y", "git", "htop"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn update_all_upgrades_the_whole_system_as_root() {
        let plan = Dnf::new()
            .plan_update_all()
            .await
            .unwrap()
            .expect("dnf offers a system update");
        assert!(plan.needs_root());
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["dnf5", "upgrade", "-y"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }
}
