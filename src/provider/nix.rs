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
use rnix::{Root, SyntaxKind, SyntaxNode};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{Bootstrap, Ecosystem, Provider, command_plan, run_capture_lax, which};
use crate::error::Result;
use crate::model::{
    InstallPlan, InstallStrategy, InstalledRecord, PackageCandidate, PkgVersion, Query,
    StrategyKind, TrustLevel,
};

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
        let mut reasons = vec![crate::t!("reason.nix_pkg")];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }
        reasons.push(crate::t!("reason.nix_installs"));
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
        let reasons = vec![crate::t!("reason.nix_pkg_many", names = names.join(", "))];
        Ok(Some(command_plan(ID, &names.join(", "), nix_argv(&sub), false, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        // `nix profile remove <name>` drops the profile entry (no root).
        let reasons = vec![crate::t!("reason.nix_remove", name = record.name.clone())];
        let argv = nix_argv(&["profile", "remove", &record.name]);
        Ok(command_plan(ID, &record.name, argv, false, reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut sub = vec!["profile", "remove"];
        sub.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.nix_remove_many", names = names.join(", "))];
        Ok(Some(command_plan(ID, &names.join(", "), nix_argv(&sub), false, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.nix_upgrade_one", name = record.name.clone())];
        let argv = nix_argv(&["profile", "upgrade", &record.name]);
        Ok(command_plan(ID, &record.name, argv, false, reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<&str> = records.iter().map(|r| r.name.as_str()).collect();
        let mut sub = vec!["profile", "upgrade"];
        sub.extend_from_slice(&names);
        let reasons = vec![crate::t!("reason.nix_upgrade_many", names = names.join(", "))];
        Ok(Some(command_plan(ID, &names.join(", "), nix_argv(&sub), false, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `nix profile upgrade --all` upgrades every package in the user profile (D10);
        // user-space, no root (like the rest of the nix provider).
        let reasons = vec![crate::t!("reason.nix_upgrade_all")];
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

    async fn install_strategies(&self, candidate: &PackageCandidate) -> Vec<InstallStrategy> {
        // Declarative Nix: only offer a choice when the user actually manages their system
        // declaratively — i.e. a config file we recognise exists on this host. A plain
        // `nix profile` user (Nix on Fedora, say) has none, so this returns empty and JII
        // installs imperatively as before (no annoying menu). When a config *is* found we
        // offer `nix profile install` (default, immediate) plus one path per detected file:
        //   • Etap B/C (ADR-0056/0058): any config we can parse → an **auto-edit** (`EditFile`)
        //     — JII splices the package in, shows a diff, backs up, and writes it. A user-owned
        //     `home.nix` is written directly; the root-owned NixOS config is written via the
        //     privilege path (`needs_root`), with the exact `sudo` commands shown first.
        //   • Etap A (ADR-0054): any file we can't confidently read/parse → **show the snippet**
        //     (`Manual`), changing nothing.
        let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) else {
            return Vec::new();
        };
        let targets = detect_targets(&home);
        if targets.is_empty() {
            return Vec::new();
        }
        let mut out = vec![InstallStrategy {
            label: crate::t!("nix.strategy_imperative"),
            hint: crate::t!("nix.strategy_imperative_hint"),
            kind: StrategyKind::Imperative,
        }];
        for t in &targets {
            let hint = if t.home {
                crate::t!("nix.strategy_hint_home")
            } else {
                crate::t!("nix.strategy_hint_system")
            };
            out.push(InstallStrategy {
                label: crate::t!("nix.strategy_add_to", file = t.file.display().to_string()),
                hint,
                kind: strategy_for_target(t, &candidate.name),
            });
        }
        out
    }
}

/// Decide *how* a declarative target is offered for `name` (ADR-0056/0058). Any config file
/// whose declarative list we can read, parse and splice into becomes an
/// [`StrategyKind::EditFile`] (auto-edit with diff/backup): a user-owned home-manager file
/// writes directly (`needs_root == false`), while the root-owned NixOS config carries
/// `needs_root == true` so the CLI performs the write via the privilege path (Etap C). Only an
/// unreadable file, an already-present package, or a shape we can't safely edit falls back to
/// [`StrategyKind::Manual`] (show the snippet, change nothing).
fn strategy_for_target(t: &NixTarget, name: &str) -> StrategyKind {
    if let Ok(source) = std::fs::read_to_string(&t.file)
        && let Insertion::Inserted(new_content) = insert_package(&source, t.attr, name)
    {
        return StrategyKind::EditFile {
            path: t.file.clone(),
            diff: line_diff(&source, &new_content),
            new_content,
            apply: t.apply.to_string(),
            needs_root: !t.home,
        };
    }
    StrategyKind::Manual { guidance: guidance(t, name) }
}

/// Outcome of splicing a package into a declarative list (see [`insert_package`]).
#[derive(Debug, PartialEq)]
enum Insertion {
    /// The full rewritten file, with the package added to the list.
    Inserted(String),
    /// The package is already declared in the list — nothing to do.
    AlreadyPresent,
    /// The attribute/list wasn't found (or the file didn't parse) — caller shows a snippet.
    NotFound,
}

/// Splice `pkg` into the declarative list assigned to `attr_path` (e.g. `home.packages`) in a
/// Nix `source`, preserving the file's exact formatting and comments. The heavy lifting is done
/// by `rnix`'s lossless concrete syntax tree: we locate the target list node and splice `pkg`
/// into the **original text** at the right byte offset — never re-print the tree (which would
/// reformat the whole file). Hand-rolling this would mean re-implementing a comment- and
/// string-aware Nix lexer, so leaning on the canonical parser is the smaller, safer surface
/// (ADR-0056). Returns [`Insertion::NotFound`] for anything we can't confidently edit, so the
/// caller can fall back to showing a snippet.
fn insert_package(source: &str, attr_path: &str, pkg: &str) -> Insertion {
    let parse = Root::parse(source);
    // A file that doesn't parse cleanly is not one we'll rewrite — show a snippet instead.
    if !parse.errors().is_empty() {
        return Insertion::NotFound;
    }
    let Some(list) = find_list(&parse.syntax(), attr_path) else {
        return Insertion::NotFound;
    };
    let items: Vec<SyntaxNode> = list.children().collect();
    // Already declared? (Bare idents like `ripgrep`, matched on their trimmed text.)
    if items.iter().any(|i| i.text().to_string().trim() == pkg) {
        return Insertion::AlreadyPresent;
    }
    let range = list.text_range();
    let (list_start, list_end) = (usize::from(range.start()), usize::from(range.end()));

    // Empty list `[]` / `[ ]`: replace the whole node with a tidy one-liner.
    let Some(first) = items.first() else {
        let replacement = format!("[ {pkg} ]");
        return Insertion::Inserted(splice(source, list_start, list_end, &replacement));
    };

    let l_brack_end = list
        .children_with_tokens()
        .find(|e| e.kind() == SyntaxKind::TOKEN_L_BRACK)
        .map(|e| usize::from(e.text_range().end()))
        .unwrap_or(list_start);
    let first_start = usize::from(first.text_range().start());
    let last_end = usize::from(items.last().unwrap().text_range().end());

    // Multi-line list (each item on its own line) vs inline `[ a b ]`: mirror the existing style.
    if source[l_brack_end..first_start].contains('\n') {
        let indent = line_indent(source, first_start);
        // Insert on a fresh line after the last item's line (past any trailing comment).
        let nl = source[last_end..].find('\n').map_or(last_end, |i| last_end + i);
        Insertion::Inserted(splice(source, nl, nl, &format!("\n{indent}{pkg}")))
    } else {
        Insertion::Inserted(splice(source, last_end, last_end, &format!(" {pkg}")))
    }
}

/// The declarative list node (`[ … ]`) assigned to `attr_path`, unwrapping a leading
/// `with pkgs;`. Returns `None` if the attribute is absent or its value isn't a plain list.
fn find_list(root: &SyntaxNode, attr_path: &str) -> Option<SyntaxNode> {
    let av = root.descendants().find(|n| {
        n.kind() == SyntaxKind::NODE_ATTRPATH_VALUE
            && n.children()
                .find(|c| c.kind() == SyntaxKind::NODE_ATTRPATH)
                .is_some_and(|p| p.text().to_string().trim() == attr_path)
    })?;
    // The value is the AttrpathValue's child node that isn't the attrpath.
    let mut value = av.children().find(|c| c.kind() != SyntaxKind::NODE_ATTRPATH)?;
    // Unwrap any `with <ns>; <body>` wrappers to reach the list.
    while value.kind() == SyntaxKind::NODE_WITH {
        value = value.children().last()?;
    }
    (value.kind() == SyntaxKind::NODE_LIST).then_some(value)
}

/// The leading whitespace of the line containing byte offset `at` (an item's indentation).
fn line_indent(source: &str, at: usize) -> String {
    let line_start = source[..at].rfind('\n').map_or(0, |i| i + 1);
    source[line_start..at].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// Replace `source[start..end]` with `replacement`, returning the new string.
fn splice(source: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut out = String::with_capacity(source.len() + replacement.len());
    out.push_str(&source[..start]);
    out.push_str(replacement);
    out.push_str(&source[end..]);
    out
}

/// A compact line diff of `old` → `new` for the confirm prompt: a few lines of unchanged
/// context, then `-` removed / `+` added lines. Since JII only ever inserts into a list, the
/// change is a small localized hunk; this trims the common prefix/suffix and shows the middle.
fn line_diff(old: &str, new: &str) -> String {
    let o: Vec<&str> = old.lines().collect();
    let n: Vec<&str> = new.lines().collect();
    let mut p = 0;
    while p < o.len() && p < n.len() && o[p] == n[p] {
        p += 1;
    }
    let mut s = 0;
    while s < o.len() - p && s < n.len() - p && o[o.len() - 1 - s] == n[n.len() - 1 - s] {
        s += 1;
    }
    const CTX: usize = 3;
    let mut out = String::new();
    for line in &o[p.saturating_sub(CTX)..p] {
        out.push_str(&format!("  {line}\n"));
    }
    for line in &o[p..o.len() - s] {
        out.push_str(&format!("- {line}\n"));
    }
    for line in &n[p..n.len() - s] {
        out.push_str(&format!("+ {line}\n"));
    }
    let tail_end = (o.len() - s + CTX).min(o.len());
    for line in &o[o.len() - s..tail_end] {
        out.push_str(&format!("  {line}\n"));
    }
    out
}

/// A recognised place a package can be declared in a user's Nix configuration.
struct NixTarget {
    /// The config file (shown to the user; never written by JII in Etap A).
    file: PathBuf,
    /// The Nix attribute the package is added to (`environment.systemPackages` / `home.packages`).
    attr: &'static str,
    /// The command that applies the change once edited.
    apply: &'static str,
    /// Whether this is a home-manager (user) target vs a NixOS (system) one — picks the hint.
    home: bool,
}

/// Candidate declarative targets, in the order they're offered. NixOS system config first,
/// then the two standard standalone home-manager locations. Split from [`detect_targets`] so
/// the "which files exist" decision is unit-testable without touching the real filesystem.
fn candidate_targets(home: &Path) -> Vec<NixTarget> {
    vec![
        NixTarget {
            file: PathBuf::from("/etc/nixos/configuration.nix"),
            attr: "environment.systemPackages",
            apply: "sudo nixos-rebuild switch",
            home: false,
        },
        NixTarget {
            file: home.join(".config/home-manager/home.nix"),
            attr: "home.packages",
            apply: "home-manager switch",
            home: true,
        },
        NixTarget {
            file: home.join(".config/nixpkgs/home.nix"),
            attr: "home.packages",
            apply: "home-manager switch",
            home: true,
        },
    ]
}

/// The declarative targets that actually exist on this host.
fn detect_targets(home: &Path) -> Vec<NixTarget> {
    detect_targets_with(home, |p| p.exists())
}

/// [`detect_targets`] with an injectable existence predicate (for tests).
fn detect_targets_with<F: Fn(&Path) -> bool>(home: &Path, exists: F) -> Vec<NixTarget> {
    candidate_targets(home)
        .into_iter()
        .filter(|t| exists(&t.file))
        .collect()
}

/// A ready-to-paste snippet declaring `name` in `attr` (e.g. an
/// `environment.systemPackages = with pkgs; [ … ];` block). Indented two spaces so it reads
/// as a nested attribute. This is code, not prose — it stays literal (not localised).
fn snippet(attr: &str, name: &str) -> String {
    format!("  {attr} = with pkgs; [\n    {name}\n  ];")
}

/// The full "here's what to add, and how to apply it" guidance for a declarative target —
/// intro + snippet + apply command + a backup note. Shown, never executed (Etap A).
fn guidance(target: &NixTarget, name: &str) -> String {
    let file = target.file.display().to_string();
    format!(
        "{intro}\n\n{snippet}\n\n{apply}\n  {cmd}\n\n{backup}",
        intro = crate::t!("nix.guidance_intro", name = name, attr = target.attr, file = file),
        snippet = snippet(target.attr, name),
        apply = crate::t!("nix.guidance_apply"),
        cmd = target.apply,
        backup = crate::t!("nix.guidance_backup"),
    )
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

    #[test]
    fn detect_targets_offers_only_existing_config_files() {
        let home = Path::new("/home/tester");
        let nixos = Path::new("/etc/nixos/configuration.nix");
        let hm = home.join(".config/home-manager/home.nix");

        // No recognised config → no declarative choice at all (plain `nix profile` user).
        assert!(detect_targets_with(home, |_| false).is_empty());

        // Only a NixOS system config present → one system target.
        let sys = detect_targets_with(home, |p| p == nixos);
        assert_eq!(sys.len(), 1);
        assert_eq!(sys[0].attr, "environment.systemPackages");
        assert!(!sys[0].home);

        // Only a standalone home-manager config present → one home target.
        let user = detect_targets_with(home, |p| p == hm);
        assert_eq!(user.len(), 1);
        assert_eq!(user[0].attr, "home.packages");
        assert!(user[0].home);
        assert_eq!(user[0].apply, "home-manager switch");
    }

    #[test]
    fn snippet_declares_the_package_in_the_attr() {
        let s = snippet("home.packages", "ripgrep");
        assert!(s.contains("home.packages = with pkgs; ["));
        assert!(s.contains("ripgrep"));
        assert!(s.contains("];"));
    }

    #[test]
    fn guidance_shows_file_snippet_and_apply_command_but_no_write() {
        let target = NixTarget {
            file: PathBuf::from("/etc/nixos/configuration.nix"),
            attr: "environment.systemPackages",
            apply: "sudo nixos-rebuild switch",
            home: false,
        };
        let g = guidance(&target, "ripgrep");
        // Locale-independent, structural assertions (the literal file/attr/command).
        assert!(g.contains("/etc/nixos/configuration.nix"));
        assert!(g.contains("environment.systemPackages = with pkgs; ["));
        assert!(g.contains("ripgrep"));
        assert!(g.contains("sudo nixos-rebuild switch"));
    }

    #[test]
    fn strategy_for_target_marks_root_config_needs_root() {
        // Etap C (ADR-0058): a readable, parseable config becomes an EditFile either way; the
        // system (non-home) target carries needs_root == true so the CLI writes it via sudo,
        // while a home-manager target writes directly.
        let dir = tempfile::tempdir().unwrap();

        let sys = dir.path().join("configuration.nix");
        std::fs::write(&sys, "{ environment.systemPackages = with pkgs; [ git ]; }\n").unwrap();
        let sys_target = NixTarget {
            file: sys,
            attr: "environment.systemPackages",
            apply: "sudo nixos-rebuild switch",
            home: false,
        };
        match strategy_for_target(&sys_target, "vim") {
            StrategyKind::EditFile { needs_root, .. } => assert!(needs_root),
            other => panic!("expected EditFile, got {other:?}"),
        }

        let home = dir.path().join("home.nix");
        std::fs::write(&home, "{ home.packages = with pkgs; [ git ]; }\n").unwrap();
        let home_target = NixTarget {
            file: home,
            attr: "home.packages",
            apply: "home-manager switch",
            home: true,
        };
        match strategy_for_target(&home_target, "vim") {
            StrategyKind::EditFile { needs_root, .. } => assert!(!needs_root),
            other => panic!("expected EditFile, got {other:?}"),
        }
    }

    #[test]
    fn strategy_for_target_falls_back_to_manual_when_unreadable() {
        // A file that doesn't exist (can't be read) → Manual snippet, never a blind EditFile.
        let target = NixTarget {
            file: PathBuf::from("/nonexistent/configuration.nix"),
            attr: "environment.systemPackages",
            apply: "sudo nixos-rebuild switch",
            home: false,
        };
        assert!(matches!(
            strategy_for_target(&target, "vim"),
            StrategyKind::Manual { .. }
        ));
    }

    // ----- Etap B: parser-driven auto-edit (insert_package / find_list / line_diff) -----

    fn inserted(source: &str, attr: &str, pkg: &str) -> String {
        match insert_package(source, attr, pkg) {
            Insertion::Inserted(s) => s,
            other => panic!("expected Inserted, got {other:?}"),
        }
    }

    #[test]
    fn inserts_into_a_multiline_list_preserving_indent_and_order() {
        let src = "{ pkgs, ... }:\n{\n  home.packages = with pkgs; [\n    ripgrep\n    fd\n  ];\n}\n";
        let out = inserted(src, "home.packages", "bat");
        assert_eq!(
            out,
            "{ pkgs, ... }:\n{\n  home.packages = with pkgs; [\n    ripgrep\n    fd\n    bat\n  ];\n}\n"
        );
    }

    #[test]
    fn inserts_into_an_inline_list_with_a_space() {
        let src = "{ home.packages = with pkgs; [ ripgrep fd ]; }\n";
        let out = inserted(src, "home.packages", "bat");
        assert_eq!(out, "{ home.packages = with pkgs; [ ripgrep fd bat ]; }\n");
    }

    #[test]
    fn inserts_into_an_empty_list() {
        assert_eq!(
            inserted("{ home.packages = [ ]; }\n", "home.packages", "bat"),
            "{ home.packages = [ bat ]; }\n"
        );
        assert_eq!(
            inserted("{ home.packages = []; }\n", "home.packages", "bat"),
            "{ home.packages = [ bat ]; }\n"
        );
    }

    #[test]
    fn insert_works_without_a_with_pkgs_wrapper() {
        let src = "{ home.packages = [\n  pkgs.ripgrep\n]; }\n";
        let out = inserted(src, "home.packages", "pkgs.bat");
        assert_eq!(out, "{ home.packages = [\n  pkgs.ripgrep\n  pkgs.bat\n]; }\n");
    }

    #[test]
    fn insert_preserves_comments_and_lands_after_a_trailing_comment() {
        let src = "{\n  home.packages = with pkgs; [\n    # editors\n    ripgrep\n    fd # finder\n  ];\n}\n";
        let out = inserted(src, "home.packages", "bat");
        assert_eq!(
            out,
            "{\n  home.packages = with pkgs; [\n    # editors\n    ripgrep\n    fd # finder\n    bat\n  ];\n}\n"
        );
    }

    #[test]
    fn already_present_is_detected_and_not_duplicated() {
        let src = "{ home.packages = with pkgs; [\n    ripgrep\n    fd\n  ]; }\n";
        assert_eq!(insert_package(src, "home.packages", "fd"), Insertion::AlreadyPresent);
    }

    #[test]
    fn missing_attribute_or_non_list_yields_not_found() {
        // Attribute absent entirely.
        assert_eq!(
            insert_package("{ home.stateVersion = \"24.05\"; }\n", "home.packages", "bat"),
            Insertion::NotFound
        );
        // Present, but not a plain list (a function call) — we won't guess at editing it.
        assert_eq!(
            insert_package("{ home.packages = lib.optionals true [ ]; }\n", "home.packages", "bat"),
            Insertion::NotFound
        );
        // Doesn't parse → never edited.
        assert_eq!(insert_package("{ home.packages = [ ", "home.packages", "bat"), Insertion::NotFound);
    }

    #[test]
    fn find_list_matches_the_system_attr_too() {
        let src = "{ environment.systemPackages = with pkgs; [ git ]; }\n";
        let out = inserted(src, "environment.systemPackages", "vim");
        assert_eq!(out, "{ environment.systemPackages = with pkgs; [ git vim ]; }\n");
    }

    #[test]
    fn line_diff_shows_the_single_added_line_with_context() {
        let old = "a\nb\nc\nd\n";
        let new = "a\nb\nX\nc\nd\n";
        let diff = line_diff(old, new);
        assert!(diff.contains("+ X\n"), "diff was: {diff}");
        // Only an addition — no removed lines.
        assert!(!diff.lines().any(|l| l.starts_with("- ")), "diff was: {diff}");
        // Surrounding lines are shown as unchanged context.
        assert!(diff.contains("  b\n") && diff.contains("  c\n"), "diff was: {diff}");
    }
}
