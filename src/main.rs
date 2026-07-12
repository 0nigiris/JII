//! JII — Just Install It. Entry point: parse args, load config, dispatch.
//!
//! Wiring only. The command surface lives in [`cli`], presentation in [`ui`], and
//! the domain model in [`model`]. See `docs/ARCHITECTURE.md` for the full picture.

mod cache;
mod cli;
mod config;
mod engine;
mod error;
mod exec;
mod i18n;
mod model;
mod platform;
mod privilege;
mod provider;
mod recommend;
mod registry;
mod selfupdate;
mod ui;

use clap::Parser;

use crate::cli::Cli;
use crate::config::Config;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

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
