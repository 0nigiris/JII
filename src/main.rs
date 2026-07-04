//! JII — Just Install It. Entry point: parse args, load config, dispatch.
//!
//! Wiring only. The command surface lives in [`cli`], presentation in [`ui`], and
//! the domain model in [`model`]. See `docs/ARCHITECTURE.md` for the full picture.

mod cache;
mod cli;
mod config;
mod engine;
mod error;
mod model;
mod platform;
mod privilege;
mod provider;
mod registry;
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

    match cli.run(config).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("✗ {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
