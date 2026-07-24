//! Presentation layer: all user-facing output goes through here so `--json` and
//! `--no-color` are honored in one place and the rest of the code stays quiet.

pub mod prompt;

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
    /// A never-colouring palette (for tests and any plain-text context).
    #[cfg(test)]
    pub fn plain() -> Self {
        Palette { enabled: false, unicode: true }
    }

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

    /// "Recommended" flag — `⭐`, or `*` where the terminal can't render it.
    pub fn mark_star(&self) -> &'static str {
        if self.unicode { "⭐" } else { "*" }
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
                let line = if let Some(p) = reading {
                    format!("  {frame} {label}  {}", render_bar(p, unicode, color))
                } else {
                    let secs = started.elapsed().as_secs();
                    let elapsed = if secs >= 3 { format!(" ({secs}s)") } else { String::new() };
                    format!("  {frame} {label}{elapsed}")
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

/// Render a compact `████████░░░░░░░░  45%` bar (with an optional `[3/41]` when a step counter
/// drove it). Unicode blocks on a capable terminal, ASCII `#`/`-` otherwise; the filled run is
/// green when colour is on. Width is fixed and small so it fits a narrow terminal on one line.
fn render_bar(p: crate::progress::Progress, unicode: bool, color: bool) -> String {
    const WIDTH: usize = 16;
    let filled = ((p.percent as usize * WIDTH) / 100).min(WIDTH);
    let (full, empty) = if unicode { ('█', '░') } else { ('#', '-') };
    let bar_full = full.to_string().repeat(filled);
    let bar_empty = empty.to_string().repeat(WIDTH - filled);
    let bar = if color {
        format!("\x1b[32m{bar_full}\x1b[0m{bar_empty}")
    } else {
        format!("{bar_full}{bar_empty}")
    };
    match p.steps {
        Some((done, total)) => format!("{bar}  {:>3}%  [{done}/{total}]", p.percent),
        None => format!("{bar}  {:>3}%", p.percent),
    }
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

    /// A warning line.
    pub fn warn(&self, msg: &str) {
        if self.json {
            self.emit_json("warn", msg);
        } else {
            let m = self.palette().mark_warn();
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
        // 25% of a 16-cell bar = 4 filled; the counter that drove it is shown too.
        let s = render_bar(Progress { percent: 25, steps: Some((1, 4)) }, true, false);
        assert_eq!(s, "████░░░░░░░░░░░░   25%  [1/4]");
    }

    #[test]
    fn bar_ascii_fallback_and_no_steps() {
        let s = render_bar(Progress { percent: 100, steps: None }, false, false);
        assert_eq!(s, "################  100%");
    }

    #[test]
    fn bar_wraps_fill_in_colour_when_enabled() {
        let s = render_bar(Progress { percent: 50, steps: None }, true, true);
        // Green SGR around the filled run, reset before the empty run.
        assert!(s.starts_with("\x1b[32m████████\x1b[0m"));
        assert!(s.contains("50%"));
    }
}
