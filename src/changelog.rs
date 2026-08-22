// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Release notes — the data behind `jii changelog` and the "here's what you just got"
//! summary printed after `jii update jii`.
//!
//! The notes live in `data/changelog.toml`, embedded at build time (`include_str!`) like the
//! locale files: `jii changelog` must work offline, on a plane, with no GitHub reachable.
//! Each entry carries its own translations (`en` is the source of truth, `ru` the
//! translation, missing `ru` degrades to English — the same rule as [`crate::i18n`]).
//!
//! Text belongs *with* the version it describes, so — unlike every other user-facing string
//! (ADR-0050) — release notes are **not** in `locales/*.toml`: a per-line locale key would
//! mean inventing `changelog.0-1-15.line3` for every bullet of every release, in two files,
//! forever. The command's own chrome (header, hints, errors) is localized normally.
//!
//! A parse failure yields an empty list, never a panic: a broken data file must not break
//! `jii update`. The tests below make that unreachable in practice — they assert the file
//! parses, is in descending version order, and contains an entry for the version this binary
//! was built as, so a release that forgets its notes fails `cargo test`, not the user.

use std::sync::OnceLock;

use serde::Deserialize;

/// The embedded notes.
const SOURCE: &str = include_str!("../data/changelog.toml");

/// One release's notes.
#[derive(Debug, Deserialize)]
pub struct Release {
    /// The released version, without a leading `v` (`0.1.15-beta`).
    pub version: String,
    /// Release date, ISO `YYYY-MM-DD`.
    pub date: String,
    /// English notes — the source of truth.
    en: Vec<String>,
    /// Russian notes; absent falls back to `en`.
    #[serde(default)]
    ru: Option<Vec<String>>,
}

impl Release {
    /// The bullets in the active interface language, falling back to English.
    pub fn notes(&self) -> &[String] {
        match crate::i18n::lang() {
            "ru" => self.ru.as_deref().unwrap_or(&self.en),
            _ => &self.en,
        }
    }
}

#[derive(Debug, Deserialize)]
struct File {
    #[serde(default)]
    release: Vec<Release>,
}

/// Every release JII knows about, newest first.
pub fn releases() -> &'static [Release] {
    static PARSED: OnceLock<Vec<Release>> = OnceLock::new();
    PARSED
        .get_or_init(|| toml::from_str::<File>(SOURCE).map(|f| f.release).unwrap_or_default())
        .as_slice()
}

/// Strip a leading `v` and any surrounding whitespace (`v0.1.15-beta` → `0.1.15-beta`).
fn normalize(version: &str) -> &str {
    let v = version.trim();
    v.strip_prefix('v').unwrap_or(v)
}

/// The numeric core of a version (`0.1.15-beta` → `[0, 1, 15]`), or `None` if it isn't a
/// dotted-number version. Versions stay opaque (ADR-0009); this is only used to order the
/// notes and to place a version we have no exact entry for.
fn core(version: &str) -> Option<Vec<u64>> {
    let core = normalize(version).split(['-', '+']).next()?;
    let parts = core
        .split('.')
        .map(|p| p.parse::<u64>().ok())
        .collect::<Option<Vec<u64>>>()?;
    (!parts.is_empty()).then_some(parts)
}

/// Look up one release. Accepts `v0.1.15-beta`, `0.1.15-beta` and the bare `0.1.15`.
pub fn find(version: &str) -> Option<&'static Release> {
    let wanted = normalize(version);
    releases()
        .iter()
        .find(|r| r.version == wanted)
        // `0.1.15` should find `0.1.15-beta`: nobody types the suffix.
        .or_else(|| releases().iter().find(|r| core(&r.version) == core(wanted)))
}

/// The notes for the version this binary was built as.
pub fn current() -> Option<&'static Release> {
    find(crate::selfupdate::current_version())
}

/// Every release *newer* than `version` — what an update from `version` actually brought.
///
/// Ordering is by the file's own order (newest first, enforced by a test): everything above
/// the matching entry. If `version` isn't in the file (a build from a branch, or a version
/// whose notes were never written) we fall back to comparing numeric cores, and if even that
/// fails we return nothing rather than guess — the caller then says so plainly.
pub fn since(version: &str) -> Vec<&'static Release> {
    let all = releases();
    if let Some(idx) = all.iter().position(|r| r.version == normalize(version)) {
        return all[..idx].iter().collect();
    }
    match core(version) {
        Some(from) => all
            .iter()
            .filter(|r| core(&r.version).is_some_and(|c| c > from))
            .collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_file_parses_and_is_not_empty() {
        assert!(!releases().is_empty(), "data/changelog.toml must parse into releases");
    }

    #[test]
    fn every_release_has_notes_in_both_languages() {
        for r in releases() {
            assert!(!r.en.is_empty(), "{} has no English notes", r.version);
            let ru =
                r.ru.as_ref()
                    .unwrap_or_else(|| panic!("{} has no Russian notes", r.version));
            assert!(!ru.is_empty(), "{} has empty Russian notes", r.version);
            assert_eq!(r.date.len(), 10, "{} needs an ISO date (YYYY-MM-DD)", r.version);
            assert!(!r.version.starts_with('v'), "{} must not carry a leading v", r.version);
        }
    }

    #[test]
    fn releases_are_newest_first() {
        let cores: Vec<Vec<u64>> = releases()
            .iter()
            .map(|r| core(&r.version).unwrap_or_else(|| panic!("{} isn't a dotted version", r.version)))
            .collect();
        for pair in cores.windows(2) {
            assert!(
                pair[0] > pair[1],
                "changelog must be newest-first: {:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    /// The release checklist, enforced: shipping a version whose notes nobody wrote fails
    /// here instead of printing an empty `jii changelog` to the user.
    #[test]
    fn this_build_has_release_notes_at_the_top() {
        let me = env!("CARGO_PKG_VERSION");
        let first = &releases()[0];
        assert_eq!(
            first.version, me,
            "add a data/changelog.toml entry for {me} (newest first) before releasing"
        );
        assert!(current().is_some());
    }

    #[test]
    fn find_accepts_v_prefix_and_a_bare_core() {
        let me = env!("CARGO_PKG_VERSION");
        assert!(find(me).is_some());
        assert!(find(&format!("v{me}")).is_some());
        let bare = me.split('-').next().unwrap();
        assert_eq!(find(bare).map(|r| r.version.as_str()), Some(me));
        assert!(find("9.9.9").is_none());
    }

    #[test]
    fn since_returns_only_newer_releases() {
        let all = releases();
        let second = &all[1];
        let newer = since(&second.version);
        assert_eq!(newer.len(), 1, "one release newer than the second-newest");
        assert_eq!(newer[0].version, all[0].version);
        // The running version has nothing above it.
        assert!(since(&all[0].version).is_empty());
        // An unknown-but-parseable version still places correctly.
        assert_eq!(since("0.0.1").len(), all.len());
        // An unparseable version yields nothing rather than a guess.
        assert!(since("not-a-version").is_empty());
    }
}
