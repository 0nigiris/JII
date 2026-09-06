// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Presentation layer: all user-facing output goes through here so `--json` and
//! `--no-color` are honored in one place and the rest of the code stays quiet.

pub mod prompt;
pub mod story;

use owo_colors::OwoColorize;

use crate::config::{ColorChoice, OutputMode};
use crate::model::{Action, InstallPlan, TrustLevel};

/// Semantic colouring for human output. Cheap (`Copy`) so it can be handed to the free
/// rendering helpers (`candidate_line`, table builders) that don't hold a `Renderer`.
/// Every method is a no-op when `enabled` is false (`--no-color`/`NO_COLOR`/JSON/no-TTY),
/// so callers never branch on colour themselves — and the *plain* text is returned
/// unchanged, keeping column widths correct for callers that pad before colouring.
#[derive(Clone, Copy)]
pub struct Palette {
    enabled: bool,
    /// Whether the terminal can render non-ASCII status glyphs (✓/✗/⚠).
    unicode: bool,
}

impl Palette {
    /// Success marker — `✓`, or `+` where the terminal can't render it.
    pub fn mark_ok(&self) -> &'static str {
        if self.unicode { "✓" } else { "+" }
    }

    /// Failure marker — `✗`, or `x` where the terminal can't render it.
    pub fn mark_bad(&self) -> &'static str {
        if self.unicode { "✗" } else { "x" }
    }

    /// Warning marker — `⚠`, or `!` where the terminal can't render it.
    pub fn mark_warn(&self) -> &'static str {
        if self.unicode { "⚠" } else { "!" }
    }


    /// Informational prefix — `ℹ`, or `i` as a fallback.
    pub fn mark_info(&self) -> &'static str {
        if self.unicode { "ℹ" } else { "i" }
    }

    /// Menu selection pointer — `❯`, or `>` where the terminal can't render it.
    pub fn mark_pointer(&self) -> &'static str {
        if self.unicode { "❯" } else { ">" }
    }

    /// A trust level in its own hue: official green, community yellow, untrusted red.
    pub fn trust(&self, level: TrustLevel) -> String {
        let s = level.display();
        if !self.enabled {
            return s;
        }
        match level {
            TrustLevel::Official => s.green().to_string(),
            TrustLevel::Community => s.yellow().to_string(),
            TrustLevel::Untrusted => s.red().to_string(),
        }
    }

    /// A source id (dnf, flatpak, cargo…) — cyan, so it stands out in a candidate line.
    pub fn source(&self, s: &str) -> String {
        if self.enabled { s.cyan().to_string() } else { s.to_string() }
    }

    /// A version string — dimmed, secondary information.
    pub fn version(&self, s: &str) -> String {
        if self.enabled { s.dimmed().to_string() } else { s.to_string() }
    }

    /// Dim any secondary text.
    pub fn dim(&self, s: &str) -> String {
        if self.enabled { s.dimmed().to_string() } else { s.to_string() }
    }

    /// Positive / recommended emphasis — green.
    pub fn good(&self, s: &str) -> String {
        if self.enabled { s.green().to_string() } else { s.to_string() }
    }

    /// A bold heading/line (used for table header rows).
    pub fn heading(&self, s: &str) -> String {
        if self.enabled { s.bold().to_string() } else { s.to_string() }
    }
}

