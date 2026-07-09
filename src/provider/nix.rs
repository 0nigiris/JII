//! Nix provider (the Nix package manager — Nixpkgs, on any distro incl. NixOS).
//!
//! Unlike apt/pacman/zypper, Nix is **not** distro-bound and installs into the user's Nix
//! **profile with no root** (like cargo/go). It self-gates on the `nix` binary (ADR-0029).
//! Every invocation passes `--extra-experimental-features "nix-command flakes"` so the
//! modern `nix` CLI works regardless of the host's global config. Search reads
//! `nix search --json` (a stable JSON schema); install/remove/upgrade use `nix profile`.
//!
//! `list_installed` reads `nix profile list --json` (schema-tolerant across Nix versions),
//! so JII sees profile packages installed outside jii too; `is_installed` additionally
//! verifies a record via the profile's `bin/<name>` symlink (the name==binary caveat go
//! carries). Trust is `community` (Nixpkgs is a curated community collection with
//! reproducible builds).

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
        // `nix profile list --json` enumerates the user profile. Its schema changed across
        // Nix versions — modern (≥2.20) keys `elements` by name; older Nix makes it an array
        // with the name only derivable from `attrPath`/`storePaths`. `parse_profile_list`
        // tolerates both (and any unknown shape → empty), so JII can now see Nix packages
        // installed outside jii too (#3), not just its own registry records.
        let out = run_capture_lax(&[
            BIN,
            "--extra-experimental-features",
            "nix-command flakes",
            "profile",
            "list",
            "--json",
        ])
        .await?;
        Ok(parse_profile_list(&out))
    }

    async fn is_installed(&self, record: &InstalledRecord) -> bool {
        nix_profile_bin(&record.name).is_some_and(|p| p.exists())
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

/// Parse `nix profile list --json` into installed records, tolerating both schema shapes:
/// modern Nix keys `elements` by the profile element name; older Nix makes `elements` an
/// array where the name must be derived from `attrPath` (last attr) or the store path. Any
/// unrecognised/empty shape yields no records rather than an error (a broken `nix` must never
/// break JII's registry-backed view).
fn parse_profile_list(stdout: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(stdout.trim()) else {
        return Vec::new();
    };
    let Some(elements) = doc.get("elements") else {
        return Vec::new();
    };
    match elements {
        // Modern (Nix ≥ 2.20): a map keyed by the profile element name.
        serde_json::Value::Object(map) => map
            .iter()
            .filter_map(|(key, val)| element_name(Some(key), val).map(|n| record(n, val, now)))
            .collect(),
        // Older Nix: an array of elements with no name key — derive it.
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|val| element_name(None, val).map(|n| record(n, val, now)))
            .collect(),
        _ => Vec::new(),
    }
}

/// The package name of a profile element. Prefers the map key (already the element name;
/// normalised to its last dotted segment just in case), then the last component of
/// `attrPath` (`legacyPackages.x86_64-linux.ripgrep` → `ripgrep`), then the store-path name.
fn element_name(key: Option<&str>, val: &serde_json::Value) -> Option<String> {
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        return Some(k.rsplit('.').next().unwrap_or(k).to_string());
    }
    if let Some(attr) = val.get("attrPath").and_then(|v| v.as_str())
        && let Some(last) = attr.rsplit('.').next().filter(|s| !s.is_empty())
    {
        return Some(last.to_string());
    }
    first_store_basename(val).and_then(|b| store_name(&b))
}

/// Build a record, deriving a best-effort version from the store path (`…-ripgrep-14.1.1`).
fn record(name: String, val: &serde_json::Value, now: chrono::DateTime<chrono::Utc>) -> InstalledRecord {
    let version = first_store_basename(val)
        .and_then(|b| store_version(&b, &name))
        .map(PkgVersion::new);
    InstalledRecord {
        name,
        source_id: ID.to_string(),
        version,
        installed_at: now,
        verification: None,
    }
}

/// The basename of the element's first store path, if any.
fn first_store_basename(val: &serde_json::Value) -> Option<String> {
    val.get("storePaths")?
        .as_array()?
        .first()?
        .as_str()?
        .rsplit('/')
        .next()
        .map(str::to_string)
}

/// The package name from a store-path basename `…-<name>-<version>`: drop the leading hash
/// (up to the first `-`), then drop trailing `-<segment>` runs that begin with a digit.
fn store_name(basename: &str) -> Option<String> {
    let after_hash = basename.split_once('-').map_or(basename, |(_, r)| r);
    let parts: Vec<&str> = after_hash.split('-').collect();
    let mut end = parts.len();
    while end > 1 && parts[end - 1].starts_with(|c: char| c.is_ascii_digit()) {
        end -= 1;
    }
    let name = parts[..end].join("-");
    (!name.is_empty()).then_some(name)
}

/// The version from a store-path basename, i.e. what follows `<hash>-<name>-`.
fn store_version(basename: &str, name: &str) -> Option<String> {
    let after_hash = basename.split_once('-').map_or(basename, |(_, r)| r);
    after_hash
        .strip_prefix(&format!("{name}-"))
        .filter(|v| !v.is_empty())
        .map(str::to_string)
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

    // `nix profile list --json` on modern Nix (≥2.20): elements keyed by name.
    const PROFILE_MAP: &str = r#"{
      "version": 2,
      "elements": {
        "ripgrep": {
          "active": true,
          "attrPath": "legacyPackages.x86_64-linux.ripgrep",
          "originalUrl": "flake:nixpkgs",
          "storePaths": ["/nix/store/abc123def456ghi789jkl012mno345pq-ripgrep-14.1.1"]
        },
        "fd": {
          "active": true,
          "attrPath": "legacyPackages.x86_64-linux.fd",
          "storePaths": ["/nix/store/zzz999yyy888xxx777www666vvv555uu-fd-10.2.0"]
        }
      }
    }"#;

    // Older Nix: elements is an array with no name key.
    const PROFILE_ARRAY: &str = r#"{
      "elements": [
        {
          "active": true,
          "attrPath": "legacyPackages.x86_64-linux.ripgrep",
          "storePaths": ["/nix/store/abc123def456ghi789jkl012mno345pq-ripgrep-14.1.1"]
        }
      ]
    }"#;

    #[test]
    fn parses_modern_map_schema_with_names_and_versions() {
        let mut recs = parse_profile_list(PROFILE_MAP);
        recs.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "fd");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "10.2.0");
        assert_eq!(recs[1].name, "ripgrep");
        assert_eq!(recs[1].version.as_ref().unwrap().0, "14.1.1");
        assert_eq!(recs[1].source_id, "nix");
    }

    #[test]
    fn parses_older_array_schema_deriving_name_from_attrpath() {
        let recs = parse_profile_list(PROFILE_ARRAY);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].name, "ripgrep");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "14.1.1");
    }

    #[test]
    fn profile_list_tolerates_empty_and_garbage() {
        assert!(parse_profile_list("{}").is_empty()); // no "elements"
        assert!(parse_profile_list("not json").is_empty());
        assert!(parse_profile_list("").is_empty());
        assert!(parse_profile_list(r#"{"elements": 42}"#).is_empty()); // unknown shape
    }

    #[test]
    fn store_name_strips_hash_and_trailing_version() {
        assert_eq!(store_name("abc-ripgrep-14.1.1").as_deref(), Some("ripgrep"));
        // A hyphenated package name keeps its non-version segments.
        assert_eq!(store_name("abc-ripgrep-all-0.10.6").as_deref(), Some("ripgrep-all"));
    }
}
