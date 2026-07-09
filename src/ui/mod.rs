//! Presentation layer: all user-facing output goes through here so `--json` and
//! `--no-color` are honored in one place and the rest of the code stays quiet.

pub mod prompt;

use owo_colors::OwoColorize;

use crate::config::{ColorChoice, OutputMode};
use crate::model::{Action, InstallPlan};

/// Renders output as either human-friendly text or machine-readable JSON.
pub struct Renderer {
    color: bool,
    json: bool,
    mode: OutputMode,
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
        Renderer { color, json, mode }
    }

    /// Whether JSON output mode is active.
    pub fn is_json(&self) -> bool {
        self.json
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
        } else if self.color {
            println!("{} {msg}", "✓".green());
        } else {
            println!("✓ {msg}");
        }
    }

    /// A warning line.
    pub fn warn(&self, msg: &str) {
        if self.json {
            self.emit_json("warn", msg);
        } else if self.color {
            eprintln!("{} {msg}", "⚠".yellow());
        } else {
            eprintln!("⚠ {msg}");
        }
    }

    /// An error line.
    pub fn error(&self, msg: &str) {
        if self.json {
            self.emit_json("error", msg);
        } else if self.color {
            eprintln!("{} {msg}", "✗".red());
        } else {
            eprintln!("✗ {msg}");
        }
    }

    /// Render an installation plan (the `--dry-run` / preview view).
    pub fn plan(&self, plan: &InstallPlan) {
        if self.json {
            println!("{}", plan_to_json(plan));
            return;
        }

        let title = format!("Plan: {} (via {})", plan.candidate_ref, plan.source_id);
        if self.color {
            println!("{}", title.bold());
        } else {
            println!("{title}");
        }

        for reason in &plan.reasons {
            println!("  ✓ {reason}");
        }
        if let Some(size) = plan.download_size {
            println!("  download: {} bytes", size);
        }
        println!(
            "  privileges: {}",
            if plan.needs_root() { "root required" } else { "user" }
        );
        if plan.actions.is_empty() {
            println!("  actions: (none)");
        } else {
            println!("  actions:");
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
        Action::Download { url, dest, verify } => {
            format!("download {url} → {} [{}]", dest.display(), verify.label())
        }
        Action::Place { dest, mode, .. } => {
            format!("place → {} (mode {mode:o})", dest.display())
        }
        Action::Extract { archive, member, dest, mode } => format!(
            "extract {member} from {} → {} (mode {mode:o})",
            archive.display(),
            dest.display()
        ),
        Action::RemoveFile { path } => format!("remove {}", path.display()),
        Action::Replace { src, dest } => {
            format!("replace {} ← {}", dest.display(), src.display())
        }
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