/// A live "still working" indicator for a step whose own output is captured.
///
/// Friendly mode hides a manager's chatter (UX #6), which left a silent terminal for however long
/// `dnf upgrade` takes — indistinguishable from a hang (the owner's report: "it looks like it
/// froze"). This animates one line on **stderr** (stdout stays clean for `--json` and pipes) and
/// erases it when the step ends, so the caller's own result line is all that remains.
///
/// Inert — no task, no output — unless there's a terminal to animate: JSON, a pipe, and Advanced
/// mode (where every action is streamed anyway) all get a no-op.
pub struct Spinner {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    progress: std::sync::Arc<std::sync::Mutex<Option<crate::progress::Progress>>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Spinner {
    /// Start animating `label`. Stop it with [`Spinner::stop`] before printing anything else,
    /// or the animation will fight the output for the line.
    pub fn start(renderer: &Renderer, label: &str) -> Self {
        let live = renderer.is_friendly() && crate::platform::Platform::detect().is_tty;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let progress = std::sync::Arc::new(std::sync::Mutex::new(None));
        if !live {
            return Spinner { stop, progress, handle: None };
        }
        let flag = stop.clone();
        let progress_read = progress.clone();
        let label = label.to_string();
        let unicode = renderer.unicode;
        let color = renderer.color;
        let handle = tokio::spawn(async move {
            use std::io::Write;
            let frames: &[&str] = if unicode {
                &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
            } else {
                &["|", "/", "-", "\\"]
            };
            let started = std::time::Instant::now();
            let mut tick = 0usize;
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                let frame = frames[tick % frames.len()];
                // A live progress reading (the manager's own `[3/41]`/`NN%`) becomes a real bar —
                // the number the owner asked to see. Until one arrives, and for jobs that emit
                // none, fall back to elapsed time: past a few seconds "it's alive" isn't enough,
                // "how long has this been going" is the actual question.
                let reading = progress_read.lock().ok().and_then(|g| *g);
                // The whole line must fit one row. `\r\x1b[2K` erases only the row the cursor
                // is on, so a line that wraps leaves its earlier rows on screen and every
                // repaint adds more — which is how one progress bar became several hundred
                // lines of debris on a tester's phone terminal (ADR-0086). Fall back to 80
                // columns when the width is unknown.
                let cols = crossterm::terminal::size().map(|(c, _)| c as usize).unwrap_or(80);
                let line = if let Some(p) = reading {
                    // Size the bar to the *live* terminal width so it fills the line like
                    // dnf/pacman and re-fits when the window is resized. The label is the
                    // part that gives: on a narrow terminal it is trimmed so the bar — the
                    // thing being watched — always survives.
                    const CHROME: usize = 6; // "  " + frame + " " + "  "
                    let room = cols.saturating_sub(CHROME + bar_min_width(p) + 1);
                    let label = fit(&label, room, unicode);
                    let prefix = format!("  {frame} {label}  ");
                    let budget = cols.saturating_sub(prefix.chars().count() + 1);
                    format!("{prefix}{}", render_bar(p, unicode, color, budget))
                } else {
                    let secs = started.elapsed().as_secs();
                    let elapsed = if secs >= 3 { format!(" ({secs}s)") } else { String::new() };
                    let room = cols.saturating_sub(4 + elapsed.chars().count() + 1);
                    format!("  {frame} {}{elapsed}", fit(&label, room, unicode))
                };
                eprint!("\r\x1b[2K{line}");
                let _ = std::io::stderr().flush();
                tick += 1;
                tokio::time::sleep(std::time::Duration::from_millis(90)).await;
            }
            eprint!("\r\x1b[2K"); // erase the line; the caller prints the outcome
            let _ = std::io::stderr().flush();
        });
        Spinner { stop, progress, handle: Some(handle) }
    }

    /// A cheap handle the streaming executor uses to push live progress readings onto this
    /// spinner. Cloneable and inert-safe: on a non-TTY spinner the readings are simply never
    /// drawn. See [`ProgressReporter`].
    pub fn reporter(&self) -> ProgressReporter {
        ProgressReporter { cell: self.progress.clone() }
    }

    /// Stop the animation and wait for the line to be erased, so nothing printed next collides
    /// with it. Safe to call on an inert spinner.
    pub async fn stop(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

impl Drop for Spinner {
    /// A spinner dropped without `stop()` (an early `?`) must not leave a task drawing over
    /// whatever is printed next.
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle.abort();
            eprint!("\r\x1b[2K");
            use std::io::Write;
            let _ = std::io::stderr().flush();
        }
    }
}

/// A cloneable handle onto a [`Spinner`]'s live progress reading. The streaming executor calls
/// [`ProgressReporter::update`] for every line the manager prints that parses as progress, and
/// [`ProgressReporter::clear`] between actions so a fresh step starts the bar over. All calls are
/// cheap and lock-guarded; on an inert (non-TTY) spinner they update a cell nobody draws.
#[derive(Clone)]
pub struct ProgressReporter {
    cell: std::sync::Arc<std::sync::Mutex<Option<crate::progress::Progress>>>,
}

impl ProgressReporter {
    /// Publish the latest progress reading for the animation loop to draw.
    pub fn update(&self, progress: crate::progress::Progress) {
        if let Ok(mut cell) = self.cell.lock() {
            *cell = Some(progress);
        }
    }

