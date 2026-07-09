//! Confirmation prompts and the trust barrier.
//!
//! `default_yes` is a *trust threshold*, not a global "always yes": a candidate at
//! or below `default_yes_max_trust` may auto-confirm; anything less trusted always
//! requires an explicit answer, even under `--auto` (unless the user opted in via
//! `trust.allow_untrusted_auto`).

use std::io::{self, Write};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::terminal::ClearType;
use crossterm::{cursor, execute, queue, terminal};

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

/// Present a menu of `options` (display lines, best first) and let the user pick one —
/// with the **arrow keys** (↑/↓ or `j`/`k`, Enter to select), the **mouse** (hover to
/// highlight, click a row to pick, scroll to move), or Esc/`q` to cancel. Returns the
/// chosen 0-based index, or `None` to cancel. `default` (0-based, the recommended
/// candidate) is pre-highlighted. Callers gate on an interactive context; picking is
/// itself the consent, so a trusted pick needs no separate confirmation (the untrusted
/// trust barrier still applies downstream). Outside a TTY (or in `--json`) there is no
/// one to prompt, so the default is taken — matching the old EOF behaviour. Any terminal
/// error also falls back to the default, and the terminal is always restored.
pub fn choose(renderer: &Renderer, header: &str, options: &[String], default: usize) -> Option<usize> {
    if renderer.is_json() || !Platform::detect().is_tty {
        return Some(default);
    }
    if options.len() <= 1 {
        return Some(default.min(options.len().saturating_sub(1)));
    }
    match run_menu(renderer, header, options, default) {
        Ok(choice) => choice,
        // A terminal error (raw mode unavailable, etc.) → take the default, don't crash.
        Err(_) => Some(default),
    }
}

/// One menu item line: `❯ text` (highlighted) or `  text`.
fn menu_line(selected: bool, text: &str, palette: crate::ui::Palette) -> String {
    if selected {
        format!("{} {}", palette.good("❯"), palette.heading(text))
    } else {
        format!("  {text}")
    }
}

/// The crossterm menu itself. Enables raw mode + mouse capture, draws the items inline,
/// and *always* restores the terminal before returning (even on error).
fn run_menu(
    renderer: &Renderer,
    header: &str,
    options: &[String],
    default: usize,
) -> io::Result<Option<usize>> {
    let palette = renderer.palette();
    let mut out = io::stdout();

    // Header + hint in normal (cooked) mode, so they stay above the menu.
    println!("{header}");
    println!("  {}", palette.dim(&crate::t!("prompt.menu_hint")));
    out.flush()?;

    terminal::enable_raw_mode()?;

    // Everything that can fail runs inside this closure; whatever it returns, the terminal
    // is restored by the cleanup below.
    let mut run = || -> io::Result<(Option<usize>, u16)> {
        execute!(out, cursor::Hide)?;

        let n = options.len();
        let mut sel = default.min(n - 1);

        // Reserve `n` lines (scrolling the viewport if we're near the bottom), then move
        // back to the top of that region and record its absolute row. Crucially this — and
        // the cursor-position query it needs — happens **before** mouse capture is enabled,
        // so the position report can't race with mouse/key events on stdin.
        for _ in 0..n {
            write!(out, "\r\n")?;
        }
        execute!(out, cursor::MoveToPreviousLine(n as u16))?;
        let (_, first) = cursor::position()?;
        execute!(out, EnableMouseCapture)?;

        let redraw = |out: &mut io::Stdout, sel: usize| -> io::Result<()> {
            for (i, opt) in options.iter().enumerate() {
                queue!(
                    out,
                    cursor::MoveTo(0, first + i as u16),
                    terminal::Clear(ClearType::CurrentLine)
                )?;
                write!(out, "{}", menu_line(i == sel, opt, palette))?;
            }
            out.flush()
        };

        redraw(&mut out, sel)?; // initial paint

        let choice = loop {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        sel = (sel + n - 1) % n;
                        redraw(&mut out, sel)?;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        sel = (sel + 1) % n;
                        redraw(&mut out, sel)?;
                    }
                    KeyCode::Home => {
                        sel = 0;
                        redraw(&mut out, sel)?;
                    }
                    KeyCode::End => {
                        sel = n - 1;
                        redraw(&mut out, sel)?;
                    }
                    KeyCode::Enter => break Some(sel),
                    KeyCode::Esc | KeyCode::Char('q') => break None,
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        break None;
                    }
                    _ => {}
                },
                Event::Mouse(m) => {
                    // Map a terminal row back to an item index (the menu occupies
                    // `first..first+n`).
                    let row_item = |row: u16| -> Option<usize> {
                        let idx = row.checked_sub(first)? as usize;
                        (idx < n).then_some(idx)
                    };
                    match m.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            if let Some(i) = row_item(m.row) {
                                break Some(i); // click a row = pick it
                            }
                        }
                        MouseEventKind::Moved => {
                            if let Some(i) = row_item(m.row)
                                && i != sel
                            {
                                sel = i;
                                redraw(&mut out, sel)?;
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            sel = (sel + 1) % n;
                            redraw(&mut out, sel)?;
                        }
                        MouseEventKind::ScrollUp => {
                            sel = (sel + n - 1) % n;
                            redraw(&mut out, sel)?;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        };
        Ok((choice, first))
    };

    let outcome = run();

    // Restore the terminal no matter what.
    let _ = execute!(out, DisableMouseCapture, cursor::Show);
    let _ = terminal::disable_raw_mode();

    match outcome {
        Ok((choice, first)) => {
            // Erase the menu items so the following output (the plan) starts clean.
            let _ = execute!(
                out,
                cursor::MoveTo(0, first),
                terminal::Clear(ClearType::FromCursorDown)
            );
            Ok(choice)
        }
        Err(e) => Err(e),
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

