//! Gentoo provider (the Portage package manager, `emerge`).
//!
//! Self-gates on `emerge` (absent elsewhere → drops out; no distro check, ADR-0029). Covers
//! the main `::gentoo` tree (GPG-verified ebuilds) → **official** trust. Portage identifies a
//! package by an **atom** `category/package` (e.g. `app-misc/ripgrep`); a bare name can live in
//! several categories, so `search` parses `emerge --search "^name$"` and emits one candidate per
//! `category/name` match (the atom is kept in `raw` for the plan). Plans run `emerge`; the
//! installed set is read from `/var/db/pkg` (canonical, no extra tooling). Parsing is pure and
//! unit-tested. Note: Portage builds **from source**, so installs can be slow — that is inherent
//! to Gentoo, not something JII hides.

use async_trait::async_trait;
use serde_json::json;
use std::path::Path;

use super::{Provider, command_plan, run_capture_lax, which};
use crate::error::Result;
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel};

/// The Portage front-end — drives search, plans and (via its presence) availability.
const BIN: &str = "emerge";

/// This provider's stable source id.
const ID: &str = "gentoo";

/// Where Portage records installed packages, one `category/PF` directory per package.
const VDB: &str = "/var/db/pkg";

/// The Gentoo (Portage) installation source.
pub struct Gentoo;

impl Gentoo {
    pub fn new() -> Self {
        Gentoo
    }
}

impl Default for Gentoo {
    fn default() -> Self {
        Gentoo::new()
    }
}

