//! Command-line surface: clap definitions, global flags, and dispatch.
//!
//! The command set is intentionally the full, stable surface from
//! `docs/ARCHITECTURE.md` §13. Commands not yet implemented return a clear
//! "not yet" message that names the phase, so the CLI shape never churns.

use clap::{Parser, Subcommand};

use crate::config::{ColorChoice, Config, Profile};
use crate::engine::Engine;
use crate::model::{InstalledRecord, PackageCandidate, Query};
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

    /// Package(s) to install when no subcommand is given, e.g. `jii fastfetch cava`.
    #[arg(value_name = "PACKAGE")]
    pub packages: Vec<String>,
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
    /// Search, rank, recommend and install one or more packages.
    Install {
        /// Package name(s).
        #[arg(required = true)]
        packages: Vec<String>,
    },
    /// Remove one or more packages using the source that installed each.
    Remove {
        /// Package name(s).
        #[arg(required = true)]
        packages: Vec<String>,
    },
    /// Update one or more packages, or everything if none are named.
    Update {
        /// Package name(s); empty updates everything.
        packages: Vec<String>,
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
    /// List installation sources (providers) and whether each is usable here.
    Sources,
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
            // Explicit `jii install <pkg…>` or bare `jii <pkg…>`.
            Some(Commands::Install { packages }) => {
                self.install(packages, config, &renderer).await
            }
            None => {
                if self.packages.is_empty() {
                    renderer.info("Usage: jii <package…>  (try `jii --help`)");
                    Ok(())
                } else {
                    self.install(&self.packages, config, &renderer).await
                }
            }

            // Implemented in Phase 2.
            Some(Commands::Remove { packages }) => self.remove(packages, config, &renderer).await,
            Some(Commands::Why { package }) => self.why(package, config, &renderer),
            Some(Commands::List) => self.list(config, &renderer),
            Some(Commands::History) => self.history(config, &renderer),

            Some(Commands::Doctor) => self.doctor(config, &renderer).await,
            Some(Commands::Audit) => self.audit(config, &renderer),

            Some(Commands::Update { packages }) => self.update(packages, config, &renderer).await,

            Some(Commands::Search { query }) => self.search(query, config, &renderer).await,
            Some(Commands::Info { package }) => self.info(package, config, &renderer).await,
            Some(Commands::Sources) => self.sources(config, &renderer).await,
        }
    }

    /// Install path (one or many packages): for each package search → rank → pick best,
    /// then let the engine group + optimize the chosen candidates into batched plans, and
    /// run them as **one** operation (one preview, one confirmation, one root escalation,
    /// one execution). A not-found package never cancels the rest (requirement: it is
    /// reported and the user is offered to continue).
    async fn install(
        &self,
        packages: &[String],
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        crate::platform::Platform::detect().require_supported()?;

        let mut engine = Engine::new(self.apply_profile(config))?;
        if !engine.has_providers() {
            renderer.error("No installation sources are enabled.");
            return Ok(());
        }

        // 1. Resolve each package to its best candidate; collect the misses separately.
        //    A single package keeps the "Also available" alternatives view; a real batch
        //    would make that too noisy, so it is shown only when installing one.
        let single = packages.len() == 1;
        let mut chosen: Vec<PackageCandidate> = Vec::new();
        let mut not_found: Vec<String> = Vec::new();
        for name in packages {
            let query = Query::name(name);
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
                not_found.push(name.clone());
                continue;
            }
            let best = ranked.remove(0);
            if single {
                self.show_alternatives(&ranked, renderer);
            }
            chosen.push(best);
        }

        // 2. Report misses. If nothing resolved, stop; otherwise offer to continue.
        if !not_found.is_empty() {
            let via = match &self.global.source {
                Some(source) => format!(" via source '{source}'"),
                None => String::new(),
            };
            renderer.error(&format!("Not found{via}: {}", not_found.join(", ")));
        }
        if chosen.is_empty() {
            return Ok(());
        }
        if !not_found.is_empty() {
            let flags = self.prompt_flags(engine.config().install.auto);
            if !prompt::confirm(renderer, "Continue installing the rest?", true, &flags) {
                renderer.info("Aborted.");
                return Ok(());
            }
        }

        // 3. Group + optimize into batched plans (merged per source where it can batch).
        let batch = engine.plan_install_batch(chosen).await?;

        // 4. Preview: grouped summary by source, then the full action preview.
        self.preview_batch(&batch, renderer);

        if self.global.dry_run {
            renderer.info("(dry-run: nothing was installed)");
            return Ok(());
        }

        // 5. One confirmation, governed by the least-trusted candidate (untrusted always
        //    needs an explicit answer, even under --auto — ADR-0006).
        let installed: Vec<String> = batch
            .iter()
            .flat_map(|b| b.candidates.iter().map(|c| c.name.clone()))
            .collect();
        let least_trusted = batch
            .iter()
            .flat_map(|b| b.candidates.iter())
            .map(|c| c.trust)
            .max()
            .unwrap_or(crate::model::TrustLevel::Official);
        let flags = self.prompt_flags(engine.config().install.auto);
        if !prompt::confirm_install_batch(
            renderer,
            least_trusted,
            installed.len(),
            engine.config(),
            &flags,
        ) {
            renderer.info("Aborted.");
            return Ok(());
        }

        // 6. One escalation, one run; records are written as each plan succeeds.
        engine.install_batch(&batch, renderer).await?;
        renderer.success(&format!("Installed {}.", installed.join(", ")));
        Ok(())
    }

    /// Batch preview: a grouped "what will be installed, by source" summary, then each
    /// plan's action preview (so the merged commands are visible before confirming).
    fn preview_batch(&self, batch: &[crate::engine::BatchPlan], renderer: &Renderer) {
        if renderer.is_json() {
            for bp in batch {
                renderer.plan(&bp.plan);
            }
            return;
        }
        renderer.info("Summary:");
        for bp in batch {
            renderer.info(&format!("{}:", bp.plan.source_id));
            for candidate in &bp.candidates {
                let version = candidate
                    .version
                    .as_ref()
                    .map(|v| format!(" (v{v})"))
                    .unwrap_or_default();
                renderer.info(&format!("  - {}{version}", candidate.name));
            }
        }
        for bp in batch {
            renderer.plan(&bp.plan);
        }
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

    /// Fold the `--profile` flag into the config.
    fn apply_profile(&self, mut config: Config) -> Config {
        if let Some(profile) = self.global.profile {
            config.install.profile = profile;
        }
        config
    }

    /// Remove path (one or many packages): resolve each to its owning record, then let the
    /// engine group + merge same-source removals into one command where the source can
    /// (`dnf remove a b c`), and run them as **one** operation (one preview, one
    /// confirmation, one root escalation, one execution). A not-installed package is
    /// reported and never cancels the rest (offer to continue).
    async fn remove(
        &self,
        packages: &[String],
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        crate::platform::Platform::detect().require_supported()?;

        let mut engine = Engine::new(config)?;

        // 1. Resolve each name to its owning record; collect the ones jii didn't install.
        let mut records: Vec<InstalledRecord> = Vec::new();
        let mut not_installed: Vec<String> = Vec::new();
        for name in packages {
            match engine.resolve_installed(name).await {
                Ok(record) => records.push(record),
                Err(_) => not_installed.push(name.clone()),
            }
        }
        if !not_installed.is_empty() {
            renderer.error(&format!("Not installed via jii: {}", not_installed.join(", ")));
        }
        if records.is_empty() {
            return Ok(());
        }
        if !not_installed.is_empty() {
            let flags = self.prompt_flags(false);
            if !prompt::confirm(renderer, "Continue removing the rest?", true, &flags) {
                renderer.info("Aborted.");
                return Ok(());
            }
        }

        // 2. Group + merge into batched plans.
        let batch = engine
            .plan_record_batch(records, crate::engine::RecordOp::Remove)
            .await?;
        for (name, reason) in &batch.unplannable {
            renderer.warn(&format!("✗ {name}: cannot plan removal ({reason})"));
        }
        if batch.plans.is_empty() {
            renderer.info("Nothing to remove.");
            return Ok(());
        }

        // 3. Preview, dry-run guard, one confirmation (default no — removal is destructive).
        self.preview_record_batch(&batch.plans, renderer);
        if self.global.dry_run {
            renderer.info("(dry-run: nothing was removed)");
            return Ok(());
        }
        let names = record_batch_names(&batch.plans);
        let flags = self.prompt_flags(false);
        let question = if names.len() == 1 {
            format!("Remove {}?", names[0])
        } else {
            format!("Remove {} packages?", names.len())
        };
        if !prompt::confirm(renderer, &question, false, &flags) {
            renderer.info("Aborted.");
            return Ok(());
        }

        // 4. One escalation, one run; records cleared as each plan succeeds.
        engine.remove_batch(&batch.plans, renderer).await?;
        renderer.success(&format!("Removed {}.", names.join(", ")));
        Ok(())
    }

    /// Batch preview for remove/update: each plan's action preview (the merged commands
    /// are visible before confirming). In JSON mode, the plans as JSON.
    fn preview_record_batch(&self, batch: &[crate::engine::RecordBatchPlan], renderer: &Renderer) {
        for bp in batch {
            renderer.plan(&bp.plan);
        }
    }

    /// Update path (one, many, or all packages): for each recorded install, re-search its
    /// owning source for the latest version, skip provably-current packages, then let the
    /// engine group + merge same-source updates into one command where the source can
    /// (`dnf upgrade a b c`) and run them as **one** operation. No per-source branching —
    /// the engine resolves each record's provider (ADR-0004/0025). A named package that is
    /// not installed is reported; a package whose source can't update is warned and
    /// skipped, never cancelling the rest.
    async fn update(
        &self,
        packages: &[String],
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        crate::platform::Platform::detect().require_supported()?;

        let mut engine = Engine::new(self.apply_profile(config))?;
        if !engine.has_providers() {
            renderer.error("No installation sources are enabled.");
            return Ok(());
        }

        // The records to consider: named packages (each must be installed), or all.
        let records = if packages.is_empty() {
            engine.registry().installed().to_vec()
        } else {
            let mut resolved = Vec::new();
            let mut not_installed = Vec::new();
            for name in packages {
                match engine.resolve_installed(name).await {
                    Ok(record) => resolved.push(record),
                    Err(_) => not_installed.push(name.clone()),
                }
            }
            if !not_installed.is_empty() {
                renderer.error(&format!("Not installed via jii: {}", not_installed.join(", ")));
            }
            resolved
        };
        if records.is_empty() {
            renderer.info("Nothing installed via jii yet.");
            return Ok(());
        }

        // Re-search each record's source for the latest version, skip those already newest,
        // and build the **post-update** records (version set to the refreshed target) plus
        // human transition lines. The engine stamps installed_at/verification on write.
        let mut refreshed: Vec<InstalledRecord> = Vec::new();
        let mut transitions: Vec<String> = Vec::new();
        let mut up_to_date = 0usize;
        for record in records {
            if let Some(source) = &self.global.source
                && &record.source_id != source
            {
                continue;
            }
            let target = self.latest_from_source(&engine, &record).await;
            // Exact version match = already newest. Conservative: differing version
            // formats never match, so we only ever *skip* a provably-current package —
            // an up-to-date system reads as a clean no-op, not a surprise reinstall.
            if let (Some(latest), Some(current)) = (&target, &record.version)
                && latest.version.as_ref() == Some(current)
            {
                up_to_date += 1;
                continue;
            }
            // Post-update version: the refreshed target, falling back to the prior version
            // when the owning source no longer reports one.
            let new_version = target
                .as_ref()
                .and_then(|c| c.version.clone())
                .or_else(|| record.version.clone());
            if let (Some(old), Some(new)) = (&record.version, &new_version)
                && old != new
            {
                transitions.push(format!("{}: {old} → {new}", record.name));
            }
            let mut post = record.clone();
            post.version = new_version;
            refreshed.push(post);
        }

        if refreshed.is_empty() {
            if up_to_date > 0 {
                renderer.success(&format!("All {up_to_date} package(s) already up to date."));
            } else {
                renderer.info("No updatable packages.");
            }
            return Ok(());
        }

        // Group + merge into batched update plans (skipping any source can't plan).
        let batch = engine
            .plan_record_batch(refreshed, crate::engine::RecordOp::Update)
            .await?;
        for (name, reason) in &batch.unplannable {
            renderer.warn(&format!("✗ {name}: cannot plan update ({reason})"));
        }
        if batch.plans.is_empty() {
            renderer.info("No updatable packages.");
            return Ok(());
        }

        // Preview: version transitions, then each plan.
        for line in &transitions {
            renderer.info(line);
        }
        self.preview_record_batch(&batch.plans, renderer);
        if up_to_date > 0 {
            renderer.info(&format!("({up_to_date} already up to date)"));
        }

        if self.global.dry_run {
            renderer.info("(dry-run: nothing was updated)");
            return Ok(());
        }

        // One confirmation, one escalation, one run; records refreshed as each succeeds.
        let names = record_batch_names(&batch.plans);
        let flags = self.prompt_flags(engine.config().install.auto);
        let question = if names.len() == 1 {
            format!("Update {}?", names[0])
        } else {
            format!("Update {} packages?", names.len())
        };
        if !prompt::confirm(renderer, &question, true, &flags) {
            renderer.info("Aborted.");
            return Ok(());
        }

        engine.update_batch(&batch.plans, renderer).await?;
        renderer.success(&format!("Updated {}.", names.join(", ")));
        Ok(())
    }

    /// Re-search an installed record's **owning** source for its latest candidate (the
    /// normal search→rank path, filtered to that source). `None` if the source no longer
    /// offers it — the update can still proceed, just without a refreshed version.
    async fn latest_from_source(
        &self,
        engine: &Engine,
        record: &InstalledRecord,
    ) -> Option<PackageCandidate> {
        let query = Query::name(&record.name);
        let mut ranked = engine.rank(engine.search(&query).await.candidates);
        ranked.retain(|c| c.source_id == record.source_id);
        ranked.into_iter().next()
    }

    /// Search path: show ranked candidates for a query without installing anything.
    /// Read-only — same search→rank the install path uses, just rendered, not executed.
    async fn search(
        &self,
        terms: &[String],
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        crate::platform::Platform::detect().require_supported()?;
        let engine = Engine::new(self.apply_profile(config))?;
        if !engine.has_providers() {
            renderer.error("No installation sources are enabled.");
            return Ok(());
        }
        let name = terms.join(" ");
        let ranked = self.ranked_for(&engine, &name, renderer).await;
        if ranked.is_empty() {
            renderer.error(&format!("No candidates found for '{name}'."));
            return Ok(());
        }
        if renderer.is_json() {
            renderer.json_value(&serde_json::json!(ranked));
            return Ok(());
        }
        renderer.info(&format!("Candidates for '{name}' (best first):"));
        for (i, candidate) in ranked.iter().enumerate() {
            let mark = if i == 0 { "→" } else { " " };
            renderer.info(&format!("{mark} {}", candidate_line(candidate)));
        }
        Ok(())
    }

    /// Info path: show every enabled source that offers a package, the recommended one,
    /// and why — for transparency, without installing. Read-only.
    async fn info(
        &self,
        package: &str,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        crate::platform::Platform::detect().require_supported()?;
        let engine = Engine::new(self.apply_profile(config))?;
        if !engine.has_providers() {
            renderer.error("No installation sources are enabled.");
            return Ok(());
        }
        let ranked = self.ranked_for(&engine, package, renderer).await;
        if ranked.is_empty() {
            renderer.error(&format!("'{package}' is not available from any enabled source."));
            return Ok(());
        }
        if renderer.is_json() {
            renderer.json_value(&serde_json::json!(ranked));
            return Ok(());
        }
        renderer.info(&format!(
            "{package} — available from {} source(s):",
            ranked.len()
        ));
        for candidate in &ranked {
            renderer.info(&format!("  {}", candidate_line(candidate)));
        }
        let best = &ranked[0];
        renderer.info(&format!("Recommended: {}", best.source_id));
        for reason in recommendation_reasons(best) {
            renderer.info(&format!("  ✓ {reason}"));
        }
        Ok(())
    }

    /// Sources path: list enabled providers and whether each is usable on this machine.
    async fn sources(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let catalog = engine.source_catalog().await;

        if renderer.is_json() {
            let rows: Vec<_> = catalog
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id, "trust": e.trust.label(), "available": e.available,
                    })
                })
                .collect();
            renderer.json_value(&serde_json::json!(rows));
            return Ok(());
        }

        let (active, inactive): (Vec<_>, Vec<_>) = catalog.iter().partition(|e| e.available);
        if !active.is_empty() {
            renderer.info("Active sources:");
            for e in &active {
                renderer.info(&format!("  ✓ {:8} ({})", e.id, e.trust.label()));
            }
        }
        if !inactive.is_empty() {
            renderer.info("Enabled but unavailable (tool not installed):");
            for e in &inactive {
                renderer.info(&format!("  ✗ {:8} ({})", e.id, e.trust.label()));
            }
        }
        renderer.info("More sources arrive in upcoming releases — see docs/ROADMAP.md.");
        Ok(())
    }

    /// Search + rank a name across enabled sources, printing any source failures (a
    /// source that was unavailable/errored). Shared by the read-only `search`/`info`
    /// paths; honors `--source`.
    async fn ranked_for(
        &self,
        engine: &Engine,
        name: &str,
        renderer: &Renderer,
    ) -> Vec<PackageCandidate> {
        let query = Query::name(name);
        let result = engine.search(&query).await;
        for (source, reason) in &result.failed {
            renderer.warn(&format!("✗ {source}: {reason}"));
        }
        let mut ranked = engine.rank(result.candidates);
        if let Some(source) = &self.global.source {
            ranked.retain(|c| &c.source_id == source);
        }
        ranked
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

/// The names covered by a remove/update batch, flattened across plans (each plan may
/// cover several records when the source merged them).
fn record_batch_names(batch: &[crate::engine::RecordBatchPlan]) -> Vec<String> {
    batch
        .iter()
        .flat_map(|bp| bp.records.iter().map(|r| r.name.clone()))
        .collect()
}

/// A compact one-line description of a candidate for `search`/`info`:
/// `source  vX  trust  — summary`.
fn candidate_line(candidate: &PackageCandidate) -> String {
    let version = candidate
        .version
        .as_ref()
        .map(|v| format!("v{v}  "))
        .unwrap_or_default();
    let summary = candidate
        .summary
        .as_deref()
        .map(|s| format!("  — {}", one_line(s, 80)))
        .unwrap_or_default();
    format!(
        "{:8} {version}{}{summary}",
        candidate.source_id,
        candidate.trust.label()
    )
}

/// Collapse a (possibly multi-line) summary to a single trimmed line, truncated to
/// `max` chars with an ellipsis — keeps `search`/`info` rows one line each.
fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        let mut s: String = flat.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    } else {
        flat
    }
}

