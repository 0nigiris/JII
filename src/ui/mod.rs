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

    /// Neutral bullet for "available but not installed" — `○`, or `-` as a fallback.
    pub fn mark_bullet(&self) -> &'static str {
        if self.unicode { "○" } else { "-" }
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