    /// Drop back to the timed spinner (e.g. between two commands in one plan).
    pub fn clear(&self) {
        if let Ok(mut cell) = self.cell.lock() {
            *cell = None;
        }
    }
}

/// Below this many cells a bar is useless, so we stop shrinking on a narrow terminal and let
/// it be the one thing that may wrap rather than vanish entirely.
const MIN_BAR_CELLS: usize = 6;

/// The fixed tail of a bar: `  45%`, plus `  [3/41]` when the manager counts steps.
fn bar_suffix(p: crate::progress::Progress) -> String {
    match p.steps {
        Some((done, total)) => format!("  {:>3}%  [{done}/{total}]", p.percent),
        None => format!("  {:>3}%", p.percent),
    }
}

/// What the spinner reserves for a bar before it trims its label — the minimum cells plus
/// the full tail, so the step counter is kept where the terminal can afford it.
///
/// The label is what gives on a narrow terminal, because the bar is the thing being watched.
/// Where not even this fits, the label goes to nothing and [`render_bar`] degrades within
/// whatever budget is left; it never returns more columns than it was given.
fn bar_min_width(p: crate::progress::Progress) -> usize {
    MIN_BAR_CELLS + bar_suffix(p).chars().count()
}

/// Trim `text` to at most `max` columns, ending in an ellipsis when it had to cut.
///
/// Pure and char-based (not byte-based), so a multi-byte label is never cut mid-character.
/// `max` under the ellipsis width yields an empty string: at that point there is no room
/// to say anything, and the caller's bar is the part worth keeping.
fn fit(text: &str, max: usize, unicode: bool) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let ellipsis = if unicode { "…" } else { "..." };
    let keep = max.saturating_sub(ellipsis.chars().count());
    if keep == 0 {
        return String::new();
    }
    let head: String = text.chars().take(keep).collect();
    format!("{head}{ellipsis}")
}

/// Render a `████████░░░░  45%  [3/41]` bar that fills `budget` columns — the width left on the
/// line after the spinner's fixed prefix. The trailing `  NN%` (and optional `  [done/total]`)
/// is reserved first; the bar cells fill whatever remains, so it stretches with the terminal
/// like dnf/pacman instead of sitting at a fixed width. Unicode blocks on a capable terminal,
/// ASCII `#`/`-` otherwise; the filled run is green when colour is on. Pure, for unit testing.
fn render_bar(p: crate::progress::Progress, unicode: bool, color: bool, budget: usize) -> String {
    // **Never wider than `budget`.** The caller sized that from the real terminal, and a bar
    // that overruns it wraps — which `\r\x1b[2K` cannot erase, so every repaint leaves the
    // old rows behind (ADR-0086). So degrade instead of overflowing: full tail with the step
    // counter, then percent only, then percent with no bar at all.
    let full = bar_suffix(p);
    let short = format!("  {:>3}%", p.percent);
    let suffix = if budget >= MIN_BAR_CELLS + full.chars().count() {
        full
    } else if budget >= MIN_BAR_CELLS + short.chars().count() {
        short
    } else {
        // No room for even a minimal bar: the number is the part worth keeping.
        return if budget >= short.chars().count() { short } else { String::new() };
    };
    let cells = budget.saturating_sub(suffix.chars().count()).max(MIN_BAR_CELLS);
    let filled = ((p.percent as usize * cells) / 100).min(cells);
    let (full, empty) = if unicode { ('█', '░') } else { ('#', '-') };
    let bar_full = full.to_string().repeat(filled);
    let bar_empty = empty.to_string().repeat(cells - filled);
    let bar = if color {
        format!("\x1b[32m{bar_full}\x1b[0m{bar_empty}")
    } else {
        format!("{bar_full}{bar_empty}")
    };
    format!("{bar}{suffix}")
}

/// Renders output as either human-friendly text or machine-readable JSON.
pub struct Renderer {
    color: bool,
    json: bool,
    mode: OutputMode,
    /// Whether the terminal can render non-ASCII status glyphs (✓/✗/⚠).
    unicode: bool,
}

