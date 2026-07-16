//! Ranking: order candidates so the best is first.
//!
//! Deterministic and explainable — no hidden ML. The primary key is the configured
//! source priority (optionally adjusted by the active profile); ties break on trust.
//!
//! Profiles: `stable` uses the configured priority as-is; `sandbox` floats Flatpak
//! to the top. `latest` (freshest version) and `minimal` (smallest footprint) need
//! data we do not collect yet (comparable versions / dependency size) and currently
//! behave like `stable` — see docs/ROADMAP.md.

use std::collections::HashSet;

use crate::config::{Config, Profile};
use crate::model::{PackageCandidate, TrustLevel};

/// Filter and order candidates, best first. `query` is the name the user typed: an
/// **exact** name match sorts above a **prefix** match above a mere substring (ADR-0042),
/// so a broadened search (`ayugram` → `ayugram-desktop`, or `jii git` catching `gitk`)
/// still recommends the closest name; source priority and trust break ties within a tier.
pub fn rank(config: &Config, query: &str, mut candidates: Vec<PackageCandidate>) -> Vec<PackageCandidate> {
    // Hard filter: drop anything incompatible with the current arch/libc.
    candidates.retain(|c| c.arch_ok);

    candidates.sort_by(|a, b| {
        name_match_tier(query, &a.name)
            .cmp(&name_match_tier(query, &b.name))
            .then(effective_rank(config, a).cmp(&effective_rank(config, b)))
            .then(a.trust.cmp(&b.trust))
            // A shorter name is the closer match among same-tier prefixes
            // (`git` before `git-core` for the query `git`).
            .then(a.name.len().cmp(&b.name.len()))
    });
    candidates
}

/// Recent downloads below which a language-registry package reads as obscure. Deliberately
/// low: the point is to catch name-squatters (tens of downloads), not to punish small tools.
const POPULARITY_FLOOR: u64 = 1_000;

/// Flag junk candidates — an obscure language-registry package whose name shadows a
/// well-known tool (the `htop`-on-PyPI / npm-squatter class the owner keeps tripping over).
///
/// `network_registry` holds the ids of sources that answer searches purely over the network
/// ([`Provider::can_search`](crate::provider::Provider::can_search)) — the tier where
/// name-squatting lives; the engine derives the set from provider *traits*, so the core
/// still never branches on a concrete source id (ADR-0004). A flagged candidate is
/// **downgraded to `untrusted`** (auto mode never installs it — ADR-0006) and marked
/// `suspicious` so the CLI warns loudly. Never a hard filter: an explicit user pick still works.
///
/// A candidate is obscure when:
///   • its recent downloads (where the registry reports them) are under [`POPULARITY_FLOOR`]; or
///   • no popularity signal exists (PyPI…) **and** the metadata carries junk markers —
///     no description at all, or a `0.0.x` version.
/// Path-style names (`owner/repo`, `github.com/x/y`) were typed knowingly and are skipped.
pub fn mark_suspicious(network_registry: &HashSet<&str>, candidates: &mut [PackageCandidate]) {
    for c in candidates.iter_mut() {
        if c.trust != TrustLevel::Community
            || !network_registry.contains(c.source_id.as_str())
            || c.name.contains('/')
        {
            continue;
        }
        // A provider may pre-mark registry-specific junk (PyPI: a years-stale sole
        // release); the generic signals below add the popularity/metadata checks.
        let obscure = c.suspicious
            || match c.popularity {
                Some(downloads) => downloads < POPULARITY_FLOOR,
                None => {
                    c.summary.is_none()
                        || c.version.as_ref().is_some_and(|v| v.0.starts_with("0.0."))
                }
            };
        if obscure {
            c.suspicious = true;
            c.trust = TrustLevel::Untrusted;
        }
    }
}

/// How closely a candidate's name matches what the user typed. Lower is better:
/// 0 exact · 1 prefix · 2 substring · 3 unrelated (all case-insensitive).
///
/// A dotted id (a Flatpak app-id like `md.obsidian.Obsidian` or `org.mozilla.firefox`) also
/// matches on its **last segment**, so `obsidian`/`firefox` count as an exact match against
/// the app-id — otherwise the reverse-DNS name loses to an unrelated same-named crate/pypi
/// package on a plain substring, and `jii firefox` reads "no exact match, closest …".
fn name_match_tier(query: &str, name: &str) -> u8 {
    let q = query.to_ascii_lowercase();
    let n = name.to_ascii_lowercase();
    let direct = tier_of(&q, &n);
    match n.rsplit('.').next() {
        Some(tail) if tail != n => direct.min(tier_of(&q, tail)),
        _ => direct,
    }
}

