//! The recommend-catalog: curated, distro-aware system-onboarding suggestions.
//!
//! This is a **data subsystem**, not core logic and not a `Provider`. The catalog is
//! authored as TOML in `data/recommend/catalog.toml`, embedded at build time so the binary
//! stays self-contained, and filtered by the host distro. `jii recommend` reports it
//! (Analyze → Explain); nothing here ever modifies the system. The core never branches on
//! the distro — entries *declare* which distros they apply to and this module filters on
//! that data (ADR-0033), so distro-awareness lives in the catalog, not in `if fedora`
//! branches (ADR-0029 preserved).

use serde::Deserialize;

/// The whole catalog, as parsed from the embedded TOML.
#[derive(Debug, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub recommendation: Vec<Recommendation>,
}

/// One curated suggestion.
#[derive(Debug, Clone, Deserialize)]
pub struct Recommendation {
    /// Stable unique slug.
    pub id: String,
    /// One-line human name.
    pub title: String,
    /// What the user gains.
    pub why: String,
    /// Grouping bucket (media | repos | drivers | fonts | gaming | power).
    pub category: String,
    /// Distro ids this applies to; empty means "all distros".
    #[serde(default)]
    pub distros: Vec<String>,
    /// JII specs to install to satisfy it (empty for a `manual`-only entry like a repo enable).
    #[serde(default)]
    pub packages: Vec<String>,
    /// An exact command the user runs themselves, for steps a package install can't express
    /// (enabling a third-party repo). Shown, never executed by JII.
    #[serde(default)]
    pub manual: Option<String>,
    /// A caveat worth surfacing (e.g. a trust boundary, or "laptops only").
    #[serde(default)]
    pub note: Option<String>,
}

impl Recommendation {
    /// Whether this entry applies to a given distro id (empty `distros` = every distro).
    fn applies_to(&self, distro_id: &str) -> bool {
        self.distros.is_empty() || self.distros.iter().any(|d| d == distro_id)
    }
}

/// The catalog TOML, embedded so the binary carries its own data (the `data/sources/`
/// declarative-provider pattern, applied to recommendations).
const CATALOG_TOML: &str = include_str!("../data/recommend/catalog.toml");

impl Catalog {
    /// Parse the embedded catalog. Fails only if the shipped TOML is malformed, which a
    /// unit test guards against — so in practice this is infallible at runtime.
    pub fn load() -> Result<Catalog, toml::de::Error> {
        toml::from_str(CATALOG_TOML)
    }

    /// The entries that apply to `distro_id`, in catalog order (authoring order is the
    /// display order, so the most foundational entries — e.g. enabling a repo — come first).
    pub fn for_distro(&self, distro_id: &str) -> Vec<&Recommendation> {
        self.recommendation
            .iter()
            .filter(|r| r.applies_to(distro_id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_parses() {
        let catalog = Catalog::load().expect("shipped catalog.toml must be valid");
        assert!(!catalog.recommendation.is_empty());
        // Every entry has the required human fields and a category.
        for r in &catalog.recommendation {
            assert!(!r.id.is_empty());
            assert!(!r.title.is_empty());
            assert!(!r.why.is_empty());
            assert!(!r.category.is_empty());
            // An entry must offer *something* to do: packages or a manual command.
            assert!(
                !r.packages.is_empty() || r.manual.is_some(),
                "{} does nothing",
                r.id
            );
        }
    }

    #[test]
    fn entry_ids_are_unique() {
        let catalog = Catalog::load().unwrap();
        let mut ids: Vec<&str> = catalog.recommendation.iter().map(|r| r.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate recommendation id");
    }

    #[test]
    fn empty_distros_applies_everywhere() {
        let r = Recommendation {
            id: "x".into(),
            title: "X".into(),
            why: "w".into(),
            category: "media".into(),
            distros: vec![],
            packages: vec!["x".into()],
            manual: None,
            note: None,
        };
        assert!(r.applies_to("fedora"));
        assert!(r.applies_to("arch"));
        assert!(r.applies_to(""));
    }

    #[test]
    fn distro_filter_selects_matching_entries() {
        let catalog = Catalog::load().unwrap();
        let fedora = catalog.for_distro("fedora");
        assert!(!fedora.is_empty(), "fedora entries expected");
        // A distro named by no entry gets only the universal (empty-distros) ones.
        let alien = catalog.for_distro("plan9");
        assert!(alien.iter().all(|r| r.distros.is_empty()));
    }
}