impl Renderer {
    /// Build a renderer from the resolved color choice, the `--json` flag, and the output
    /// mode (Friendly/Advanced — U5).
    pub fn new(color: ColorChoice, json: bool, mode: OutputMode) -> Self {
        let color = match color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            // In JSON mode never colorize; otherwise only when stdout is a terminal.
            ColorChoice::Auto => !json && crate::platform::Platform::detect().is_tty,
        };
        let unicode = crate::platform::Platform::detect().unicode;
        Renderer { color, json, mode, unicode }
    }

    /// Whether JSON output mode is active.
    pub fn is_json(&self) -> bool {
        self.json
    }

    /// Whether this terminal renders non-ASCII glyphs (for callers building their own
    /// decorations rather than using the `Palette::mark_*` set).
    pub fn unicode(&self) -> bool {
        self.unicode
    }

    /// The semantic colour palette for this renderer (a no-op when colour is off).
    pub fn palette(&self) -> Palette {
        Palette { enabled: self.color, unicode: self.unicode }
    }

    /// A bold section heading (falls back to plain text when colour is off / JSON).
    pub fn heading(&self, msg: &str) {
        if self.json {
            self.emit_json("info", msg);
        } else if self.color {
            println!("{}", msg.bold());
        } else {
            println!("{msg}");
        }
    }

    /// Whether we're in Friendly mode (short, human) — never in JSON mode, where the
    /// structure is fixed. Advanced mode returns false, showing full detail.
    pub fn is_friendly(&self) -> bool {
        !self.json && matches!(self.mode, OutputMode::Friendly)
    }

    /// Print a JSON value verbatim (for list/history machine output).
    pub fn json_value(&self, value: &serde_json::Value) {
        println!("{value}");
    }

    /// A neutral informational line.
    pub fn info(&self, msg: &str) {
        if self.json {
            self.emit_json("info", msg);
        } else {
            println!("{msg}");
        }
    }

    /// A success line (green check in color mode).
    pub fn success(&self, msg: &str) {
        if self.json {
            self.emit_json("success", msg);
        } else {
            let m = self.palette().mark_ok();
            if self.color {
                println!("{} {msg}", m.green());
            } else {
                println!("{m} {msg}");
            }
        }
    }

    /// Flush stdout before writing to stderr.
    ///
    /// Warnings and errors go to stderr so a script can separate them; everything else goes to
    /// stdout. On a terminal both are unbuffered and the order is whatever we printed. The
    /// moment either is redirected — `jii doctor > log`, `jii doctor 2>&1 | tee`, a tester's
    /// captured session — Rust line-buffers stdout while stderr stays unbuffered, and every
    /// warning jumps ahead of the output it belongs under. A doctor report then reads as if the
    /// advice sat beneath the wrong check. One flush at the boundary keeps the two streams in
    /// the order they were written, at the cost of nothing a human can measure.
    fn flush_stdout() {
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }

    /// A warning line.
    pub fn warn(&self, msg: &str) {
        if self.json {
            self.emit_json("warn", msg);
        } else {
            let m = self.palette().mark_warn();
            Self::flush_stdout();
            if self.color {
                eprintln!("{} {msg}", m.yellow());
            } else {
                eprintln!("{m} {msg}");
            }
        }
    }

    /// An error line.
    pub fn error(&self, msg: &str) {
        if self.json {
            self.emit_json("error", msg);
        } else {
            let m = self.palette().mark_bad();
            Self::flush_stdout();
            if self.color {
                eprintln!("{} {msg}", m.red());
            } else {
                eprintln!("{m} {msg}");
            }
        }
    }

    /// Render an installation plan (the `--dry-run` / preview view).
    pub fn plan(&self, plan: &InstallPlan) {
        if self.json {
            println!("{}", plan_to_json(plan));
            return;
        }

        let title = crate::t!(
            "plan.title",
            name = plan.candidate_ref.clone(),
            source = plan.source_id.clone()
        );
        if self.color {
            println!("{}", title.bold());
        } else {
            println!("{title}");
        }

        let mark = self.palette().mark_ok();
        let check = if self.color { mark.green().to_string() } else { mark.to_string() };
        for reason in &plan.reasons {
            println!("  {check} {reason}");
        }
        if let Some(size) = plan.download_size {
            println!("  {}", crate::t!("plan.download", size = size.to_string()));
        }
        let level = if plan.needs_root() {
            crate::t!("plan.priv_root")
        } else {
            crate::t!("plan.priv_user")
        };
        println!("  {}", crate::t!("plan.privileges", level = level));
        if plan.actions.is_empty() {
            println!("  {}", crate::t!("plan.actions_none"));
        } else {
            println!("  {}", crate::t!("plan.actions"));
            for action in &plan.actions {
                println!("    {}", describe_action(action));
            }
        }
    }

    fn emit_json(&self, level: &str, msg: &str) {
        let obj = serde_json::json!({ "level": level, "message": msg });
        println!("{obj}");
    }
}

