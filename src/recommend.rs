// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
    /// Stable slug, unique within the catalog. Used as the anchor a dependent entry's
    /// [`requires`](Recommendation::requires) points at (e.g. codecs → `rpmfusion`).
    pub id: String,
    /// One-line human name, in English (the catalog's source language).
    #[serde(rename = "title")]
    pub title_en: String,
    /// Russian rendering of the name, when the catalog carries one.
    #[serde(default)]
    pub title_ru: Option<String>,
    /// What the user gains, in English.
    #[serde(rename = "why")]
    pub why_en: String,
    /// Russian rendering of the gain.
    #[serde(default)]
    pub why_ru: Option<String>,
    /// Grouping bucket (media | repos | drivers | fonts | gaming | power).
    pub category: String,
    /// Distro ids this applies to; empty means "all distros".
    #[serde(default)]
    pub distros: Vec<String>,
    /// JII specs to install to satisfy it (empty for a `manual`-only entry like a repo enable).
    #[serde(default)]
    pub packages: Vec<String>,
    /// An exact command the user runs to satisfy a step a package install can't express
    /// (enabling a third-party repo). Shown before it runs; `doctor` executes it on "yes"
    /// (interactive), a read-only run only prints it (ADR-0055).
    #[serde(default)]
    pub manual: Option<String>,
    /// The [`id`](Recommendation::id) of a prerequisite entry that must be satisfied **first**
    /// (e.g. codecs and VLC live in RPM Fusion, so they `requires = "rpmfusion"`). When the
    /// user applies this suggestion and the prerequisite isn't yet present, `doctor` enables
    /// the prerequisite before this one — so a dependent never fails with a bare "not found"
    /// because its repo was skipped (ADR-0055). No hard-coded dependency lives in code; it is
    /// declared here in the data.
    #[serde(default)]
    pub requires: Option<String>,
    /// A caveat worth surfacing (e.g. a trust boundary, or "laptops only").
    #[serde(default, rename = "note")]
    pub note_en: Option<String>,
    /// Russian rendering of the caveat.
    #[serde(default)]
    pub note_ru: Option<String>,
    /// Optional identifier whose installed presence means this entry is **already
    /// satisfied**, so `doctor` skips offering it (#1). Use it when the installed name
    /// differs from the install spec — a Flatpak app-id (`com.valvesoftware.Steam` for
    /// `steam:flatpak`) or a repo's release package (`rpmfusion-free-release`). When
    /// absent, satisfaction is derived from `packages` (their bare names).
    #[serde(default)]
    pub check: Option<String>,
}

impl Recommendation {
    /// The name in the active UI language.
    ///
    /// The catalog is data, not `locales/*.toml`, so its prose travels with the entry
    /// (ADR-0090): an entry carries `title_ru` alongside `title`, and a language with no
    /// translation yet falls back to English rather than showing a key. Without this a
    /// Russian user got a Russian program listing English advice, which is exactly the
    /// "written by a machine" feeling the house voice exists to remove.
    pub fn title(&self) -> &str {
        pick(&self.title_ru, &self.title_en)
    }

    /// What the user gains, in the active UI language. See [`title`](Self::title).
    pub fn why(&self) -> &str {
        pick(&self.why_ru, &self.why_en)
    }

    /// The caveat, in the active UI language, if the entry has one.
    pub fn note(&self) -> Option<&str> {
        match (crate::i18n::lang(), &self.note_ru, &self.note_en) {
            ("ru", Some(ru), _) => Some(ru.as_str()),
            (_, _, en) => en.as_deref(),
        }
    }
}

/// Choose the Russian rendering when the active language is Russian and one exists.
fn pick<'a>(ru: &'a Option<String>, en: &'a str) -> &'a str {
    match (crate::i18n::lang(), ru) {
        ("ru", Some(ru)) => ru.as_str(),
        _ => en,
    }
}

