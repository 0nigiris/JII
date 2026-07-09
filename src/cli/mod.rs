//! Command-line surface: clap definitions, global flags, and dispatch.
//!
//! The command set is intentionally the full, stable surface from
//! `docs/ARCHITECTURE.md` §13. Commands not yet implemented return a clear
//! "not yet" message that names the phase, so the CLI shape never churns.

use clap::{Parser, Subcommand};

use crate::config::{ColorChoice, Config, Profile};
use crate::engine::Engine;
use crate::model::{InstalledRecord, PackageCandidate, PackageSpec, Query};
use crate::provider::Bootstrap;
use crate::ui::Renderer;
use crate::ui::prompt::{self, PromptFlags};

/// Just Install It — a smart universal package installer for Linux.
#[derive(Debug, Parser)]
#[command(name = "jii", version, about)]
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
    #[arg(short = 'd', long, global = true)]
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
    /// Explain how JII would install (or did install) a package.
    #[command(alias = "why")]
    How {
        /// Package name.
        package: String,
    },
    /// Report source availability, latency and health, plus host system checks.
    Doctor {
        /// Offer to fix what can be fixed (install git/curl, add the Flathub remote).
        /// Each fix is previewed and confirmed; nothing is changed silently.
        #[arg(long)]
        fix: bool,
    },
    /// Audit installed software: source, trust, verification and concerns.
    Audit,
    /// List software installed via JII.
    List,
    /// Show installation history.
    History,
    /// List installation sources (providers) and whether each is usable here.
    Sources,
    /// Manage the ecosystem managers themselves (npm, cargo, brew, Flatpak…): show what
    /// is installed and bootstrap a missing one.
    Providers {
        #[command(subcommand)]
        action: Option<ProvidersAction>,
    },
    /// Run the first-run setup wizard again (choose mode, optional system check).
    Setup,
}

/// Actions under `jii providers` (bare `jii providers` lists them).
#[derive(Debug, Subcommand)]
pub enum ProvidersAction {
    /// Bootstrap a missing ecosystem manager, e.g. `jii providers add npm`.
    Add {
        /// The ecosystem id (npm, cargo, go, pipx, flatpak, snap, brew, nix).
        name: String,
    },
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
        // `-v` forces Advanced for this run; otherwise the configured mode (default Friendly).
        let mode = if self.global.verbose > 0 {
            crate::config::OutputMode::Advanced
        } else {
            config.ui.mode
        };
        let renderer = Renderer::new(self.color_choice(&config), self.global.json, mode);

        match &self.command {
            // Explicit `jii install <pkg…>` or bare `jii <pkg…>`.
            Some(Commands::Install { packages }) => {
                self.install(packages, config, &renderer).await
            }
            None => {
                if self.packages.is_empty() {
                    // Very first bare `jii` on an interactive terminal → a warm welcome + the
                    // 30-second setup wizard (once). Otherwise the usual usage hint.
                    if config.is_first_run() && self.interactive(&renderer) {
                        self.setup(config, &renderer, true).await
                    } else {
                        renderer.info("Usage: jii <package…>  (try `jii --help`)");
                        Ok(())
                    }
                } else {
                    self.install(&self.packages, config, &renderer).await
                }
            }

            // Implemented in Phase 2.
            Some(Commands::Remove { packages }) => self.remove(packages, config, &renderer).await,
            Some(Commands::How { package }) => self.how(package, config, &renderer),
            Some(Commands::List) => self.list(config, &renderer),
            Some(Commands::History) => self.history(config, &renderer),

            Some(Commands::Doctor { fix }) => self.doctor(*fix, config, &renderer).await,
            Some(Commands::Audit) => self.audit(config, &renderer),

            Some(Commands::Update { packages }) => self.update(packages, config, &renderer).await,

            Some(Commands::Search { query }) => self.search(query, config, &renderer).await,
            Some(Commands::Info { package }) => self.info(package, config, &renderer).await,
            Some(Commands::Sources) => self.sources(config, &renderer).await,
            Some(Commands::Providers { action }) => match action {
                None => self.providers(config, &renderer).await,
                Some(ProvidersAction::Add { name }) => {
                    self.providers_add(name, config, &renderer).await
                }
            },
            Some(Commands::Setup) => self.setup(config, &renderer, false).await,
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
        let mut engine = Engine::new(self.apply_profile(config))?;
        if !self.ensure_usable_source(&engine, renderer).await {
            return Ok(());
        }

        // 0. Parse each argument as a package spec — `name[:source][@ref]` (ADR-0031). Parsing
        //    lives only here (via PackageSpec::parse); the rest of the flow works on name +
        //    optional source. Version/channel pinning (`@ref`) is parsed but not yet
        //    implemented, so reject it clearly rather than silently installing the latest.
        let specs = match self.parse_specs(packages, renderer) {
            Some(specs) => specs,
            None => return Ok(()),
        };

        // 1. Resolve each package to its best candidate; collect the misses separately.
        //    A single package keeps the "Also available" alternatives view; a real batch
        //    would make that too noisy, so it is shown only when installing one.
        let single = specs.len() == 1;
        let effective_auto = self.global.auto || engine.config().install.auto;
        let mut chosen: Vec<PackageCandidate> = Vec::new();
        let mut not_found: Vec<String> = Vec::new();
        let mut chose_interactively = false;
        // A single lively "Searching…" in Friendly; Advanced narrates each package below.
        if renderer.is_friendly() {
            renderer.info("Searching…");
        }
        for spec in &specs {
            // A per-package `:source` (ADR-0031) pins the provider and, like `--source`,
            // suppresses the chooser; it takes precedence over the whole-command `--source`.
            let pkg_source = spec.source.as_ref().or(self.global.source.as_ref());
            let name = &spec.name;
            let query = Query::name(name);
            if !renderer.is_friendly() {
                renderer.info(&format!("Searching for '{}'...", query.raw));
            }
            let result = engine.search(&query).await;
            self.report_source_failures(&result.failed, renderer);
            let mut ranked = engine.rank(result.candidates);
            if let Some(source) = pkg_source {
                ranked.retain(|c| &c.source_id == source);
            }
            if ranked.is_empty() {
                not_found.push(name.clone());
                continue;
            }

            // Cooperate with the system, don't clobber it (UX #3): if the package is
            // already installed, say so instead of planning a pointless reinstall. We can
            // only compare versions *within the same owning source* — versions are opaque
            // across sources (ADR-0009), so a package present via another source reads as
            // "already installed", not "outdated". `resolve_installed` uses the registry
            // hint first, then a provider scan, so it also spots installs done outside jii.
            let recommended_source = ranked[0].source_id.clone();
            let available = ranked[0].version.clone();
            if let Some(record) = engine.installed_lookup(name, &recommended_source).await {
                let same_source = record.source_id == recommended_source;
                let outdated = same_source && available.is_some() && available != record.version;
                if !outdated {
                    let v = record
                        .version
                        .as_ref()
                        .map(|v| format!(" ({v})"))
                        .unwrap_or_default();
                    renderer.success(&format!(
                        "{name} already installed via {}{v}",
                        record.source_id
                    ));
                    continue;
                }
                // Same source, a newer version is available → offer an in-place update
                // (which is exactly what re-installing via this source does). A real batch
                // includes it without prompting; a single install asks once.
                renderer.info(&format!(
                    "{name} is already installed via {} ({}). Available: {}.",
                    record.source_id,
                    version_or_unknown(record.version.as_ref()),
                    version_or_unknown(available.as_ref()),
                ));
                if single && !self.global.dry_run {
                    let flags = self.prompt_flags(engine.config().install.auto);
                    if !prompt::confirm(renderer, "Update now?", true, &flags) {
                        renderer.info("Keeping the installed version.");
                        continue;
                    }
                }
                // Confirming the update is itself the consent, so a trusted-enough in-place
                // update skips the redundant batch confirm below (same rule as a chooser pick).
                chose_interactively = true;
                chosen.push(ranked.remove(0));
                continue;
            }

            // When a single install has genuine choice and the session is interactive,
            // let the user pick which source rather than silently taking the top rank
            // (the recommendation is the pre-selected default — Enter installs it). Batch
            // installs stay auto-picked to avoid a prompt storm, and --source/--auto/
            // --yes/--no or a non-TTY skip the chooser too (they already express intent).
            let offer_choice = single
                && ranked.len() > 1
                && pkg_source.is_none()
                && !effective_auto
                && !self.global.yes
                && !self.global.no
                && self.interactive(renderer);
            let best = if offer_choice {
                // The top (index 0) candidate is the recommendation — tag it so the menu says
                // *which* to pick and why it's first (#4); the rest are honest alternatives.
                let labels: Vec<String> = ranked
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        if i == 0 {
                            format!("{}  ⭐ recommended", candidate_line(c))
                        } else {
                            candidate_line(c)
                        }
                    })
                    .collect();
                let header = format!("'{name}' is available from several sources — you choose:");
                match prompt::choose(renderer, &header, &labels, 0) {
                    Some(index) => {
                        chose_interactively = true;
                        ranked.remove(index)
                    }
                    None => {
                        renderer.info("Aborted.");
                        return Ok(());
                    }
                }
            } else {
                let best = ranked.remove(0);
                if single {
                    self.show_alternatives(&ranked, renderer);
                }
                best
            };
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

