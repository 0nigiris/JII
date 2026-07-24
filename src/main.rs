//! JII — Just Install It. Entry point: parse args, load config, dispatch.
//!
//! Wiring only. The command surface lives in [`cli`], presentation in [`ui`], and
//! the domain model in [`model`]. See `docs/ARCHITECTURE.md` for the full picture.

mod cache;
mod cli;
mod config;
mod devtest;
mod engine;
mod error;
mod exec;
mod i18n;
mod model;
mod platform;
mod privilege;
mod progress;
mod provider;
mod recommend;
mod registry;
mod selfupdate;
mod ui;

use crate::cli::Cli;
use crate::config::Config;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = parse_cli();

    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            report(&e);
            return std::process::ExitCode::FAILURE;
        }
    };

    // Resolve the UI language once, before anything renders: --lang › config › env › English.
    crate::i18n::init(cli.global.lang.as_deref(), &config.ui.locale);

    match cli.run(config).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            report(&e);
            std::process::ExitCode::FAILURE
        }
    }
}

/// Parse the CLI, appending a dynamic note to `jii --help` that names the config file's real
/// path — a common ask ("how do I change the language / defaults?") the static help can't answer
/// because the location is XDG-resolved at runtime. Building the command by hand (instead of
/// `Cli::parse`) lets us inject `after_help`; `get_matches` still auto-exits on `--help`/`--version`.
fn parse_cli() -> Cli {
    use clap::{CommandFactory, FromArgMatches};
    let mut cmd = Cli::command();
    if let Some(path) = Config::default_path() {
        cmd = cmd.after_help(format!(
            "Config file: {}\nCreate it to change defaults (language, priorities…); `jii doctor` shows it too.",
            path.display()
        ));
    }
    let matches = cmd.get_matches();
    Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit())
}

/// Print a top-level failure, followed by its actionable remedy when it has one (D7).
/// Kept here (not the `Renderer`) because a config-load failure happens before a renderer
/// exists; JSON callers surface their own structured errors upstream.
fn report(err: &crate::error::JiiError) {
    let uni = crate::platform::Platform::detect().unicode;
    let (bad, arrow) = if uni { ("✗", "→") } else { ("x", "->") };
    eprintln!("{bad} {err}");
    if let Some(remedy) = err.remedy() {
        eprintln!("  {arrow} {remedy}");
    }
}
