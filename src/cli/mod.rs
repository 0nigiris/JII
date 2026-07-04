//! Command-line surface: clap definitions, global flags, and dispatch.
//!
//! The command set is intentionally the full, stable surface from
//! `docs/ARCHITECTURE.md` §13. Commands not yet implemented return a clear
//! "not yet" message that names the phase, so the CLI shape never churns.

use clap::{Parser, Subcommand};

use crate::config::{ColorChoice, Config, Profile};
use crate::engine::Engine;
use crate::model::Query;
use crate::ui::Renderer;
use crate::ui::prompt::{self, PromptFlags};

/// Just Install It — a smart universal package installer for Linux.
#[derive(Debug, Parser)]
#[command(name = "jii", version, about, args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Package to install when no subcommand is given, e.g. `jii fastfetch`.
    #[arg(value_name = "PACKAGE")]
    pub package: Option<String>,
}

/// Flags available on every command.
#[derive(Debug, clap::Args)]
pub struct GlobalArgs {
    /// Assume "yes" to prompts.
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// Assume "no" to prompts.
    #[arg(short = 'n', long, global = true)]
    pub no: bool,

    /// Install the recommended option without confirmation (within trust limits).
    #[arg(long, global = true)]
    pub auto: bool,

    /// Force a specific source id (e.g. `--source flatpak`).
    #[arg(long, value_name = "ID", global = true)]
    pub source: Option<String>,

    /// Ranking profile preset.
    #[arg(long, value_enum, global = true)]
    pub profile: Option<Profile>,

    /// Show the plan without executing anything.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Increase verbosity (repeatable).
    #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,

    /// Disable colored output.
    #[arg(long, global = true)]
    pub no_color: bool,
}

/// Sub-commands. See `docs/ARCHITECTURE.md` §13.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Search, rank, recommend and install a package.
    Install {
        /// Package name.
        package: String,
    },
    /// Remove a package using the source that installed it.
    Remove {
        /// Package name.
        package: String,
    },
    /// Update one package, or everything if omitted.
    Update {
        /// Package name (optional).
        package: Option<String>,
    },
    /// Show candidates without installing.
    Search {
        /// Query terms.
        #[arg(required = true)]
        query: Vec<String>,
    },
    /// Show availability, versions, trust and size for a package.
    Info {
        /// Package name.
        package: String,
    },
    /// Explain how and why a package was (or would be) installed.
    Why {
        /// Package name.
        package: String,
    },
    /// Report source availability, latency and health.
    Doctor,
    /// List software installed via JII.
    List,
}

impl Cli {
    /// Resolve the effective color choice from flags and config.
    fn color_choice(&self, config: &Config) -> ColorChoice {
        if self.global.no_color {
            ColorChoice::Never
        } else {
            config.ui.color
        }
    }

    /// Dispatch the parsed command.
    pub async fn run(self, config: Config) -> crate::error::Result<()> {
        let renderer = Renderer::new(self.color_choice(&config), self.global.json);

        match &self.command {
            // Explicit `jii install <pkg>` or bare `jii <pkg>`.
            Some(Commands::Install { package }) => {
                self.install(package, config, &renderer).await
            }
            None => match &self.package {
                Some(package) => self.install(package, config, &renderer).await,
                None => {
                    renderer.info("Usage: jii <package>  (try `jii --help`)");
                    Ok(())
                }
            },

            // Everything else is stubbed until its phase (ROADMAP.md).
            Some(Commands::Remove { .. }) => not_yet(&renderer, "remove", "Phase 2"),
            Some(Commands::Update { .. }) => not_yet(&renderer, "update", "Phase 5"),
            Some(Commands::Search { .. }) => not_yet(&renderer, "search", "Phase 3"),
            Some(Commands::Info { .. }) => not_yet(&renderer, "info", "Phase 3"),
            Some(Commands::Why { .. }) => not_yet(&renderer, "why", "Phase 2"),
            Some(Commands::Doctor) => not_yet(&renderer, "doctor", "Phase 3"),
            Some(Commands::List) => not_yet(&renderer, "list", "Phase 2"),
        }
    }

    /// Install path: search → rank → plan → confirm → execute (Phase 1: DNF).
    async fn install(
        &self,
        package: &str,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        crate::platform::Platform::detect().require_supported()?;

        let engine = Engine::new(config);
        if !engine.has_providers() {
            renderer.error("No installation sources are enabled.");
            return Ok(());
        }

        let query = Query::name(package);
        renderer.info(&format!("Searching for '{}'...", query.raw));

        let result = engine.search(&query).await;
        for (source, reason) in &result.failed {
            renderer.warn(&format!("✗ {source}: {reason}"));
        }

        let ranked = engine.rank(result.candidates);
        let best = match ranked.first() {
            Some(c) => c,
            None => {
                renderer.error(&format!("No installation candidate found for '{package}'."));
                return Ok(());
            }
        };

        let plan = engine.plan_install(best).await?;
        renderer.plan(&plan);

        if self.global.dry_run {
            renderer.info("(dry-run: nothing was installed)");
            return Ok(());
        }

        let flags = PromptFlags {
            auto: self.global.auto || engine.config().install.auto,
            yes: self.global.yes,
            no: self.global.no,
        };
        if !prompt::confirm_install(renderer, best, engine.config(), &flags) {
            renderer.info("Aborted.");
            return Ok(());
        }

        engine.execute(&plan, renderer).await?;
        renderer.success(&format!("Installed {package} via {}.", plan.source_id));
        Ok(())
    }
}

fn not_yet(renderer: &Renderer, cmd: &str, phase: &str) -> crate::error::Result<()> {
    renderer.warn(&format!("`jii {cmd}` is not implemented yet (arrives in {phase})."));
    Ok(())
}