        // 4. Preview. Friendly (and not a dry-run) gets one short line per package — name,
        //    version, source, a one-word "why", and whether it needs sudo. `--dry-run` and
        //    Advanced still show the full plan (the whole point of a dry-run is the detail).
        if renderer.is_friendly() && !self.global.dry_run {
            self.preview_batch_friendly(&batch, &engine, renderer);
        } else {
            self.preview_batch(&batch, renderer);
        }

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
        // An interactive chooser pick is itself the consent for a trusted-enough source,
        // so we don't ask twice; an untrusted pick still hits the trust barrier below
        // (ADR-0006 — untrusted always needs an explicit answer).
        let skip_confirm =
            chose_interactively && least_trusted <= engine.config().install.default_yes_max_trust;
        if !skip_confirm
            && !prompt::confirm_install_batch(
                renderer,
                least_trusted,
                installed.len(),
                engine.config(),
                &flags,
            )
        {
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
        // The grouped "what, by source" summary earns its space only for a real batch
        // (more than one plan or more than one package). A single-package install would
        // just repeat the Plan below it, so we skip straight to the plan (UX #9).
        let total: usize = batch.iter().map(|bp| bp.candidates.len()).sum();
        if batch.len() > 1 || total > 1 {
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
        }
        for bp in batch {
            renderer.plan(&bp.plan);
        }
    }