/// A one-line, human-readable description of an action (used for `--dry-run`
/// preview and the execution log, so every action is previewable).
pub fn describe_action(action: &Action) -> String {
    match action {
        Action::RunCommand { argv, needs_root } => {
            let marker = if *needs_root { "#" } else { "$" };
            format!("{marker} {}", argv.join(" "))
        }
        Action::Download { url, dest, verify } => crate::t!(
            "plan.act_download",
            url = url.clone(),
            dest = dest.display().to_string(),
            verify = verify.label().to_string()
        ),
        Action::Place { dest, mode, .. } => crate::t!(
            "plan.act_place",
            dest = dest.display().to_string(),
            mode = format!("{mode:o}")
        ),
        Action::Extract { archive, member, dest, mode } => crate::t!(
            "plan.act_extract",
            member = member.clone(),
            archive = archive.display().to_string(),
            dest = dest.display().to_string(),
            mode = format!("{mode:o}")
        ),
        Action::RemoveFile { path } => {
            crate::t!("plan.act_remove", path = path.display().to_string())
        }
        Action::Replace { src, dest } => crate::t!(
            "plan.act_replace",
            dest = dest.display().to_string(),
            src = src.display().to_string()
        ),
    }
}

/// Serialize a plan to a stable JSON shape (kept here so the schema lives with the UI).
fn plan_to_json(plan: &InstallPlan) -> serde_json::Value {
    serde_json::json!({
        "candidate": plan.candidate_ref,
        "source": plan.source_id,
        "needs_root": plan.needs_root(),
        "download_size": plan.download_size,
        "reasons": plan.reasons,
        "actions": plan.actions.iter().map(action_to_json).collect::<Vec<_>>(),
    })
}

