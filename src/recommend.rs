//! The recommend-catalog: curated, distro-aware system-onboarding suggestions.
//!
//! This is a **data subsystem**, not core logic and not a `Provider`. The catalog is
//! authored as TOML in `data/recommend/catalog.toml`, embedded at build time so the binary
//! stays self-contained, and filtered by the host distro. `jii doctor` reports it at its tail
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
    /// One-line human name. (Entries still carry a `id` slug in the TOML for authoring, but
    /// nothing reads it now that suggestions are applied by running the shown command.)
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
    /// Optional identifier whose installed presence means this entry is **already
    /// satisfied**, so `doctor` skips offering it (#1). Use it when the installed name
    /// differs from the install spec — a Flatpak app-id (`com.valvesoftware.Steam` for
    /// `steam:flatpak`) or a repo's release package (`rpmfusion-free-release`). When
    /// absent, satisfaction is derived from `packages` (their bare names).
    #[serde(default)]
    pub check: Option<String>,
}

impl Recommendation {
    /// Whether this entry applies to a given distro id (empty `distros` = every distro).
    fn applies_to(&self, distro_id: &str) -> bool {
        self.distros.is_empty() || self.distros.iter().any(|d| d == distro_id)
    }

    /// Identifiers whose installed presence means this suggestion is already done. The
    /// explicit `check` wins; otherwise the `packages` with any `:source`/`@ref` stripped.
    /// Empty means "can't tell" — such an entry is always offered.
    pub fn satisfied_ids(&self) -> Vec<String> {
        if let Some(check) = &self.check {
            return vec![check.clone()];
        }
        self.packages
            .iter()
            .map(|p| p.split([':', '@']).next().unwrap_or(p).to_string())
            .collect()
    }

    /// Whether every identifier of this suggestion is present in the installed set — i.e.
    /// the user has already done it. An entry with no derivable identifiers is never
    /// considered satisfied (we'd rather offer than wrongly hide it).
    pub fn is_satisfied(&self, installed: &std::collections::HashSet<String>) -> bool {
        let ids = self.satisfied_ids();
        !ids.is_empty() && ids.iter().all(|id| installed.contains(id))
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
            assert!(!r.title.is_empty());
            assert!(!r.why.is_empty());
            assert!(!r.category.is_empty());
            // An entry must offer *something* to do: packages or a manual command.
            assert!(
                !r.packages.is_empty() || r.manual.is_some(),
                "{} does nothing",
                r.title
            );
        }
    }

    #[test]
    fn entry_titles_are_unique() {
        let catalog = Catalog::load().unwrap();
        let mut titles: Vec<&str> = catalog.recommendation.iter().map(|r| r.title.as_str()).collect();
        titles.sort_unstable();
        let before = titles.len();
        titles.dedup();
        assert_eq!(before, titles.len(), "duplicate recommendation title");
    }

    #[test]
    fn empty_distros_applies_everywhere() {
        let r = Recommendation {
            title: "X".into(),
            why: "w".into(),
            category: "media".into(),
            distros: vec![],
            packages: vec!["x".into()],
            manual: None,
            note: None,
            check: None,
        };
        assert!(r.applies_to("fedora"));
        assert!(r.applies_to("arch"));
        assert!(r.applies_to(""));
    }

    #[test]
    fn satisfied_ids_prefers_check_then_strips_package_specs() {
        let mut r = Recommendation {
            title: "Steam".into(),
            why: "games".into(),
            category: "gaming".into(),
            distros: vec!["fedora".into()],
            packages: vec!["steam:flatpak".into()],
            manual: None,
            note: None,
            check: Some("com.valvesoftware.Steam".into()),
        };
        // Explicit check wins (Flatpak app-id, not the "steam" spec).
        assert_eq!(r.satisfied_ids(), vec!["com.valvesoftware.Steam".to_string()]);
        // Without a check, the package spec's bare name is used.
        r.check = None;
        assert_eq!(r.satisfied_ids(), vec!["steam".to_string()]);
    }

    #[test]
    fn is_satisfied_only_when_all_ids_installed() {
        let r = Recommendation {
            title: "Codecs".into(),
            why: "media".into(),
            category: "media".into(),
            distros: vec![],
            packages: vec!["a".into(), "b".into()],
            manual: None,
            note: None,
            check: None,
        };
        let mut set = std::collections::HashSet::new();
        set.insert("a".to_string());
        assert!(!r.is_satisfied(&set)); // only one of two present
        set.insert("b".to_string());
        assert!(r.is_satisfied(&set)); // both present → already done

        // An entry with no identifiers (manual-only, no check) is never "satisfied".
        let manual = Recommendation {
            title: "Repo".into(),
            why: "w".into(),
            category: "repos".into(),
            distros: vec![],
            packages: vec![],
            manual: Some("do it".into()),
            note: None,
            check: None,
        };
        assert!(!manual.is_satisfied(&set));
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