/// Why the recommended candidate was chosen, derived only from **source-agnostic** model
/// fields (trust, signature, version, arch) — deliberately no branching on the concrete
/// source id (ADR-0004 holds in the UI too). Richer, plan-level reasons appear at install
/// time; this is the lightweight read-only rationale for `jii info`.
fn recommendation_reasons(candidate: &PackageCandidate) -> Vec<String> {
    let mut reasons = vec![format!("{} source", candidate.trust.label())];
    if candidate.signed {
        reasons.push("signature/checksum verifiable".to_string());
    }
    if let Some(version) = &candidate.version {
        reasons.push(format!("version {version}"));
    }
    if !candidate.arch_ok {
        reasons.push("⚠ may not match this architecture".to_string());
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PkgVersion, TrustLevel};

    fn candidate(trust: TrustLevel, signed: bool, version: Option<&str>) -> PackageCandidate {
        PackageCandidate {
            name: "example".into(),
            source_id: "dnf".into(),
            version: version.map(PkgVersion::new),
            trust,
            arch_ok: true,
            signed,
            summary: None,
            raw: serde_json::Value::Null,
        }
    }

    #[test]
    fn reasons_lead_with_trust_and_include_signature_and_version() {
        let reasons = recommendation_reasons(&candidate(TrustLevel::Official, true, Some("1.2")));
        assert_eq!(reasons[0], "official source");
        assert!(reasons.iter().any(|s| s.contains("verifiable")));
        assert!(reasons.iter().any(|s| s == "version 1.2"));
    }

    #[test]
    fn unsigned_candidate_omits_signature_reason() {
        let reasons = recommendation_reasons(&candidate(TrustLevel::Untrusted, false, None));
        assert_eq!(reasons, vec!["untrusted source".to_string()]);
    }

    #[test]
    fn arch_mismatch_is_flagged() {
        let mut c = candidate(TrustLevel::Community, false, None);
        c.arch_ok = false;
        assert!(
            recommendation_reasons(&c)
                .iter()
                .any(|s| s.contains("architecture"))
        );
    }

    #[test]
    fn one_line_flattens_and_truncates() {
        assert_eq!(one_line("a\n  b   c", 80), "a b c");
        let long = "word ".repeat(40);
        let out = one_line(&long, 20);
        assert_eq!(out.chars().count(), 20);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn candidate_line_includes_source_version_trust() {
        let line = candidate_line(&candidate(TrustLevel::Official, true, Some("2.0")));
        assert!(line.contains("dnf"));
        assert!(line.contains("v2.0"));
        assert!(line.contains("official"));
    }
}
