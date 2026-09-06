//! Zypper provider (openSUSE, SLE).
//!
//! Self-gates on `zypper` (absent elsewhere → drops out; no distro check, ADR-0029).
//! Search uses zypper's **XML output** (`--xmlout`, its stable machine format) rather than
//! the human table; the installed list comes from `rpm` (openSUSE is rpm-based). Plans run
//! zypper non-interactively. The XML attribute extraction is a pure, unit-tested function —
//! and deliberately dependency-free (a couple of attributes off a known-shape tag need no
//! XML crate).

use async_trait::async_trait;
use serde_json::json;

use super::{Provider, command_plan, run_capture, run_capture_lax, which};
use crate::error::Result;
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel};

/// The zypper binary (drives search and plans).
const BIN: &str = "zypper";

/// This provider's stable source id.
const ID: &str = "zypper";

/// The Zypper installation source.
pub struct Zypper;

impl Zypper {
    pub fn new() -> Self {
        Zypper
    }
}

impl Default for Zypper {
    fn default() -> Self {
        Zypper::new()
    }
}

#[async_trait]
impl Provider for Zypper {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // openSUSE OSS repositories are signed and official.
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
        // `zypper --xmlout search --details --match-exact --type package <name>` emits a
        // `<solvable>` element per matching version. zypper exits 104 ("nothing found") for
        // an unknown package — that is "no candidate", not a source failure, so read laxly.
        let out = run_capture_lax(&[
            BIN,
            "--xmlout",
            "--non-interactive",
            "search",
            "--details",
            "--match-exact",
            "--type",
            "package",
            &query.raw,
        ])
        .await?;
        Ok(parse_search_xml(&out, self.id(), self.trust()))
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let repo = candidate
            .raw
            .get("repo")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let mut reasons = vec![
            crate::t!("reason.zypper_official"),
            crate::t!("reason.repository", repo = repo),
        ];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }
        Ok(root_plan(&candidate.name, &["install", &candidate.name], reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // One `zypper -n install a b c` for the whole group.
        let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
        let mut args = vec!["install"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.zypper_official_many", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.remove_one", name = record.name.clone(), mgr = "zypper")];
        Ok(root_plan(&record.name, &["remove", &record.name], reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["remove"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.remove_many", mgr = "zypper", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.update_one", name = record.name.clone(), mgr = "zypper")];
        Ok(root_plan(&record.name, &["update", &record.name], reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["update"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.update_many", mgr = "zypper", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `zypper --non-interactive update` (root_plan injects --non-interactive) updates
        // every installed package (D10).
        let reasons = vec![crate::t!("reason.zypper_upgrade_all")];
        Ok(Some(root_plan("system", &["update"], reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // openSUSE is rpm-based; rpm gives clean `name<TAB>version` output that the shared
        // installed-records parser already understands.
        let out = run_capture(&[
            "rpm",
            "-qa",
            "--qf",
            "%{NAME}\t%{VERSION}-%{RELEASE}\n",
        ])
        .await?;
        Ok(super::parse_installed_records(&out, self.id()))
    }
}

/// Build a single-command, root-requiring `zypper --non-interactive <args>` plan. The
/// `--non-interactive` global option auto-confirms the transaction (JII already showed and
/// confirmed the plan).
fn root_plan(name: &str, args: &[&str], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![BIN.to_string(), "--non-interactive".to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    command_plan(ID, name, argv, true, reasons)
}

/// Read a `key="value"` attribute out of a single XML tag's text. Dependency-free: zypper's
/// attribute values (names, editions, repo labels) contain no escaped quotes, so a scan to
/// the next `"` is correct and robust for this known-shape output.
fn attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("{key}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Parse `zypper --xmlout search` output into at most one candidate: the **first**
/// `<solvable>` element (the container is `<solvable-list>`, so we match `"<solvable "` with
/// the trailing space to skip it). `edition` is the version; `repository` is kept for the
/// plan's reason.
fn parse_search_xml(stdout: &str, source_id: &str, trust: TrustLevel) -> Vec<PackageCandidate> {
    let Some(start) = stdout.find("<solvable ") else {
        return Vec::new();
    };
    let rest = &stdout[start..];
    let end = rest.find('>').unwrap_or(rest.len());
    let tag = &rest[..end];

    let Some(name) = attr(tag, "name").map(str::trim).filter(|n| !n.is_empty()) else {
        return Vec::new();
    };
    let version = attr(tag, "edition").map(str::trim).filter(|v| !v.is_empty());
    let repo = attr(tag, "repository").map(str::trim).unwrap_or("");

    vec![PackageCandidate {
        name: name.to_string(),
        source_id: source_id.to_string(),
        version: version.map(PkgVersion::new),
        trust,
        arch_ok: true,
        signed: true,
        summary: None,
        popularity: None,
        suspicious: false,
        raw: json!({ "repo": repo }),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version='1.0'?>
<stream>
<search-result version="0.0">
<solvable-list>
<solvable status="not-installed" name="ripgrep" kind="package" edition="14.1.1-1.2" arch="x86_64" repository="Main Repository (OSS)"/>
<solvable status="not-installed" name="ripgrep" kind="package" edition="13.0.0-2.1" arch="x86_64" repository="Update"/>
</solvable-list>
</search-result>
</stream>
"#;

    #[test]
    fn parses_first_solvable_skipping_the_list_container() {
        let cands = parse_search_xml(SAMPLE, "zypper", TrustLevel::Official);
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(c.name, "ripgrep");
        assert_eq!(c.version.as_ref().unwrap().0, "14.1.1-1.2");
        assert_eq!(c.source_id, "zypper");
        assert_eq!(c.trust, TrustLevel::Official);
        assert_eq!(c.raw.get("repo").unwrap().as_str(), Some("Main Repository (OSS)"));
    }

    #[test]
    fn unknown_package_yields_no_candidate() {
        let empty = "<?xml version='1.0'?>\n<stream>\n<search-result version=\"0.0\">\n<solvable-list>\n</solvable-list>\n</search-result>\n</stream>\n";
        assert!(parse_search_xml(empty, "zypper", TrustLevel::Official).is_empty());
        assert!(parse_search_xml("", "zypper", TrustLevel::Official).is_empty());
    }

    #[test]
    fn attr_reads_value_up_to_next_quote() {
        let tag = r#"<solvable name="foo" edition="1.2-3" repository="repo x">"#;
        assert_eq!(attr(tag, "name"), Some("foo"));
        assert_eq!(attr(tag, "edition"), Some("1.2-3"));
        assert_eq!(attr(tag, "repository"), Some("repo x"));
        assert_eq!(attr(tag, "missing"), None);
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
                assert!(needs_root, "zypper transactions need root");
                argv.clone()
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn batch_install_is_one_non_interactive_root_command() {
        let cands = parse_search_xml(SAMPLE, "zypper", TrustLevel::Official);
        let extra = PackageCandidate { name: "htop".into(), ..cands[0].clone() };
        let refs = vec![&cands[0], &extra];
        let plan = Zypper::new()
            .plan_install_many(&refs)
            .await
            .unwrap()
            .expect("zypper batches");
        assert_eq!(
            argv_of(&plan),
            &["zypper", "--non-interactive", "install", "ripgrep", "htop"]
        );
    }

    #[tokio::test]
    async fn batch_update_uses_update_verb() {
        let (a, b) = (rec("git"), rec("htop"));
        let refs = vec![&a, &b];
        let plan = Zypper::new()
            .plan_update_many(&refs)
            .await
            .unwrap()
            .expect("zypper batches");
        assert_eq!(
            argv_of(&plan),
            &["zypper", "--non-interactive", "update", "git", "htop"]
        );
    }
}