    /// Friendly install preview: one short line per package — `Install <name> (<version>) via
    /// <source> — <why>  [needs sudo]` — instead of the full Plan block. Keeps a normal install
    /// quiet and scannable (U5); the full plan is still shown under `--dry-run`/Advanced.
    fn preview_batch_friendly(
        &self,
        batch: &[crate::engine::BatchPlan],
        engine: &Engine,
        renderer: &Renderer,
    ) {
        for bp in batch {
            let sudo = if bp.plan.needs_root() { "  [needs sudo]" } else { "" };
            for candidate in &bp.candidates {
                let version = candidate
                    .version
                    .as_ref()
                    .map(|v| format!(" ({v})"))
                    .unwrap_or_default();
                let why = engine
                    .candidate_highlights(candidate)
                    .into_iter()
                    .next()
                    .map(|h| format!(" — {h}"))
                    .unwrap_or_default();
                renderer.info(&format!(
                    "Install {}{version} via {}{why}{sudo}",
                    candidate.name, candidate.source_id
                ));
            }
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

    /// Whether we can hold an interactive prompt here: a real terminal and not JSON mode.
    /// Gates the candidate chooser (and any future interactive selection).
    fn interactive(&self, renderer: &Renderer) -> bool {
        !renderer.is_json() && crate::platform::Platform::detect().is_tty
    }

    /// Report sources that errored/timed out during a search. Friendly mode stays quiet — a
    /// secondary source hiccup (e.g. a slow COPR) that didn't change the result is noise (UX
    /// #1/#8); Advanced (`-v`) lists each. Also drops the doubled marker — `warn` already
    /// prefixes `⚠`, so the old `✗ {source}` read as `⚠ ✗ {source}`.
    fn report_source_failures(&self, failed: &[(String, String)], renderer: &Renderer) {
        if renderer.is_friendly() {
            return;
        }
        for (source, reason) in failed {
            renderer.warn(&format!("{source}: {reason}"));
        }
    }

    /// Parse each argument into a [`PackageSpec`] (`name[:source][@ref]`, ADR-0031) — the one
    /// place package tokens are parsed, shared by install/remove/update/info. Returns `None`
    /// (after rendering a clear error) if any token is malformed, names an unknown source, or
    /// carries a version/channel `@ref`: the spec grammar is locked for 1.0 but version
    /// selection isn't built yet, so we reject `@ref` explicitly rather than act on a version
    /// we can't honour (respecting the user's intent).
    fn parse_specs(&self, packages: &[String], renderer: &Renderer) -> Option<Vec<PackageSpec>> {
        let mut specs = Vec::with_capacity(packages.len());
        for raw in packages {
            match PackageSpec::parse(raw) {
                Ok(spec) => specs.push(spec),
                Err(reason) => {
                    renderer.error(&format!("Invalid package '{raw}': {reason}"));
                    return None;
                }
            }
        }
        // A pinned source must be a real source id — catch a typo (`:flatpakk`) here with the
        // known list, rather than letting it silently become a "not found" after a search.
        // ADR-0031 places this validation (the did-you-mean) in the CLI, which holds the config.
        for spec in &specs {
            if let Some(source) = &spec.source
                && !crate::config::KNOWN_SOURCES.contains(&source.as_str())
            {
                renderer.error(&format!(
                    "Unknown source '{source}' in '{}:{source}'. Known sources: {}.",
                    spec.name,
                    crate::config::KNOWN_SOURCES.join(", ")
                ));
                return None;
            }
        }
        if let Some(spec) = specs.iter().find(|s| s.reference.is_some()) {
            let r = spec.reference.as_deref().unwrap_or("");
            renderer.error(&format!(
                "Version/channel pinning ('@{r}') isn't supported yet — it's coming with the \
                 version chooser. Use '{}' without the '@' part for now.",
                spec.name
            ));
            return None;
        }
        Some(specs)
    }

    /// Fold the `--profile` flag into the config.
    fn apply_profile(&self, mut config: Config) -> Config {
        if let Some(profile) = self.global.profile {
            config.install.profile = profile;
        }
        config
    }

    /// Guard shared by every source-touching command (install/remove/update/search/info):
    /// stop early with a clear message when JII has no usable source on this machine. This
    /// replaces the old Fedora-only `require_supported` wall with an honest, source-based
    /// notion of "supported" (ADR-0029) — "any enabled source whose backing tool is present
    /// here?". It distinguishes "none enabled" (config) from "none available" (no tool
    /// installed), and improves on the distro wall even on Fedora (disabling every source now
    /// says so plainly). Returns `true` to proceed, `false` after rendering the reason.
    async fn ensure_usable_source(&self, engine: &Engine, renderer: &Renderer) -> bool {
        if !engine.has_providers() {
            renderer.error("No installation sources are enabled.");
            return false;
        }
        if !engine.any_source_available().await {
            renderer.error("No usable installation source found on this system.");
            return false;
        }
        true
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
        let mut engine = Engine::new(config)?;
        if !self.ensure_usable_source(&engine, renderer).await {
            return Ok(());
        }

        // Parse arguments as package specs (ADR-0031). A per-package `:source` pins which copy
        // to remove and — like the multi-owner chooser it replaces — is the explicit answer to
        // "which one?". `@ref` is rejected (jii removes what's installed, not a version).
        let specs = match self.parse_specs(packages, renderer) {
            Some(specs) => specs,
            None => return Ok(()),
        };

        // 1. Resolve each name to its owning record(s). A package can be installed via more
        //    than one source (e.g. ripgrep via dnf *and* cargo); when it is, let the user pick
        //    which copy — or all — instead of guessing (UX #11). A pinned `:source` (or the
        //    whole-command `--source`) narrows to one and skips the chooser; a non-interactive
        //    session takes every owner (the removal preview + confirm below still gate it).
        //    Names jii can't find anywhere are collected as not-installed.
        let mut records: Vec<InstalledRecord> = Vec::new();
        let mut not_installed: Vec<String> = Vec::new();
        for spec in &specs {
            let pkg_source = spec.source.as_ref().or(self.global.source.as_ref());
            let name = &spec.name;
            let mut owners = engine.resolve_all_installed(name).await;
            if let Some(source) = pkg_source {
                owners.retain(|r| &r.source_id == source);
            }
            match owners.len() {
                0 => not_installed.push(name.clone()),
                1 => records.push(owners.pop().expect("len 1")),
                _ if !self.interactive(renderer) => records.extend(owners),
                _ => {
                    let mut labels: Vec<String> = owners
                        .iter()
                        .map(|r| format!("{} ({})", r.source_id, version_or_unknown(r.version.as_ref())))
                        .collect();
                    labels.push("all of them".to_string());
                    let header = format!("'{name}' is installed via several sources:");
                    match prompt::choose(renderer, &header, &labels, 0) {
                        // The extra last option ("all") sits at index owners.len().
                        Some(index) if index == owners.len() => records.extend(owners),
                        Some(index) => records.push(owners.swap_remove(index)),
                        None => {
                            renderer.info("Aborted.");
                            return Ok(());
                        }
                    }
                }
            }
        }
        if !not_installed.is_empty() {
            renderer.error(&format!("Not installed: {}", not_installed.join(", ")));
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
        // Bare `jii update` = update the whole system (every manager's bulk upgrade, D10).
        if packages.is_empty() {
            return self.update_system(config, renderer).await;
        }

        let mut engine = Engine::new(self.apply_profile(config))?;
        if !self.ensure_usable_source(&engine, renderer).await {
            return Ok(());
        }

        // The records to consider: the named packages (each must be installed).
        let records = {
            // Parse arguments as package specs (ADR-0031): a `:source` picks which installed
            // copy to update; `@ref` is rejected (targeting a version is the version chooser's
            // job). Without a pinned source we resolve the owning record cheaply (registry hint
            // first); the fan-out only runs when a source needs matching.
            let specs = match self.parse_specs(packages, renderer) {
                Some(specs) => specs,
                None => return Ok(()),
            };
            let mut resolved = Vec::new();
            let mut not_installed = Vec::new();
            for spec in &specs {
                let pkg_source = spec.source.as_ref().or(self.global.source.as_ref());
                let record = match pkg_source {
                    Some(source) => engine
                        .resolve_all_installed(&spec.name)
                        .await
                        .into_iter()
                        .find(|r| &r.source_id == source),
                    None => engine.resolve_installed(&spec.name).await.ok(),
                };
                match record {
                    Some(record) => resolved.push(record),
                    None => not_installed.push(spec.name.clone()),
                }
            }
            if !not_installed.is_empty() {
                renderer.error(&format!("Not installed: {}", not_installed.join(", ")));
            }
            resolved
        };
        if records.is_empty() {
            // Every named package was unresolved; the `Not installed: …` line above already
            // said so. Don't follow it with a misleading "nothing installed" ledger claim.
            return Ok(());
        }

        // Re-search each record's source for the latest version, skipping provably-current ones.
        let (refreshed, transitions, up_to_date) = self.refresh_for_update(&engine, records).await;

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

    /// Update the whole system (bare `jii update`, D10): aggregate every manager's bulk
    /// "update everything I own" plan (`dnf upgrade`, `flatpak update`, …), and — so nothing
    /// JII installed is missed — fall back to per-record updates for the sources that have no
    /// bulk path (github, cargo, go). One preview, one confirmation, one privilege escalation,
    /// one run. The bulk plans upgrade the system beyond JII's registry, so they aren't
    /// recorded; only the per-record fallbacks refresh the registry.
    async fn update_system(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let mut engine = Engine::new(self.apply_profile(config))?;
        if !self.ensure_usable_source(&engine, renderer).await {
            return Ok(());
        }

        let system = engine.plan_update_all().await?;
        let covered: std::collections::HashSet<&str> =
            system.sources.iter().map(|s| s.as_str()).collect();

        // JII-tracked packages whose source offers no bulk update-all still get updated,
        // per-record, so a bare `jii update` misses nothing it installed.
        let fallback_records: Vec<InstalledRecord> = engine
            .registry()
            .installed()
            .iter()
            .filter(|r| !covered.contains(r.source_id.as_str()))
            .cloned()
            .collect();
        let (refreshed, transitions, up_to_date) =
            self.refresh_for_update(&engine, fallback_records).await;
        let fallback = engine
            .plan_record_batch(refreshed, crate::engine::RecordOp::Update)
            .await?;
        for (name, reason) in &fallback.unplannable {
            renderer.warn(&format!("✗ {name}: cannot plan update ({reason})"));
        }

        if system.plans.is_empty() && fallback.plans.is_empty() {
            renderer.info("Nothing to update.");
            return Ok(());
        }

        // Preview: the bulk managers, then any per-record fallbacks + version transitions.
        if !system.plans.is_empty() {
            renderer.info(&format!("System update via: {}", system.sources.join(", ")));
        }
        if renderer.is_friendly() && !self.global.dry_run {
            for plan in &system.plans {
                let why = plan.reasons.first().cloned().unwrap_or_default();
                let sudo = if plan.needs_root() { "  [needs sudo]" } else { "" };
                renderer.info(&format!("  {why}{sudo}"));
            }
        } else {
            for plan in &system.plans {
                renderer.plan(plan);
            }
        }
        for line in &transitions {
            renderer.info(line);
        }
        self.preview_record_batch(&fallback.plans, renderer);
        if up_to_date > 0 {
            renderer.info(&format!("({up_to_date} tracked package(s) already up to date)"));
        }

        if self.global.dry_run {
            renderer.info("(dry-run: nothing was updated)");
            return Ok(());
        }

        let flags = self.prompt_flags(engine.config().install.auto);
        if !prompt::confirm(renderer, "Update your system now?", true, &flags) {
            renderer.info("Aborted.");
            return Ok(());
        }

        engine
            .run_system_update(&system.plans, &fallback.plans, renderer)
            .await?;
        renderer.success("System update complete.");
        Ok(())
    }

    /// Re-search each record's owning source for its latest version, skip the provably-current
    /// ones (exact version-string match — conservative, so we only ever *skip* what is surely
    /// current), and return the post-update records (version set to the refreshed target),
    /// human `old → new` transition lines, and how many were already current. Shared by the
    /// named-package and system-fallback update paths; the engine stamps installed_at/
    /// verification on write.
    async fn refresh_for_update(
        &self,
        engine: &Engine,
        records: Vec<InstalledRecord>,
    ) -> (Vec<InstalledRecord>, Vec<String>, usize) {
        let mut refreshed = Vec::new();
        let mut transitions = Vec::new();
        let mut up_to_date = 0usize;
        for record in records {
            let target = self.latest_from_source(engine, &record).await;
            if let (Some(latest), Some(current)) = (&target, &record.version)
                && latest.version.as_ref() == Some(current)
            {
                up_to_date += 1;
                continue;
            }
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
        (refreshed, transitions, up_to_date)
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
        let engine = Engine::new(self.apply_profile(config))?;
        if !self.ensure_usable_source(&engine, renderer).await {
            return Ok(());
        }
        // `search` is free-text discovery, not a package spec (ADR-0031) — the terms are the
        // query verbatim; `--source` still narrows the results.
        let name = terms.join(" ");
        let ranked = self.ranked_for(&engine, &name, self.global.source.as_ref(), renderer).await;
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
        let engine = Engine::new(self.apply_profile(config))?;
        if !self.ensure_usable_source(&engine, renderer).await {
            return Ok(());
        }
        // Parse the argument as a package spec (ADR-0031): a `:source` narrows `info` to that
        // provider; `@ref` is rejected until version selection lands.
        let specs = match self.parse_specs(&[package.to_string()], renderer) {
            Some(specs) => specs,
            None => return Ok(()),
        };
        let spec = &specs[0];
        let name = &spec.name;
        let pkg_source = spec.source.as_ref().or(self.global.source.as_ref());
        let ranked = self.ranked_for(&engine, name, pkg_source, renderer).await;
        if ranked.is_empty() {
            renderer.error(&format!("'{name}' is not available from any enabled source."));
            return Ok(());
        }
        if renderer.is_json() {
            renderer.json_value(&serde_json::json!(ranked));
            return Ok(());
        }
        renderer.info(&format!(
            "{name} — available from {} source(s):",
            ranked.len()
        ));
        for candidate in &ranked {
            renderer.info(&format!("  {}", candidate_line(candidate)));
        }
        let best = &ranked[0];
        renderer.info(&format!("Recommended: {}", best.source_id));
        let highlights = engine.candidate_highlights(best);
        for reason in recommendation_reasons(best, highlights) {
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
        Ok(())
    }

    /// `jii providers` — the ecosystem *marketplace* view (#7): which language/app managers
    /// (npm, cargo, brew, Flatpak…) are installed on this host, and how to bootstrap the
    /// missing ones. Read-only; base system repos (dnf/apt) and non-managers (github) don't
    /// appear — you don't install those, they *are* the system.
    async fn providers(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let catalog = engine.ecosystem_catalog().await;

        if renderer.is_json() {
            let rows: Vec<_> = catalog
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id, "label": e.label, "binary": e.binary,
                        "trust": e.trust.label(), "installed": e.installed,
                    })
                })
                .collect();
            renderer.json_value(&serde_json::json!(rows));
            return Ok(());
        }

        let (have, missing): (Vec<_>, Vec<_>) = catalog.iter().partition(|e| e.installed);
        if !have.is_empty() {
            renderer.info("Installed ecosystems:");
            for e in &have {
                renderer.info(&format!("  ✓ {}", e.label));
            }
        }
        if !missing.is_empty() {
            if !have.is_empty() {
                renderer.info("");
            }
            renderer.info("Available to install:");
            for e in &missing {
                renderer.info(&format!("  ○ {} — jii providers add {}", e.label, e.id));
            }
        }
        Ok(())
    }

    /// `jii providers add <name>` — bootstrap a missing ecosystem manager (#8). Two honest
    /// paths, no magic: a manager that lives in the distro repos (npm, cargo, go, pipx,
    /// flatpak, snap) is resolved cross-distro (`nodejs-npm` on Fedora, `npm` elsewhere) and
    /// handed to the **normal install path** — so it gets the same preview → confirm →
    /// execute → record flow as any package (the doctor `--fix` pattern). A manager that
    /// bootstraps via its own upstream script (Homebrew, Nix) is **shown, never run** — JII
    /// does not pipe an installer into your shell (ADR-0005/0006 trust boundary).
    async fn providers_add(
        &self,
        name: &str,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let engine = Engine::new(config.clone())?;
        let catalog = engine.ecosystem_catalog().await;

        let Some(eco) = catalog.iter().find(|e| e.id == name) else {
            renderer.error(&format!("Unknown ecosystem: {name}"));
            let known: Vec<_> = catalog.iter().map(|e| e.id).collect();
            renderer.info(&format!("Known ecosystems: {}", known.join(", ")));
            return Ok(());
        };

        if eco.installed {
            renderer.success(&format!("{} is already installed.", eco.label));
            return Ok(());
        }

        let label = eco.label;
        let bootstrap = eco.bootstrap;
        match bootstrap {
            Bootstrap::Packages(names) => {
                renderer.info(&format!("Looking for a package that provides {label}…"));
                match engine.first_available_package(names).await {
                    Some(pkg) => self.install(&[pkg], config, renderer).await,
                    None => {
                        renderer.error(&format!(
                            "Couldn't find a package for {label} in your active sources."
                        ));
                        renderer.info(&format!("Tried: {}", names.join(", ")));
                        Ok(())
                    }
                }
            }
            Bootstrap::Script(cmd) => {
                renderer.info(&format!(
                    "{label} isn't in your distro's repositories — it installs via its own script."
                ));
                renderer.info("JII won't run an installer script for you. To install it, run:");
                renderer.info(&format!("  {cmd}"));
                Ok(())
            }
        }
    }

    /// The first-run wizard (and `jii setup`). Warm, short, jargon-free — written for someone
    /// who just opened a terminal. `first_run` is true when it fires automatically on the very
    /// first bare `jii`; then a decline is honored *and* still marks first-run done so it never
    /// nags again. It only asks and only changes the config it saves — it never touches the
    /// system without consent (the optional `doctor` it offers is read-only today; the
    /// system-helping doctor lands in U6).
    async fn setup(
        &self,
        mut config: Config,
        renderer: &Renderer,
        first_run: bool,
    ) -> crate::error::Result<()> {
        let flags = self.prompt_flags(false);

        if first_run {
            renderer.info("Welcome to JII 👋");
            renderer.info("");
            renderer.info("JII installs Linux software for you: it searches the sources you already");
            renderer.info("have (dnf, Flatpak, …), picks the best one, and tells you why.");
            renderer.info("");
            if !prompt::confirm(renderer, "Spend 30 seconds setting it up?", true, &flags) {
                config.meta.first_run_completed = true;
                if let Err(e) = config.save() {
                    renderer.warn(&format!("Could not save settings: {e}"));
                }
                renderer.info("No problem — try `jii firefox` to install something, or `jii setup` anytime.");
                return Ok(());
            }
        }

        // Step 1 — how much detail (Friendly vs Advanced).
        renderer.info("");
        let mode = match prompt::choose(
            renderer,
            "How much should JII tell you?",
            &[
                "Friendly — short, clear output (recommended)".to_string(),
                "Advanced — full detail, source rationale, diagnostics".to_string(),
            ],
            0,
        ) {
            Some(1) => crate::config::OutputMode::Advanced,
            _ => crate::config::OutputMode::Friendly,
        };
        config.ui.mode = mode;

        // Step 2 — optional system check (read-only here; the wizard never auto-fixes).
        if prompt::confirm(renderer, "Run a quick system check (jii doctor) now?", true, &flags) {
            renderer.info("");
            self.doctor(false, config.clone(), renderer).await?;
        }

        // Persist the choices and mark the wizard done.
        config.meta.first_run_completed = true;
        if let Err(e) = config.save() {
            renderer.warn(&format!("Could not save settings: {e}"));
        }

        renderer.info("");
        renderer.success("Setup complete.");
        Ok(())
    }

    /// Search + rank a name across enabled sources, printing any source failures (a source
    /// that was unavailable/errored). Shared by the read-only `search`/`info` paths; `source`
    /// (a `:source` spec or `--source`) narrows the result to one provider when given.
    async fn ranked_for(
        &self,
        engine: &Engine,
        name: &str,
        source: Option<&String>,
        renderer: &Renderer,
    ) -> Vec<PackageCandidate> {
        let query = Query::name(name);
        let result = engine.search(&query).await;
        self.report_source_failures(&result.failed, renderer);
        let mut ranked = engine.rank(result.candidates);
        if let Some(source) = source {
            ranked.retain(|c| &c.source_id == source);
        }
        ranked
    }

    /// Explain how a package was (or would be) installed (from the registry).
    fn how(&self, package: &str, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
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
        let rows: Vec<Vec<String>> = items
            .iter()
            .map(|record| {
                vec![
                    record.name.clone(),
                    record.source_id.clone(),
                    version_or_unknown(record.version.as_ref()),
                ]
            })
            .collect();
        for line in table_lines(&["NAME", "SOURCE", "VERSION"], &rows) {
            renderer.info(&line);
        }
        Ok(())
    }

    /// Report source availability, latency and health (per-source), then a short set of
    /// **Tier-1 system checks** about JII itself working — is `~/.local/bin` on `PATH` (where
    /// user-space installs land), is a GitHub token set (rate limit). Read-only: each check
    /// reports and advises; nothing is changed (Analyze → Explain, no auto-apply — U6/D6).
    async fn doctor(&self, fix: bool, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        // Capture what the system checks (and any `--fix`) need before `config` moves
        // into the engine.
        let token_env = config.network.github_token_env.clone();
        let config_for_fix = config.clone();
        let engine = Engine::new(config)?;
        let diagnostics = engine.diagnose().await;

        if renderer.is_json() {
            // JSON stays the stable per-source array; the Tier-1 checks are a human-facing
            // addition and don't change the machine schema.
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

        renderer.info("Sources:");
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

        // System checks: probe the host environment (network, common tools, PATH, Flathub).
        let facts = gather_system_facts(&token_env).await;
        let checks = system_checks(&facts);
        renderer.info("");
        renderer.info("System checks:");
        let mut warnings = 0usize;
        for c in &checks {
            if c.ok {
                renderer.success(&c.label);
            } else {
                warnings += 1;
                // A blocker (no network) reads as an error; a papercut stays a warning.
                if c.critical {
                    renderer.error(&c.label);
                } else {
                    renderer.warn(&c.label);
                }
            }
            if let Some(advice) = &c.advice {
                renderer.info(&format!("    → {advice}"));
            }
        }
        renderer.info("");
        if warnings == 0 {
            renderer.success("Everything looks good.");
        } else {
            let plural = if warnings == 1 { "" } else { "s" };
            renderer.info(&format!("{warnings} thing{plural} to look at above."));
        }

        // Curated suggestions (the folded-in recommend catalog) — informational, before any
        // interactive fixing so the fix prompts stay last.
        self.suggestions(renderer);

        if fix {
            self.doctor_fix(&checks, config_for_fix, renderer).await?;
        } else if checks.iter().any(|c| !c.ok && c.fix.is_some()) {
            renderer.info("");
            renderer.info("Run `jii doctor --fix` to fix the fixable ones.");
        }
        Ok(())
    }

    /// Apply the fixable checks (`doctor --fix`). Analyze → Explain → Ask → Apply: package
    /// fixes route through the normal install path (which previews and confirms itself);
    /// the Flathub remote is a plain command shown before it runs (Flatpak elevates via its
    /// own polkit). `--dry-run` previews every fix without asking or changing anything.
    async fn doctor_fix(
        &self,
        checks: &[SystemCheck],
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let fixes: Vec<(&SystemCheck, &Fix)> = checks
            .iter()
            .filter_map(|c| c.fix.as_ref().filter(|_| !c.ok).map(|f| (c, f)))
            .collect();

        renderer.info("");
        if fixes.is_empty() {
            renderer.info("Nothing here can be fixed automatically — the items above need a manual step.");
            return Ok(());
        }

        let flags = self.prompt_flags(config.install.auto);
        for (check, fix) in fixes {
            renderer.info(&format!("→ {}", check.label));
            match fix {
                Fix::Install(pkg) => {
                    // install() previews, confirms, and honors --dry-run itself.
                    self.install(&[pkg.to_string()], config.clone(), renderer).await?;
                }
                Fix::Command { argv, show } => {
                    if self.global.dry_run {
                        renderer.info(&format!("  would run:  {show}"));
                        continue;
                    }
                    renderer.info(&format!("  runs:  {show}"));
                    if !prompt::confirm(renderer, "  Run it?", true, &flags) {
                        continue;
                    }
                    match run_plain_command(argv).await {
                        Ok(()) => renderer.success("  Done."),
                        Err(e) => renderer.error(&format!("  Failed: {e}")),
                    }
                }
            }
        }
        Ok(())
    }

    /// Print curated, distro-aware suggestions at the tail of `doctor` (the old
    /// `jii recommend`, now folded in — a fresh system's "worth adding" list). Compact:
    /// one line per entry — title, why, and the exact way to add it (`jii …` for
    /// installable entries; the documented command for a third-party repo an install
    /// can't express). Purely informational: nothing is changed, the user runs what they
    /// like. Silent when the catalog has nothing for this distro, so it never nags.
    fn suggestions(&self, renderer: &Renderer) {
        let catalog = match crate::recommend::Catalog::load() {
            Ok(c) => c,
            Err(_) => return, // a broken catalog must never break `doctor`
        };
        let distro_id = crate::platform::Platform::detect().distro.id();
        let entries = catalog.for_distro(distro_id);
        if entries.is_empty() {
            return;
        }

        renderer.info("");
        renderer.info("Suggestions for your system:");
        let mut last_category: Option<&str> = None;
        for r in &entries {
            if last_category != Some(r.category.as_str()) {
                renderer.info(&format!("  [{}]", r.category));
                last_category = Some(r.category.as_str());
            }
            let how = if !r.packages.is_empty() {
                format!("jii {}", r.packages.join(" "))
            } else if let Some(manual) = &r.manual {
                format!("run: {manual}")
            } else {
                String::new()
            };
            renderer.info(&format!("    {} — {}  ·  {}", r.title, r.why, how));
            if let Some(note) = &r.note {
                renderer.info(&format!("        note: {note}"));
            }
        }
        renderer.info("Informational — nothing was changed. Add one by running the command shown.");
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
        let rows: Vec<Vec<String>> = events
            .iter()
            .rev()
            .map(|event| {
                vec![
                    event.at.format("%Y-%m-%d %H:%M").to_string(),
                    event.action.label().to_string(),
                    event.name.clone(),
                    event.source_id.clone(),
                ]
            })
            .collect();
        for line in table_lines(&["WHEN", "ACTION", "PACKAGE", "SOURCE"], &rows) {
            renderer.info(&line);
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

        let mut flagged = 0;
        let rows: Vec<Vec<String>> = entries
            .iter()
            .map(|e| {
                let trust = e.trust.map(|t| t.label()).unwrap_or("unknown");
                let status = if e.concerns.is_empty() {
                    "ok".to_string()
                } else {
                    flagged += 1;
                    let reasons: Vec<&str> = e.concerns.iter().map(|c| c.message()).collect();
                    format!("⚠ {}", reasons.join(", "))
                };
                vec![
                    e.name.clone(),
                    e.source_id.clone(),
                    trust.to_string(),
                    e.verification.label().to_string(),
                    status,
                ]
            })
            .collect();
        for line in table_lines(&["NAME", "SOURCE", "TRUST", "VERIFIED", "STATUS"], &rows) {
            renderer.info(&line);
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

/// Render an optional version, or `unknown` when a source doesn't report one.
fn version_or_unknown(version: Option<&crate::model::PkgVersion>) -> String {
    version.map(|v| v.to_string()).unwrap_or_else(|| "unknown".to_string())
}

/// Render an aligned text table: a header row then one line per data row, each
/// column padded to the widest cell in that column (the final column is left
/// unpadded so trailing content never carries stray spaces). Widths are computed
/// from the data so long names don't break alignment. Returns the rendered lines.
fn table_lines(headers: &[&str], rows: &[Vec<String>]) -> Vec<String> {
    let cols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(cols) {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let render = |cells: &[String]| -> String {
        let mut line = String::new();
        for (i, cell) in cells.iter().enumerate().take(cols) {
            if i > 0 {
                line.push_str("  ");
            }
            if i + 1 == cols {
                line.push_str(cell); // last column: no trailing pad
            } else {
                let pad = widths[i].saturating_sub(cell.chars().count());
                line.push_str(cell);
                line.push_str(&" ".repeat(pad));
            }
        }
        line
    };
    let header_cells: Vec<String> = headers.iter().map(|h| h.to_string()).collect();
    let mut out = vec![render(&header_cells)];
    out.extend(rows.iter().map(|r| render(r)));
    out
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

/// Source-agnostic complementary facts about a candidate (signature, version, arch) — no
/// trust/source-specific text. Shared by the recommendation rationale.
fn model_facts(candidate: &PackageCandidate) -> Vec<String> {
    let mut facts = Vec::new();
    if candidate.signed {
        facts.push("signature/checksum verifiable".to_string());
    }
    if let Some(version) = &candidate.version {
        facts.push(format!("version {version}"));
    }
    if !candidate.arch_ok {
        facts.push("⚠ may not match this architecture".to_string());
    }
    facts
}

/// Why the recommended candidate was chosen. The **source-specific** `highlights` (supplied
/// by the owning provider — D5) lead; when a source offers none we fall back to the trust
/// label. The source-agnostic model facts (signature/version/arch) follow. The CLI still
/// never branches on the source id — the source-specific text comes *from the provider*
/// (ADR-0004 holds in the UI). This is the lightweight read-only rationale for `jii info`.
fn recommendation_reasons(candidate: &PackageCandidate, highlights: Vec<String>) -> Vec<String> {
    let mut reasons = if highlights.is_empty() {
        vec![format!("{} source", candidate.trust.label())]
    } else {
        highlights
    };
    reasons.extend(model_facts(candidate));
    reasons
}

/// How `doctor --fix` can remedy a failing check. A check with no `Fix` is manual-only
/// (JII won't edit your shell rc or invent a GitHub token for you).
#[derive(Debug)]
enum Fix {
    /// Install a package through JII's normal path — which previews and confirms itself.
    Install(&'static str),
    /// Run a plain command JII shows first. Used for the Flathub remote, which Flatpak
    /// elevates via its own polkit (like its installs), so JII wraps no sudo/pkexec.
    Command {
        argv: Vec<String>,
        /// Human-readable rendering of `argv`, shown before running.
        show: String,
    },
}

/// One `doctor` system check about the host environment (not a source's live health).
/// `ok` renders a green ✓; otherwise a yellow ⚠ with the optional `advice` beneath it.
/// `critical` marks a check whose failure blocks real work (no internet) versus a mere
/// papercut (an optional token) — it only tunes wording today, not behavior. `fix`, when
/// present, is what `doctor --fix` offers to do about a failing check.
struct SystemCheck {
    ok: bool,
    critical: bool,
    label: String,
    advice: Option<String>,
    fix: Option<Fix>,
}

impl SystemCheck {
    fn pass(label: impl Into<String>) -> Self {
        SystemCheck { ok: true, critical: false, label: label.into(), advice: None, fix: None }
    }
    fn warn(label: impl Into<String>, advice: impl Into<String>) -> Self {
        SystemCheck {
            ok: false,
            critical: false,
            label: label.into(),
            advice: Some(advice.into()),
            fix: None,
        }
    }
    fn critical(mut self) -> Self {
        self.critical = true;
        self
    }
    fn fixable(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// Host facts gathered once (with I/O) in `doctor`, then handed to the pure
/// [`system_checks`] builder so the verdicts and wording stay unit-testable. The core
/// never branches on a source here — these are environment facts (network, common
/// tools, well-known directories), not per-provider logic.
struct SystemFacts {
    /// `~/.local/bin` — where user-space installs (cargo/npm/pipx/go/GitHub) land.
    local_bin: std::path::PathBuf,
    local_bin_on_path: bool,
    /// `~/.cargo/bin` — only worth flagging when cargo is present or the dir exists.
    cargo_bin: std::path::PathBuf,
    cargo_bin_relevant: bool,
    cargo_bin_on_path: bool,
    /// Can we reach the network at all? Almost every non-distro source needs it.
    internet: bool,
    /// Common CLI tools other flows lean on (git for cargo-git deps, curl for scripts).
    git: bool,
    curl: bool,
    /// Whether Flatpak is installed and, if so, whether the Flathub remote is wired up.
    flatpak: bool,
    flathub: bool,
    /// The env var that holds a GitHub token, and whether it is set.
    token_env: String,
    token_set: bool,
}

/// Compute the environment checks from already-gathered [`SystemFacts`]. Pure (no I/O —
/// the caller does the probing) so the wording and pass/fail logic are unit-tested.
fn system_checks(f: &SystemFacts) -> Vec<SystemCheck> {
    let mut checks = Vec::new();

    // Network: the one failure that silently breaks most sources, so it leads.
    checks.push(if f.internet {
        SystemCheck::pass("Internet is reachable")
    } else {
        SystemCheck::warn(
            "No internet connection",
            "Most sources (GitHub, Flatpak, cargo, npm…) need the network. Check your \
             connection or proxy settings.",
        )
        .critical()
    });

    // Common tools JII and its sources lean on — and which JII can itself install.
    checks.push(if f.git {
        SystemCheck::pass("git is installed")
    } else {
        SystemCheck::warn(
            "git is not installed",
            "Some installs (cargo git dependencies, source builds) need it. Add it with:  jii git",
        )
        .fixable(Fix::Install("git"))
    });
    checks.push(if f.curl {
        SystemCheck::pass("curl is installed")
    } else {
        SystemCheck::warn(
            "curl is not installed",
            "Handy for scripts and manual downloads. Add it with:  jii curl",
        )
        .fixable(Fix::Install("curl"))
    });

    // ~/.local/bin on PATH — user-space installs land there.
    let local = f.local_bin.display();
    checks.push(if f.local_bin_on_path {
        SystemCheck::pass(format!("{local} is on your PATH"))
    } else {
        SystemCheck::warn(
            format!("{local} is not on your PATH"),
            "User-space installs (cargo, npm, pipx, go, GitHub binaries) land there, so their \
             commands won't be found. Add it, e.g.:  echo 'export PATH=\"$HOME/.local/bin:$PATH\"' \
             >> ~/.bashrc && exec $SHELL",
        )
    });

    // ~/.cargo/bin on PATH — only when cargo is actually in play.
    if f.cargo_bin_relevant {
        let cargo = f.cargo_bin.display();
        checks.push(if f.cargo_bin_on_path {
            SystemCheck::pass(format!("{cargo} is on your PATH"))
        } else {
            SystemCheck::warn(
                format!("{cargo} is not on your PATH"),
                "Cargo installs binaries there. Add it, e.g.:  echo 'export \
                 PATH=\"$HOME/.cargo/bin:$PATH\"' >> ~/.bashrc && exec $SHELL",
            )
        });
    }

    // Flathub — only meaningful when Flatpak is installed.
    if f.flatpak {
        checks.push(if f.flathub {
            SystemCheck::pass("Flathub remote is configured")
        } else {
            SystemCheck::warn(
                "Flatpak is installed but the Flathub remote is missing",
                "Most Flatpak apps live on Flathub. Add it with:  flatpak remote-add \
                 --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo",
            )
            .fixable(Fix::Command {
                argv: vec![
                    "flatpak".into(),
                    "remote-add".into(),
                    "--if-not-exists".into(),
                    "flathub".into(),
                    "https://flathub.org/repo/flathub.flatpakrepo".into(),
                ],
                show: "flatpak remote-add --if-not-exists flathub \
                       https://flathub.org/repo/flathub.flatpakrepo"
                    .into(),
            })
        });
    }

    // GitHub token — a rate-limit papercut, never a blocker.
    checks.push(if f.token_set {
        SystemCheck::pass(format!("{} is set — GitHub requests aren't rate-limited", f.token_env))
    } else {
        SystemCheck::warn(
            format!("{} is not set — GitHub is limited to ~60 requests/hour", f.token_env),
            format!(
                "Optional: export {}=<a GitHub token> to lift the anonymous rate limit.",
                f.token_env
            ),
        )
    });

    checks
}

/// Probe host facts for `doctor` (the one place these environment I/O calls live). Runs
/// the independent tool/network probes concurrently so `doctor` stays snappy.
async fn gather_system_facts(token_env: &str) -> SystemFacts {
    let base = directories::BaseDirs::new();
    let home = base.as_ref().map(|b| b.home_dir().to_path_buf());
    let local_bin = home
        .as_ref()
        .map(|h| h.join(".local/bin"))
        .unwrap_or_else(|| std::path::PathBuf::from("~/.local/bin"));
    let cargo_bin = home
        .as_ref()
        .map(|h| h.join(".cargo/bin"))
        .unwrap_or_else(|| std::path::PathBuf::from("~/.cargo/bin"));

    let platform = crate::platform::Platform::detect();
    let local_bin_on_path = home
        .as_ref()
        .map(|_| platform.is_on_path(&local_bin))
        .unwrap_or(true); // can't resolve HOME → don't cry wolf
    let cargo_bin_on_path = platform.is_on_path(&cargo_bin);

    // Independent probes run concurrently.
    let (internet, git, curl, cargo, flatpak) = tokio::join!(
        check_internet(),
        crate::provider::which("git"),
        crate::provider::which("curl"),
        crate::provider::which("cargo"),
        crate::provider::which("flatpak"),
    );
    let flathub = if flatpak { flathub_configured().await } else { false };
    let cargo_bin_relevant = cargo || cargo_bin.exists();
    let token_set = std::env::var(token_env).map(|v| !v.is_empty()).unwrap_or(false);

    SystemFacts {
        local_bin,
        local_bin_on_path,
        cargo_bin,
        cargo_bin_relevant,
        cargo_bin_on_path,
        internet,
        git,
        curl,
        flatpak,
        flathub,
        token_env: token_env.to_string(),
        token_set,
    }
}

/// A fast connectivity probe: any HTTP response (even a rate-limit 403) proves the
/// network is up. A short timeout keeps `doctor` from hanging on a dead link.
async fn check_internet() -> bool {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client.head("https://api.github.com").send().await.is_ok()
}

/// Run a plain, non-JII-elevated command for `doctor --fix`, letting it inherit the
/// terminal so a tool's own polkit prompt (Flatpak) is visible. Errors on spawn failure
/// or a non-zero exit so the caller can report it.
async fn run_plain_command(argv: &[String]) -> crate::error::Result<()> {
    let status = tokio::process::Command::new(&argv[0])
        .args(&argv[1..])
        .status()
        .await
        .map_err(|e| crate::error::JiiError::spawn(&argv[0], e))?;
    if status.success() {
        Ok(())
    } else {
        Err(crate::error::JiiError::Other(anyhow::anyhow!(
            "`{}` exited with {status}",
            argv.join(" ")
        )))
    }
}

/// Whether the Flathub remote is registered (system or user). Best-effort: any error
/// reading remotes reports "not configured" rather than a false positive.
async fn flathub_configured() -> bool {
    tokio::process::Command::new("flatpak")
        .args(["remotes", "--columns=name"])
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == "flathub"))
        .unwrap_or(false)
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
    fn table_pads_columns_to_the_widest_cell() {
        let rows = vec![
            vec!["fastfetch".into(), "dnf".into(), "2.21".into()],
            vec!["x".into(), "flatpak".into(), "1.0".into()],
        ];
        let lines = table_lines(&["NAME", "SOURCE", "VERSION"], &rows);
        // Header + one line per row.
        assert_eq!(lines.len(), 3);
        // NAME column is as wide as "fastfetch" (9); each row's SOURCE column starts
        // at the same offset.
        let name_w = "fastfetch".len();
        assert!(lines[1].starts_with("fastfetch  "));
        assert_eq!(lines[2].find("flatpak"), Some(name_w + 2));
        // Last column carries no trailing padding.
        assert!(lines[1].ends_with("2.21"));
        assert!(!lines[0].ends_with(' '));
    }

    #[test]
    fn table_header_widens_when_it_is_the_longest_cell() {
        // A header longer than any datum still sets the column width.
        let rows = vec![vec!["a".into(), "b".into()]];
        let lines = table_lines(&["PACKAGE", "SRC"], &rows);
        assert!(lines[1].starts_with("a      ")); // padded to len("PACKAGE") == 7
        assert_eq!(lines[1].find('b'), Some("PACKAGE".len() + 2));
    }

    #[test]
    fn action_labels_are_human_readable_past_tense() {
        use crate::registry::Action;
        assert_eq!(Action::Install.label(), "installed");
        assert_eq!(Action::Remove.label(), "removed");
        assert_eq!(Action::Update.label(), "updated");
    }

    #[test]
    fn reasons_fall_back_to_trust_without_highlights() {
        // No provider highlights → the trust label leads (fallback), then the model facts.
        let reasons =
            recommendation_reasons(&candidate(TrustLevel::Official, true, Some("1.2")), vec![]);
        assert_eq!(reasons[0], "official source");
        assert!(reasons.iter().any(|s| s.contains("verifiable")));
        assert!(reasons.iter().any(|s| s == "version 1.2"));
    }

    #[test]
    fn highlights_lead_and_replace_the_trust_line() {
        // Source-specific highlights (D5) lead; the generic trust line is dropped; model
        // facts still follow.
        let reasons = recommendation_reasons(
            &candidate(TrustLevel::Official, true, Some("1.2")),
            vec!["Official Fedora package".to_string()],
        );
        assert_eq!(reasons[0], "Official Fedora package");
        assert!(!reasons.iter().any(|s| s == "official source"));
        assert!(reasons.iter().any(|s| s == "version 1.2"));
    }

    #[test]
    fn unsigned_candidate_omits_signature_reason() {
        let reasons = recommendation_reasons(&candidate(TrustLevel::Untrusted, false, None), vec![]);
        assert_eq!(reasons, vec!["untrusted source".to_string()]);
    }

    #[test]
    fn arch_mismatch_is_flagged() {
        let mut c = candidate(TrustLevel::Community, false, None);
        c.arch_ok = false;
        assert!(
            recommendation_reasons(&c, vec![])
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

    fn facts_all_good() -> SystemFacts {
        SystemFacts {
            local_bin: std::path::PathBuf::from("/home/x/.local/bin"),
            local_bin_on_path: true,
            cargo_bin: std::path::PathBuf::from("/home/x/.cargo/bin"),
            cargo_bin_relevant: true,
            cargo_bin_on_path: true,
            internet: true,
            git: true,
            curl: true,
            flatpak: true,
            flathub: true,
            token_env: "GITHUB_TOKEN".to_string(),
            token_set: true,
        }
    }

    #[test]
    fn system_checks_all_pass_when_environment_is_healthy() {
        let checks = system_checks(&facts_all_good());
        assert!(checks.iter().all(|c| c.ok));
        assert!(checks.iter().all(|c| c.advice.is_none()));
    }

    #[test]
    fn system_checks_flag_missing_path_with_advice() {
        let mut f = facts_all_good();
        f.local_bin_on_path = false;
        let checks = system_checks(&f);
        let path_check = checks.iter().find(|c| c.label.contains(".local/bin")).unwrap();
        assert!(!path_check.ok);
        assert!(path_check.advice.as_deref().unwrap().contains("PATH"));
    }

    #[test]
    fn system_checks_flag_missing_token_with_its_env_name() {
        let mut f = facts_all_good();
        f.token_env = "GH_PAT".to_string();
        f.token_set = false;
        let checks = system_checks(&f);
        let token_check = checks.iter().find(|c| c.label.contains("GH_PAT")).unwrap();
        assert!(!token_check.ok);
        assert!(token_check.advice.as_deref().unwrap().contains("GH_PAT"));
    }

    #[test]
    fn no_internet_is_critical() {
        let mut f = facts_all_good();
        f.internet = false;
        let checks = system_checks(&f);
        let net = checks.iter().find(|c| c.label.contains("internet") || c.label.contains("Internet")).unwrap();
        assert!(!net.ok);
        assert!(net.critical);
    }

    #[test]
    fn cargo_bin_check_is_skipped_when_irrelevant() {
        let mut f = facts_all_good();
        f.cargo_bin_relevant = false;
        let checks = system_checks(&f);
        assert!(!checks.iter().any(|c| c.label.contains(".cargo/bin")));
    }

    #[test]
    fn flathub_check_is_skipped_without_flatpak() {
        let mut f = facts_all_good();
        f.flatpak = false;
        let checks = system_checks(&f);
        assert!(!checks.iter().any(|c| c.label.contains("Flathub")));
    }

    #[test]
    fn missing_git_and_curl_are_flagged_with_jii_install_advice() {
        let mut f = facts_all_good();
        f.git = false;
        f.curl = false;
        let checks = system_checks(&f);
        let git = checks.iter().find(|c| c.label.starts_with("git")).unwrap();
        assert!(!git.ok);
        assert!(git.advice.as_deref().unwrap().contains("jii git"));
        let curl = checks.iter().find(|c| c.label.starts_with("curl")).unwrap();
        assert!(curl.advice.as_deref().unwrap().contains("jii curl"));
    }

    #[test]
    fn missing_git_and_curl_carry_an_install_fix() {
        let mut f = facts_all_good();
        f.git = false;
        f.curl = false;
        let checks = system_checks(&f);
        let git = checks.iter().find(|c| c.label.starts_with("git")).unwrap();
        assert!(matches!(git.fix, Some(Fix::Install("git"))));
        let curl = checks.iter().find(|c| c.label.starts_with("curl")).unwrap();
        assert!(matches!(curl.fix, Some(Fix::Install("curl"))));
    }

    #[test]
    fn missing_flathub_carries_a_command_fix_with_the_repo_url() {
        let mut f = facts_all_good();
        f.flathub = false;
        let checks = system_checks(&f);
        let flathub = checks.iter().find(|c| c.label.contains("Flathub")).unwrap();
        match &flathub.fix {
            Some(Fix::Command { argv, show }) => {
                assert_eq!(argv[0], "flatpak");
                assert!(argv.iter().any(|a| a.contains("flathub.flatpakrepo")));
                assert!(show.contains("remote-add"));
            }
            other => panic!("expected a Command fix, got {other:?}"),
        }
    }

    #[test]
    fn passing_checks_and_manual_ones_carry_no_fix() {
        // A healthy env: no fixes anywhere.
        assert!(system_checks(&facts_all_good()).iter().all(|c| c.fix.is_none()));
        // The PATH papercut is real but manual-only (JII won't edit your shell rc).
        let mut f = facts_all_good();
        f.local_bin_on_path = false;
        let checks = system_checks(&f);
        let path = checks.iter().find(|c| c.label.contains(".local/bin")).unwrap();
        assert!(!path.ok && path.fix.is_none());
    }
}
