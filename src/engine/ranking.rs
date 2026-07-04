//! Ranking: order candidates so the best is first.
//!
//! Phase 1 is deliberately minimal — filter incompatible candidates, then sort by
//! configured source priority and trust. The weighted scoring with tie-breakers
//! (freshness, health, size) arrives in Phase 3; the signature stays the same.

use crate::config::Config;
use crate::model::PackageCandidate;

/// Filter and order candidates, best first.
pub fn rank(config: &Config, mut candidates: Vec<PackageCandidate>) -> Vec<PackageCandidate> {
    // Hard filter: drop anything incompatible with the current arch/libc.
    candidates.retain(|c| c.arch_ok);

    // Primary key: configured source priority. Tie-breaker: trust (more trusted
    // first — `TrustLevel` orders Official < Community < Untrusted).
    candidates.sort_by(|a, b| {
        config
            .source_rank(&a.source_id)
            .cmp(&config.source_rank(&b.source_id))
            .then(a.trust.cmp(&b.trust))
    });
    candidates
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

    #[test]
    fn orders_by_configured_source_priority() {
        let cfg = Config::default(); // dnf ranks above flatpak
        let ranked = rank(
            &cfg,
            vec![
                candidate("flatpak", TrustLevel::Official, true),
                candidate("dnf", TrustLevel::Official, true),
            ],
        );
        assert_eq!(ranked[0].source_id, "dnf");
        assert_eq!(ranked[1].source_id, "flatpak");
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
}
