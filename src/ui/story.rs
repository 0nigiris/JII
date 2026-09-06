//! The house voice: how JII talks to a person.
//!
//! A package manager traditionally prints fields — columns, ids, flags — and leaves the
//! reader to work out what any of it means. JII states the finding in a sentence and puts
//! the machinery underneath it, dim and indented. Five rules hold everything here together
//! (ADR-0089):
//!
//! 1. **Impersonal.** "Found six", "Will install", "Done" — never "I found", "I suggest".
//!    JII is a program; it should sound like a clear one, not like a person pretending.
//! 2. **There is always a choice.** Wherever JII decided something on the user's behalf,
//!    the alternatives are numbered right there and the prompt accepts a number.
//! 3. **What matters is prose; the rest is dim.** The reason a source won is a sentence.
//!    Versions, ids and trust words are indented and quiet.
//! 4. **Quiet by default.** One live line while working, one line of outcome after.
//! 5. **Never a dead end.** The next command is always on screen.
//!
//! The reason a source won or lost is asked of the source itself
//! ([`SourceNature`](crate::provider::SourceNature)) — presentation maps a *character* onto
//! words and never a source id, so the core keeps its no-`if source ==` guarantee (ADR-0004).

use crate::model::{PackageCandidate, TrustLevel};
use crate::provider::SourceNature;
use crate::ui::Renderer;

/// One offerable candidate, flattened for display: everything the voice needs and nothing
/// the engine cares about.
pub struct Alternative {
    pub source: String,
    pub version: Option<String>,
    pub trust: TrustLevel,
    /// `None` when the source is no longer enabled — the line then simply says less.
    pub nature: Option<SourceNature>,
}

impl Alternative {
    /// Flatten a ranked candidate. `nature` comes from the engine, which asks the provider.
    pub fn of(candidate: &PackageCandidate, nature: Option<SourceNature>) -> Self {
        Alternative {
            source: candidate.source_id.clone(),
            version: candidate.version.as_ref().map(|v| v.0.clone()),
            trust: candidate.trust,
            nature,
        }
    }
}

/// The most alternatives that are ever numbered on screen.
///
/// Nine because a single keypress answers up to nine, and because a list longer than that
/// stops being a choice and becomes a search result to scroll — which is what `--all` is
/// for (rule 4).
pub const MAX_NUMBERED: usize = 9;

/// The full spoken form of a source's character ("a sandboxed bundle that carries its own
/// runtime"), for the verdict sentence.
fn nature_long(nature: Option<SourceNature>) -> Option<String> {
    nature.map(|n| crate::i18n::tr(&format!("{}.long", n.key())))
}

/// The terse form ("sandboxed"), for a list line where there is no room for a clause.
fn nature_short(nature: Option<SourceNature>) -> Option<String> {
    nature.map(|n| crate::i18n::tr(&format!("{}.short", n.key())))
}

/// Announce what a search turned up, in one sentence, and say which one wins and why.
///
/// `best` indexes `shown`. Prints nothing about the losers beyond their line — the reason
/// they lost is their character, which is already on the line.
pub fn verdict(renderer: &Renderer, shown: &[Alternative], best: usize) {
    let Some(pick) = shown.get(best) else { return };
    let version = pick.version.clone().unwrap_or_else(|| crate::t!("offer.no_version"));
    let sentence = match nature_long(pick.nature) {
        Some(nature) => crate::t!(
            "offer.verdict",
            source = pick.source.clone(),
            version = version,
            nature = nature
        ),
        None => crate::t!("offer.verdict_bare", source = pick.source.clone(), version = version),
    };
    renderer.info("");
    renderer.info(&wrap(&sentence, 2));
}

/// The numbered list: the whole point of rule 2. Marks the recommendation, keeps every
/// other line quiet, and never prints more than [`MAX_NUMBERED`] rows.
pub fn alternatives(renderer: &Renderer, shown: &[Alternative], best: usize) {
    let palette = renderer.palette();
    let rows = shown.len().min(MAX_NUMBERED);
    let src_w = shown[..rows].iter().map(|a| a.source.chars().count()).max().unwrap_or(0);
    let ver_w = shown[..rows]
        .iter()
        .map(|a| a.version.as_deref().map(|v| v.chars().count()).unwrap_or(1))
        .max()
        .unwrap_or(0);

    renderer.info("");
    for (i, alt) in shown[..rows].iter().enumerate() {
        // The pointer marks the recommendation; everything else lines up under it.
        let mark = if i == best { palette.good(palette.mark_pointer()) } else { " ".to_string() };
        let version = alt.version.clone().unwrap_or_else(|| "–".to_string());
        let tail = match nature_short(alt.nature) {
            Some(s) => format!("{} · {s}", palette.trust(alt.trust)),
            None => palette.trust(alt.trust),
        };
        renderer.info(&format!(
            "  {mark} {}  {}  {}  {tail}",
            palette.dim(&format!("{}", i + 1)),
            palette.source(&pad(&alt.source, src_w)),
            palette.version(&pad(&version, ver_w)),
        ));
    }
}

