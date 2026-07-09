//! Ranking: order candidates so the best is first.
//!
//! Deterministic and explainable — no hidden ML. The primary key is the configured
//! source priority (optionally adjusted by the active profile); ties break on trust.
//!
//! Profiles: `stable` uses the configured priority as-is; `sandbox` floats Flatpak
//! to the top. `latest` (freshest version) and `minimal` (smallest footprint) need
//! data we do not collect yet (comparable versions / dependency size) and currently
//! behave like `stable` — see docs/ROADMAP.md.

use crate::config::{Config, Profile};
use crate::model::PackageCandidate;

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

/// How closely a candidate's name matches what the user typed. Lower is better:
/// 0 exact · 1 prefix · 2 substring · 3 unrelated (all case-insensitive).
fn name_match_tier(query: &str, name: &str) -> u8 {
    let q = query.to_ascii_lowercase();
    let n = name.to_ascii_lowercase();
    if n == q {
        0
    } else if n.starts_with(&q) {
        1
    } else if n.contains(&q) {
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
