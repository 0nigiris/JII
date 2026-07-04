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
    /// Audit installed software: source, trust, verification and concerns.
    Audit,
    /// List software installed via JII.
    List,
    /// Show installation history.
    History,
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

            // Implemented in Phase 2.
            Some(Commands::Remove { package }) => self.remove(package, config, &renderer).await,
            Some(Commands::Why { package }) => self.why(package, config, &renderer),
            Some(Commands::List) => self.list(config, &renderer),
            Some(Commands::History) => self.history(config, &renderer),

            Some(Commands::Doctor) => self.doctor(config, &renderer).await,
            Some(Commands::Audit) => self.audit(config, &renderer),

            // Stubbed until their phase (ROADMAP.md).
            Some(Commands::Update { .. }) => not_yet(&renderer, "update", "Phase 5"),
            Some(Commands::Search { .. }) => not_yet(&renderer, "search", "Phase 3"),
            Some(Commands::Info { .. }) => not_yet(&renderer, "info", "Phase 3"),
        }
    }

    /// Install path: search → rank → plan → confirm → execute.
    async fn install(
        &self,
        package: &str,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        crate::platform::Platform::detect().require_supported()?;

        let mut engine = Engine::new(self.apply_profile(config))?;
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

        let mut ranked = engine.rank(result.candidates);
        if let Some(source) = &self.global.source {
            ranked.retain(|c| &c.source_id == source);
        }
        if ranked.is_empty() {
            renderer.error(&self.no_candidate_message(package));
            return Ok(());
        }

        // First is the recommendation; the rest are alternatives.
        let best = ranked.remove(0);
        let plan = engine.plan_install(&best).await?;
        renderer.plan(&plan);
        self.show_alternatives(&ranked, renderer);

        if self.global.dry_run {
            renderer.info("(dry-run: nothing was installed)");
            return Ok(());
        }

        let flags = self.prompt_flags(engine.config().install.auto);
        if !prompt::confirm_install(renderer, &best, engine.config(), &flags) {
            renderer.info("Aborted.");
            return Ok(());
        }

        engine.install(&plan, &best, renderer).await?;
        renderer.success(&format!("Installed {package} via {}.", plan.source_id));
        Ok(())
    }

    /// Print the non-recommended candidates as a compact "also available" list.
    fn show_alternatives(&self, alternatives: &[crate::model::PackageCandidate], renderer: &Renderer) {
        if alternatives.is_empty() || renderer.is_json() {
            return;
        }
        renderer.info("Also available:");
        for candidate in alternatives {
            let version = candidate
                .version
                .as_ref()
                .map(|v| format!("v{v}, "))
                .unwrap_or_default();
            renderer.info(&format!(
                "  {} ({}{})",
                candidate.source_id,
                version,
                candidate.trust.label()
            ));
        }
    }

    /// Error message for an empty candidate list, mentioning `--source` if set.
    fn no_candidate_message(&self, package: &str) -> String {
        match &self.global.source {
            Some(source) => format!("'{package}' is not available via source '{source}'."),
            None => format!("No installation candidate found for '{package}'."),
        }
    }

    /// Fold the `--profile` flag into the config.
    fn apply_profile(&self, mut config: Config) -> Config {
        if let Some(profile) = self.global.profile {
            config.install.profile = profile;
        }
        config
    }

    /// Remove path: resolve the owning source (registry + verification), plan, and
    /// execute the removal.
    async fn remove(
        &self,
        package: &str,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        crate::platform::Platform::detect().require_supported()?;

        let mut engine = Engine::new(config)?;
        let record = match engine.resolve_installed(package).await {
            Ok(r) => r,
            Err(e) => {
                renderer.error(&e.to_string());
                return Ok(());
            }
        };

        let plan = engine.plan_remove(&record).await?;
        renderer.plan(&plan);

        if self.global.dry_run {
            renderer.info("(dry-run: nothing was removed)");
            return Ok(());
        }

        let flags = self.prompt_flags(false);
        if !prompt::confirm(renderer, &format!("Remove {package}?"), false, &flags) {
            renderer.info("Aborted.");
            return Ok(());
        }

        engine.remove(&plan, &record, renderer).await?;
        renderer.success(&format!("Removed {package} via {}.", record.source_id));
        Ok(())
    }

    /// Explain how and why a package was installed (from the registry).
    fn why(&self, package: &str, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        match engine.registry().get(package) {
            None => {
                renderer.warn(&format!(
                    "'{package}' was not installed by jii (no record). Try `jii {package}`."
                ));
            }
            Some(record) => {
                let trust = engine
                    .source_trust(&record.source_id)
                    .map(|t| t.label())
                    .unwrap_or("unknown");
                let version = record
                    .version
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                renderer.info(&format!(
                    "Installed via {} on {}",
                    record.source_id,
                    record.installed_at.format("%Y-%m-%d %H:%M")
                ));
                renderer.info(&format!("  ✓ Version {version}"));
                renderer.info(&format!("  ✓ Source trust: {trust}"));
            }
        }
        Ok(())
    }

    /// List software installed via jii.
    fn list(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let items = engine.registry().installed();

        if renderer.is_json() {
            renderer.json_value(&serde_json::json!(items));
            return Ok(());
        }
        if items.is_empty() {
            renderer.info("Nothing installed via jii yet.");
            return Ok(());
        }
        for record in items {
            let version = record
                .version
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_default();
            renderer.info(&format!(
                "{}  {}  {}",
                record.name, record.source_id, version
            ));
        }
        Ok(())
    }

    /// Report source availability, latency and health.
    async fn doctor(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let diagnostics = engine.diagnose().await;

        if renderer.is_json() {
            let rows: Vec<_> = diagnostics
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "source": d.id,
                        "available": d.available,
                        "latency_ms": d.latency.as_millis(),
                        "health": d.health.label(),
                        "detail": d.detail,
                    })
                })
                .collect();
            renderer.json_value(&serde_json::json!(rows));
            return Ok(());
        }

        for d in &diagnostics {
            let mark = if d.available { "✓" } else { "✗" };
            let detail = match &d.detail {
                Some(text) => format!("  ({text})"),
                None => String::new(),
            };
            renderer.info(&format!(
                "{mark} {:8}  {:12}  {} ms{detail}",
                d.id,
                d.health.label(),
                d.latency.as_millis()
            ));
        }
        Ok(())
    }

    /// Show installation history, newest first.
    fn history(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let events = engine.registry().history();

        if renderer.is_json() {
            renderer.json_value(&serde_json::json!(events));
            return Ok(());
        }
        if events.is_empty() {
            renderer.info("No history yet.");
            return Ok(());
        }
        for event in events.iter().rev() {
            renderer.info(&format!(
                "{}  {:?}  {} ({})",
                event.at.format("%Y-%m-%d %H:%M"),
                event.action,
                event.name,
                event.source_id
            ));
        }
        Ok(())
    }

    /// Audit installed software: where each came from, its trust, how it was
    /// verified, and anything that needs attention.
    fn audit(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let entries = engine.audit();

        if renderer.is_json() {
            let rows: Vec<_> = entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "name": e.name,
                        "source": e.source_id,
                        "version": e.version.as_ref().map(|v| v.to_string()),
                        "installed_at": e.installed_at,
                        "trust": e.trust.map(|t| t.label()),
                        "verification": e.verification.label(),
                        "concerns": e.concerns.iter().map(|c| c.message()).collect::<Vec<_>>(),
                    })
                })
                .collect();
            renderer.json_value(&serde_json::json!(rows));
            return Ok(());
        }

        if entries.is_empty() {
            renderer.info("Nothing installed via jii yet.");
            return Ok(());
        }

        renderer.info(&format!(
            "{:20} {:8} {:10} {:14} {}",
            "NAME", "SOURCE", "TRUST", "VERIFIED", "STATUS"
        ));
        let mut flagged = 0;
        for e in &entries {
            let trust = e.trust.map(|t| t.label()).unwrap_or("unknown");
            let status = if e.concerns.is_empty() {
                "ok".to_string()
            } else {
                flagged += 1;
                let reasons: Vec<&str> = e.concerns.iter().map(|c| c.message()).collect();
                format!("⚠ {}", reasons.join(", "))
            };
            renderer.info(&format!(
                "{:20} {:8} {:10} {:14} {status}",
                e.name,
                e.source_id,
                trust,
                e.verification.label(),
            ));
        }

        if flagged > 0 {
            renderer.warn(&format!("{flagged} of {} need attention.", entries.len()));
        } else {
            renderer.success(&format!("All {} install(s) look fine.", entries.len()));
        }
        Ok(())
    }

    /// Build prompt flags from CLI globals, folding in a config `auto` default.
    fn prompt_flags(&self, config_auto: bool) -> PromptFlags {
        PromptFlags {
            auto: self.global.auto || config_auto,
            yes: self.global.yes,
            no: self.global.no,
        }
    }
}

fn not_yet(renderer: &Renderer, cmd: &str, phase: &str) -> crate::error::Result<()> {
    renderer.warn(&format!("`jii {cmd}` is not implemented yet (arrives in {phase})."));
    Ok(())
}
