//! Presentation layer: all user-facing output goes through here so `--json` and
//! `--no-color` are honored in one place and the rest of the code stays quiet.

pub mod prompt;

use owo_colors::OwoColorize;

use crate::config::ColorChoice;
use crate::model::InstallPlan;

/// Renders output as either human-friendly text or machine-readable JSON.
pub struct Renderer {
    color: bool,
    json: bool,
}

impl Renderer {
    /// Build a renderer from the resolved color choice and the `--json` flag.
    pub fn new(color: ColorChoice, json: bool) -> Self {
        let color = match color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            // In JSON mode never colorize; otherwise only when stdout is a terminal.
            ColorChoice::Auto => !json && crate::platform::Platform::detect().is_tty,
        };
        Renderer { color, json }
    }

    /// Whether JSON output mode is active.
    pub fn is_json(&self) -> bool {
        self.json
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
            if plan.needs_root { "root required" } else { "user" }
        );
        if plan.steps.is_empty() {
            println!("  steps: (none yet — placeholder plan)");
        } else {
            println!("  steps:");
            for step in &plan.steps {
                let root = if step.needs_root { "# " } else { "$ " };
                println!("    {root}{}", step.argv.join(" "));
            }
        }
    }

    fn emit_json(&self, level: &str, msg: &str) {
        let obj = serde_json::json!({ "level": level, "message": msg });
        println!("{obj}");
    }
}

/// Serialize a plan to a stable JSON shape (kept here so the schema lives with the UI).
fn plan_to_json(plan: &InstallPlan) -> serde_json::Value {
    serde_json::json!({
        "candidate": plan.candidate_ref,
        "source": plan.source_id,
        "needs_root": plan.needs_root,
        "download_size": plan.download_size,
        "reasons": plan.reasons,
        "steps": plan.steps.iter().map(|s| serde_json::json!({
            "argv": s.argv,
            "needs_root": s.needs_root,
        })).collect::<Vec<_>>(),
    })
}
