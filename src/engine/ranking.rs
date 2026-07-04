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

/// Filter and order candidates, best first.
pub fn rank(config: &Config, mut candidates: Vec<PackageCandidate>) -> Vec<PackageCandidate> {
    // Hard filter: drop anything incompatible with the current arch/libc.
    candidates.retain(|c| c.arch_ok);

    candidates.sort_by(|a, b| {
        effective_rank(config, a)
            .cmp(&effective_rank(config, b))
            .then(a.trust.cmp(&b.trust))
    });
    candidates
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
        rank(config, cands)
            .into_iter()
            .map(|c| c.source_id)
            .collect()
    }

    #[test]
    fn orders_by_configured_source_priority() {
        let cfg = Config::default(); // dnf ranks above flatpak
        assert_eq!(ranked_sources(&cfg, &["flatpak", "dnf"]), ["dnf", "flatpak"]);
    }

    #[test]
    fn drops_arch_incompatible_candidates() {
        let cfg = Config::default();
        let ranked = rank(&cfg, vec![candidate("dnf", TrustLevel::Official, false)]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn trust_breaks_ties_within_same_source() {
        let cfg = Config::default();
        let ranked = rank(
            &cfg,
            vec![
                candidate("dnf", TrustLevel::Community, true),
                candidate("dnf", TrustLevel::Official, true),
            ],
        );
        assert_eq!(ranked[0].trust, TrustLevel::Official);
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