/// Match tier of `query` against a single candidate string (both already lowercased).
fn tier_of(q: &str, n: &str) -> u8 {
    if n == q {
        0
    } else if n.starts_with(q) {
        1
    } else if n.contains(q) {
        2
    } else {
        3
    }
}

/// A candidate's source priority after applying the active profile. Lower is better.
fn effective_rank(config: &Config, candidate: &PackageCandidate) -> i64 {
    let base = config.source_rank(&candidate.source_id) as i64;
    base + profile_adjustment(config.install.profile, &candidate.source_id)
}

/// Per-profile adjustment to a source's rank (negative floats it up).
fn profile_adjustment(profile: Profile, source_id: &str) -> i64 {
    match profile {
        // Prefer sandboxed Flatpak above everything else.
        Profile::Sandbox if source_id == "flatpak" => -1000,
        // `latest`/`minimal` are reserved until we have version/footprint data.
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PkgVersion, TrustLevel};
    use serde_json::json;

    fn candidate(source: &str, trust: TrustLevel, arch_ok: bool) -> PackageCandidate {
        PackageCandidate {
            name: "pkg".into(),
            source_id: source.into(),
            version: Some(PkgVersion::new("1.0")),
            trust,
            arch_ok,
            signed: true,
            summary: None,
            popularity: None,
            suspicious: false,
            raw: json!({}),
        }
    }

    fn ranked_sources(config: &Config, sources: &[&str]) -> Vec<String> {
        let cands = sources
            .iter()
            .map(|s| candidate(s, TrustLevel::Official, true))
            .collect();
        // All candidates share the name "pkg"; querying "pkg" keeps them in one match tier,
        // so ordering is decided by priority/trust (what these tests exercise).
        rank(config, "pkg", cands)
            .into_iter()
            .map(|c| c.source_id)
            .collect()
    }

    /// A named candidate from one source, for the name-match-tier tests.
    fn named(source: &str, name: &str) -> PackageCandidate {
        let mut c = candidate(source, TrustLevel::Official, true);
        c.name = name.into();
        c
    }

    #[test]
    fn orders_by_configured_source_priority() {
        let cfg = Config::default(); // dnf ranks above flatpak
        assert_eq!(ranked_sources(&cfg, &["flatpak", "dnf"]), ["dnf", "flatpak"]);
    }

    #[test]
    fn drops_arch_incompatible_candidates() {
        let cfg = Config::default();
        let ranked = rank(&cfg, "pkg", vec![candidate("dnf", TrustLevel::Official, false)]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn trust_breaks_ties_within_same_source() {
        let cfg = Config::default();
        let ranked = rank(
            &cfg,
            "pkg",
            vec![
                candidate("dnf", TrustLevel::Community, true),
                candidate("dnf", TrustLevel::Official, true),
            ],
        );
        assert_eq!(ranked[0].trust, TrustLevel::Official);
    }

    #[test]
    fn exact_name_match_outranks_a_higher_priority_prefix_match() {
        // flatpak ranks below dnf by priority, but an *exact* flatpak name beats a dnf
        // package that merely starts with the query — the closest name wins (ADR-0042).
        let cfg = Config::default();
        let ranked = rank(
            &cfg,
            "ayugram",
            vec![
                named("dnf", "ayugram-desktop"), // prefix match, higher-priority source
                named("flatpak", "ayugram"),     // exact match, lower-priority source
            ],
        );
        assert_eq!(ranked[0].name, "ayugram");
        assert_eq!(ranked[1].name, "ayugram-desktop");
    }

    #[test]
    fn among_prefix_matches_the_shorter_name_is_closer() {
        // No exact match: `git` → both are prefix matches; the shorter name is the tighter
        // fit and sorts first.
        let cfg = Config::default();
        let ranked = rank(
            &cfg,
            "git",
            vec![named("dnf", "git-core"), named("dnf", "gitk")],
        );
        assert_eq!(ranked[0].name, "gitk"); // 4 chars < "git-core" (8)
    }

    #[test]
    fn name_match_tiers_order_exact_prefix_substring_unrelated() {
        assert_eq!(name_match_tier("git", "git"), 0);
        assert_eq!(name_match_tier("git", "GIT"), 0); // case-insensitive
        assert_eq!(name_match_tier("git", "gitk"), 1);
        assert_eq!(name_match_tier("git", "libgit2"), 2);
        assert_eq!(name_match_tier("git", "firefox"), 3);
    }

    #[test]
    fn dotted_appid_matches_on_its_last_segment() {
        // A Flatpak app-id is an exact match on its tail, so `firefox`/`obsidian` don't lose
        // to a plain substring — fixes "no exact match, closest org.mozilla.firefox".
        assert_eq!(name_match_tier("firefox", "org.mozilla.firefox"), 0);
        assert_eq!(name_match_tier("obsidian", "md.obsidian.Obsidian"), 0);
        // A plain (dot-free) name is unaffected.
        assert_eq!(name_match_tier("git", "gitk"), 1);
    }

    #[test]
    fn exact_appid_tail_outranks_an_unrelated_same_named_package() {
        // `jii obsidian`: the Flatpak app-id (exact on its tail) must beat an unrelated pypi
        // crate literally named "Obsidian", even though pipx ranks below flatpak by priority.
        let cfg = Config::default();
        let ranked = rank(
            &cfg,
            "obsidian",
            vec![
                named("pipx", "Obsidian"),              // unrelated same-name, lower priority
                named("flatpak", "md.obsidian.Obsidian"), // the real one
            ],
        );
        assert_eq!(ranked[0].name, "md.obsidian.Obsidian");
    }

    fn netsrc() -> HashSet<&'static str> {
        ["pipx", "npm", "cargo", "flatpak"].into_iter().collect()
    }

    /// A community candidate from a network registry, with tunable junk signals.
    fn registry_candidate(
        source: &str,
        popularity: Option<u64>,
        summary: Option<&str>,
        version: Option<&str>,
    ) -> PackageCandidate {
        let mut c = candidate(source, TrustLevel::Community, true);
        c.name = "htop".into();
        c.popularity = popularity;
        c.summary = summary.map(str::to_string);
        c.version = version.map(PkgVersion::new);
        c
    }

    #[test]
    fn low_or_missing_popularity_with_junk_markers_is_suspicious() {
        let mut cands = vec![
            registry_candidate("npm", Some(12), Some("legit summary"), Some("3.1")), // low downloads
            registry_candidate("pipx", None, None, Some("3.1")),                     // no summary
            registry_candidate("pipx", None, Some("desc"), Some("0.0.3")),           // 0.0.x squat
        ];
        mark_suspicious(&netsrc(), &mut cands);
        for c in &cands {
            assert!(c.suspicious, "{}: expected suspicious", c.source_id);
            assert_eq!(c.trust, TrustLevel::Untrusted);
        }
    }

    #[test]
    fn healthy_registry_candidates_are_left_alone() {
        let mut cands = vec![
            registry_candidate("npm", Some(2_000_000), Some("prettier"), Some("3.9")), // popular
            registry_candidate("pipx", None, Some("real app"), Some("24.1.0")),        // rich metadata
        ];
        mark_suspicious(&netsrc(), &mut cands);
        for c in &cands {
            assert!(!c.suspicious);
            assert_eq!(c.trust, TrustLevel::Community);
        }
    }

    #[test]
    fn official_local_and_pathstyle_candidates_are_never_flagged() {
        // An official source is out of scope; a non-network source too; a path-style name
        // (go module / owner/repo) was typed knowingly.
        let mut official = vec![candidate("dnf", TrustLevel::Official, true)];
        mark_suspicious(&netsrc(), &mut official);
        assert!(!official[0].suspicious);

        let mut local = vec![registry_candidate("nix", None, None, Some("0.0.1"))];
        mark_suspicious(&netsrc(), &mut local); // "nix" not in the network set
        assert!(!local[0].suspicious);

        let mut path = registry_candidate("npm", Some(5), None, Some("0.0.1"));
        path.name = "github.com/x/y".into();
        let mut v = vec![path];
        mark_suspicious(&netsrc(), &mut v);
        assert!(!v[0].suspicious);
    }

    #[test]
    fn sandbox_profile_floats_flatpak_to_the_top() {
        let mut cfg = Config::default();
        cfg.install.profile = Profile::Sandbox;
        assert_eq!(ranked_sources(&cfg, &["dnf", "flatpak"]), ["flatpak", "dnf"]);
    }

    #[test]
    fn stable_profile_keeps_distro_first() {
        let mut cfg = Config::default();
        cfg.install.profile = Profile::Stable;
        assert_eq!(ranked_sources(&cfg, &["flatpak", "dnf"]), ["dnf", "flatpak"]);
    }
}
