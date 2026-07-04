//! DNF provider (Fedora, dnf5).
//!
//! Uses `dnf5 repoquery` with an explicit `--queryformat` so we parse stable,
//! machine-readable output instead of human text. All parsing is done by pure
//! functions (`parse_candidates`, `parse_installed`) so they can be unit-tested on
//! fixed samples.

use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;

use super::Provider;
use crate::error::{JiiError, Result};
use crate::model::{
    InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, Step, TrustLevel,
};

/// Field separator embedded in the `--queryformat`. A real tab is sent to dnf5
/// (Rust interprets the escape); dnf5 does not itself expand `\t`.
const SEP: char = '\t';

/// The dnf5 binary name.
const BIN: &str = "dnf5";

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
        "dnf"
    }

    fn trust(&self) -> TrustLevel {
        // Fedora repositories are official and signed.
        TrustLevel::Official
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // Exact-name, latest-version, available packages. Empty output = no match
        // (dnf5 exits 0 even when nothing matches).
        let qf = format!("%{{name}}{SEP}%{{evr}}{SEP}%{{repoid}}{SEP}%{{summary}}\n");
        let out = run_capture(&[
            BIN,
            "repoquery",
            "-q",
            "--available",
            "--latest-limit=1",
            &query.raw,
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

        let mut reasons = vec!["Official Fedora package".to_string()];
        reasons.push(format!("Repository: {repoid}"));
        if let Some(v) = &candidate.version {
            reasons.push(format!("Version {v}"));
        }

        Ok(InstallPlan {
            candidate_ref: candidate.name.clone(),
            source_id: self.id().to_string(),
            steps: vec![Step {
                argv: vec![
                    BIN.to_string(),
                    "install".to_string(),
                    "-y".to_string(),
                    candidate.name.clone(),
                ],
                needs_root: true,
                cwd: None,
            }],
            verification: Vec::new(),
            download_size: None,
            needs_root: true,
            reasons,
        })
    }

    // plan_remove / plan_update / list_installed are part of the Provider contract,
    // implemented now and exercised from Phase 2+ (remove, update, list).
    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        Ok(single_step_plan(
            self.id(),
            &record.name,
            &["remove", "-y", &record.name],
            vec![format!("Remove {} (installed via dnf)", record.name)],
        ))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        Ok(single_step_plan(
            self.id(),
            &record.name,
            &["upgrade", "-y", &record.name],
            vec![format!("Update {} via dnf", record.name)],
        ))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        let qf = format!("%{{name}}{SEP}%{{evr}}\n");
        let out = run_capture(&[BIN, "repoquery", "-q", "--installed", "--qf", &qf]).await?;
        Ok(parse_installed(&out, self.id()))
    }
}

/// Build a simple one-command root plan (used for remove/update in Phase 2).
#[allow(dead_code)]
fn single_step_plan(source: &str, name: &str, args: &[&str], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![BIN.to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    InstallPlan {
        candidate_ref: name.to_string(),
        source_id: source.to_string(),
        steps: vec![Step {
            argv,
            needs_root: true,
            cwd: None,
        }],
        verification: Vec::new(),
        download_size: None,
        needs_root: true,
        reasons,
    }
}

/// Parse `repoquery` search output into candidates. Lines are
/// `name<TAB>evr<TAB>repoid<TAB>summary`; malformed lines are skipped.
fn parse_candidates(stdout: &str, source_id: &str, trust: TrustLevel) -> Vec<PackageCandidate> {
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
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

/// Parse `repoquery --installed` output into records. Lines are `name<TAB>evr`.
/// Wired into `list_installed` and used by registry verification in Phase 2.
#[allow(dead_code)]
fn parse_installed(stdout: &str, source_id: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut fields = line.splitn(2, SEP);
            let name = fields.next()?.trim();
            if name.is_empty() {
                return None;
            }
            let evr = fields.next().unwrap_or("").trim();
            Some(InstalledRecord {
                name: name.to_string(),
                source_id: source_id.to_string(),
                version: (!evr.is_empty()).then(|| PkgVersion::new(evr)),
                installed_at: now,
            })
        })
        .collect()
}

/// Run a command and return its stdout as a string. Errors if the binary cannot be
/// spawned or exits non-zero.
async fn run_capture(argv: &[&str]) -> Result<String> {
    let output = Command::new(argv[0])
        .args(&argv[1..])
        .output()
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("failed to run {}: {e}", argv[0])))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(JiiError::Other(anyhow::anyhow!(
            "{} failed: {}",
            argv.join(" "),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether an executable is found on PATH.
async fn which(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn parses_installed_lines() {
        let sample = "bash\t5.3.9-3.fc44\nfastfetch\t2.63.1-1.fc44\n";
        let recs = parse_installed(sample, "dnf");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "bash");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "5.3.9-3.fc44");
        assert_eq!(recs[1].name, "fastfetch");
    }
}