impl Recommendation {
    /// Whether this entry applies to a host's distro *family* — its `ID` plus every
    /// `ID_LIKE` token (empty `distros` = every distro).
    ///
    /// Matching the bare `ID` was too strict: Linux Mint saw none of Debian's entries and
    /// Nobara none of Fedora's, though both name their parent in `/etc/os-release`.
    fn applies_to(&self, distro_ids: &[String]) -> bool {
        self.distros.is_empty() || self.distros.iter().any(|d| distro_ids.iter().any(|id| id == d))
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

/// The prerequisite entry that must be enabled **before** `chosen` — or `None` when there is
/// none, it's already satisfied on the system, or it was already enabled this run. Pure, so
/// doctor's ordering (enable RPM Fusion before codecs/VLC, ADR-0055) is unit-tested without a
/// live system. `all` is the full distro-filtered catalog (prerequisites are looked up there,
/// since a satisfied one is filtered out of the offered list); `installed` is the installed-id
/// set; `enabled` is the repos enabled so far in this run (dedupe).
pub fn prerequisite<'a>(
    chosen: &Recommendation,
    all: &[&'a Recommendation],
    installed: &std::collections::HashSet<String>,
    enabled: &std::collections::HashSet<String>,
) -> Option<&'a Recommendation> {
    let req = chosen.requires.as_deref()?;
    if enabled.contains(req) {
        return None;
    }
    let prereq = all.iter().copied().find(|e| e.id == req)?;
    (!prereq.is_satisfied(installed)).then_some(prereq)
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
    pub fn for_distro(&self, distro_ids: &[String]) -> Vec<&Recommendation> {
        let matching: Vec<&Recommendation> =
            self.recommendation.iter().filter(|r| r.applies_to(distro_ids)).collect();

        // Grouped by category, categories in the order they first appear. Both renderers
        // print a header whenever the category changes, so an entry that applies to every
        // distro — Steam — would otherwise open a second `[gaming]` section wherever the
        // per-distro blocks happen to leave it in the file. Authoring order still decides
        // everything else, so the foundational entries (enabling a repo) stay first.
        let mut order: Vec<&str> = Vec::new();
        for r in &matching {
            if !order.contains(&r.category.as_str()) {
                order.push(&r.category);
            }
        }
        order
            .into_iter()
            .flat_map(|cat| matching.iter().copied().filter(move |r| r.category == cat))
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
            assert!(!r.title_en.is_empty());
            assert!(!r.why_en.is_empty());
            assert!(!r.category.is_empty());
            // An entry must offer *something* to do: packages or a manual command.
            assert!(
                !r.packages.is_empty() || r.manual.is_some(),
                "{} does nothing",
                r.title_en
            );
        }
    }

#[test]
    fn every_entry_is_translated_into_every_shipped_language() {
        // The locale files have this guarantee already (en/ru key parity); the catalog is
        // the other half of the user-facing prose and needs the same one, or a Russian
        // session quietly reads English advice (ADR-0090).
        let catalog = Catalog::load().expect("catalog parses");
        for r in &catalog.recommendation {
            assert!(r.title_ru.is_some(), "{} has no title_ru", r.id);
            assert!(r.why_ru.is_some(), "{} has no why_ru", r.id);
            assert_eq!(
                r.note_en.is_some(),
                r.note_ru.is_some(),
                "{} has a note in only one language",
                r.id
            );
        }
    }

    #[test]
    fn entry_titles_are_unique() {
        // Titles must be unique *within a distro* (that's all a user ever sees at once);
        // the same title across distros — "VLC media player" for Fedora and Arch — is fine.
        let catalog = Catalog::load().unwrap();
        for distro in ["fedora", "arch", "debian", "ubuntu", "opensuse"] {
            let family = vec![distro.to_string()];
            let mut titles: Vec<&str> =
                catalog.for_distro(&family).iter().map(|r| r.title_en.as_str()).collect();
            titles.sort_unstable();
            let before = titles.len();
            titles.dedup();
            assert_eq!(before, titles.len(), "duplicate recommendation title on {distro}");
        }
    }

    #[test]
    fn empty_distros_applies_everywhere() {
        let r = Recommendation {
            id: "x".into(),
            title_ru: None,
            why_ru: None,
            note_ru: None,
            title_en: "X".into(),
            why_en: "w".into(),
            category: "media".into(),
            distros: vec![],
            packages: vec!["x".into()],
            manual: None,
            requires: None,
            note_en: None,
            check: None,
        };
        assert!(r.applies_to(&["fedora".to_string()]));
        assert!(r.applies_to(&["arch".to_string()]));
        assert!(r.applies_to(&[]));
    }

    #[test]
    fn satisfied_ids_prefers_check_then_strips_package_specs() {
        let mut r = Recommendation {
            id: "steam".into(),
            title_ru: None,
            why_ru: None,
            note_ru: None,
            title_en: "Steam".into(),
            why_en: "games".into(),
            category: "gaming".into(),
            distros: vec!["fedora".into()],
            packages: vec!["steam:flatpak".into()],
            manual: None,
            requires: None,
            note_en: None,
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
            id: "codecs".into(),
            title_ru: None,
            why_ru: None,
            note_ru: None,
            title_en: "Codecs".into(),
            why_en: "media".into(),
            category: "media".into(),
            distros: vec![],
            packages: vec!["a".into(), "b".into()],
            manual: None,
            requires: None,
            note_en: None,
            check: None,
        };
        let mut set = std::collections::HashSet::new();
        set.insert("a".to_string());
        assert!(!r.is_satisfied(&set)); // only one of two present
        set.insert("b".to_string());
        assert!(r.is_satisfied(&set)); // both present → already done

        // An entry with no identifiers (manual-only, no check) is never "satisfied".
        let manual = Recommendation {
            id: "repo".into(),
            title_ru: None,
            why_ru: None,
            note_ru: None,
            title_en: "Repo".into(),
            why_en: "w".into(),
            category: "repos".into(),
            distros: vec![],
            packages: vec![],
            manual: Some("do it".into()),
            requires: None,
            note_en: None,
            check: None,
        };
        assert!(!manual.is_satisfied(&set));
    }