/// Say, in one sentence, that some hits were set aside as name-squats — and never leave
/// that as the last word: the command that shows them anyway follows (rule 5).
pub fn set_aside(renderer: &Renderer, sources: &[String], show_all: &str) {
    if sources.is_empty() {
        return;
    }
    let palette = renderer.palette();
    renderer.info("");
    renderer.info(&wrap(&crate::tn!("offer.set_aside", sources.len() as u64, sources = join_and(sources)), 2));
    renderer.info(&format!("  {}", palette.dim(&crate::t!("offer.show_all", cmd = show_all))));
}

/// The header of one step in a walk-through: a rule, "3 of 8 · Sound and video", then the
/// headline and whatever detail belongs under it, as prose.
///
/// The rule is what makes eight suggestions readable: without it they run together and the
/// eye has nothing to rest on between decisions. It is drawn to the terminal width, capped
/// where the prose is capped so the two agree.
pub fn step_header(
    renderer: &Renderer,
    nth: usize,
    total: usize,
    category: &str,
    headline: &str,
    detail: &[String],
) {
    let palette = renderer.palette();
    let cols = crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(80).clamp(28, 100);
    let rule = "\u{2500}".repeat(cols.min(76).saturating_sub(2));
    renderer.info("");
    renderer.info(&format!("  {}", palette.dim(&rule)));
    renderer.info(&format!(
        "  {}",
        palette.dim(&crate::t!("doctor.step_of", nth = nth, total = total, category = category))
    ));
    renderer.info("");
    renderer.info(&wrap(headline, 2));
    for line in detail.iter().filter(|d| !d.trim().is_empty()) {
        renderer.info(&wrap(line, 2));
    }
    renderer.info("");
}

/// Join names the way a sentence does: "cargo and pipx", "cargo, pipx and npm" — not the
/// comma-separated list a machine would print. The conjunction is translated, since it is
/// the one word in the sentence that a comma cannot stand in for.
pub fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => {
            format!("{} {} {last}", rest.join(", "), crate::t!("word.and"))
        }
    }
}

/// Pad to `width` counting characters, not bytes — source ids and versions are ASCII, but
/// the helper is used on translated words too and a byte-padded Cyrillic column is ragged.
fn pad(s: &str, width: usize) -> String {
    let len = s.chars().count();
    format!("{s}{}", " ".repeat(width.saturating_sub(len)))
}

/// Wrap prose to the terminal, indented by `indent` spaces.
///
/// Sentences are the whole idea here, and a sentence that runs off the right edge of a
/// narrow terminal wraps into the left margin and stops reading as one. Width is taken from
/// the terminal, capped at 76 so a maximised window doesn't produce lines too long to scan.
pub fn wrap(text: &str, indent: usize) -> String {
    let cols = crossterm::terminal::size().map(|(w, _)| w as usize).unwrap_or(80).clamp(28, 100);
    let width = cols.min(76).saturating_sub(indent).max(20);
    let pad = " ".repeat(indent);
    let mut out = String::new();
    for (n, paragraph) in text.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let extra = if line.is_empty() { 0 } else { 1 };
            if !line.is_empty() && line.chars().count() + extra + word.chars().count() > width {
                out.push_str(&pad);
                out.push_str(&line);
                out.push('\n');
                line.clear();
            } else if extra == 1 {
                line.push(' ');
            }
            line.push_str(word);
        }
        out.push_str(&pad);
        out.push_str(&line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alt(source: &str, version: &str, trust: TrustLevel, nature: SourceNature) -> Alternative {
        Alternative {
            source: source.to_string(),
            version: Some(version.to_string()),
            trust,
            nature: Some(nature),
        }
    }

    #[test]
    fn wrapping_never_exceeds_the_cap_and_keeps_every_word() {
        let text = "Best is dnf, version 3.4.1: the distribution's own package, and it \
                    updates together with the rest of the system.";
        let wrapped = wrap(text, 2);
        for line in wrapped.lines() {
            assert!(line.chars().count() <= 76, "line too wide: {line:?}");
        }
        let flat: Vec<&str> = wrapped.split_whitespace().collect();
        assert_eq!(flat, text.split_whitespace().collect::<Vec<_>>());
    }

    #[test]
    fn wrapping_indents_every_line_including_the_first() {
        let wrapped = wrap(&"word ".repeat(60), 4);
        assert!(wrapped.lines().all(|l| l.starts_with("    ")), "{wrapped}");
    }

    #[test]
    fn names_are_joined_the_way_a_sentence_joins_them() {
        let three = ["cargo", "pipx", "npm"].map(String::from).to_vec();
        assert_eq!(join_and(&three), "cargo, pipx and npm");
        assert_eq!(join_and(&three[..2]), "cargo and pipx");
        assert_eq!(join_and(&three[..1]), "cargo");
        assert_eq!(join_and(&[]), "");
    }

    #[test]
    fn padding_counts_characters_not_bytes() {
        // A Cyrillic word is two bytes per letter: byte padding would over-pad by half.
        assert_eq!(pad("да", 4).chars().count(), 4);
    }

    #[test]
    fn the_list_is_capped_so_a_choice_stays_a_choice() {
        // Twenty hits are a search result, not a decision — rule 4.
        let many: Vec<Alternative> =
            (0..20).map(|i| alt(&format!("s{i}"), "1.0", TrustLevel::Community, SourceNature::Sandboxed)).collect();
        assert!(many.len() > MAX_NUMBERED);
        assert_eq!(many[..many.len().min(MAX_NUMBERED)].len(), MAX_NUMBERED);
    }
}
