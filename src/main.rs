//! JII — Just Install It. Entry point: parse args, load config, dispatch.
//!
//! Wiring only. The command surface lives in [`cli`], presentation in [`ui`], and
//! the domain model in [`model`]. See `docs/ARCHITECTURE.md` for the full picture.

// The domain model and provider API are defined ahead of use, phase by phase
// (see docs/ROADMAP.md). Allow dead code during scaffolding; tighten as later
// phases consume these types.
#![allow(dead_code)]

mod cli;
mod config;
mod error;
mod model;
mod platform;
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
            eprintln!("✗ {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    match cli.run(config) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("✗ {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