#[async_trait]
impl Provider for Gentoo {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // The main ::gentoo tree ships GPG-verified ebuilds.
        TrustLevel::Official
    }

    /// Compiled here on the machine.
    fn nature(&self) -> super::SourceNature {
        super::SourceNature::BuiltFromSource
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // `emerge --search "^name$"` anchors the package-name regex; it lists each matching
        // `category/name` block. Read stdout laxly — an unsynced tree or no match is "no
        // candidate", not a source failure. (Portage search scans the tree and can be slow.)
        let name = query.raw.trim();
        let regex = format!("^{name}$");
        let out = run_capture_lax(&[BIN, "--search", "--quiet", &regex]).await?;
        Ok(parse_search(&out, name, self.id(), self.trust()))
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let mut reasons = vec![crate::t!("reason.gentoo_official")];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }
        Ok(root_plan(&candidate.name, &["--ask=n", &atom(candidate)], reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // One `emerge --ask=n cat/a cat/b` for the whole group.
        let atoms: Vec<String> = candidates.iter().map(|c| atom(c)).collect();
        let names: Vec<&str> = candidates.iter().map(|c| c.name.as_str()).collect();
        let mut args = vec!["--ask=n"];
        args.extend(atoms.iter().map(|s| s.as_str()));
        let reasons = vec![crate::t!("reason.gentoo_official_many", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // `emerge --unmerge` removes just this package (depclean is the user's own call — JII
        // removes what it's asked to, one target at a time). The full command is shown first.
        let reasons = vec![crate::t!("reason.remove_one", name = record.name.clone(), mgr = "emerge")];
        Ok(root_plan(&record.name, &["--unmerge", "--ask=n", &record.name], reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["--unmerge", "--ask=n"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.remove_many", mgr = "emerge", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.update_one", name = record.name.clone(), mgr = "emerge")];
        Ok(root_plan(&record.name, &["--update", "--ask=n", &record.name], reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut args = vec!["--update", "--ask=n"];
        args.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.update_many", mgr = "emerge", names = names.join(", "))];
        Ok(Some(root_plan(&names.join(", "), &args, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `emerge --update --deep --newuse @world` — the standard whole-system upgrade (D10).
        let reasons = vec![crate::t!("reason.upgrade_all_system", mgr = "emerge")];
        Ok(Some(root_plan(
            "@world",
            &["--update", "--deep", "--newuse", "--ask=n", "@world"],
            reasons,
        )))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // Portage's installed database is `/var/db/pkg/<category>/<PF>` — always present, no
        // gentoolkit/portage-utils dependency. Read it directly (a broken/missing tree → empty).
        Ok(read_vdb(Path::new(VDB), self.id()))
    }
}

/// The atom to emerge for a candidate: the full `category/package` from `raw` when search
/// captured it, else the bare name (Portage will resolve it, possibly ambiguously).
fn atom(candidate: &PackageCandidate) -> String {
    candidate
        .raw
        .get("atom")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| candidate.name.clone())
}

/// Build a single-command, root-requiring `emerge <args>` plan.
fn root_plan(name: &str, args: &[&str], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![BIN.to_string()];
    argv.extend(args.iter().map(|s| s.to_string()));
    command_plan(ID, name, argv, true, reasons)
}

/// Split a Portage `PF` (`package-version[-rN]`) into its package name and version. The version
/// is the run of hyphen-separated components starting at the **last** component that begins with
/// a digit (`foo-bar-1.2-r1` → `foo-bar` / `1.2-r1`; `gtk+-3.24.38` → `gtk+` / `3.24.38`). A PF
/// with no such component (shouldn't happen for a real install) yields the whole string as the
/// name and no version.
fn split_pf(pf: &str) -> (String, Option<String>) {
    let parts: Vec<&str> = pf.split('-').collect();
    let ver_start = parts
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, p)| p.starts_with(|c: char| c.is_ascii_digit()))
        .map(|(i, _)| i)
        .next_back();
    match ver_start {
        Some(i) => (parts[..i].join("-"), Some(parts[i..].join("-"))),
        None => (pf.to_string(), None),
    }
}

/// One parsed `emerge --search` block (only the fields we use).
#[derive(Default)]
struct Block {
    atom: String,
    version: Option<String>,
    description: Option<String>,
}

/// Parse `emerge --search "^name$"` output into candidates — one per `category/name` block whose
/// package part matches `name` exactly (the regex is anchored, but we re-check in code so a stray
/// match never installs the wrong package). Blocks begin with `*  category/package`; the fields
/// we read are the indented `Latest version available:` and `Description:` lines.
fn parse_search(stdout: &str, name: &str, source_id: &str, trust: TrustLevel) -> Vec<PackageCandidate> {
    let mut blocks: Vec<Block> = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("* ") {
            blocks.push(Block { atom: rest.trim().to_string(), ..Block::default() });
            continue;
        }
        let Some(current) = blocks.last_mut() else {
            continue;
        };
        if let Some(v) = trimmed.strip_prefix("Latest version available:") {
            current.version = Some(v.trim().to_string()).filter(|s| !s.is_empty());
        } else if let Some(d) = trimmed.strip_prefix("Description:") {
            current.description = Some(d.trim().to_string()).filter(|s| !s.is_empty());
        }
    }

    blocks
        .into_iter()
        .filter(|b| b.atom.rsplit('/').next() == Some(name))
        .map(|b| PackageCandidate {
            name: name.to_string(),
            source_id: source_id.to_string(),
            version: b.version.as_deref().map(PkgVersion::new),
            trust,
            arch_ok: true,
            signed: true,
            summary: b.description,
            popularity: None,
            suspicious: false,
            raw: json!({ "atom": b.atom }),
        })
        .collect()
}

/// Read Portage's installed database at `root` (`/var/db/pkg/<category>/<PF>`) into records.
/// Two directory levels: category, then each package's `PF` directory. A missing/unreadable
/// tree yields no records (never an error — a broken VDB must not break JII's registry view).
fn read_vdb(root: &Path, source_id: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    let mut out = Vec::new();
    let Ok(categories) = std::fs::read_dir(root) else {
        return out;
    };
    for category in categories.flatten() {
        if !category.path().is_dir() {
            continue;
        }
        let Ok(pkgs) = std::fs::read_dir(category.path()) else {
            continue;
        };
        for pkg in pkgs.flatten() {
            if !pkg.path().is_dir() {
                continue;
            }
            let pf = pkg.file_name().to_string_lossy().into_owned();
            let (name, version) = split_pf(&pf);
            if name.is_empty() {
                continue;
            }
            out.push(InstalledRecord {
                name,
                source_id: source_id.to_string(),
                version: version.as_deref().map(PkgVersion::new),
                installed_at: now,
                verification: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = "\
Searching...
[ Results for search key : ripgrep ]
[ Applications found : 1 ]

*  app-misc/ripgrep
      Latest version available: 14.1.1
      Latest version installed: [ Not Installed ]
      Size of files: 2,469 KiB
      Homepage:      https://github.com/BurntSushi/ripgrep
      Description:   A search tool that combines the usability of ag with the raw speed of grep
      License:       Apache-2.0 MIT Unlicense
";

    #[test]
    fn parses_atom_version_and_description() {
        let cands = parse_search(SEARCH, "ripgrep", "gentoo", TrustLevel::Official);
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(c.name, "ripgrep");
        assert_eq!(c.version.as_ref().unwrap().0, "14.1.1");
        assert_eq!(c.raw.get("atom").unwrap().as_str(), Some("app-misc/ripgrep"));
        assert_eq!(c.trust, TrustLevel::Official);
        assert!(c.signed);
        assert!(c.summary.as_deref().unwrap().starts_with("A search tool"));
    }

    #[test]
    fn only_exact_package_name_matches() {
        // A near-name block (ripgrep-all) must not become a `ripgrep` candidate.
        let sample = "*  app-misc/ripgrep-all\n      Latest version available: 0.10.6\n";
        assert!(parse_search(sample, "ripgrep", "gentoo", TrustLevel::Official).is_empty());
        assert!(parse_search("", "ripgrep", "gentoo", TrustLevel::Official).is_empty());
    }

    #[test]
    fn same_name_in_two_categories_yields_two_candidates() {
        let sample = "\
*  dev-util/foo
      Latest version available: 1.0
*  app-misc/foo
      Latest version available: 2.0
";
        let cands = parse_search(sample, "foo", "gentoo", TrustLevel::Official);
        assert_eq!(cands.len(), 2);
        let atoms: Vec<&str> = cands.iter().filter_map(|c| c.raw.get("atom").and_then(|a| a.as_str())).collect();
        assert!(atoms.contains(&"dev-util/foo"));
        assert!(atoms.contains(&"app-misc/foo"));
    }

    #[test]
    fn split_pf_finds_version_start() {
        assert_eq!(split_pf("ripgrep-14.1.1"), ("ripgrep".into(), Some("14.1.1".into())));
        assert_eq!(split_pf("gtk+-3.24.38"), ("gtk+".into(), Some("3.24.38".into())));
        // A revision stays with the version; a numeric package suffix stays with the name.
        assert_eq!(split_pf("foo-bar-1.2-r1"), ("foo-bar".into(), Some("1.2-r1".into())));
        assert_eq!(split_pf("libpng16-1.6.40"), ("libpng16".into(), Some("1.6.40".into())));
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
                assert!(needs_root, "emerge transactions need root");
                argv.clone()
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn install_uses_the_full_atom() {
        let c = parse_search(SEARCH, "ripgrep", "gentoo", TrustLevel::Official).remove(0);
        let plan = Gentoo::new().plan_install(&c).await.unwrap();
        assert_eq!(argv_of(&plan), &["emerge", "--ask=n", "app-misc/ripgrep"]);
    }

    #[tokio::test]
    async fn batch_install_merges_atoms() {
        let a = parse_search(SEARCH, "ripgrep", "gentoo", TrustLevel::Official).remove(0);
        let b = PackageCandidate { name: "htop".into(), raw: json!({ "atom": "sys-process/htop" }), ..a.clone() };
        let plan = Gentoo::new().plan_install_many(&[&a, &b]).await.unwrap().expect("gentoo batches");
        assert_eq!(
            argv_of(&plan),
            &["emerge", "--ask=n", "app-misc/ripgrep", "sys-process/htop"]
        );
    }

    #[tokio::test]
    async fn update_all_upgrades_world() {
        let plan = Gentoo::new().plan_update_all().await.unwrap().expect("gentoo bulk-updates");
        assert_eq!(
            argv_of(&plan),
            &["emerge", "--update", "--deep", "--newuse", "--ask=n", "@world"]
        );
    }

    #[test]
    fn read_vdb_parses_category_and_pf_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("app-misc/ripgrep-14.1.1")).unwrap();
        std::fs::create_dir_all(root.join("sys-process/htop-3.4.0")).unwrap();
        // A stray file at the category level is ignored.
        std::fs::write(root.join("app-misc/stray"), b"x").unwrap();

        let mut recs = read_vdb(root, "gentoo");
        recs.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "htop");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "3.4.0");
        assert_eq!(recs[1].name, "ripgrep");
        assert_eq!(recs[1].version.as_ref().unwrap().0, "14.1.1");
        assert_eq!(recs[1].source_id, "gentoo");
    }

    #[test]
    fn read_vdb_missing_tree_is_empty() {
        assert!(read_vdb(Path::new("/nonexistent/var/db/pkg"), "gentoo").is_empty());
    }

    #[tokio::test]
    async fn remove_unmerges_by_name() {
        let plan = Gentoo::new().plan_remove(&rec("htop")).await.unwrap();
        assert_eq!(argv_of(&plan), &["emerge", "--unmerge", "--ask=n", "htop"]);
    }
}
