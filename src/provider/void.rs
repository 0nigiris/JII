//! Void Linux provider (the XBPS package manager).
//!
//! Self-gates on `xbps-install` (absent elsewhere → drops out; no distro check, ADR-0029).
//! Covers Void's **official** signed repositories. Search reads an exact-name property
//! stanza (`xbps-query -R <name>`, the analogue of `pacman -Si`); plans run `xbps-install`/
//! `xbps-remove`; the installed list comes from `xbps-query -l`. Parsing is pure and
//! unit-tested. XBPS transactions need root (system packages), like apt/pacman/zypper.

use async_trait::async_trait;
use serde_json::json;

use super::{Provider, command_plan, nonempty_lines, run_capture, run_capture_lax, which};
use crate::error::Result;
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel};

/// The install binary — also what proves XBPS is present.
const BIN: &str = "xbps-install";

/// This provider's stable source id.
const ID: &str = "void";

/// The Void Linux (XBPS) installation source.
pub struct Void;

impl Void {
    pub fn new() -> Self {
        Void
    }
}

impl Default for Void {
    fn default() -> Self {
        Void::new()
    }
}

#[async_trait]
impl Provider for Void {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // Void's official repositories are RSA-signed.
        TrustLevel::Official
    }

    /// The distro's own packaging — the default answer, and the one that keeps working.
    fn nature(&self) -> super::SourceNature {
        super::SourceNature::SystemNative
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // `xbps-query -R <name>` prints the property stanza for an *exact* package name from
        // the remote repodata, and exits non-zero for an unknown package — which is "no
        // candidate", not a source failure — so read stdout laxly (empty → no match).
        let out = run_capture_lax(&["xbps-query", "-R", query.raw.trim()]).await?;
        Ok(parse_show(&out, query.raw.trim(), self.id(), self.trust()))
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let mut reasons = vec![crate::t!("reason.void_official")];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }
        // -S syncs the repodata, -y assumes yes (JII already confirmed the plan).
        Ok(root_plan(&candidate.name, &["-Sy", &candidate.name], reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // One `xbps-install -Sy a b c` for the whole group.
        let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
        let mut args = vec!["-Sy"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.void_official_many", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // -R also removes dependencies no longer required by anything else (the closest
        // analogue to dnf/apt cleanup); the full command is shown before it runs.
        let reasons = vec![crate::t!("reason.remove_one", name = record.name.clone(), mgr = "xbps")];
        Ok(remove_plan(&record.name, &["-Ry", &record.name], reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["-Ry"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.remove_many", mgr = "xbps", names = names.join(", "))];
        Ok(Some(remove_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // `xbps-install -Suy <name>` syncs then updates just that package to the newest
        // build in the repos (a full `-Su` is the user's own call — JII updates the
        // packages it tracks, one target at a time).
        let reasons = vec![crate::t!("reason.update_one", name = record.name.clone(), mgr = "xbps")];
        Ok(root_plan(&record.name, &["-Suy", &record.name], reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["-Suy"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.update_many", mgr = "xbps", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `xbps-install -Suy` (no package args) = sync the repodata and upgrade the whole
        // system (D10).
        let reasons = vec![crate::t!("reason.upgrade_all_system", mgr = "xbps")];
        Ok(Some(root_plan("system", &["-Suy"], reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // `xbps-query -l` prints `<state> <name-version_revision>  <desc>` per installed pkg.
        let out = run_capture(&["xbps-query", "-l"]).await?;
        Ok(parse_query_list(&out, self.id()))
    }
}

/// Build a single-command, root-requiring `xbps-install <args>` plan.
fn root_plan(name: &str, args: &[&str], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![BIN.to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    command_plan(ID, name, argv, true, reasons)
}

/// Build a single-command, root-requiring `xbps-remove <args>` plan.
fn remove_plan(name: &str, args: &[&str], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec!["xbps-remove".to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    command_plan(ID, name, argv, true, reasons)
}

/// Split an XBPS `pkgver` (`name-version_revision`) into its name and display version.
/// The version always follows the **final** hyphen and carries the `_revision` suffix, so
/// the last hyphen is the split point (`python3-requests-2.28_1` → `python3-requests`,
/// `2.28`). The `_revision` is dropped for a clean display version.
fn split_pkgver(pkgver: &str) -> (String, Option<String>) {
    match pkgver.rsplit_once('-') {
        Some((name, ver_rev)) if !name.is_empty() => {
            let version = ver_rev.split_once('_').map_or(ver_rev, |(v, _)| v);
            let version = (!version.is_empty()).then(|| version.to_string());
            (name.to_string(), version)
        }
        _ => (pkgver.to_string(), None),
    }
}

/// Parse `xbps-query -R <name>` output — a single package's `key: value` property stanza —
/// into at most one candidate. The first `:` is the separator, so values containing `:`
/// (homepage URLs) survive. Only emit when `pkgname` matches the query **exactly**, so a
/// pattern that resolved to a near-name never installs the wrong package. Empty output
/// (unknown package) → no candidate.
fn parse_show(stdout: &str, query: &str, source_id: &str, trust: TrustLevel) -> Vec<PackageCandidate> {
    let mut pkgname: Option<String> = None;
    let mut pkgver: Option<String> = None;
    let mut summary: Option<String> = None;
    let mut repo: Option<String> = None;

    for line in stdout.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "pkgname" => pkgname = Some(value.to_string()),
            "pkgver" => pkgver = Some(value.to_string()),
            "short_desc" if !value.is_empty() => summary = Some(value.to_string()),
            "repository" if !value.is_empty() => repo = Some(value.to_string()),
            _ => {}
        }
    }

    let Some(name) = pkgname.filter(|n| n == query) else {
        return Vec::new();
    };
    let version = pkgver.and_then(|pv| split_pkgver(&pv).1).map(|v| PkgVersion::new(&v));
    vec![PackageCandidate {
        name,
        source_id: source_id.to_string(),
        version,
        trust,
        arch_ok: true,
        signed: true,
        summary,
        popularity: None,
        suspicious: false,
        raw: json!({ "repo": repo.unwrap_or_default() }),
    }]
}

/// Parse `xbps-query -l` output into installed records. Each line is
/// `<state> <name-version_revision>  <short desc>` (state is a 2-char flag like `ii`).
fn parse_query_list(stdout: &str, source_id: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    nonempty_lines(stdout)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let _state = parts.next()?;
            let pkgver = parts.next()?;
            let (name, version) = split_pkgver(pkgver);
            Some(InstalledRecord {
                name,
                source_id: source_id.to_string(),
                version: version.map(|v| PkgVersion::new(&v)),
                installed_at: now,
                verification: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // `xbps-query -R ripgrep` — a single package's property stanza.
    const SHOW: &str = "architecture: x86_64
filename-size: 1310720
homepage: https://github.com/BurntSushi/ripgrep
install-date: n/a
pkgname: ripgrep
pkgver: ripgrep-14.1.1_1
repository: https://repo-default.voidlinux.org/current
short_desc: Line-oriented search tool that recursively searches directories
state: uninstalled
";

    #[test]
    fn parses_exact_stanza() {
        let cands = parse_show(SHOW, "ripgrep", "void", TrustLevel::Official);
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(c.name, "ripgrep");
        assert_eq!(c.version.as_ref().unwrap().0, "14.1.1");
        assert_eq!(c.source_id, "void");
        assert_eq!(c.trust, TrustLevel::Official);
        assert!(c.signed);
        assert_eq!(c.raw.get("repo").unwrap().as_str(), Some("https://repo-default.voidlinux.org/current"));
        assert!(c.summary.as_deref().unwrap().starts_with("Line-oriented"));
    }

    #[test]
    fn near_name_pattern_does_not_install_wrong_package() {
        // A stanza whose pkgname isn't the queried name yields nothing.
        assert!(parse_show(SHOW, "ripgrep-all", "void", TrustLevel::Official).is_empty());
    }

    #[test]
    fn unknown_package_yields_no_candidate() {
        assert!(parse_show("", "nope", "void", TrustLevel::Official).is_empty());
    }

    #[test]
    fn splits_hyphenated_pkgver() {
        assert_eq!(split_pkgver("ripgrep-14.1.1_1"), ("ripgrep".into(), Some("14.1.1".into())));
        assert_eq!(split_pkgver("xbps-0.59_1"), ("xbps".into(), Some("0.59".into())));
        assert_eq!(
            split_pkgver("python3-requests-2.28.0_2"),
            ("python3-requests".into(), Some("2.28.0".into()))
        );
    }

    #[test]
    fn parses_installed_list() {
        let sample = "ii ripgrep-14.1.1_1     Line-oriented search tool\nii htop-3.4.0_1         Interactive process viewer\n";
        let recs = parse_query_list(sample, "void");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "ripgrep");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "14.1.1");
        assert_eq!(recs[0].source_id, "void");
        assert_eq!(recs[1].name, "htop");
        assert_eq!(recs[1].version.as_ref().unwrap().0, "3.4.0");
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
                assert!(needs_root, "xbps transactions need root");
                argv.clone()
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn install_plan_is_one_root_sync_install() {
        let c = parse_show(SHOW, "ripgrep", "void", TrustLevel::Official).remove(0);
        let plan = Void::new().plan_install(&c).await.unwrap();
        assert_eq!(argv_of(&plan), &["xbps-install", "-Sy", "ripgrep"]);
    }

    #[tokio::test]
    async fn batch_install_merges_into_one_root_command() {
        let a = parse_show(SHOW, "ripgrep", "void", TrustLevel::Official).remove(0);
        let b = PackageCandidate { name: "htop".into(), ..a.clone() };
        let plan = Void::new()
            .plan_install_many(&[&a, &b])
            .await
            .unwrap()
            .expect("void batches");
        assert_eq!(argv_of(&plan), &["xbps-install", "-Sy", "ripgrep", "htop"]);
    }

    #[tokio::test]
    async fn batch_remove_uses_recursive_flag() {
        let (a, b) = (rec("git"), rec("htop"));
        let plan = Void::new()
            .plan_remove_many(&[&a, &b])
            .await
            .unwrap()
            .expect("void batches");
        assert_eq!(argv_of(&plan), &["xbps-remove", "-Ry", "git", "htop"]);
    }

    #[tokio::test]
    async fn update_all_syncs_and_upgrades_everything() {
        let plan = Void::new().plan_update_all().await.unwrap().expect("void bulk-updates");
        assert_eq!(argv_of(&plan), &["xbps-install", "-Suy"]);
    }
}
