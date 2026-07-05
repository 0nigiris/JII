//! APT provider (Debian, Ubuntu and derivatives).
//!
//! Self-gates on `apt-get` (absent on Fedora/Arch → this provider simply drops out),
//! so no distro check is needed anywhere (ADR-0029). Search reads the machine-readable
//! deb822 stanzas from `apt-cache show`; plans run `apt-get`; the installed list comes
//! from `dpkg-query`. Parsing is a pure function (`parse_show`) unit-tested on fixed
//! samples, per the "isolate parsers" rule.

use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::{Provider, command_plan, run_capture, which};
use crate::error::{JiiError, Result};
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel};

/// The apt binary that runs (privileged) transactions. `apt-get` has a stable scripting
/// interface (unlike `apt`, which warns it has none), so plans and availability use it.
const BIN: &str = "apt-get";

/// This provider's stable source id.
const ID: &str = "apt";

/// The APT installation source.
pub struct Apt;

impl Apt {
    pub fn new() -> Self {
        Apt
    }
}

impl Default for Apt {
    fn default() -> Self {
        Apt::new()
    }
}

#[async_trait]
impl Provider for Apt {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // Debian/Ubuntu archives are GPG-signed and official.
        TrustLevel::Official
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // `apt-cache show <name>` prints one deb822 stanza per available version (exact
        // name only). It exits non-zero for an unknown package — that is "no candidate",
        // not a source failure, so we read stdout laxly (empty → no match) rather than
        // erroring the whole search the way `run_capture` would.
        let out = run_lax(&["apt-cache", "show", &query.raw]).await?;
        Ok(parse_show(&out, self.id(), self.trust()))
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let mut reasons = vec!["System package (apt)".to_string()];
        if let Some(v) = &candidate.version {
            reasons.push(format!("Version {v}"));
        }
        Ok(root_plan(&candidate.name, &["install", "-y", &candidate.name], reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // One `apt-get install -y a b c` for the whole group (apt resolves them together).
        let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
        let mut args = vec!["install", "-y"];
        args.extend_from_slice(&names);
        let reasons = vec![format!("System packages (apt): {}", names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![format!("Remove {} (installed via apt)", record.name)];
        Ok(root_plan(&record.name, &["remove", "-y", &record.name], reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // One `apt-get remove -y a b c` (one escalation, one transaction).
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["remove", "-y"];
        args.extend_from_slice(&names);
        let reasons = vec![format!("Remove (via apt): {}", names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // apt upgrades a single package by installing its newest version in place.
        let reasons = vec![format!("Update {} via apt", record.name)];
        Ok(root_plan(
            &record.name,
            &["install", "--only-upgrade", "-y", &record.name],
            reasons,
        ))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        // One `apt-get install --only-upgrade -y a b c` for the whole group.
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["install", "--only-upgrade", "-y"];
        args.extend_from_slice(&names);
        let reasons = vec![format!("Update (via apt): {}", names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // dpkg-query emits `name<TAB>version` lines — exactly the shape the shared
        // installed-records parser expects.
        let out = run_capture(&["dpkg-query", "-W", "-f=${Package}\t${Version}\n"]).await?;
        Ok(super::parse_installed_records(&out, self.id()))
    }
}

/// Build a single-command, root-requiring `apt-get <args>` plan.
fn root_plan(name: &str, args: &[&str], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![BIN.to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    command_plan(ID, name, argv, true, reasons)
}

/// Run a command, returning stdout even on a non-zero exit. `apt-cache show` exits 100 for
/// an unknown package (writing to stderr); to JII that is simply "no candidate", so stderr
/// is ignored and empty stdout means no match. A spawn failure (apt-cache absent) still
/// errors. Local to apt for now; extract to a shared `run_lax` once a second system
/// provider needs the same lenient read (pacman/zypper — the 3× rule).
async fn run_lax(argv: &[&str]) -> Result<String> {
    let output = Command::new(argv[0])
        .args(&argv[1..])
        .output()
        .await
        .map_err(|e| JiiError::spawn(argv[0], e))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse `apt-cache show` output into at most one candidate: the **first stanza** (apt
/// lists the highest version first) of the deb822 record. Stanzas are separated by a blank
/// line; folded description bodies (continuation lines start with a space) are skipped, so
/// only the first `Description` line becomes the summary. `Description-md5` is a checksum,
/// not text, so it is excluded.
fn parse_show(stdout: &str, source_id: &str, trust: TrustLevel) -> Vec<PackageCandidate> {
    let stanza = stdout.split("\n\n").next().unwrap_or("");

    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut summary: Option<String> = None;

    for line in stanza.lines() {
        // Continuation line of a folded field (e.g. the description body) — not a key.
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key == "Package" {
            name = Some(value.to_string());
        } else if key == "Version" {
            version = Some(value.to_string());
        } else if summary.is_none()
            && !value.is_empty()
            && (key == "Description"
                || (key.starts_with("Description-") && key != "Description-md5"))
        {
            summary = Some(value.to_string());
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
        raw: json!({}),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Package: ripgrep
Version: 13.0.0-4build1
Priority: optional
Section: rust
Installed-Size: 4977
Description-md5: 9f2b1e5cabc0011a3f0d2b7c9c0f1234
Description: Recursively search directories for a regex pattern
 ripgrep (rg) is a line-oriented search tool that recursively searches
 the current directory for a regex pattern.

Package: ripgrep
Version: 12.1.1-1
Description: an older version
";

    #[test]
    fn parses_first_stanza() {
        let cands = parse_show(SAMPLE, "apt", TrustLevel::Official);
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(c.name, "ripgrep");
        // First stanza wins (apt lists the highest version first).
        assert_eq!(c.version.as_ref().unwrap().0, "13.0.0-4build1");
        assert_eq!(c.source_id, "apt");
        assert_eq!(c.trust, TrustLevel::Official);
        assert_eq!(
            c.summary.as_deref(),
            Some("Recursively search directories for a regex pattern")
        );
    }

    #[test]
    fn ignores_description_md5_and_folded_body() {
        // The md5 checksum must not be mistaken for the description, and the folded
        // continuation lines must not leak into the summary.
        let cands = parse_show(SAMPLE, "apt", TrustLevel::Official);
        let summary = cands[0].summary.as_deref().unwrap();
        assert!(!summary.contains("ripgrep (rg)"));
        assert!(!summary.chars().all(|ch| ch.is_ascii_hexdigit() || ch == ' '));
    }

    #[test]
    fn unknown_package_yields_no_candidate() {
        assert!(parse_show("", "apt", TrustLevel::Official).is_empty());
        assert!(parse_show("\n\n", "apt", TrustLevel::Official).is_empty());
    }

    #[test]
    fn version_optional() {
        let cands = parse_show("Package: lonely\n", "apt", TrustLevel::Official);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].name, "lonely");
        assert!(cands[0].version.is_none());
        assert!(cands[0].summary.is_none());
    }

    fn cand(name: &str) -> PackageCandidate {
        PackageCandidate {
            name: name.to_string(),
            source_id: ID.to_string(),
            version: None,
            trust: TrustLevel::Official,
            arch_ok: true,
            signed: true,
            summary: None,
            raw: json!({}),
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

    fn argv_of(plan: &InstallPlan) -> Vec<String> {
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, needs_root } => {
                assert!(needs_root, "apt transactions need root");
                argv.clone()
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn batch_install_merges_into_one_root_apt_command() {
        let (a, b) = (cand("git"), cand("htop"));
        let refs = vec![&a, &b];
        let plan = Apt::new()
            .plan_install_many(&refs)
            .await
            .unwrap()
            .expect("apt batches");
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(argv_of(&plan), &["apt-get", "install", "-y", "git", "htop"]);
    }

    #[tokio::test]
    async fn batch_remove_merges_into_one_root_apt_command() {
        let (a, b) = (rec("git"), rec("htop"));
        let refs = vec![&a, &b];
        let plan = Apt::new()
            .plan_remove_many(&refs)
            .await
            .unwrap()
            .expect("apt batches");
        assert_eq!(argv_of(&plan), &["apt-get", "remove", "-y", "git", "htop"]);
    }

    #[tokio::test]
    async fn batch_update_uses_only_upgrade() {
        let (a, b) = (rec("git"), rec("htop"));
        let refs = vec![&a, &b];
        let plan = Apt::new()
            .plan_update_many(&refs)
            .await
            .unwrap()
            .expect("apt batches");
        assert_eq!(
            argv_of(&plan),
            &["apt-get", "install", "--only-upgrade", "-y", "git", "htop"]
        );
    }

    #[tokio::test]
    async fn single_update_upgrades_in_place() {
        let plan = Apt::new().plan_update(&rec("git")).await.unwrap();
        assert_eq!(
            argv_of(&plan),
            &["apt-get", "install", "--only-upgrade", "-y", "git"]
        );
    }
}
