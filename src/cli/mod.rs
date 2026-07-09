//! Command-line surface: clap definitions, global flags, and dispatch.
//!
//! The command set is intentionally the full, stable surface from
//! `docs/ARCHITECTURE.md` §13. Commands not yet implemented return a clear
//! "not yet" message that names the phase, so the CLI shape never churns.

use clap::{Parser, Subcommand};

use crate::config::{ColorChoice, Config, Profile};
use crate::engine::Engine;
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, PackageSpec, Query};
use crate::provider::Bootstrap;
use crate::selfupdate;
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

    /// UI language, e.g. `en` or `ru`. Overrides the config `[ui] locale` and
    /// `$LC_MESSAGES`/`$LANG`.
    #[arg(long, value_name = "LANG", global = true)]
    pub lang: Option<String>,
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
    /// Diagnose sources + host, then interactively offer to set up what's missing
    /// (git/curl, PATH, Flathub, RPM Fusion, codecs, fonts…). Each item is a yes/no
    /// question, applied on "yes"; nothing changes without your answer.
    Doctor {
        /// Deprecated: setup is now interactive by default, so this flag is a no-op
        /// (kept so existing `jii doctor --fix` invocations still work).
        #[arg(long, hide = true)]
        fix: bool,
    },
    /// List software installed via JII. Add `--audit` for the security view (source,
    /// trust, verification, concerns).
    List {
        /// Show a security audit instead of the plain list: source, trust, artifact
        /// verification, and any concerns per install.
        #[arg(long)]
        audit: bool,
    },
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
    /// Remove JII itself (same as `jii remove jii`).
    Uninstall,
    /// Print a shell completion script for the given shell (bash, zsh, fish, …).
    #[command(hide = true)]
    Completions {
        /// Target shell.
        shell: clap_complete::Shell,
    },
    /// Print the roff man page to stdout (bundled as `jii.1` in the packages).
    #[command(hide = true)]
    Man,
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

    /// Build a renderer for the given config (mode: `-v` forces Advanced; else configured).
    fn renderer_for(&self, config: &Config) -> Renderer {
        let mode = if self.global.verbose > 0 {
            crate::config::OutputMode::Advanced
        } else {
            config.ui.mode
        };
        Renderer::new(self.color_choice(config), self.global.json, mode)
    }

    /// A short human echo of the invocation (`jii fastfetch`, `jii search foo`), used to tell
    /// the user which command will run after first-run onboarding. `None` for commands that
    /// shouldn't be preceded by the wizard: `setup` (it *is* the wizard), `doctor` (it runs a
    /// setup of its own — onboarding would double it), `uninstall`, the hidden plumbing
    /// (`completions`/`man`), and bare `jii` (its own welcome arm handles first-run).
    fn onboarding_task_summary(&self) -> Option<String> {
        match &self.command {
            Some(Commands::Setup)
            | Some(Commands::Doctor { .. })
            | Some(Commands::Uninstall)
            | Some(Commands::Completions { .. })
            | Some(Commands::Man) => None,
            Some(Commands::Install { packages }) => Some(format!("jii {}", packages.join(" "))),
            Some(Commands::Remove { packages }) => {
                Some(format!("jii remove {}", packages.join(" ")))
            }
            Some(Commands::Update { packages }) if packages.is_empty() => {
                Some("jii update".to_string())
            }
            Some(Commands::Update { packages }) => {
                Some(format!("jii update {}", packages.join(" ")))
            }
            Some(Commands::Search { query }) => Some(format!("jii search {}", query.join(" "))),
            Some(Commands::Info { package }) => Some(format!("jii info {package}")),
            Some(Commands::How { package }) => Some(format!("jii how {package}")),
            Some(Commands::List { audit }) => {
                Some(if *audit { "jii list --audit".to_string() } else { "jii list".to_string() })
            }
            Some(Commands::History) => Some("jii history".to_string()),
            Some(Commands::Sources) => Some("jii sources".to_string()),
            Some(Commands::Providers { .. }) => Some("jii providers".to_string()),
            None => (!self.packages.is_empty()).then(|| format!("jii {}", self.packages.join(" "))),
        }
    }

    /// Dispatch the parsed command.
    pub async fn run(self, config: Config) -> crate::error::Result<()> {
        let renderer = self.renderer_for(&config);

        // First-run onboarding for *any* task (not just bare `jii`): the very first time JII is
        // used on an interactive terminal, run the setup wizard first, then continue with the
        // original invocation. The user is told up-front which command will run after the
        // (optional) setup. Excluded commands (setup/doctor/uninstall/plumbing, bare `jii`)
        // return `None` from `onboarding_task_summary` and fall straight through.
        let config = if config.is_first_run() && self.interactive(&renderer) {
            if let Some(summary) = self.onboarding_task_summary() {
                renderer.info(&crate::t!("setup.first_use"));
                renderer.info(&crate::t!("setup.will_run_after", cmd = summary.clone()));
                renderer.info("");
                self.setup(config.clone(), &renderer, true).await?;
                renderer.info("");
                renderer.info(&crate::t!("setup.now_running", cmd = summary));
                renderer.info("");
                // Reload so the dispatched command sees the wizard's saved choices.
                Config::load().unwrap_or(config)
            } else {
                config
            }
        } else {
            config
        };
        // Rebuild the renderer in case the wizard changed the output mode.
        let renderer = self.renderer_for(&config);

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
                        renderer.info(&crate::t!("common.usage_hint"));
                        Ok(())
                    }
                } else {
                    self.install(&self.packages, config, &renderer).await
                }
            }

            // Implemented in Phase 2.
            Some(Commands::Remove { packages }) => self.remove(packages, config, &renderer).await,
            Some(Commands::How { package }) => self.how(package, config, &renderer),
            Some(Commands::List { audit }) => self.list(*audit, config, &renderer),
            Some(Commands::History) => self.history(config, &renderer),

            Some(Commands::Doctor { fix: _ }) => self.doctor(config, &renderer).await,

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
            Some(Commands::Uninstall) => self.self_uninstall(config, &renderer).await,
            Some(Commands::Completions { shell }) => {
                let mut cmd = <Cli as clap::CommandFactory>::command();
                clap_complete::generate(*shell, &mut cmd, "jii", &mut std::io::stdout());
                Ok(())
            }
            Some(Commands::Man) => {
                let cmd = <Cli as clap::CommandFactory>::command();
                clap_mangen::Man::new(cmd)
                    .render(&mut std::io::stdout())
                    .map_err(|e| crate::error::JiiError::Other(anyhow::anyhow!("man: {e}")))
            }
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
        self.install_inner(packages, config, renderer, false, true).await
    }

    /// The install flow. `assume_yes` lets a caller that already obtained consent (the
    /// `doctor` questionnaire) skip the redundant final confirmation — the trust barrier
    /// still gates untrusted sources (ADR-0006), so this only auto-confirms trusted-enough
    /// candidates. `route_managers` enables the bare-manager-name → bootstrap routing (#4);
    /// it is **off** when installing a bootstrap package (whose name, e.g. `pipx`, may itself
    /// be a manager id — routing it would loop) and for doctor's explicit package installs.
    async fn install_inner(
        &self,
        packages: &[String],
        config: Config,
        renderer: &Renderer,
        assume_yes: bool,
        route_managers: bool,
    ) -> crate::error::Result<()> {
        let mut engine = Engine::new(self.apply_profile(config.clone()))?;

        // #4: a bare ecosystem-manager name (npm, cargo, pipx, flatpak, snap, go, brew, nix)
        // means "install that manager", not "find a package called npm" — route it to
        // bootstrap. A pinned source (`npm:dnf` or `--source`) opts out. Runs before the
        // usable-source gate so a fresh box can still bootstrap its very first manager.
        let rest = if route_managers {
            self.route_managers(&engine, packages, config, renderer).await?
        } else {
            packages.to_vec()
        };
        if rest.is_empty() {
            return Ok(());
        }
        let packages: &[String] = &rest;

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
            renderer.info(&crate::t!("install.searching"));
        }
        for spec in &specs {
            // A per-package `:source` (ADR-0031) pins the provider and, like `--source`,
            // suppresses the chooser; it takes precedence over the whole-command `--source`.
            let pkg_source = spec.source.as_ref().or(self.global.source.as_ref());
            let name = &spec.name;
            let query = Query::name(name);
            if !renderer.is_friendly() {
                renderer.info(&crate::t!("install.searching_for", name = query.raw));
            }
            let result = engine.search(&query).await;
            self.report_source_failures(&result.failed, renderer);
            let mut ranked = engine.rank(name, result.candidates);
            // No exact match? Broaden the search (ADR-0042): `ayugram` → `ayugram-desktop`,
            // and a trailing typo like `ayugramm` still reaches it. The recommend + confirm
            // below is the "did you mean" — the resolved name is shown and can be declined.
            if ranked.is_empty() {
                ranked = engine.broaden_search(name).await;
            }
            if let Some(source) = pkg_source {
                ranked.retain(|c| &c.source_id == source);
            }
            if ranked.is_empty() {
                not_found.push(name.clone());
                continue;
            }
            // Be explicit when the best match isn't what was typed, so a broadened result
            // never silently installs a differently-named package.
            if !ranked[0].name.eq_ignore_ascii_case(name) {
                renderer.info(&crate::t!(
                    "install.no_exact_match",
                    name = name,
                    closest = ranked[0].name
                ));
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
                    renderer.success(&crate::t!(
                        "install.already_installed",
                        name = name,
                        source = record.source_id,
                        version = v
                    ));
                    continue;
                }
                // Same source, a newer version is available → offer an in-place update
                // (which is exactly what re-installing via this source does). A real batch
                // includes it without prompting; a single install asks once.
                renderer.info(&crate::t!(
                    "install.already_installed_outdated",
                    name = name,
                    source = record.source_id,
                    current = version_or_unknown(record.version.as_ref()),
                    available = version_or_unknown(available.as_ref())
                ));
                if single && !self.global.dry_run {
                    let flags = self.prompt_flags(engine.config().install.auto).with_yes(assume_yes);
                    if !prompt::confirm(renderer, &crate::t!("install.update_now"), true, &flags) {
                        renderer.info(&crate::t!("install.keeping"));
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
                let palette = renderer.palette();
                let labels: Vec<String> = ranked
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        if i == 0 {
                            format!(
                                "{}  ⭐ {}",
                                candidate_line(c, palette),
                                crate::t!("install.recommended_tag")
                            )
                        } else {
                            candidate_line(c, palette)
                        }
                    })
                    .collect();
                let header = crate::t!("install.choose_header", name = name);
                match prompt::choose(renderer, &header, &labels, 0) {
                    Some(index) => {
                        chose_interactively = true;
                        ranked.remove(index)
                    }
                    None => {
                        renderer.info(&crate::t!("common.aborted"));
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
            let names = not_found.join(", ");
            let msg = match &self.global.source {
                Some(source) => crate::t!("install.not_found_via", source = source, names = names),
                None => crate::t!("install.not_found", names = names),
            };
            renderer.error(&msg);
            // #9: a name that "isn't found" is often a *library* (npm/cargo ship no CLI, so
            // they offer no candidate). Explain that instead of leaving the user puzzled.
            for name in &not_found {
                if let Some(msg) = engine.explain_miss(name).await {
                    renderer.info(&format!("  → {msg}"));
                }
            }
        }
        if chosen.is_empty() {
            return Ok(());
        }
        if !not_found.is_empty() {
            let flags = self.prompt_flags(engine.config().install.auto).with_yes(assume_yes);
            if !prompt::confirm(renderer, &crate::t!("install.continue_rest"), true, &flags) {
                renderer.info(&crate::t!("common.aborted"));
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
            renderer.info(&crate::t!("common.dry_run_not_installed"));
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
        let flags = self.prompt_flags(engine.config().install.auto).with_yes(assume_yes);
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
            renderer.info(&crate::t!("common.aborted"));
            return Ok(());
        }

        // 6. One escalation, one run; records are written as each plan succeeds.
        engine.install_batch(&batch, renderer).await?;
        renderer.success(&crate::t!("install.installed", names = installed.join(", ")));
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
            let palette = renderer.palette();
            renderer.heading(&crate::t!("install.summary"));
            for bp in batch {
                renderer.info(&format!("{}:", palette.source(&bp.plan.source_id)));
                for candidate in &bp.candidates {
                    let version = candidate
                        .version
                        .as_ref()
                        .map(|v| format!(" {}", palette.version(&format!("(v{v})"))))
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
        let palette = renderer.palette();
        for bp in batch {
            let sudo = if bp.plan.needs_root() {
                palette.dim(&crate::t!("common.needs_sudo"))
            } else {
                String::new()
            };
            for candidate in &bp.candidates {
                let version = candidate
                    .version
                    .as_ref()
                    .map(|v| format!(" {}", palette.version(&format!("({v})"))))
                    .unwrap_or_default();
                let why = engine
                    .candidate_highlights(candidate)
                    .into_iter()
                    .next()
                    .map(|h| format!(" — {h}"))
                    .unwrap_or_default();
                renderer.info(&crate::t!(
                    "install.preview",
                    name = candidate.name.clone(),
                    version = version,
                    source = palette.source(&candidate.source_id),
                    why = why,
                    sudo = sudo
                ));
            }
        }
    }

    /// Print the non-recommended candidates as a compact "also available" list.
    fn show_alternatives(&self, alternatives: &[crate::model::PackageCandidate], renderer: &Renderer) {
        if alternatives.is_empty() || renderer.is_json() {
            return;
        }
        // A broadened search (ADR-0042) can surface many near-name matches; cap the list so
        // it stays a hint, not a wall. The name is shown because alternatives may now differ
        // from the recommended one (e.g. `git` → also `gitk`, `git-lfs`).
        const MAX: usize = 6;
        let palette = renderer.palette();
        renderer.heading(&crate::t!("install.also_available"));
        for candidate in alternatives.iter().take(MAX) {
            let version = candidate
                .version
                .as_ref()
                .map(|v| format!("{}, ", palette.version(&format!("v{v}"))))
                .unwrap_or_default();
            renderer.info(&format!(
                "  {} — {} ({}{})",
                candidate.name,
                palette.source(&candidate.source_id),
                version,
                palette.trust(candidate.trust)
            ));
        }
        let extra = alternatives.len().saturating_sub(MAX);
        if extra > 0 {
            renderer.info(&palette.dim(&format!("  {}", crate::t!("install.and_more", count = extra))));
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
                    renderer.error(&crate::t!("parse.invalid", raw = raw.clone(), reason = reason));
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
                renderer.error(&crate::t!(
                    "parse.unknown_source",
                    source = source.clone(),
                    name = spec.name.clone(),
                    known = crate::config::KNOWN_SOURCES.join(", ")
                ));
                return None;
            }
        }
        if let Some(spec) = specs.iter().find(|s| s.reference.is_some()) {
            let r = spec.reference.as_deref().unwrap_or("");
            renderer.error(&crate::t!(
                "parse.pin_unsupported",
                pin = r,
                name = spec.name.clone()
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
            renderer.error(&crate::t!("common.no_sources_enabled"));
            return false;
        }
        if !engine.any_source_available().await {
            renderer.error(&crate::t!("common.no_usable_source"));
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
        // `jii remove jii` removes JII itself (see also `jii uninstall`). Handle it, then
        // remove any remaining names normally.
        let wants_self = packages.iter().any(|p| p == selfupdate::SELF_NAME);
        if wants_self {
            self.self_uninstall(config.clone(), renderer).await?;
        }
        let rest: Vec<String> = packages
            .iter()
            .filter(|p| *p != selfupdate::SELF_NAME)
            .cloned()
            .collect();
        if wants_self && rest.is_empty() {
            return Ok(());
        }
        let packages = &rest;

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
                            renderer.info(&crate::t!("common.aborted"));
                            return Ok(());
                        }
                    }
                }
            }
        }
        if !not_installed.is_empty() {
            renderer.error(&crate::t!("common.not_installed", names = not_installed.join(", ")));
        }
        if records.is_empty() {
            return Ok(());
        }
        if !not_installed.is_empty() {
            let flags = self.prompt_flags(false);
            if !prompt::confirm(renderer, &crate::t!("remove.continue_rest"), true, &flags) {
                renderer.info(&crate::t!("common.aborted"));
                return Ok(());
            }
        }

        // 2. Group + merge into batched plans.
        let batch = engine
            .plan_record_batch(records, crate::engine::RecordOp::Remove)
            .await?;
        for (name, reason) in &batch.unplannable {
            renderer.warn(&crate::t!("remove.cannot_plan", name = name, reason = reason));
        }
        if batch.plans.is_empty() {
            renderer.info(&crate::t!("remove.nothing"));
            return Ok(());
        }

        // 3. Preview, dry-run guard, one confirmation (default no — removal is destructive).
        self.preview_record_batch(&batch.plans, renderer);
        if self.global.dry_run {
            renderer.info(&crate::t!("common.dry_run_not_removed"));
            return Ok(());
        }
        let names = record_batch_names(&batch.plans);
        let flags = self.prompt_flags(false);
        let question = if names.len() == 1 {
            crate::t!("remove.prompt_one", name = names[0])
        } else {
            crate::t!("remove.prompt_many", count = names.len())
        };
        if !prompt::confirm(renderer, &question, false, &flags) {
            renderer.info(&crate::t!("common.aborted"));
            return Ok(());
        }

        // 4. One escalation, one run; records cleared as each plan succeeds.
        engine.remove_batch(&batch.plans, renderer).await?;
        renderer.success(&crate::t!("remove.removed", names = names.join(", ")));
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
        // `jii update jii` (or `jii` among the names) updates JII **itself** — a self-managed
        // action, not a registry package. Handle it, then update any remaining names.
        let wants_self = packages.iter().any(|p| p == selfupdate::SELF_NAME);
        if wants_self {
            self.self_update(config.clone(), renderer).await?;
        }
        let rest: Vec<String> = packages
            .iter()
            .filter(|p| *p != selfupdate::SELF_NAME)
            .cloned()
            .collect();
        if wants_self && rest.is_empty() {
            return Ok(());
        }
        let packages = &rest;

        // Bare `jii update` updates **everything**: the whole system (every manager's bulk
        // upgrade, D10) and then JII itself. Self-update runs last so the atomic binary swap
        // happens after the system upgrade completes.
        if packages.is_empty() {
            self.update_system(config.clone(), renderer).await?;
            self.self_update(config, renderer).await?;
            return Ok(());
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
                renderer.error(&crate::t!("common.not_installed", names = not_installed.join(", ")));
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
                renderer.success(&crate::t!("update.all_up_to_date", count = up_to_date));
            } else {
                renderer.info(&crate::t!("update.none"));
            }
            return Ok(());
        }

        // Group + merge into batched update plans (skipping any source can't plan).
        let batch = engine
            .plan_record_batch(refreshed, crate::engine::RecordOp::Update)
            .await?;
        for (name, reason) in &batch.unplannable {
            renderer.warn(&crate::t!("update.cannot_plan", name = name, reason = reason));
        }
        if batch.plans.is_empty() {
            renderer.info(&crate::t!("update.none"));
            return Ok(());
        }

        // Preview: version transitions, then each plan.
        for line in &transitions {
            renderer.info(line);
        }
        self.preview_record_batch(&batch.plans, renderer);
        if up_to_date > 0 {
            renderer.info(&crate::t!("update.some_up_to_date", count = up_to_date));
        }

        if self.global.dry_run {
            renderer.info(&crate::t!("common.dry_run_not_updated"));
            return Ok(());
        }

        // One confirmation, one escalation, one run; records refreshed as each succeeds.
        let names = record_batch_names(&batch.plans);
        let flags = self.prompt_flags(engine.config().install.auto);
        let question = if names.len() == 1 {
            crate::t!("update.prompt_one", name = names[0])
        } else {
            crate::t!("update.prompt_many", count = names.len())
        };
        if !prompt::confirm(renderer, &question, true, &flags) {
            renderer.info(&crate::t!("common.aborted"));
            return Ok(());
        }

        engine.update_batch(&batch.plans, renderer).await?;
        renderer.success(&crate::t!("update.updated", names = names.join(", ")));
        Ok(())
    }

    /// Update the whole system (bare `jii update`, D10): aggregate every manager's bulk
    /// "update everything I own" plan (`dnf upgrade`, `flatpak update`, …), and — so nothing
    /// JII installed is missed — fall back to per-record updates for the sources that have no
    /// bulk path (github, cargo, go). One preview, one confirmation, one privilege escalation,
    /// one run. The bulk plans upgrade the system beyond JII's registry, so they aren't
    /// recorded; only the per-record fallbacks refresh the registry.
    /// `jii update jii` — update JII itself from the newest GitHub release, the right way
    /// for how it was installed (user-space binary swap, or a `.rpm`/`.deb` via dnf/apt).
    /// Everything is a previewable plan; `--dry-run` shows it and stops.
    async fn self_update(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let install = selfupdate::detect_install().await?;
        renderer.info(&crate::t!("selfupdate.checking"));
        let latest = match selfupdate::latest_release().await {
            Ok(l) => l,
            Err(e) => {
                renderer.error(&crate::t!("selfupdate.check_failed", error = e));
                return Ok(());
            }
        };
        if !selfupdate::update_available(&latest.tag) {
            renderer.success(&crate::t!(
                "selfupdate.up_to_date",
                version = selfupdate::current_version()
            ));
            return Ok(());
        }
        let plan = selfupdate::plan_update(&install, &latest).await?;
        renderer.info(&crate::t!(
            "selfupdate.available",
            current = selfupdate::current_version(),
            latest = selfupdate::normalize_tag(&latest.tag)
        ));
        self.preview_self_plan(&plan, renderer);
        if self.global.dry_run {
            renderer.info(&crate::t!("common.dry_run_unchanged"));
            return Ok(());
        }
        let flags = self.prompt_flags(engine.config().install.auto);
        if !prompt::confirm(renderer, &crate::t!("selfupdate.update_now"), true, &flags) {
            renderer.info(&crate::t!("common.aborted"));
            return Ok(());
        }
        engine.run_self_plan(&plan, renderer).await?;
        renderer.success(&crate::t!(
            "selfupdate.updated",
            version = selfupdate::normalize_tag(&latest.tag)
        ));
        Ok(())
    }

    /// `jii uninstall` / `jii remove jii` — remove JII itself: delete the user-space binary,
    /// or uninstall the package via dnf/apt. Previewable; defaults the prompt to "no".
    async fn self_uninstall(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let install = selfupdate::detect_install().await?;
        let plan = selfupdate::plan_uninstall(&install);
        renderer.info(&crate::t!("selfupdate.removes"));
        self.preview_self_plan(&plan, renderer);
        if self.global.dry_run {
            renderer.info(&crate::t!("common.dry_run_unchanged"));
            return Ok(());
        }
        let flags = self.prompt_flags(engine.config().install.auto);
        if !prompt::confirm(renderer, &crate::t!("selfupdate.remove_prompt"), false, &flags) {
            renderer.info(&crate::t!("common.aborted"));
            return Ok(());
        }
        engine.run_self_plan(&plan, renderer).await?;
        renderer.success(&crate::t!("selfupdate.removed"));
        Ok(())
    }

    /// Preview a self-management plan: the human reasons, the actions, and a root note.
    fn preview_self_plan(&self, plan: &InstallPlan, renderer: &Renderer) {
        for reason in &plan.reasons {
            renderer.info(&format!("  {reason}"));
        }
        renderer.info(&format!("  {}", crate::t!("common.steps")));
        for action in &plan.actions {
            renderer.info(&format!("    {}", crate::ui::describe_action(action)));
        }
        if plan.needs_root() {
            renderer.info(&format!("  {}", crate::t!("plan.priv_root_shown")));
        }
    }

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
            renderer.warn(&crate::t!("update.cannot_plan", name = name.clone(), reason = reason.clone()));
        }

        if system.plans.is_empty() && fallback.plans.is_empty() {
            renderer.info(&crate::t!("update.nothing_to_update"));
            return Ok(());
        }

        // Preview: the bulk managers, then any per-record fallbacks + version transitions.
        if !system.plans.is_empty() {
            renderer.info(&crate::t!("update.sys_via", sources = system.sources.join(", ")));
        }
        if renderer.is_friendly() && !self.global.dry_run {
            for plan in &system.plans {
                let why = plan.reasons.first().cloned().unwrap_or_default();
                let sudo = if plan.needs_root() { crate::t!("common.needs_sudo") } else { String::new() };
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
            renderer.info(&crate::t!("update.tracked_up_to_date", count = up_to_date));
        }

        if self.global.dry_run {
            renderer.info(&crate::t!("common.dry_run_not_updated"));
            return Ok(());
        }

        let flags = self.prompt_flags(engine.config().install.auto);
        if !prompt::confirm(renderer, &crate::t!("update.prompt_system"), true, &flags) {
            renderer.info(&crate::t!("common.aborted"));
            return Ok(());
        }

        engine
            .run_system_update(&system.plans, &fallback.plans, renderer)
            .await?;
        renderer.success(&crate::t!("update.complete"));
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
        let mut ranked = engine.rank(&record.name, engine.search(&query).await.candidates);
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
            renderer.error(&crate::t!("search.none", name = name));
            if let Some(msg) = engine.explain_miss(&name).await {
                renderer.info(&format!("  → {msg}"));
            }
            return Ok(());
        }
        if renderer.is_json() {
            renderer.json_value(&serde_json::json!(ranked));
            return Ok(());
        }
        let palette = renderer.palette();
        renderer.heading(&crate::t!("search.header", name = name));
        for (i, candidate) in ranked.iter().enumerate() {
            let mark = if i == 0 { palette.good("→") } else { " ".to_string() };
            renderer.info(&format!("{mark} {}", candidate_line(candidate, palette)));
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
            // Nothing installable — but `info` *shows*, it doesn't install (#6). Look for an
            // informational card by name (ADR-0045): an npm/cargo library, for instance, is
            // real and describable even though JII wouldn't install it as a program.
            if let Some(card) = engine.reference(name).await {
                return self.render_reference(&card, renderer);
            }
            renderer.error(&crate::t!("info.none_found", name = name));
            return Ok(());
        }
        if renderer.is_json() {
            let info = engine.candidate_info(&ranked[0]).await;
            renderer.json_value(&serde_json::json!({
                "candidates": ranked,
                "recommended": ranked[0].source_id,
                "info": info,
            }));
            return Ok(());
        }

        // The app card (#4): name → description → a metadata block of whatever the source
        // cheaply knows (license, links, author), then the source list + recommendation.
        // `describe` degrades gracefully — a source with no card leaves the block sparse.
        let best = &ranked[0];
        let info = engine.candidate_info(best).await.unwrap_or_default();

        let palette = renderer.palette();
        renderer.heading(name);
        if let Some(desc) = info.description.as_ref().or(best.summary.as_ref()) {
            renderer.info(desc);
        }
        renderer.info("");
        let row = |label: &str, value: &str| {
            format!("  {}{value}", palette.dim(&format!("{label:<11}")))
        };
        renderer.info(&row(
            &crate::t!("info.row_source"),
            &format!("{} ({})", palette.source(&best.source_id), palette.trust(best.trust)),
        ));
        if let Some(v) = &best.version {
            renderer.info(&row(&crate::t!("info.row_version"), &v.to_string()));
        }
        if let Some(l) = &info.license {
            renderer.info(&row(&crate::t!("info.row_license"), l));
        }
        if let Some(h) = &info.homepage {
            renderer.info(&row(&crate::t!("info.row_homepage"), h));
        }
        if let Some(r) = &info.repository {
            renderer.info(&row(&crate::t!("info.row_repository"), r));
        }
        if let Some(a) = &info.author {
            renderer.info(&row(&crate::t!("info.row_author"), a));
        }
        renderer.info("");

        renderer.heading(&crate::t!("info.available_from", count = ranked.len()));
        for candidate in &ranked {
            renderer.info(&format!("  {}", candidate_line(candidate, palette)));
        }
        renderer.info(&crate::t!(
            "info.recommended",
            source = palette.source(&best.source_id)
        ));
        let highlights = engine.candidate_highlights(best);
        let check = palette.good("✓");
        for reason in recommendation_reasons(best, highlights) {
            renderer.info(&format!("  {check} {reason}"));
        }
        Ok(())
    }

    /// Render an informational `Reference` card (ADR-0045): `jii info` for a name that isn't
    /// an installable program (e.g. an npm library). Shows what it is — description, links,
    /// and a clarifying note — with **no install phrasing**, keeping `info` purely a "show".
    fn render_reference(
        &self,
        card: &crate::model::Reference,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        if renderer.is_json() {
            renderer.json_value(&serde_json::json!(card));
            return Ok(());
        }
        let palette = renderer.palette();
        renderer.heading(&card.name);
        if let Some(desc) = &card.info.description {
            renderer.info(desc);
        }
        renderer.info("");
        let row = |label: &str, value: &str| {
            format!("  {}{value}", palette.dim(&format!("{label:<11}")))
        };
        renderer.info(&row(&crate::t!("info.row_source"), &palette.source(&card.source_id)));
        if let Some(v) = &card.version {
            renderer.info(&row(&crate::t!("info.row_version"), &v.to_string()));
        }
        if let Some(h) = &card.info.homepage {
            renderer.info(&row(&crate::t!("info.row_homepage"), h));
        }
        if let Some(r) = &card.info.repository {
            renderer.info(&row(&crate::t!("info.row_repository"), r));
        }
        if let Some(note) = &card.note {
            renderer.info("");
            renderer.info(&format!("ℹ {note}"));
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

        let palette = renderer.palette();
        let (active, inactive): (Vec<_>, Vec<_>) = catalog.iter().partition(|e| e.available);
        if !active.is_empty() {
            renderer.heading(&crate::t!("sources.active"));
            for e in &active {
                let mark = palette.good("✓");
                renderer.info(&format!(
                    "  {mark} {} ({})",
                    palette.source(&format!("{:8}", e.id)),
                    palette.trust(e.trust)
                ));
            }
        }
        if !inactive.is_empty() {
            renderer.heading(&crate::t!("sources.unavailable"));
            for e in &inactive {
                renderer.info(&palette.dim(&format!("  ✗ {:8} ({})", e.id, e.trust.display())));
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

        let palette = renderer.palette();
        let (have, missing): (Vec<_>, Vec<_>) = catalog.iter().partition(|e| e.installed);
        if !have.is_empty() {
            renderer.heading(&crate::t!("providers.installed"));
            for e in &have {
                renderer.info(&format!("  {} {}", palette.good("✓"), e.label));
            }
        }
        if !missing.is_empty() {
            if !have.is_empty() {
                renderer.info("");
            }
            renderer.heading(&crate::t!("providers.available"));
            for e in &missing {
                renderer.info(&palette.dim(&format!(
                    "  ○ {}",
                    crate::t!("providers.add_hint", label = e.label, id = e.id)
                )));
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
            renderer.error(&crate::t!("providers.unknown", name = name));
            let known: Vec<_> = catalog.iter().map(|e| e.id).collect();
            renderer.info(&crate::t!("providers.known", names = known.join(", ")));
            return Ok(());
        };
        self.bootstrap_ecosystem(&engine, eco, config, renderer).await
    }

    /// Split ecosystem-manager names out of an install request and bootstrap each (#4),
    /// returning the remaining ordinary packages. A name counts as a manager only when it is
    /// unpinned (no `:source`, no `--source`) and matches a known ecosystem id; a cheap pure
    /// id check means an ordinary `jii vlc` pays nothing (no catalog probe).
    async fn route_managers(
        &self,
        engine: &Engine,
        packages: &[String],
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<Vec<String>> {
        let ids = engine.ecosystem_ids();
        let pinned_globally = self.global.source.is_some();
        let bare_name = |p: &str| p.split([':', '@']).next().unwrap_or(p).to_string();
        let is_manager_name =
            |p: &str| !pinned_globally && !p.contains(':') && ids.iter().any(|id| *id == bare_name(p));

        // Common case (no manager among the names): return untouched, no catalog I/O.
        if !packages.iter().any(|p| is_manager_name(p)) {
            return Ok(packages.to_vec());
        }

        let catalog = engine.ecosystem_catalog().await;
        let mut rest = Vec::new();
        for p in packages {
            if is_manager_name(p)
                && let Some(eco) = catalog.iter().find(|e| e.id == bare_name(p))
            {
                self.bootstrap_ecosystem(engine, eco, config.clone(), renderer).await?;
            } else {
                rest.push(p.clone());
            }
        }
        Ok(rest)
    }

    /// Install (bootstrap) one ecosystem manager. Shared by `jii providers add <m>` and the
    /// install-path routing of a bare manager name (#4). If it's already present, say so — a
    /// manager is something JII *drives*, so re-"installing" it is a no-op worth explaining.
    async fn bootstrap_ecosystem(
        &self,
        engine: &Engine,
        eco: &crate::engine::EcosystemStatus,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        if eco.installed {
            renderer.success(&crate::t!("providers.already_installed", label = eco.label));
            return Ok(());
        }
        let label = eco.label;
        match eco.bootstrap {
            Bootstrap::Packages(names) => {
                renderer.info(&crate::t!("providers.looking", label = label));
                match engine.first_available_package(names).await {
                    // route_managers=false: the bootstrap package's name (e.g. `pipx`) may
                    // itself be a manager id — routing it would loop. Box::pin breaks the
                    // async recursion cycle.
                    Some(pkg) => {
                        Box::pin(self.install_inner(&[pkg], config, renderer, false, false)).await
                    }
                    None => {
                        renderer.error(&crate::t!("providers.not_found", label = label));
                        renderer.info(&crate::t!("providers.tried", names = names.join(", ")));
                        Ok(())
                    }
                }
            }
            Bootstrap::Script(cmd) => {
                renderer.info(&crate::t!("providers.script_only", label = label));
                renderer.info(&crate::t!("providers.script_wont_run"));
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
            renderer.info(&crate::t!("setup.welcome"));
            renderer.info("");
            renderer.info(&crate::t!("setup.intro1"));
            renderer.info(&crate::t!("setup.intro2"));
            renderer.info("");
            if !prompt::confirm(renderer, &crate::t!("setup.confirm_spend"), true, &flags) {
                config.meta.first_run_completed = true;
                if let Err(e) = config.save() {
                    renderer.warn(&crate::t!("setup.couldnt_save", error = e));
                }
                renderer.info(&crate::t!("setup.declined"));
                return Ok(());
            }
        }

        // Step 1 — how much detail (Friendly vs Advanced).
        renderer.info("");
        let mode = match prompt::choose(
            renderer,
            &crate::t!("setup.detail_q"),
            &[
                crate::t!("setup.detail_friendly"),
                crate::t!("setup.detail_advanced"),
            ],
            0,
        ) {
            Some(1) => crate::config::OutputMode::Advanced,
            _ => crate::config::OutputMode::Friendly,
        };
        config.ui.mode = mode;

        // Step 2 — optional system check + setup. `doctor` is interactive: it diagnoses,
        // then offers to set up what's missing (each item a separate yes/no — the user stays
        // in control, and can skip every one with Enter).
        if prompt::confirm(renderer, &crate::t!("setup.run_doctor_q"), true, &flags) {
            renderer.info("");
            self.doctor(config.clone(), renderer).await?;
        }

        // Step 3 — explain the optional GitHub token (biggest single reliability win for the
        // GitHub source). Informational: JII never creates or stores a token for you.
        renderer.info("");
        self.github_token_help(&config, renderer);

        // Persist the choices and mark the wizard done.
        config.meta.first_run_completed = true;
        if let Err(e) = config.save() {
            renderer.warn(&crate::t!("setup.couldnt_save", error = e));
        }

        renderer.info("");
        renderer.success(&crate::t!("setup.complete"));
        Ok(())
    }

    /// Explain the optional GitHub token: what it buys you (a 60→5000 requests/hour lift) and
    /// exactly how to create + export it. Read-only guidance — JII never mints or stores a
    /// token. If one is already present in the environment, we just confirm it.
    fn github_token_help(&self, config: &Config, renderer: &Renderer) {
        let palette = renderer.palette();
        let env = &config.network.github_token_env;
        renderer.heading(&crate::t!("setup.gh_header"));
        renderer.info(&crate::t!("setup.gh_benefit"));

        if std::env::var(env).is_ok_and(|v| !v.is_empty()) {
            renderer.success(&crate::t!("setup.gh_already", env = env.clone()));
            return;
        }

        renderer.info("");
        renderer.info(&crate::t!("setup.gh_step_create"));
        renderer.info(&crate::t!("setup.gh_step_export"));
        renderer.info(&palette.dim(&format!("   export {env}=\"ghp_your_token_here\"")));
        renderer.info(&crate::t!("setup.gh_step_reload"));
        renderer.info(&palette.dim(&crate::t!("setup.gh_never")));
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
        let mut ranked = engine.rank(name, result.candidates);
        // Broaden on an exact miss (ADR-0042) so `search`/`info` find `ayugram-desktop`
        // from `ayugram` (and tolerate a trailing typo) — same behaviour as install.
        if ranked.is_empty() {
            ranked = engine.broaden_search(name).await;
        }
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
                renderer.warn(&crate::t!("how.no_record", package = package));
            }
            Some(record) => {
                let trust = engine
                    .source_trust(&record.source_id)
                    .map(|t| t.display())
                    .unwrap_or_else(|| crate::t!("common.unknown"));
                let version = record
                    .version
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| crate::t!("common.unknown"));
                renderer.info(&crate::t!(
                    "how.installed_on",
                    source = record.source_id.clone(),
                    date = record.installed_at.format("%Y-%m-%d %H:%M").to_string()
                ));
                renderer.info(&format!("  ✓ {}", crate::t!("how.version_line", version = version)));
                renderer.info(&format!("  ✓ {}", crate::t!("how.trust_line", trust = trust)));
            }
        }
        Ok(())
    }

    /// List software installed via jii.
    fn list(&self, audit: bool, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        // `jii list --audit` is the security view (#5): the same ledger, but with trust,
        // verification, and concerns per install. Folded in from the former `jii audit`.
        if audit {
            return self.audit_view(&engine, renderer);
        }
        let items = engine.registry().installed();

        if renderer.is_json() {
            renderer.json_value(&serde_json::json!(items));
            return Ok(());
        }
        if items.is_empty() {
            renderer.info(&crate::t!("list.empty"));
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
        let headers = [
            crate::t!("list.col_name"),
            crate::t!("list.col_source"),
            crate::t!("list.col_version"),
        ];
        let palette = renderer.palette();
        let mut lines = table_lines(&headers, &rows).into_iter();
        if let Some(header) = lines.next() {
            renderer.info(&palette.heading(&header));
        }
        for line in lines {
            renderer.info(&line);
        }
        Ok(())
    }

    /// Report source availability, latency and health (per-source), then a set of **system
    /// checks** about the host (network, common tools, `PATH`, Flathub, GitHub token). In an
    /// interactive terminal `doctor` then becomes a **setup questionnaire** (ADR-0041): each
    /// fixable check and each distro-appropriate suggestion (RPM Fusion, codecs, fonts…) is
    /// offered as a yes/no question and, on "yes", applied on the spot. It stays read-only in
    /// `--json`, under `-n/--no`, or with no TTY (Analyze → Explain → Ask → Apply).
    async fn doctor(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        // Capture what the system checks (and the questionnaire) need before `config` moves
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

        let palette = renderer.palette();
        renderer.heading(&crate::t!("doctor.sources_header"));
        for d in &diagnostics {
            let mark = if d.available { palette.good("✓") } else { palette.dim("✗") };
            let detail = match &d.detail {
                Some(text) => palette.dim(&format!("  ({text})")),
                None => String::new(),
            };
            renderer.info(&format!(
                "{mark} {}  {:12}  {} ms{detail}",
                palette.source(&format!("{:8}", d.id)),
                d.health.display(),
                d.latency.as_millis()
            ));
        }

        // System checks: probe the host environment (network, common tools, PATH, Flathub).
        let facts = gather_system_facts(&token_env).await;
        let checks = system_checks(&facts);

        // Interactive (a TTY, not JSON, not `--no`) → we'll turn actionable items into a
        // yes/no questionnaire below. Suppress a fixable check's manual advice then: the
        // upcoming question replaces it (and would otherwise contradict "we'll do it").
        let interactive = self.interactive(renderer) && !self.global.no;

        renderer.info("");
        renderer.heading(&crate::t!("doctor.checks_header"));
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
                let offered = interactive && c.fix.is_some();
                if !offered {
                    renderer.info(&format!("    → {advice}"));
                }
            }
        }
        renderer.info("");
        if warnings == 0 {
            renderer.success(&crate::t!("doctor.all_looks_good"));
        } else {
            renderer.info(&crate::t!("doctor.things_to_look", count = warnings));
        }

        if interactive {
            // The setup questionnaire: offer each fixable check and each suggestion, apply on yes.
            self.doctor_offer(&engine, &checks, config_for_fix, renderer).await?;
        } else {
            // Read-only run (JSON handled earlier; here it's --no or a non-TTY). List the
            // suggestions catalog for reference and point at the interactive run.
            self.list_suggestions(renderer);
            if checks.iter().any(|c| !c.ok && c.fix.is_some()) {
                renderer.info("");
                renderer.info(&crate::t!("doctor.run_interactive"));
            }
        }
        Ok(())
    }

    /// The `doctor` setup questionnaire (ADR-0041). Walks every actionable item — each fixable
    /// system check (git/curl, Flathub, PATH) and each distro-appropriate catalog suggestion
    /// (RPM Fusion, codecs, fonts, …) — asks a plain yes/no (Enter = skip, default no), and on
    /// "yes" applies it immediately. The single question is the consent, so installs don't ask
    /// twice (`with_yes`); the trust barrier (ADR-0006) still gates anything untrusted.
    /// `--dry-run` shows what each "yes" *would* do without changing anything.
    async fn doctor_offer(
        &self,
        engine: &Engine,
        checks: &[SystemCheck],
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let fixes: Vec<(&SystemCheck, &Fix)> = checks
            .iter()
            .filter_map(|c| c.fix.as_ref().filter(|_| !c.ok).map(|f| (c, f)))
            .collect();

        let catalog = crate::recommend::Catalog::load().ok();
        let distro_id = crate::platform::Platform::detect().distro.id();
        let all_suggestions = catalog
            .as_ref()
            .map(|c| c.for_distro(distro_id))
            .unwrap_or_default();

        // Analyse the system first (#1): drop suggestions the user has already done, so
        // doctor is real diagnostics — not a canned list. One installed-scan for the batch.
        let installed = engine.installed_index().await;
        let suggestions: Vec<_> = all_suggestions
            .into_iter()
            .filter(|r| !r.is_satisfied(&installed))
            .collect();

        if fixes.is_empty() && suggestions.is_empty() {
            renderer.info("");
            renderer.success(&crate::t!("doctor.all_good"));
            return Ok(());
        }

        let flags = self.prompt_flags(config.install.auto);
        renderer.info("");
        renderer.info(&crate::t!("doctor.setup_intro"));

        // A) Fixable system checks.
        for (check, fix) in fixes {
            let question = match fix {
                Fix::Install(pkg) => crate::t!("doctor.q_install", pkg = pkg),
                Fix::PathExport { dir } => crate::t!("doctor.q_add_path", dir = dir.display()),
                Fix::Command { .. } => crate::t!("doctor.q_fix", label = check.label),
            };
            if !prompt::confirm(renderer, &format!("  {question}"), false, &flags) {
                continue;
            }
            self.apply_fix(fix, config.clone(), renderer).await?;
        }

        // B) Curated, distro-aware suggestions (the folded-in recommend catalog).
        let mut last_category: Option<&str> = None;
        for r in &suggestions {
            if last_category != Some(r.category.as_str()) {
                renderer.info(&format!("  [{}]", r.category));
                last_category = Some(r.category.as_str());
            }
            renderer.info(&format!("    {} — {}", r.title, r.why));
            if let Some(note) = &r.note {
                renderer.info(&format!("        {}", crate::t!("common.note", note = note)));
            }
            let question = crate::t!("doctor.q_setup", title = r.title);
            if !prompt::confirm(renderer, &format!("    {question}"), false, &flags) {
                continue;
            }
            self.apply_suggestion(r, config.clone(), renderer).await?;
        }
        Ok(())
    }

    /// Apply one fixable system check. Installs route through the normal path with the
    /// questionnaire's "yes" carried through (`assume_yes`); a `Command` is shown then run;
    /// a `PathExport` appends the right line to the user's shell rc.
    async fn apply_fix(
        &self,
        fix: &Fix,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        match fix {
            Fix::Install(pkg) => {
                self.install_inner(&[pkg.to_string()], config, renderer, true, false).await?;
            }
            Fix::Command { argv, show } => {
                if self.global.dry_run {
                    renderer.info(&format!("  {}", crate::t!("doctor.would_run", cmd = show)));
                } else {
                    renderer.info(&format!("  {}", crate::t!("doctor.runs", cmd = show)));
                    match run_plain_command(argv).await {
                        Ok(()) => renderer.success(&format!("  {}", crate::t!("doctor.done"))),
                        Err(e) => renderer.error(&format!("  {}", crate::t!("doctor.failed", error = e))),
                    }
                }
            }
            Fix::PathExport { dir } => self.apply_path_export(dir, renderer),
        }
        Ok(())
    }

    /// Apply one catalog suggestion: install its packages (via the normal path, consent
    /// already given) or run its documented `manual` command (a repo-enable etc., which may
    /// use shell syntax like `$(rpm -E %fedora)`, so it runs through `sh -c`).
    async fn apply_suggestion(
        &self,
        r: &crate::recommend::Recommendation,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        if !r.packages.is_empty() {
            self.install_inner(&r.packages, config, renderer, true, false).await?;
        } else if let Some(manual) = &r.manual {
            if self.global.dry_run {
                renderer.info(&format!("  {}", crate::t!("doctor.would_run", cmd = manual)));
            } else {
                renderer.info(&format!("  {}", crate::t!("doctor.runs", cmd = manual)));
                match run_shell_command(manual).await {
                    Ok(()) => renderer.success(&format!("  {}", crate::t!("doctor.done"))),
                    Err(e) => renderer.error(&format!("  {}", crate::t!("doctor.failed", error = e))),
                }
            }
        }
        Ok(())
    }

    /// Put `dir` on `PATH` by appending the right line to the user's shell rc (ADR-0041).
    /// Picks the rc file and syntax from `$SHELL` (fish → `fish_add_path`; else an
    /// `export PATH=…` line), is idempotent (skips if the rc already references the dir),
    /// and honors `--dry-run`.
    fn apply_path_export(&self, dir: &std::path::Path, renderer: &Renderer) {
        let Some(base) = directories::BaseDirs::new() else {
            renderer.error(&format!("  {}", crate::t!("doctor.no_home")));
            return;
        };
        let shell = std::env::var("SHELL")
            .ok()
            .and_then(|s| {
                std::path::Path::new(&s)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let dir_str = dir.display().to_string();
        let (rc_rel, line) = path_export_edit(&shell, &dir_str);
        let rc_path = base.home_dir().join(rc_rel);

        if self.global.dry_run {
            renderer.info(&format!(
                "  {}",
                crate::t!("doctor.would_add", file = rc_path.display(), line = line)
            ));
            return;
        }
        // Idempotent: if the rc already references this dir, don't add a second line.
        if let Ok(existing) = std::fs::read_to_string(&rc_path)
            && existing.contains(&dir_str)
        {
            renderer.info(&format!(
                "  {}",
                crate::t!("doctor.already_on_path", file = rc_path.display(), dir = dir_str)
            ));
            return;
        }
        if let Some(parent) = rc_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::OpenOptions::new().create(true).append(true).open(&rc_path) {
            Ok(mut file) => {
                use std::io::Write;
                if let Err(e) = writeln!(file, "\n# Added by jii — put {dir_str} on PATH\n{line}") {
                    renderer.error(&format!(
                        "  {}",
                        crate::t!("doctor.couldnt_update", file = rc_path.display(), error = e)
                    ));
                    return;
                }
                renderer.success(&format!(
                    "  {}",
                    crate::t!("doctor.added_to_path", file = rc_path.display())
                ));
            }
            Err(e) => renderer.error(&format!(
                "  {}",
                crate::t!("doctor.couldnt_write", file = rc_path.display(), error = e)
            )),
        }
    }

    /// List the curated, distro-aware catalog for a read-only `doctor` run (`--no`, no TTY):
    /// title, why, and the exact way to add it. Nothing is changed. Silent when the catalog
    /// has nothing for this distro, so it never nags.
    fn list_suggestions(&self, renderer: &Renderer) {
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
        renderer.info(&crate::t!("doctor.suggestions_header"));
        let mut last_category: Option<&str> = None;
        for r in &entries {
            if last_category != Some(r.category.as_str()) {
                renderer.info(&format!("  [{}]", r.category));
                last_category = Some(r.category.as_str());
            }
            let how = if !r.packages.is_empty() {
                format!("jii {}", r.packages.join(" "))
            } else if let Some(manual) = &r.manual {
                crate::t!("how.run", cmd = manual)
            } else {
                String::new()
            };
            renderer.info(&format!("    {} — {}  ·  {}", r.title, r.why, how));
            if let Some(note) = &r.note {
                renderer.info(&format!("        {}", crate::t!("common.note", note = note)));
            }
        }
        renderer.info(&crate::t!("doctor.suggestions_info"));
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
            renderer.info(&crate::t!("history.empty"));
            return Ok(());
        }
        let rows: Vec<Vec<String>> = events
            .iter()
            .rev()
            .map(|event| {
                vec![
                    event.at.format("%Y-%m-%d %H:%M").to_string(),
                    event.action.display(),
                    event.name.clone(),
                    event.source_id.clone(),
                ]
            })
            .collect();
        let headers = [
            crate::t!("history.col_when"),
            crate::t!("history.col_action"),
            crate::t!("history.col_package"),
            crate::t!("history.col_source"),
        ];
        let palette = renderer.palette();
        let mut lines = table_lines(&headers, &rows).into_iter();
        if let Some(header) = lines.next() {
            renderer.info(&palette.heading(&header));
        }
        for line in lines {
            renderer.info(&line);
        }
        Ok(())
    }

    /// Audit installed software: where each came from, its trust, how it was
    /// verified, and anything that needs attention.
    /// The security view behind `jii list --audit` (#5): provenance, trust, artifact
    /// verification and concerns per install. Registry-based and fast — no live provider
    /// calls (the engine's `audit` reads the ledger).
    fn audit_view(&self, engine: &Engine, renderer: &Renderer) -> crate::error::Result<()> {
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
            renderer.info(&crate::t!("list.empty"));
            return Ok(());
        }

        let mut flagged = 0;
        let rows: Vec<Vec<String>> = entries
            .iter()
            .map(|e| {
                let trust = e.trust.map_or_else(|| crate::t!("common.unknown"), |t| t.display());
                let status = if e.concerns.is_empty() {
                    crate::t!("list.status_ok")
                } else {
                    flagged += 1;
                    let reasons: Vec<&str> = e.concerns.iter().map(|c| c.message()).collect();
                    format!("⚠ {}", reasons.join(", "))
                };
                vec![
                    e.name.clone(),
                    e.source_id.clone(),
                    trust,
                    e.verification.label().to_string(),
                    status,
                ]
            })
            .collect();
        let headers = [
            crate::t!("list.col_name"),
            crate::t!("list.col_source"),
            crate::t!("list.col_trust"),
            crate::t!("list.col_verified"),
            crate::t!("list.col_status"),
        ];
        let mut lines = table_lines(&headers, &rows).into_iter();
        if let Some(header) = lines.next() {
            renderer.info(&renderer.palette().heading(&header));
        }
        for line in lines {
            renderer.info(&line);
        }

        if flagged > 0 {
            renderer.warn(&crate::t!("audit.need_attention", flagged = flagged, total = entries.len()));
        } else {
            renderer.success(&crate::t!("audit.all_fine", total = entries.len()));
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
    version.map(|v| v.to_string()).unwrap_or_else(|| crate::t!("common.unknown"))
}

/// Render an aligned text table: a header row then one line per data row, each
/// column padded to the widest cell in that column (the final column is left
/// unpadded so trailing content never carries stray spaces). Widths are computed
/// from the data so long names don't break alignment. Returns the rendered lines.
fn table_lines(headers: &[String], rows: &[Vec<String>]) -> Vec<String> {
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
    let header_cells: Vec<String> = headers.to_vec();
    let mut out = vec![render(&header_cells)];
    out.extend(rows.iter().map(|r| render(r)));
    out
}

/// A compact one-line description of a candidate for `search`/`info`:
/// `source  vX  trust  — summary`.
fn candidate_line(candidate: &PackageCandidate, palette: crate::ui::Palette) -> String {
    // Pad the source id to width *before* colouring so the ANSI codes don't skew alignment.
    let src = palette.source(&format!("{:8}", candidate.source_id));
    let version = candidate
        .version
        .as_ref()
        .map(|v| format!("{}  ", palette.version(&format!("v{v}"))))
        .unwrap_or_default();
    let summary = candidate
        .summary
        .as_deref()
        .map(|s| palette.dim(&format!("  — {}", one_line(s, 80))))
        .unwrap_or_default();
    format!("{src} {version}{}{summary}", palette.trust(candidate.trust))
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
        facts.push(crate::t!("facts.signed"));
    }
    if let Some(version) = &candidate.version {
        facts.push(crate::t!("facts.version", version = version.clone()));
    }
    if !candidate.arch_ok {
        facts.push(crate::t!("facts.arch_mismatch"));
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
        vec![crate::t!("facts.trust_source", trust = candidate.trust.display())]
    } else {
        highlights
    };
    reasons.extend(model_facts(candidate));
    reasons
}

/// How the `doctor` questionnaire can remedy a failing check. A check with no `Fix` is
/// manual-only (JII won't invent a GitHub token for you).
#[derive(Debug)]
enum Fix {
    /// Install a package through JII's normal path.
    Install(&'static str),
    /// Run a plain command JII shows first. Used for the Flathub remote, which Flatpak
    /// elevates via its own polkit (like its installs), so JII wraps no sudo/pkexec.
    Command {
        argv: Vec<String>,
        /// Human-readable rendering of `argv`, shown before running.
        show: String,
    },
    /// Put a directory on `PATH` by appending an export line to the user's shell rc.
    /// JII edits your shell rc only on an explicit yes in the questionnaire (ADR-0041).
    PathExport { dir: std::path::PathBuf },
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
        SystemCheck::pass(crate::t!("check.internet_ok"))
    } else {
        SystemCheck::warn(
            crate::t!("check.internet_missing"),
            crate::t!("check.internet_advice"),
        )
        .critical()
    });

    // Common tools JII and its sources lean on — and which JII can itself install.
    checks.push(if f.git {
        SystemCheck::pass(crate::t!("check.git_ok"))
    } else {
        SystemCheck::warn(crate::t!("check.git_missing"), crate::t!("check.git_advice"))
            .fixable(Fix::Install("git"))
    });
    checks.push(if f.curl {
        SystemCheck::pass(crate::t!("check.curl_ok"))
    } else {
        SystemCheck::warn(crate::t!("check.curl_missing"), crate::t!("check.curl_advice"))
            .fixable(Fix::Install("curl"))
    });

    // ~/.local/bin on PATH — user-space installs land there.
    let local = f.local_bin.display();
    checks.push(if f.local_bin_on_path {
        SystemCheck::pass(crate::t!("check.path_ok", dir = local))
    } else {
        SystemCheck::warn(
            crate::t!("check.path_missing", dir = local),
            crate::t!("check.local_path_advice"),
        )
        .fixable(Fix::PathExport { dir: f.local_bin.clone() })
    });

    // ~/.cargo/bin on PATH — only when cargo is actually in play.
    if f.cargo_bin_relevant {
        let cargo = f.cargo_bin.display();
        checks.push(if f.cargo_bin_on_path {
            SystemCheck::pass(crate::t!("check.path_ok", dir = cargo))
        } else {
            SystemCheck::warn(
                crate::t!("check.path_missing", dir = cargo),
                crate::t!("check.cargo_path_advice"),
            )
            .fixable(Fix::PathExport { dir: f.cargo_bin.clone() })
        });
    }

    // Flathub — only meaningful when Flatpak is installed.
    if f.flatpak {
        checks.push(if f.flathub {
            SystemCheck::pass(crate::t!("check.flathub_ok"))
        } else {
            SystemCheck::warn(
                crate::t!("check.flathub_missing"),
                crate::t!("check.flathub_advice"),
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
        SystemCheck::pass(crate::t!("check.token_ok", env = f.token_env))
    } else {
        SystemCheck::warn(
            crate::t!("check.token_missing", env = f.token_env),
            crate::t!("check.token_advice", env = f.token_env),
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
    let token_set = std::env::var(token_env).is_ok_and(|v| !v.is_empty());

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

/// Run a plain, non-JII-elevated command for the `doctor` questionnaire, letting it inherit
/// the terminal so a tool's own polkit prompt (Flatpak) is visible. Errors on spawn failure
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

/// Run a documented catalog `manual` command through `sh -c` — it may use shell syntax
/// (e.g. `$(rpm -E %fedora)`) and can carry its own `sudo`, whose password prompt is
/// visible because the child inherits the terminal. Errors on spawn failure or non-zero exit.
async fn run_shell_command(cmd: &str) -> crate::error::Result<()> {
    let status = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()
        .await
        .map_err(|e| crate::error::JiiError::spawn("sh", e))?;
    if status.success() {
        Ok(())
    } else {
        Err(crate::error::JiiError::Other(anyhow::anyhow!(
            "`{cmd}` exited with {status}"
        )))
    }
}

/// Pick the shell rc file (relative to `$HOME`) and the line that puts `dir` on `PATH`,
/// from the shell's basename. Pure, so the wording is unit-tested. Fish uses
/// `fish_add_path`; every POSIX shell gets an `export PATH="…:$PATH"` line. An unknown
/// shell falls back to `~/.bashrc` (the most common interactive default).
fn path_export_edit(shell: &str, dir: &str) -> (&'static str, String) {
    match shell {
        "fish" => (".config/fish/config.fish", format!("fish_add_path {dir}")),
        "zsh" => (".zshrc", format!("export PATH=\"{dir}:$PATH\"")),
        _ => (".bashrc", format!("export PATH=\"{dir}:$PATH\"")),
    }
}

/// Whether the Flathub remote is registered (system or user). Best-effort: any error
/// reading remotes reports "not configured" rather than a false positive.
async fn flathub_configured() -> bool {
    tokio::process::Command::new("flatpak")
        .args(["remotes", "--columns=name"])
        .output()
        .await
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == "flathub"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PkgVersion, TrustLevel};

    #[test]
    fn cli_definition_is_valid() {
        // clap's own validation of the whole command tree (conflicting args, bad flags,
        // duplicate subcommands…). Cheap insurance that the CLI surface always parses.
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn path_export_edit_picks_rc_and_syntax_per_shell() {
        // Fish has its own PATH command and config file.
        let (rc, line) = path_export_edit("fish", "/home/u/.cargo/bin");
        assert_eq!(rc, ".config/fish/config.fish");
        assert_eq!(line, "fish_add_path /home/u/.cargo/bin");

        // zsh → ~/.zshrc with an export line that prepends the dir.
        let (rc, line) = path_export_edit("zsh", "/home/u/.local/bin");
        assert_eq!(rc, ".zshrc");
        assert_eq!(line, "export PATH=\"/home/u/.local/bin:$PATH\"");

        // An unknown/empty shell falls back to bash's rc, never panics.
        let (rc, line) = path_export_edit("", "/x");
        assert_eq!(rc, ".bashrc");
        assert_eq!(line, "export PATH=\"/x:$PATH\"");
    }

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
        let lines = table_lines(
            &["NAME".to_string(), "SOURCE".to_string(), "VERSION".to_string()],
            &rows,
        );
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
        let lines = table_lines(&["PACKAGE".to_string(), "SRC".to_string()], &rows);
        assert!(lines[1].starts_with("a      ")); // padded to len("PACKAGE") == 7
        assert_eq!(lines[1].find('b'), Some("PACKAGE".len() + 2));
    }

    #[test]
    fn action_labels_are_human_readable_past_tense() {
        use crate::registry::Action;
        // Default locale (English) in the test binary.
        assert_eq!(Action::Install.display(), "installed");
        assert_eq!(Action::Remove.display(), "removed");
        assert_eq!(Action::Update.display(), "updated");
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
        let line = candidate_line(
            &candidate(TrustLevel::Official, true, Some("2.0")),
            crate::ui::Palette::plain(),
        );
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
    fn system_checks_flag_missing_path_with_a_pathexport_fix() {
        let mut f = facts_all_good();
        f.local_bin_on_path = false;
        let checks = system_checks(&f);
        let path_check = checks.iter().find(|c| c.label.contains(".local/bin")).unwrap();
        assert!(!path_check.ok);
        assert!(path_check.label.contains("PATH"));
        match &path_check.fix {
            Some(Fix::PathExport { dir }) => assert!(dir.ends_with(".local/bin")),
            other => panic!("expected a PathExport fix, got {other:?}"),
        }
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
    fn a_healthy_env_carries_no_fixes() {
        // Every check passes → nothing to offer in the questionnaire.
        assert!(system_checks(&facts_all_good()).iter().all(|c| c.fix.is_none()));
    }

    #[test]
    fn missing_cargo_path_carries_a_pathexport_fix() {
        // The cargo/bin papercut is now fixable — JII offers to add it to PATH (ADR-0041).
        let mut f = facts_all_good();
        f.cargo_bin_on_path = false;
        let checks = system_checks(&f);
        let cargo = checks.iter().find(|c| c.label.contains(".cargo/bin")).unwrap();
        assert!(!cargo.ok);
        assert!(matches!(&cargo.fix, Some(Fix::PathExport { dir }) if dir.ends_with(".cargo/bin")));
    }
}