    fn entry(id: &str, requires: Option<&str>, check: Option<&str>) -> Recommendation {
        Recommendation {
            id: id.into(),
            title_ru: None,
            why_ru: None,
            note_ru: None,
            title_en: id.into(),
            why_en: "w".into(),
            category: "media".into(),
            distros: vec![],
            packages: vec![],
            manual: requires.is_none().then(|| "enable repo".into()),
            requires: requires.map(str::to_string),
            note_en: None,
            check: check.map(str::to_string),
        }
    }

    #[test]
    fn prerequisite_fires_only_when_needed() {
        let rpmfusion = entry("rpmfusion", None, Some("rpmfusion-free-release"));
        let codecs = entry("codecs", Some("rpmfusion"), None);
        let all = vec![&rpmfusion, &codecs];
        let empty = std::collections::HashSet::new();

        // Prereq missing + not yet enabled → enable it first.
        let got = prerequisite(&codecs, &all, &empty, &empty);
        assert_eq!(got.map(|r| r.id.as_str()), Some("rpmfusion"));

        // Already installed on the system → nothing to do.
        let mut installed = std::collections::HashSet::new();
        installed.insert("rpmfusion-free-release".to_string());
        assert!(prerequisite(&codecs, &all, &installed, &empty).is_none());

        // Already enabled earlier this run → not re-run.
        let mut enabled = std::collections::HashSet::new();
        enabled.insert("rpmfusion".to_string());
        assert!(prerequisite(&codecs, &all, &empty, &enabled).is_none());

        // An entry with no prerequisite never triggers one.
        assert!(prerequisite(&rpmfusion, &all, &empty, &empty).is_none());

        // A dangling `requires` (no such entry) resolves to nothing, not a panic.
        let dangling = entry("x", Some("nope"), None);
        let all2 = vec![&dangling];
        assert!(prerequisite(&dangling, &all2, &empty, &empty).is_none());
    }

    #[test]
    fn prerequisites_point_at_a_real_entry() {
        let catalog = Catalog::load().unwrap();
        let ids: std::collections::HashSet<&str> =
            catalog.recommendation.iter().map(|r| r.id.as_str()).collect();
        // Every `requires` must name an existing entry (no dangling prerequisite).
        for r in &catalog.recommendation {
            if let Some(req) = &r.requires {
                assert!(ids.contains(req.as_str()), "{} requires missing entry {req}", r.id);
            }
        }
        // The codec + VLC entries depend on RPM Fusion (the report that motivated ADR-0055).
        let requires = |id: &str| {
            catalog
                .recommendation
                .iter()
                .find(|r| r.id == id)
                .and_then(|r| r.requires.clone())
        };
        assert_eq!(requires("multimedia-codecs").as_deref(), Some("rpmfusion"));
        assert_eq!(requires("vlc").as_deref(), Some("rpmfusion"));
    }

    #[test]
    fn entries_come_grouped_by_category() {
        // Each category must appear exactly once, or the renderer opens a second section
        // with the same header — which a cross-distro entry at the end of the file did.
        let catalog = Catalog::load().unwrap();
        for family in [vec!["fedora"], vec!["arch"], vec!["ubuntu", "debian"], vec!["opensuse"]] {
            let family: Vec<String> = family.iter().map(|s| s.to_string()).collect();
            let mut seen: Vec<&str> = Vec::new();
            let mut last: Option<&str> = None;
            for r in catalog.for_distro(&family) {
                if last != Some(r.category.as_str()) {
                    assert!(
                        !seen.contains(&r.category.as_str()),
                        "category '{}' reopens for {family:?}",
                        r.category
                    );
                    seen.push(&r.category);
                    last = Some(&r.category);
                }
            }
        }
    }

    #[test]
    fn distro_filter_selects_matching_entries() {
        let catalog = Catalog::load().unwrap();
        let fedora = catalog.for_distro(&["fedora".to_string()]);
        assert!(!fedora.is_empty(), "fedora entries expected");
        // A distro named by no entry gets only the universal (empty-distros) ones.
        let alien = catalog.for_distro(&["plan9".to_string()]);
        assert!(alien.iter().all(|r| r.distros.is_empty()));

        // A derivative inherits its parent's entries through ID_LIKE — the reason
        // `applies_to` takes a family and not one id.
        let nobara = catalog.for_distro(&["nobara".to_string(), "fedora".to_string()]);
        assert_eq!(nobara.len(), fedora.len(), "a Fedora derivative sees Fedora's entries");
        let mint = catalog.for_distro(&["linuxmint".to_string(), "ubuntu".to_string(), "debian".to_string()]);
        assert!(!mint.is_empty(), "a Debian derivative must see Debian's entries");
    }
}
