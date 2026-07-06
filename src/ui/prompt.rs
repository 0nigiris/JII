//! Confirmation prompts and the trust barrier.
//!
//! `default_yes` is a *trust threshold*, not a global "always yes": a candidate at
//! or below `default_yes_max_trust` may auto-confirm; anything less trusted always
//! requires an explicit answer, even under `--auto` (unless the user opted in via
//! `trust.allow_untrusted_auto`).

use std::io::{self, Write};

use crate::config::Config;
use crate::model::TrustLevel;
use crate::platform::Platform;
use crate::ui::Renderer;

/// User intent from CLI flags relevant to confirmation.
pub struct PromptFlags {
    pub auto: bool,
    pub yes: bool,
    pub no: bool,
}

/// Decide whether to proceed with installing a batch of one or more packages, governed
/// by its **least-trusted** candidate: if anything in the batch is below the auto-confirm
/// threshold, the whole batch requires an explicit answer (even under `--auto`/`--yes`,
/// unless the user opted in via `trust.allow_untrusted_auto`) — ADR-0006. `count` only
/// tunes the wording (a single package reads "Install?", many read "Install all?").
pub fn confirm_install_batch(
    renderer: &Renderer,
    least_trusted: TrustLevel,
    count: usize,
    config: &Config,
    flags: &PromptFlags,
) -> bool {
    if flags.no {
        return false;
    }
    let question = if count == 1 { "Install?" } else { "Install all?" };

    let auto_ok = least_trusted <= config.install.default_yes_max_trust;
    if !auto_ok {
        // Trust barrier: at least one source is below the auto-confirm threshold.
        if (flags.auto || flags.yes) && config.trust.allow_untrusted_auto {
            return true;
        }
        renderer.warn(&format!(
            "This install includes a less-trusted source ({}) — explicit confirmation required.",
            least_trusted.label(),
        ));
        // Default to "no" when a less-trusted source is involved.
        return ask(renderer, question, false);
    }

    // Trusted enough: --auto or --yes skip the prompt.
    if flags.auto || flags.yes {
        return true;
    }
    ask(renderer, question, config.install.default_yes)
}

/// How a user answered an interactive candidate chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    /// Empty input — take the marked default (the recommended candidate).
    Default,
    /// Cancel the whole install.
    Cancel,
    /// A concrete 0-based index into the option list.
    Pick(usize),
    /// Unrecognized input — re-ask.
    Invalid,
}

/// Parse a chooser answer against an option count `n` (pure, so it is unit-tested).
/// Empty → `Default`; `n`/`no`/`q`/`quit`/`cancel` → `Cancel`; a number in `1..=n` →
/// `Pick(k-1)`; anything else → `Invalid`.
fn parse_choice(input: &str, n: usize) -> Choice {
    match input.trim().to_ascii_lowercase().as_str() {
        "" => Choice::Default,
        "n" | "no" | "q" | "quit" | "cancel" => Choice::Cancel,
        other => match other.parse::<usize>() {
            Ok(k) if (1..=n).contains(&k) => Choice::Pick(k - 1),
            _ => Choice::Invalid,
        },
    }
}

/// Present a numbered menu of `options` (display lines, best first) and let the user
/// pick one; returns the chosen 0-based index, or `None` to cancel. `default` (0-based,
/// the recommended candidate) is marked and taken on empty input. This always prompts,
/// so callers must gate on an interactive context (TTY, not `--auto`/`--yes`/`--no`/
/// `--json`/`--source`); picking is itself the consent, so a trusted pick needs no
/// separate confirmation (the untrusted trust barrier still applies downstream).
pub fn choose(renderer: &Renderer, header: &str, options: &[String], default: usize) -> Option<usize> {
    renderer.info(header);
    for (i, opt) in options.iter().enumerate() {
        let mark = if i == default { "→" } else { " " };
        renderer.info(&format!("{mark} {}) {}", i + 1, opt));
    }
    loop {
        print!("Your choice [{}] (or 'n' to cancel): ", default + 1);
        let _ = io::stdout().flush();

        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            return Some(default);
        }
        match parse_choice(&line, options.len()) {
            Choice::Default => return Some(default),
            Choice::Cancel => return None,
            Choice::Pick(index) => return Some(index),
            // EOF yields an empty line → Default above, so this cannot spin forever.
            Choice::Invalid => renderer.warn(&format!(
                "Enter a number 1-{} or 'n' to cancel.",
                options.len()
            )),
        }
    }
}

/// A plain yes/no confirmation (e.g. for removal). Honors `--no`/`--yes`/`--auto`;
/// otherwise asks with the given default.
pub fn confirm(renderer: &Renderer, question: &str, default_yes: bool, flags: &PromptFlags) -> bool {
    if flags.no {
        return false;
    }
    if flags.yes || flags.auto {
        return true;
    }
    ask(renderer, question, default_yes)
}

/// Ask a yes/no `question` with a default. Falls back to the default when there is
/// no interactive terminal (so scripts behave predictably).
fn ask(renderer: &Renderer, question: &str, default_yes: bool) -> bool {
    // In JSON mode or without a TTY there is no one to prompt; use the default.
    if renderer.is_json() || !Platform::detect().is_tty {
        return default_yes;
    }

    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("{question} {hint} ");
    let _ = io::stdout().flush();

    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return default_yes;
    }
    match line.trim().to_ascii_lowercase().as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default_yes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_choice_takes_the_default() {
        assert_eq!(parse_choice("", 3), Choice::Default);
        assert_eq!(parse_choice("   \n", 3), Choice::Default);
    }

    #[test]
    fn cancel_words_cancel() {
        for word in ["n", "N", "no", "q", "Quit", "cancel"] {
            assert_eq!(parse_choice(word, 3), Choice::Cancel, "{word}");
        }
    }

    #[test]
    fn in_range_number_picks_zero_based() {
        assert_eq!(parse_choice("1", 3), Choice::Pick(0));
        assert_eq!(parse_choice(" 2 ", 3), Choice::Pick(1));
        assert_eq!(parse_choice("3", 3), Choice::Pick(2));
    }

    #[test]
    fn out_of_range_or_garbage_is_invalid() {
        assert_eq!(parse_choice("0", 3), Choice::Invalid);
        assert_eq!(parse_choice("4", 3), Choice::Invalid);
        assert_eq!(parse_choice("x", 3), Choice::Invalid);
        assert_eq!(parse_choice("1.5", 3), Choice::Invalid);
    }
}
