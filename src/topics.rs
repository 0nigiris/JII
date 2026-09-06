// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Topic search: answering what a person *meant*, when what they typed isn't a package name.
//!
//! Every source searches by name, so `jii search markdown` used to answer with a library
//! literally called `markdown` — while the person wanted a markdown editor. A topic collects
//! the words someone might type for a concept, in every shipped language, and names the
//! programs that actually answer them (`data/topics.toml`, ADR-0091).
//!
//! This is a **data** layer, not intelligence: no model, no scoring, nothing that changes
//! behind the user's back. The engine consults it only when the literal search found no exact
//! name match, so typing a real package name always wins, and `--exact` skips it entirely.

use serde::Deserialize;

/// The embedded topic catalog (compiled in, like the locales and the recommend catalog).
const TOPICS_TOML: &str = include_str!("../data/topics.toml");

/// The whole catalog.
#[derive(Debug, Clone, Deserialize)]
pub struct Topics {
    #[serde(default)]
    pub topic: Vec<Topic>,
}

/// One concept and the programs that answer it.
#[derive(Debug, Clone, Deserialize)]
pub struct Topic {
    /// Stable slug, unique within the catalog.
    pub id: String,
    /// What the user might type, lowercased on match. Any shipped language.
    pub terms: Vec<String>,
    /// English name of the topic.
    #[serde(rename = "title")]
    pub title_en: String,
    /// Russian name (required — the same parity rule the recommend catalog has, ADR-0090).
    pub title_ru: String,
    /// Program names, best first. Bare names: every source gets to answer and the usual
    /// ranking and trust rules decide, so the core still learns nothing about sources.
    pub picks: Vec<String>,
}

impl Topic {
    /// The topic's name in the active UI language.
    pub fn title(&self) -> &str {
        match crate::i18n::lang() {
            "ru" => &self.title_ru,
            _ => &self.title_en,
        }
    }
}

impl Topics {
    /// Parse the embedded catalog. Fails only on malformed shipped TOML, which a unit test
    /// guards against — so in practice this is infallible at runtime.
    pub fn load() -> Result<Topics, toml::de::Error> {
        toml::from_str(TOPICS_TOML)
    }

    /// The topic a query names, if any.
    ///
    /// Matching is deliberately strict: the whole query must equal one of a topic's terms,
    /// case- and whitespace-insensitively. A substring rule would make `jii search vim`
    /// answer with "virtual machines" (`vm` is a term), and a fuzzy one would guess. When
    /// JII cannot be sure what someone meant, saying nothing is the honest answer — the
    /// literal results are already on screen.
    pub fn lookup(&self, query: &str) -> Option<&Topic> {
        let q = normalize(query);
        if q.is_empty() {
            return None;
        }
        self.topic.iter().find(|t| t.terms.iter().any(|term| normalize(term) == q))
    }
}

/// Lowercase, collapse inner whitespace, trim. `"  Web   Browser "` and `"web browser"`
/// are the same question.
fn normalize(s: &str) -> String {
    s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Topics {
        Topics::load().expect("the shipped catalog parses")
    }

    #[test]
    fn embedded_catalog_parses_and_is_not_empty() {
        assert!(!catalog().topic.is_empty());
    }

    #[test]
    fn ids_are_unique_and_every_topic_names_programs() {
        let topics = catalog();
        let mut ids: Vec<&str> = topics.topic.iter().map(|t| t.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate topic id");
        for t in &topics.topic {
            assert!(!t.picks.is_empty(), "{} names no programs", t.id);
            assert!(!t.terms.is_empty(), "{} has no terms", t.id);
        }
    }

    #[test]
    fn a_term_belongs_to_exactly_one_topic() {
        // Two topics claiming "editor" would make the answer depend on file order, which is
        // not an answer. The catalog has to decide.
        let topics = catalog();
        let mut seen: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
        for t in &topics.topic {
            for term in &t.terms {
                let key = normalize(term);
                if let Some(other) = seen.insert(key.clone(), &t.id) {
                    panic!("term {key:?} is claimed by both {other} and {}", t.id);
                }
            }
        }
    }

    #[test]
    fn a_query_matches_whole_terms_only() {
        let topics = catalog();
        assert_eq!(topics.lookup("markdown").map(|t| t.id.as_str()), Some("markdown-editor"));
        assert_eq!(topics.lookup("  Web   Browser ").map(|t| t.id.as_str()), Some("browser"));
        assert_eq!(topics.lookup("браузер").map(|t| t.id.as_str()), Some("browser"));
        // `vm` is a term of the virtualization topic; `vim` is a program and must not match it.
        assert!(topics.lookup("vim").is_none());
        assert!(topics.lookup("").is_none());
    }

    #[test]
    fn both_shipped_languages_name_every_topic() {
        for t in &catalog().topic {
            assert!(!t.title_en.trim().is_empty(), "{} has no English title", t.id);
            assert!(!t.title_ru.trim().is_empty(), "{} has no Russian title", t.id);
        }
    }
}
