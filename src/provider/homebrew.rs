//! Homebrew provider (Linuxbrew — `brew`, the Homebrew formula ecosystem on Linux).
//!
//! `brew install <formula>` installs into Homebrew's own prefix (`/home/linuxbrew`
//! or `~/.linuxbrew`) — a **user-space** install, no root (like cargo/npm/pipx/go).
//! Search resolves an exact formula via the Homebrew formulae API. Homebrew on Linux
//! is **formula-only** (casks are macOS GUI apps), so we query the formula endpoint.
//!
//! Like pipx/go (ADR-0023), the formula API exposes no reliable program-vs-library
//! signal, and virtually every formula installs an executable, so JII does **not**
//! pre-filter — it offers the formula and lets `brew install` be the authority.
//!
//! All network access is in `search`; the plans are pure. Trust is `community`:
//! Homebrew is a community project and `brew` verifies bottle checksums itself, so
//! each plan is a single unprivileged `brew` command with no separate download/verify.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{
    Bootstrap, Ecosystem, Provider, command_plan, get_json_opt, nonempty_lines, run_capture, which,
};
use crate::error::Result;
use crate::model::{
    InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
};

const ID: &str = "brew";
const BIN: &str = "brew";
const API: &str = "https://formulae.brew.sh/api/formula";

/// Where Homebrew's own installer puts `brew`, in the order it prefers them. A freshly
/// installed Homebrew is **not** on the current shell's PATH — its installer only prints
/// instructions for the user to add it — so JII resolves it here instead of dead-ending on
/// "open a new shell and try again" (ADR-0080). Public so the bootstrap path can offer to
/// wire the same binary into the user's shell rc.
pub const PREFIXES: &[&str] = &["/home/linuxbrew/.linuxbrew/bin/brew", "~/.linuxbrew/bin/brew"];

/// The `brew` to run: the one on PATH when there is one, else the first standard prefix that
/// exists. Deliberately uncached — Homebrew may be installed *during* this very run (the T6
/// bootstrap), and a cached "not here" would make the install that follows fail.
pub fn brew_bin() -> String {
    if super::on_path(BIN) {
        return BIN.to_string();
    }
    super::first_existing(PREFIXES).unwrap_or_else(|| BIN.to_string())
}

/// The Homebrew (Linuxbrew) installation source.
pub struct Homebrew;

impl Homebrew {
    pub fn new() -> Self {
        Homebrew
    }
}

impl Default for Homebrew {
    fn default() -> Self {
        Homebrew::new()
    }
}

