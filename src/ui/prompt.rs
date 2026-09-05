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
        format!("{} {}", palette.good(palette.mark_pointer()), palette.heading(text))
    } else {
        format!("  {text}")
    }
}

/// Truncate a possibly-ANSI-coloured string to at most `max` *visible* columns, copying escape
/// sequences verbatim (they take no width) and closing with a reset. Keeps every menu item on a
/// single terminal row, so the per-row redraw and mouse hit-testing never desync on a long line.
fn truncate_display(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut visible = 0usize;
    let mut chars = s.chars().peekable();
    let mut truncated = false;
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Copy the whole escape sequence (up to its letter terminator) without counting it.
            out.push(c);
            for e in chars.by_ref() {
                out.push(e);
                if e.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if visible + 1 >= max {
            truncated = true;
            break;
        }
        out.push(c);
        visible += 1;
    }
    if truncated {
        out.push('…');
    }
    out.push_str("\u{1b}[0m");
    out
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

        // A list taller than the terminal can't be drawn row-per-item — reserve a window of
        // at most (height − 3) rows and scroll the selection within it (the repo picker's
        // "show more" grows the list without bound). A list that fits behaves as before.
        let term_rows = terminal::size().map(|(_, h)| h).unwrap_or(24) as usize;
        let visible = n.min(term_rows.saturating_sub(3).max(3));
        let mut offset = sel.saturating_sub(visible - 1);

        // Reserve `visible` lines (scrolling the viewport if we're near the bottom), then move
        // back to the top of that region and record its absolute row. Crucially this — and
        // the cursor-position query it needs — happens **before** mouse capture is enabled,
        // so the position report can't race with mouse/key events on stdin.
        for _ in 0..visible {
            write!(out, "\r\n")?;
        }
        execute!(out, cursor::MoveToPreviousLine(visible as u16))?;
        let (_, first) = cursor::position()?;
        execute!(out, EnableMouseCapture)?;

        // Keep every item on one row: truncate to the terminal width (minus a column of slack).
        let width = terminal::size().map(|(w, _)| w).unwrap_or(80).max(20) as usize - 1;

        let redraw = |out: &mut io::Stdout, sel: usize, offset: usize| -> io::Result<()> {
            for row in 0..visible {
                let i = offset + row;
                queue!(
                    out,
                    cursor::MoveTo(0, first + row as u16),
                    terminal::Clear(ClearType::CurrentLine)
                )?;
                let mut line = menu_line(i == sel, &options[i], palette);
                // Edge markers so a windowed list is visibly scrollable.
                if row == 0 && offset > 0 {
                    line.push_str(&palette.dim("  ↑"));
                } else if row + 1 == visible && offset + visible < n {
                    line.push_str(&palette.dim("  ↓"));
                }
                write!(out, "{}", truncate_display(&line, width))?;
            }
            out.flush()
        };

        // Keep the selection inside the window, then repaint.
        let clamp = |sel: usize, offset: usize| -> usize {
            if sel < offset {
                sel
            } else if sel >= offset + visible {
                sel + 1 - visible
            } else {
                offset
            }
        };

        redraw(&mut out, sel, offset)?; // initial paint

        let choice = loop {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        sel = (sel + n - 1) % n;
                        offset = clamp(sel, offset);
                        redraw(&mut out, sel, offset)?;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        sel = (sel + 1) % n;
                        offset = clamp(sel, offset);
                        redraw(&mut out, sel, offset)?;
                    }
                    KeyCode::Home => {
                        sel = 0;
                        offset = 0;
                        redraw(&mut out, sel, offset)?;
                    }
                    KeyCode::End => {
                        sel = n - 1;
                        offset = clamp(sel, offset);
                        redraw(&mut out, sel, offset)?;
                    }
                    KeyCode::Enter => break Some(sel),
                    KeyCode::Esc | KeyCode::Char('q') => break None,
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        break None;
                    }
                    _ => {}
                },
                Event::Mouse(m) => {
                    // Map a terminal row back to an item index (the menu occupies the
                    // `visible`-row window starting at `first`, showing `offset..offset+visible`).
                    let row_item = |row: u16| -> Option<usize> {
                        let idx = row.checked_sub(first)? as usize;
                        (idx < visible).then_some(offset + idx)
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
                                redraw(&mut out, sel, offset)?;
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            sel = (sel + 1) % n;
                            offset = clamp(sel, offset);
                            redraw(&mut out, sel, offset)?;
                        }
                        MouseEventKind::ScrollUp => {
                            sel = (sel + n - 1) % n;
                            offset = clamp(sel, offset);
                            redraw(&mut out, sel, offset)?;
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
    // Prefer a single keypress (y/n acts immediately, no Enter). Fall back to line input
    // if raw mode is unavailable (a limited terminal), so we never lose the ability to ask.
    match ask_key(question, hint, default_yes) {
        Ok(answer) => answer,
        Err(_) => ask_line(question, hint, default_yes),
    }
}

/// Read a single y/n keypress — no Enter needed. `y`/`n` answer immediately; Enter takes
/// the default; Esc or Ctrl-C cancel to "no". Raw mode is always restored, and the chosen
/// letter is echoed (raw mode doesn't echo) so the transcript still reads naturally.
/// Returns `Err` if raw mode can't be enabled, so the caller can fall back to line input.
fn ask_key(question: &str, hint: &str, default_yes: bool) -> io::Result<bool> {
    let mut out = io::stdout();
    write!(out, "{question} {hint} ")?;
    out.flush()?;

    terminal::enable_raw_mode()?;
    let result = (|| -> io::Result<bool> {
        loop {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    // Russian «д/н» answer too — a ru-locale user shouldn't have to switch layouts.
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('д') | KeyCode::Char('Д') => return Ok(true),
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('н') | KeyCode::Char('Н') => return Ok(false),
                    KeyCode::Enter => return Ok(default_yes),
                    KeyCode::Esc => return Ok(false),
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(false);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    })();
    // Restore the terminal whatever happened, then echo the resolved answer + newline.
    let _ = terminal::disable_raw_mode();
    if let Ok(answer) = result {
        let _ = writeln!(out, "{}", if answer { "y" } else { "n" });
        let _ = out.flush();
    }
    result
}

/// Read a secret from the terminal **without echoing it**.
///
/// Used by `jii ghtoken` so a token can be pasted instead of passed as an argument, where it
/// would land in the shell history and in `ps` — the same exposure ADR-0083 took out of
/// `~/.bashrc`. Nothing typed is drawn, nothing is stored anywhere but the caller's `String`.
///
/// Without a terminal (a pipe, `jii ghtoken < token.txt`, CI) it reads one line from stdin,
/// which is the scripted equivalent and needs no echo suppression to begin with. Cancelling
/// with Esc or Ctrl+C yields an empty string; the caller treats that as "no token given".
pub fn read_secret(renderer: &Renderer, prompt: &str) -> io::Result<String> {
    if !Platform::detect().is_tty {
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        return Ok(line.trim().to_string());
    }

    renderer.info(prompt);
    let mut out = io::stdout();
    let _ = out.flush();
    terminal::enable_raw_mode()?;
    let mut secret = String::new();
    let outcome = loop {
        match event::read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Enter => break Ok(()),
                KeyCode::Esc => {
                    secret.clear();
                    break Ok(());
                }
                KeyCode::Backspace => {
                    secret.pop();
                }
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    secret.clear();
                    break Ok(());
                }
                KeyCode::Char(c) => secret.push(c),
                _ => {}
            },
            Ok(_) => {}
            Err(e) => break Err(e),
        }
    };
    // Leave raw mode whatever happened, so a failure can't stick the user's terminal.
    let _ = terminal::disable_raw_mode();
    let _ = writeln!(out);
    let _ = out.flush();
    outcome.map(|()| secret.trim().to_string())
}

/// Line-based yes/no fallback (Enter to submit) for terminals without raw mode.
fn ask_line(question: &str, hint: &str, default_yes: bool) -> bool {
    print!("{question} {hint} ");
    let _ = io::stdout().flush();

    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return default_yes;
    }
    match line.trim().to_lowercase().as_str() {
        "" => default_yes,
        "y" | "yes" | "д" | "да" => true,
        "n" | "no" | "н" | "нет" => false,
        _ => default_yes,
    }
}