/// Stable JSON for one action, tagged by `kind`.
fn action_to_json(action: &Action) -> serde_json::Value {
    match action {
        Action::RunCommand { argv, needs_root } => serde_json::json!({
            "kind": "run", "argv": argv, "needs_root": needs_root,
        }),
        Action::Download { url, dest, verify } => serde_json::json!({
            "kind": "download", "url": url, "dest": dest, "verify": verify.label(),
        }),
        Action::Place { src, dest, mode } => serde_json::json!({
            "kind": "place", "src": src, "dest": dest, "mode": mode,
        }),
        Action::Extract { archive, member, dest, mode } => serde_json::json!({
            "kind": "extract", "archive": archive, "member": member, "dest": dest, "mode": mode,
        }),
        Action::RemoveFile { path } => serde_json::json!({
            "kind": "remove", "path": path,
        }),
        Action::Replace { src, dest } => serde_json::json!({
            "kind": "replace", "src": src, "dest": dest,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::Progress;

    #[test]
    fn bar_fills_proportionally_and_shows_percent_and_steps() {
        // 25% of a 16-cell bar = 4 filled; the counter that drove it is shown too. Budget 29 =
        // 16 bar cells + the 13-char "   25%  [1/4]" suffix.
        let s = render_bar(Progress { percent: 25, steps: Some((1, 4)) }, true, false, 29);
        assert_eq!(s, "████░░░░░░░░░░░░   25%  [1/4]");
    }

    #[test]
    fn bar_ascii_fallback_and_no_steps() {
        // Budget 22 = 16 bar cells + the 6-char "  100%" suffix.
        let s = render_bar(Progress { percent: 100, steps: None }, false, false, 22);
        assert_eq!(s, "################  100%");
    }

    #[test]
    fn bar_wraps_fill_in_colour_when_enabled() {
        let s = render_bar(Progress { percent: 50, steps: None }, true, true, 22);
        // Green SGR around the filled run, reset before the empty run.
        assert!(s.starts_with("\x1b[32m████████\x1b[0m"));
        assert!(s.contains("50%"));
    }

    #[test]
    fn bar_stretches_to_fill_a_wider_budget() {
        // The pacman/dnf behaviour: a wider line yields a wider bar. Budget 40, suffix "  100%"
        // is 6 cols → 34 cells, all filled at 100%.
        let s = render_bar(Progress { percent: 100, steps: None }, true, false, 40);
        assert_eq!(s.chars().filter(|&c| c == '█').count(), 34);
    }

    #[test]
    fn the_whole_progress_line_fits_a_phone_terminal() {
        // The tester's case, reproduced arithmetically: a ~100-character label and a 40-column
        // terminal. The rendered row must not exceed the terminal, or `\r\x1b[2K` cannot erase
        // it and every repaint leaves debris behind.
        let label = "installing gstreamer1-plugins-bad-free, gstreamer1-plugins-ugly, \
                     gstreamer1-plugin-openh264 via dnf";
        let p = crate::progress::Progress { percent: 100, steps: Some((33, 33)) };
        for cols in [24usize, 40, 80, 120] {
            const CHROME: usize = 6;
            let room = cols.saturating_sub(CHROME + bar_min_width(p) + 1);
            let trimmed = fit(label, room, true);
            let prefix = format!("  X {trimmed}  ");
            let budget = cols.saturating_sub(prefix.chars().count() + 1);
            // Colour off, so the measured width is the visible width.
            let width = prefix.chars().count() + render_bar(p, true, false, budget).chars().count();
            assert!(width <= cols, "{width} columns rendered into {cols}");
        }
    }

    #[test]
    fn a_long_label_is_trimmed_so_the_line_fits_one_row() {
        // The tester's phone: a ~95-character label ("installing gstreamer1-plugins-bad-free,
        // …, gstreamer1-plugin-openh264 via dnf") on a ~40-column terminal. The wrapped line
        // could not be erased, so each repaint left the previous rows behind.
        let long = "installing gstreamer1-plugins-bad-free, gstreamer1-plugins-ugly via dnf";
        let cut = fit(long, 24, true);
        assert_eq!(cut.chars().count(), 24);
        assert!(cut.ends_with('…'));
        assert!(long.starts_with(cut.trim_end_matches('…')));
    }

    #[test]
    fn a_label_that_already_fits_is_untouched() {
        assert_eq!(fit("installing htop", 40, true), "installing htop");
        assert_eq!(fit("installing htop", 15, true), "installing htop");
    }

    #[test]
    fn without_unicode_the_ellipsis_is_ascii_and_never_splits_a_character() {
        assert_eq!(fit("abcdefgh", 5, false), "ab...");
        // Multi-byte input must be cut on character boundaries, not bytes.
        assert_eq!(fit("установка пакета", 6, true), "устан…");
        // No room even for the ellipsis: say nothing rather than something malformed.
        assert_eq!(fit("abcdefgh", 1, false), "");
    }

    #[test]
    fn bar_keeps_a_minimum_on_a_narrow_terminal() {
        // A budget just under suffix + minimum: the bar stops shrinking at MIN_BAR_CELLS
        // rather than dwindling to nothing.
        let p = Progress { percent: 100, steps: None };
        let s = render_bar(p, true, false, MIN_BAR_CELLS + 6);
        assert_eq!(s.chars().filter(|&c| c == '█').count(), MIN_BAR_CELLS);
    }

    #[test]
    fn a_bar_never_overruns_the_budget_it_was_given() {
        // The invariant the whole fix rests on. A bar wider than the room it was given wraps,
        // and a wrapped line cannot be erased — that is how one line became several hundred.
        for steps in [None, Some((3, 41)), Some((33, 33))] {
            for percent in [0u8, 45, 100] {
                let p = Progress { percent, steps };
                for budget in 0..40usize {
                    let width = render_bar(p, true, false, budget).chars().count();
                    assert!(width <= budget, "{width} columns into a budget of {budget}");
                }
            }
        }
    }

    #[test]
    fn a_bar_with_no_room_for_itself_still_shows_the_number() {
        // Two columns is not a bar. Print the reading, not a malformed row.
        let s = render_bar(Progress { percent: 100, steps: None }, true, false, 6);
        assert_eq!(s.trim(), "100%");
        assert!(!s.contains('█'));
    }
}