#[async_trait]
impl Provider for Homebrew {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // Homebrew is a community project (not distro-official); brew verifies bottles.
        TrustLevel::Community
    }

    /// Compiled here on the machine.
    fn nature(&self) -> super::SourceNature {
        super::SourceNature::BuiltFromSource
    }

    async fn is_available(&self) -> bool {
        which(&brew_bin()).await
    }

    /// The Homebrew formulae API is a network call — no brew needed to find a match, so a host
    /// without Homebrew can still be offered "install brew, then this" (ADR-0061 part B).
    fn can_search(&self) -> bool {
        true
    }

    fn web_url(&self, candidate: &PackageCandidate) -> Option<String> {
        Some(format!("https://formulae.brew.sh/formula/{}", candidate.name))
    }

    fn ecosystem(&self) -> Option<Ecosystem> {
        Some(Ecosystem {
            label: "Homebrew",
            // Homebrew is not in any distro repo — it bootstraps via its own installer.
            bootstrap: Bootstrap::Script {
                cmd: "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"",
                // Homebrew's installer ends by printing a `brew shellenv` line for the user to
                // paste. That's the line — JII offers to add it so nobody has to (ADR-0080).
                shell: Some(super::ShellSetup {
                    bins: PREFIXES,
                    rc_line: "eval \"$({bin} shellenv)\"",
                }),
            },
        })
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        let url = format!("{API}/{}.json", query.raw.trim());
        let Some(formula) = get_json_opt::<Formula>(ID, &url).await? else {
            return Ok(Vec::new()); // no such formula
        };
        Ok(vec![candidate(&formula)])
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let mut reasons = vec![crate::t!("reason.brew_formula")];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }
        reasons.push(crate::t!("reason.brew_installs"));
        Ok(brew_plan(&candidate.name, "install", reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // `brew install a b c` installs the whole group in one unprivileged run.
        let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
        let reasons = vec![crate::t!("reason.brew_formula_many", names = names.join(", "))];
        Ok(Some(brew_many("install", &names, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.remove_one", name = record.name.clone(), mgr = "brew")];
        Ok(brew_plan(&record.name, "uninstall", reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<String> = records.iter().map(|r| r.name.clone()).collect();
        let reasons = vec![crate::t!("reason.remove_many", mgr = "brew", names = names.join(", "))];
        Ok(Some(brew_many("uninstall", &names, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.update_one", name = record.name.clone(), mgr = "brew")];
        Ok(brew_plan(&record.name, "upgrade", reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<String> = records.iter().map(|r| r.name.clone()).collect();
        let reasons = vec![crate::t!("reason.update_many", mgr = "brew", names = names.join(", "))];
        Ok(Some(brew_many("upgrade", &names, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `brew upgrade` with no names upgrades every outdated formula (D10); user-space.
        let reasons = vec![crate::t!("reason.brew_upgrade_all")];
        let argv = vec![brew_bin(), "upgrade".to_string()];
        Ok(Some(command_plan(ID, "all formulae", argv, false, reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        let brew = brew_bin();
        let out = run_capture(&[&brew, "list", "--versions"]).await?;
        Ok(parse_versions(&out, ID))
    }
}

/// A Homebrew formula lookup (only the fields we use).
#[derive(Debug, Deserialize)]
struct Formula {
    name: String,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    versions: Versions,
}

#[derive(Debug, Default, Deserialize)]
struct Versions {
    #[serde(default)]
    stable: Option<String>,
}

/// Build a candidate from a formula lookup. No program-vs-library filter (ADR-0023):
/// the formula API can't reliably say, and nearly all formulae install a binary, so we
/// offer it and let `brew install` reject a non-app.
fn candidate(formula: &Formula) -> PackageCandidate {
    let version = formula
        .versions
        .stable
        .clone()
        .filter(|s| !s.is_empty())
        .map(PkgVersion::new);
    PackageCandidate {
        name: formula.name.clone(),
        source_id: ID.to_string(),
        version,
        trust: TrustLevel::Community,
        // brew resolves a per-arch bottle (or builds from source), so arch is compatible;
        // brew verifies the bottle checksum itself.
        arch_ok: true,
        signed: true,
        summary: formula.desc.clone().filter(|d| !d.is_empty()),
        popularity: None,
        suspicious: false,
        raw: json!({}),
    }
}

/// A single unprivileged `brew <verb> <name>` plan (install/uninstall/upgrade).
fn brew_plan(name: &str, verb: &str, reasons: Vec<String>) -> InstallPlan {
    let argv = vec![brew_bin(), verb.to_string(), name.to_string()];
    command_plan(ID, name, argv, false, reasons)
}

/// One `brew <verb> a b c` for a whole group (still no root).
fn brew_many(verb: &str, names: &[String], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![brew_bin(), verb.to_string()];
    argv.extend(names.iter().cloned());
    command_plan(ID, &names.join(", "), argv, false, reasons)
}

/// Parse `brew list --versions` into installed records. Each line is
/// `name version [version…]`; we keep the name and the first version.
fn parse_versions(stdout: &str, source_id: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    nonempty_lines(stdout)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?.trim();
            if name.is_empty() {
                return None;
            }
            let version = parts.next();
            Some(InstalledRecord {
                name: name.to_string(),
                source_id: source_id.to_string(),
                version: version.map(PkgVersion::new),
                installed_at: now,
                // brew verifies bottle checksums itself — a self-verifying manager.
                verification: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIPGREP: &str = r#"{
        "name": "ripgrep",
        "desc": "Search tool like grep and The Silver Searcher",
        "versions": {"stable": "14.1.1", "head": null, "bottle": true}
    }"#;

    fn parse(sample: &str) -> Formula {
        serde_json::from_str(sample).unwrap()
    }

    #[test]
    fn formula_becomes_a_community_candidate() {
        let c = candidate(&parse(RIPGREP));
        assert_eq!(c.name, "ripgrep");
        assert_eq!(c.source_id, "brew");
        assert_eq!(c.version.as_ref().unwrap().0, "14.1.1");
        assert_eq!(c.trust, TrustLevel::Community);
        assert!(c.arch_ok && c.signed);
        assert_eq!(
            c.summary.as_deref(),
            Some("Search tool like grep and The Silver Searcher")
        );
    }

    #[test]
    fn formula_without_stable_version_still_offered() {
        // No reliable library filter (ADR-0023): offer it, let brew decide.
        let c = candidate(&parse(r#"{"name": "x", "versions": {}}"#));
        assert_eq!(c.name, "x");
        assert!(c.version.is_none());
    }

    #[test]
    fn install_plan_is_one_unprivileged_brew_command() {
        let c = candidate(&parse(RIPGREP));
        let plan = brew_plan(&c.name, "install", vec![]);
        assert_eq!(plan.source_id, "brew");
        assert!(!plan.needs_root());
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, needs_root } => {
                // Not the literal "brew": `brew_bin()` resolves to an absolute prefix path on a
                // host where Homebrew is installed off PATH (ADR-0080), so asserting the string
                // would make this test depend on the machine it runs on.
                assert_eq!(argv, &[brew_bin(), "install".into(), "ripgrep".into()]);
                assert!(!needs_root);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[test]
    fn parses_list_versions() {
        let sample = "ripgrep 14.1.1\nbat 0.24.0 0.23.0\nfd 10.1.0\n";
        let recs = parse_versions(sample, "brew");
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].name, "ripgrep");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "14.1.1");
        // A formula with several installed versions keeps the first.
        assert_eq!(recs[1].name, "bat");
        assert_eq!(recs[1].version.as_ref().unwrap().0, "0.24.0");
        assert_eq!(recs[2].name, "fd");
    }

    #[test]
    fn list_versions_tolerates_blank_lines() {
        assert!(parse_versions("\n   \n", "brew").is_empty());
    }

    #[tokio::test]
    async fn batch_merges_into_one_unprivileged_brew_command() {
        let mk = |name: &str| PackageCandidate {
            name: name.to_string(),
            source_id: ID.to_string(),
            version: None,
            trust: TrustLevel::Community,
            arch_ok: true,
            signed: true,
            summary: None,
            popularity: None,
            suspicious: false,
            raw: json!({}),
        };
        let (a, b) = (mk("ripgrep"), mk("bat"));
        let plan = Homebrew::new()
            .plan_install_many(&[&a, &b])
            .await
            .unwrap()
            .expect("brew batches");
        assert_eq!(plan.actions.len(), 1);
        assert!(!plan.needs_root());
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, .. } => {
                assert_eq!(
                    argv,
                    &[brew_bin(), "install".into(), "ripgrep".into(), "bat".into()]
                );
            }
            other => panic!("expected run, got {other:?}"),
        }
    }
}
