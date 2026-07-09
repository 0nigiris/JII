//! Confirmation prompts and the trust barrier.
//!
//! `default_yes` is a *trust threshold*, not a global "always yes": a candidate at
//! or below `default_yes_max_trust` may auto-confirm; anything less trusted always
//! requires an explicit answer, even under `--auto` (unless the user opted in via
//! `trust.allow_untrusted_auto`).

use std::io::{self, Write};

use dialoguer::Select;
use dialoguer::theme::ColorfulTheme;

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

impl PromptFlags {
    /// Fold in a caller-side "assume yes" — e.g. the `doctor` questionnaire, whose gate
    /// question ("Set up X?") is itself the consent, so the ensuing install must not ask
    /// again. The trust barrier (ADR-0006) still applies: an untrusted source is not
    /// auto-confirmed by this, only by an explicit `allow_untrusted_auto`.
    pub fn with_yes(mut self, yes: bool) -> Self {
        self.yes = self.yes || yes;
        self
    }
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
    let question = if count == 1 {
        crate::t!("prompt.install_one")
    } else {
        crate::t!("prompt.install_all")
    };

    let auto_ok = least_trusted <= config.install.default_yes_max_trust;
    if !auto_ok {
        // Trust barrier: at least one source is below the auto-confirm threshold.
        if (flags.auto || flags.yes) && config.trust.allow_untrusted_auto {
            return true;
        }
        renderer.warn(&crate::t!(
            "prompt.less_trusted",
            level = least_trusted.label()
        ));
        // Default to "no" when a less-trusted source is involved.
        return ask(renderer, &question, false);
    }

    // Trusted enough: --auto or --yes skip the prompt.
    if flags.auto || flags.yes {
        return true;
    }
    ask(renderer, &question, config.install.default_yes)
}

/// Present a menu of `options` (display lines, best first) and let the user pick one
/// with the arrow keys (↑/↓ to move, Enter to select, Esc/q to cancel); returns the
/// chosen 0-based index, or `None` to cancel. `default` (0-based, the recommended
/// candidate) is pre-highlighted. Callers gate on an interactive context; picking is
/// itself the consent, so a trusted pick needs no separate confirmation (the untrusted
/// trust barrier still applies downstream). Outside a TTY (or in `--json`) there is no
/// one to prompt, so the default is taken — matching the old EOF behaviour.
pub fn choose(renderer: &Renderer, header: &str, options: &[String], default: usize) -> Option<usize> {
    if renderer.is_json() || !Platform::detect().is_tty {
        return Some(default);
    }
    match Select::with_theme(&ColorfulTheme::default())
        .with_prompt(header)
        .items(options)
        .default(default)
        .interact_opt()
    {
        // Ok(Some(i)) = a pick; Ok(None) = Esc/q cancelled the whole thing.
        Ok(choice) => choice,
        // A terminal error (no interactive stdin after all) falls back to the default.
        Err(_) => Some(default),
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

