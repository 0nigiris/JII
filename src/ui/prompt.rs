//! Confirmation prompts and the trust barrier.
//!
//! `default_yes` is a *trust threshold*, not a global "always yes": a candidate at
//! or below `default_yes_max_trust` may auto-confirm; anything less trusted always
//! requires an explicit answer, even under `--auto` (unless the user opted in via
//! `trust.allow_untrusted_auto`).

use std::io::{self, Write};

use crate::config::Config;
use crate::model::{PackageCandidate, TrustLevel};
use crate::platform::Platform;
use crate::ui::Renderer;

/// User intent from CLI flags relevant to confirmation.
pub struct PromptFlags {
    pub auto: bool,
    pub yes: bool,
    pub no: bool,
}

/// Decide whether to proceed with installing `candidate`.
pub fn confirm_install(
    renderer: &Renderer,
    candidate: &PackageCandidate,
    config: &Config,
    flags: &PromptFlags,
) -> bool {
    if flags.no {
        return false;
    }

    let auto_ok = candidate.trust <= config.install.default_yes_max_trust;

    if !auto_ok {
        // Trust barrier: this source is below the auto-confirm threshold.
        if (flags.auto || flags.yes) && config.trust.allow_untrusted_auto {
            return true;
        }
        renderer.warn(&format!(
            "'{}' comes from a less-trusted source ({}) — explicit confirmation required.",
            candidate.name,
            trust_label(candidate.trust),
        ));
        // Default to "no" for less-trusted sources.
        return ask(renderer, false);
    }

    // Trusted enough: --auto or --yes skip the prompt.
    if flags.auto || flags.yes {
        return true;
    }
    ask(renderer, config.install.default_yes)
}

/// Human label for a trust level.
fn trust_label(trust: TrustLevel) -> &'static str {
    match trust {
        TrustLevel::Official => "official",
        TrustLevel::Community => "community",
        TrustLevel::Untrusted => "untrusted",
    }
}

/// Ask a yes/no question with a default. Falls back to the default when there is no
/// interactive terminal (so scripts behave predictably).
fn ask(renderer: &Renderer, default_yes: bool) -> bool {
    // In JSON mode or without a TTY there is no one to prompt; use the default.
    if renderer.is_json() || !Platform::detect().is_tty {
        return default_yes;
    }

    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("Install? {hint} ");
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
