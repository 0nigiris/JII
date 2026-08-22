// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Command-line surface: clap definitions, global flags, and dispatch.
//!
//! The command set is intentionally the full, stable surface from
//! `docs/ARCHITECTURE.md` §13. Commands not yet implemented return a clear
//! "not yet" message that names the phase, so the CLI shape never churns.

use clap::{Parser, Subcommand};

use crate::config::{ColorChoice, Config, DeclarativePref, Profile};
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
    /// Answer "yes" to every prompt, so it runs without stopping to ask (still within trust limits).
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// Answer "no" to every prompt — it previews, then declines and aborts (nothing is changed).
    /// Not the same as `-d`/`--dry-run`: `--no` still resolves and previews the *real* action it
    /// would take; `--dry-run` shows the full plan and exits.
    #[arg(short = 'n', long, global = true)]
    pub no: bool,

    /// Install the recommended option without confirmation (within trust limits).
    #[arg(long, global = true)]
    pub auto: bool,

    /// Force a specific source id (e.g. `-s flatpak`). Even shorter per-package: `jii htop:dnf`.
    #[arg(short = 's', long, value_name = "ID", global = true)]
    pub source: Option<String>,

    /// Ranking profile preset.
    #[arg(long, value_enum, global = true)]
    pub profile: Option<Profile>,

    /// Preview: show the full plan and exit without doing anything (alias: `--preview`). This is
    /// the flag to reach for when you just want to see what would happen — unlike `-n`/`--no`,
    /// which answers "no" to a prompt on the real action.
    #[arg(short = 'd', long, visible_alias = "preview", global = true)]
    pub dry_run: bool,

    /// Launch it once it's installed: `jii htop --run`. Only for a single package, and only
    /// when the install actually happened (never under `--dry-run`).
    #[arg(long, global = true)]
    pub run: bool,

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

    /// Prefer editing the Nix config (home-manager) over an imperative `nix profile install`
    /// for this run — overrides `[install] prefer_declarative`.
    #[arg(long, global = true, conflicts_with = "nix_imperative")]
    pub nix_config: bool,

    /// Force an imperative install this run even if the config prefers a declarative edit.
    #[arg(long, global = true)]
    pub nix_imperative: bool,
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
    /// Show your JII achievements — the ones you've unlocked and the ones still to find
    /// (secret ones show as `???` until earned).
    #[command(alias = "achievement")]
    Achievements,
    /// Show what changed in JII: this version by default, any past version by name
    /// (`jii changelog 0.1.12`), or the whole history with `--all`. Works offline.
    #[command(alias = "whatsnew")]
    Changelog {
        /// Version to show, e.g. `0.1.14-beta` (the `-beta` suffix is optional).
        version: Option<String>,
        /// Show every release JII knows about, newest first.
        #[arg(long, conflicts_with_all = ["version", "since"])]
        all: bool,
        /// Show every release newer than this one — what an update from it brought you.
        #[arg(long, value_name = "VERSION", conflicts_with = "version")]
        since: Option<String>,
    },
    /// List installation sources and whether each is usable here (native managers for other
    /// distros are hidden by default; `--all` shows all). Ecosystem managers (Flatpak, Snap,
    /// cargo, npm, AUR helpers…) are annotated with how to add or remove them.
    /// `jii sources disable|enable <id>` turns a source off/on; `jii sources add|remove <id>`
    /// installs/uninstalls an ecosystem manager (system managers can't be removed).
    Sources {
        /// Show every source, including native managers that don't apply to this system.
        #[arg(long)]
        all: bool,
        #[command(subcommand)]
        action: Option<SourcesAction>,
    },
    /// Show or set the interface language, saved to the config so it sticks. `auto` follows
    /// the system locale. Example: `jii lang ru`. (Per-run override: the global `--lang`.)
    Lang {
        /// Language to set: en, ru, or auto. Omit to show the current setting.
        code: Option<String>,
    },
    /// Show the search-cache location, or clear it. `jii cache` prints the path; `jii cache
    /// clear` deletes it (JII rebuilds it on the next search).
    Cache {
        #[command(subcommand)]
        action: Option<CacheAction>,
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
    /// The tester checklist (testers only — see docs/TESTING.md): real installs, an
    /// interactive "looks right?" verdict per step, a full scrubbed .log, one-keypress
    /// upload, and a pre-filled GitHub issue link. Hidden from --help and the README.
    #[command(name = "yes-I-am-dev-and-want-to-test", hide = true)]
    DevTest,
}

/// Actions under `jii cache` (bare `jii cache` shows the path).
#[derive(Debug, Subcommand)]
pub enum CacheAction {
    /// Delete the on-disk search cache (rebuilt on the next search).
    Clear,
}

/// Actions under `jii sources` (bare `jii sources` lists them).
#[derive(Debug, Subcommand)]
pub enum SourcesAction {
    /// Turn a source off — JII stops considering it (e.g. `jii sources disable snap`).
    Disable {
        /// The source id (dnf, flatpak, cargo, snap…).
        id: String,
    },
    /// Turn a previously disabled source back on.
    Enable {
        /// The source id to re-enable.
        id: String,
    },
    /// Bootstrap a missing ecosystem manager, e.g. `jii sources add flatpak` (or `yay`/`paru`
    /// on Arch). Managers in the distro repos install through the normal flow; script-only
    /// ones (Homebrew, Nix, an AUR helper) are shown, never run.
    Add {
        /// The ecosystem id (flatpak, snap, cargo, npm, pipx, go, brew, nix, yay, paru…).
        id: String,
    },
    /// Uninstall an ecosystem manager from the system (confirmed; exact command shown first).
    /// **System package managers (dnf/apt/pacman…) are refused** — removing them would break
    /// the OS. Script-installed managers (Homebrew, Nix) print manual removal steps.
    Remove {
        /// The ecosystem id to uninstall (flatpak, snap, cargo, pipx, go, yay, paru…).
        id: String,
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
            | Some(Commands::Lang { .. })
            | Some(Commands::Cache { .. })
            | Some(Commands::Doctor { .. })
            | Some(Commands::Uninstall)
            | Some(Commands::Completions { .. })
            | Some(Commands::Man)
            | Some(Commands::DevTest) => None,
            Some(Commands::Install { packages }) => Some(format!("jii {}", packages.join(" "))),
            Some(Commands::Remove { packages }) => Some(format!("jii remove {}", packages.join(" "))),
            Some(Commands::Update { packages }) if packages.is_empty() => Some("jii update".to_string()),
            Some(Commands::Update { packages }) => Some(format!("jii update {}", packages.join(" "))),
            Some(Commands::Search { query }) => Some(format!("jii search {}", query.join(" "))),
            Some(Commands::Info { package }) => Some(format!("jii info {package}")),
            Some(Commands::How { package }) => Some(format!("jii how {package}")),
            Some(Commands::List { audit }) => Some(if *audit {
                "jii list --audit".to_string()
            } else {
                "jii list".to_string()
            }),
            Some(Commands::History) => Some("jii history".to_string()),
            Some(Commands::Achievements) => None,
            Some(Commands::Changelog { .. }) => None,
            Some(Commands::Sources { .. }) => None,
            None => (!self.packages.is_empty()).then(|| format!("jii {}", self.packages.join(" "))),
        }
    }

    /// Dispatch the parsed command.
    pub async fn run(self, config: Config) -> crate::error::Result<()> {
        let renderer = self.renderer_for(&config);

        // Anti-tamper (ADR-0074): if the achievements ledger's signature doesn't verify, someone
        // hand-edited it (or copied it from another machine). `load` has already wiped it in
        // memory; react once, in-character, and persist the clean, freshly-signed ledger so the
        // scolding doesn't repeat every command. Best-effort and silent in JSON mode.
        if let Ok(store) = crate::achievements::Achievements::load()
            && store.tampered()
        {
            if !renderer.is_json() {
                renderer.warn(&crate::t!("achieve.tamper.line1"));
                renderer.info(&crate::t!("achieve.tamper.line2"));
            }
            let _ = store.save();
        }

        // The secret install path (the `secret` branch's Sans-fight installer) drops a
        // sentinel that JII picks up on its very next run to grant the hidden `sans`
        // achievement. Consumed once, best-effort, and silent in JSON mode.
        if crate::achievements::Achievements::take_sentinel() {
            self.grant_achievement("sans", &renderer);
        }

        // The boss-fight installers drop their own sentinels, whose contents record how the fight
        // ended (`spare`/`kill`) — JII grants the matching hidden achievement and shows the path.
        // Both Jevil fights share one sentinel because they share the 🃏 achievement.
        for boss in crate::achievements::BOSSES {
            if let Some(variant) = crate::achievements::Achievements::take_boss_sentinel(boss) {
                self.grant_boss(boss.id, &variant, &renderer);
            }
        }

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
                self.setup(config.clone(), &renderer, true, true).await?;
                renderer.info("");
                renderer.info(&crate::t!("setup.now_running", cmd = summary));
                renderer.info("");
                // Reload so the dispatched command sees the wizard's saved choices.
                Config::load().unwrap_or(config)
            } else if matches!(self.command, Some(Commands::Doctor { .. })) {
                // First-ever run *is* `jii doctor`: previously this skipped onboarding entirely
                // (no mode choice, no token hint, and first-run stayed unmarked so the *next*
                // command re-onboarded). Give the wizard now — but with offer_doctor=false, so
                // the real doctor below runs once instead of being offered here and again.
                renderer.info(&crate::t!("setup.first_use"));
                renderer.info("");
                self.setup(config.clone(), &renderer, true, false).await?;
                renderer.info("");
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
            Some(Commands::Install { packages }) => self.install(packages, config, &renderer).await,
            None => {
                if self.packages.is_empty() {
                    // Very first bare `jii` on an interactive terminal → a warm welcome + the
                    // 30-second setup wizard (once). Otherwise the usual usage hint.
                    if config.is_first_run() && self.interactive(&renderer) {
                        self.setup(config, &renderer, true, true).await
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
            Some(Commands::How { package }) => self.how(package, config, &renderer).await,
            Some(Commands::List { audit }) => self.list(*audit, config, &renderer),
            Some(Commands::History) => self.history(config, &renderer),
            Some(Commands::Achievements) => self.achievements(&renderer),
            Some(Commands::Changelog { version, all, since }) => {
                self.changelog(version.as_deref(), *all, since.as_deref(), &renderer)
            }

            Some(Commands::Doctor { fix: _ }) => self.doctor(config, &renderer).await,

            Some(Commands::Update { packages }) => self.update(packages, config, &renderer).await,

            Some(Commands::Search { query }) => self.search(query, config, &renderer).await,
            Some(Commands::Info { package }) => self.info(package, config, &renderer).await,
            Some(Commands::Sources { all, action }) => match action {
                None => self.sources(*all, config, &renderer).await,
                Some(SourcesAction::Disable { id }) => self.sources_set_enabled(id, false, config, &renderer),
                Some(SourcesAction::Enable { id }) => self.sources_set_enabled(id, true, config, &renderer),
                Some(SourcesAction::Add { id }) => self.sources_add(id, config, &renderer).await,
                Some(SourcesAction::Remove { id }) => self.sources_remove(id, config, &renderer).await,
            },
            Some(Commands::Lang { code }) => self.lang(code.as_deref(), config, &renderer),
            Some(Commands::Cache { action }) => self.cache(action.as_ref(), &renderer),
            Some(Commands::Setup) => self.setup(config, &renderer, false, true).await,
            Some(Commands::Uninstall) => self.self_uninstall(config, &renderer).await,
            Some(Commands::Completions { shell }) => {
                let mut cmd = <Cli as clap::CommandFactory>::command();
                clap_complete::generate(*shell, &mut cmd, "jii", &mut std::io::stdout());
                Ok(())
            }
            Some(Commands::Man) => self.man().await,
            Some(Commands::DevTest) => crate::devtest::run().await,
        }
    }

    /// `jii man` — the manual page.
    ///
    /// Redirected or piped (`jii man > jii.1` — how the .rpm/.deb build theirs) it emits the raw
    /// roff source, unchanged. On a terminal that source is unreadable line noise, so it is handed
    /// to `man` to format and page it like any other manual. If `man` isn't installed or can't run
    /// it, the raw roff is printed rather than nothing.
    async fn man(&self) -> crate::error::Result<()> {
        use std::io::Write;

        let cmd = <Cli as clap::CommandFactory>::command();
        let mut roff: Vec<u8> = Vec::new();
        clap_mangen::Man::new(cmd)
            .render(&mut roff)
            .map_err(|e| crate::error::JiiError::Other(anyhow::anyhow!("man: {e}")))?;

        if crate::platform::Platform::detect().is_tty && crate::provider::which("man").await {
            // `man` reads a file, not a stream: hand it one in the temp dir and clean up after.
            let dir = std::env::temp_dir().join(format!("jii-man-{}", std::process::id()));
            let page = dir.join("jii.1");
            let shown = std::fs::create_dir_all(&dir).is_ok()
                && std::fs::write(&page, &roff).is_ok()
                && matches!(
                    tokio::process::Command::new("man").arg("-l").arg(&page).status().await,
                    Ok(status) if status.success()
                );
            let _ = std::fs::remove_dir_all(&dir);
            if shown {
                return Ok(());
            }
        }
        std::io::stdout()
            .write_all(&roff)
            .map_err(|e| crate::error::JiiError::Other(anyhow::anyhow!("man: {e}")))
    }

    /// Install path (one or many packages): for each package search → rank → pick best,
    /// then let the engine group + optimize the chosen candidates into batched plans, and
    /// run them as **one** operation (one preview, one confirmation, one root escalation,
    /// one execution). A not-found package never cancels the rest (requirement: it is
    /// reported and the user is offered to continue).
    async fn install(&self, packages: &[String], config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        self.install_inner(packages, config, renderer, false, true).await
    }

    /// The install flow. `assume_yes` lets a caller that already obtained consent (the
    /// `doctor` questionnaire) skip the redundant final confirmation — the trust barrier
    /// still gates untrusted sources (ADR-0006), so this only auto-confirms trusted-enough
    /// candidates. `route_managers` enables the bare-manager-name → bootstrap routing (#4);
    /// it is **off** when installing a bootstrap package (whose name, e.g. `pipx`, may itself
    /// be a manager id — routing it would loop) and for doctor's explicit package installs.
    async fn install_inner(
        &self, packages: &[String], config: Config, renderer: &Renderer, assume_yes: bool, route_managers: bool,
    ) -> crate::error::Result<()> {
        let mut engine = Engine::new(self.apply_profile(config.clone()))?;

        // #4: a bare ecosystem-manager name (npm, cargo, pipx, flatpak, snap, go, brew, nix)
        // means "install that manager", not "find a package called npm" — route it to
        // bootstrap. A pinned source (`npm:dnf` or `--source`) opts out. Runs before the
        // usable-source gate so a fresh box can still bootstrap its very first manager.
        let rest = if route_managers {
            self.route_managers(&engine, packages, config.clone(), renderer).await?
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
            // The rejection (a bad `@ref`, an unknown `:source`) is already on screen as a red
            // ✗ — but it is still a refusal to do what was asked, so the run exits non-zero.
            None => return Err(crate::error::JiiError::AlreadyReported),
        };

        // 1. Resolve each package to its best candidate; collect the misses separately.
        //    A single package keeps the "Also available" alternatives view; a real batch
        //    would make that too noisy, so it is shown only when installing one.
        let single = specs.len() == 1;
        let effective_auto = self.global.auto || engine.config().install.auto;
        let mut chosen: Vec<PackageCandidate> = Vec::new();
        let mut not_found: Vec<String> = Vec::new();
        let mut chose_interactively = false;
        for spec in &specs {
            // A per-package `:source` (ADR-0031) pins the provider and, like `--source`,
            // suppresses the chooser; it takes precedence over the whole-command `--source`.
            let pkg_source = spec.source.as_ref().or(self.global.source.as_ref());
            let name = &spec.name;
            let query = Query::name(name);
            if !renderer.is_friendly() {
                renderer.info(&crate::t!("install.searching_for", name = query.raw));
            }
            // Friendly's "Searching…" is a spinner, not a printed line: every source is queried
            // over the network, which on a slow one is seconds of otherwise-silent terminal.
            // Stopped before anything else prints (the chooser needs the line back).
            let spinner = crate::ui::Spinner::start(renderer, &crate::t!("install.searching"));
            let result = engine.search(&query).await;
            spinner.stop().await;
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
                // Bare-name miss + interactive + a forge offers repo search → the GitHub-style
                // repo picker (ADR-0053) before giving up. `owner/repo` already resolves on the
                // normal path, so only slash-free names reach here; a pinned source or any
                // intent-expressing flag (--source/--auto/--yes/--no) skips it, as does a batch.
                if single
                    && pkg_source.is_none()
                    && !name.contains('/')
                    && !effective_auto
                    && !self.global.yes
                    && !self.global.no
                    && self.interactive(renderer)
                    && engine.has_repo_search()
                    && let Some(cand) = self.repo_picker(&engine, name, renderer).await
                {
                    // Picking a repo is only the *selection*; a forge candidate is untrusted,
                    // so the batch confirm below still asks explicitly (ADR-0006).
                    chosen.push(cand);
                    continue;
                }
                not_found.push(name.clone());
                continue;
            }
            // Be explicit when the best match isn't what was typed, so a broadened result
            // never silently installs a differently-named package. A Flatpak app-id counts as
            // a match on its last segment (`firefox` == `org.mozilla.firefox`), so we don't
            // cry "no exact match" when the app-id *is* exactly what the user meant (the
            // openSUSE `jii firefox` papercut).
            let best_name = &ranked[0].name;
            let tail = best_name.rsplit('.').next().unwrap_or(best_name);
            if !best_name.eq_ignore_ascii_case(name) && !tail.eq_ignore_ascii_case(name) {
                renderer.info(&crate::t!(
                    "install.no_exact_match",
                    name = name,
                    closest = ranked[0].name.clone()
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
                    let v = record.version.as_ref().map(|v| format!(" ({v})")).unwrap_or_default();
                    renderer.success(&crate::t!(
                        "install.already_installed",
                        name = name,
                        source = record.source_id,
                        version = v
                    ));
                    // `jii htop --run` on an already-installed htop should still start it —
                    // "install and run" with the install already done is just "run" (this
                    // execs, so it doesn't come back).
                    if self.global.run
                        && single
                        && !self.global.dry_run
                        && let Some(candidate) = ranked.iter().find(|c| c.source_id == record.source_id)
                    {
                        self.launch(&engine, candidate, renderer).await;
                    }
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
                // The recommendation is the closest match that is *trustworthy enough* to crown —
                // never an untrusted name-squat, even when it's the exact-name top rank (ADR-0006:
                // auto never installs untrusted, so it's never presented as the pick either). When
                // nothing trusted matches, we star nothing and say so, leaving an explicit choice
                // (the `jii google` report: an untrusted `google` crate was wrongly "recommended").
                let palette = renderer.palette();
                let rec = crate::engine::ranking::recommended_index(&ranked);
                if rec.is_none() {
                    renderer.warn(&crate::t!("install.no_trusted_match", name = name));
                }
                let labels: Vec<String> = ranked
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        if Some(i) == rec {
                            format!(
                                "{}  {} {}",
                                candidate_line(c, palette),
                                palette.mark_star(),
                                crate::t!("install.recommended_tag")
                            )
                        } else {
                            candidate_line(c, palette)
                        }
                    })
                    .collect();
                let header = crate::t!("install.choose_header", name = name);
                match prompt::choose(renderer, &header, &labels, rec.unwrap_or(0)) {
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

        // 1a-bis. T6 (ADR-0065): a chosen candidate may come from an ecosystem manager that isn't
        //   installed — a `can_search` source (Flatpak, Snap, cargo, npm, pipx, go, brew) answered
        //   over the network, so an uninstalled-Flatpak `obsidian` outranks the last-resort GitHub
        //   binary. Set the manager up first (then the app installs through it) instead of building
        //   a command that would fail; a declined or script-only manager drops its apps with a note.
        //   github has no `ecosystem()`, so it never routes here — it stays the plain binary.
        chosen = self
            .bootstrap_missing_managers(&engine, chosen, &config, renderer, assume_yes)
            .await?;
        // No early return on an empty `chosen` here. When *nothing* resolved, `chosen` was
        // already empty on the way in, and returning at this point skipped step 2 below — so
        // `jii <a-name-that-does-not-exist>` printed absolutely nothing and exited 0. The one
        // `chosen.is_empty()` check that matters lives after the misses are reported.

        // 1b. Declarative-vs-imperative install choice (ADR-0054/0056 + the `prefer_declarative`
        //     follow-up). The owning source may offer alternative install *strategies* (Nix:
        //     editing a config file / showing a snippet alongside `nix profile install`). Only Nix
        //     opts in, and only when it detects a config file on this host — so this is silent for
        //     every other source and for plain `nix profile` users.
        //     The CLI only decides *whether* to prefer a declarative strategy; which strategies
        //     exist is entirely the provider's business (no core source-branch — the menu / route
        //     just acts on whatever `install_strategies` returns, which is empty for every source
        //     but Nix-with-a-detected-config). The preference is `ask` (default) / `always` /
        //     `never`, overridable per-run by `--nix-config` / `--nix-imperative`.
        match self.declarative_pref(engine.config()) {
            // Never: everything installs imperatively (the historical fall-through).
            DeclarativePref::Never => {}
            // Ask: the single-package interactive chooser. A batch stays imperative to avoid a
            //   prompt-storm — a batch user who wants the config route sets `always`/`--nix-config`.
            DeclarativePref::Ask => {
                if single
                    && chosen.len() == 1
                    && !effective_auto
                    && !self.global.yes
                    && !self.global.no
                    && !self.global.dry_run
                    && self.interactive(renderer)
                {
                    let candidate = &chosen[0];
                    let strategies = engine.install_strategies(&candidate.source_id, candidate).await;
                    if !strategies.is_empty() {
                        let palette = renderer.palette();
                        let labels: Vec<String> = strategies
                            .iter()
                            .map(|s| format!("{}  —  {}", s.label, palette.dim(&s.hint)))
                            .collect();
                        let header = crate::t!("nix.strategy_header", name = candidate.name.clone());
                        match prompt::choose(renderer, &header, &labels, 0) {
                            None => {
                                renderer.info(&crate::t!("common.aborted"));
                                return Ok(());
                            }
                            Some(index) => match &strategies[index].kind {
                                crate::model::StrategyKind::Manual { guidance } => {
                                    renderer.info(guidance);
                                    renderer.info(&crate::t!("nix.guidance_footer"));
                                    return Ok(());
                                }
                                kind @ crate::model::StrategyKind::EditFile { .. } => {
                                    self.apply_edit_file(engine.config().install.auto, kind, assume_yes, renderer)
                                        .await;
                                    return Ok(());
                                }
                                // Imperative → fall through to the normal preview/confirm/install.
                                crate::model::StrategyKind::Imperative => {}
                            },
                        }
                    }
                }
            }
            // Always: route each package that offers a declarative edit into it — single, batch
            //   or scripted. Handled packages leave `chosen`; the rest install imperatively.
            DeclarativePref::Always => {
                let mut remaining = Vec::with_capacity(chosen.len());
                for candidate in std::mem::take(&mut chosen) {
                    if self.route_declarative(&engine, &candidate, assume_yes, renderer).await {
                        continue;
                    }
                    remaining.push(candidate);
                }
                chosen = remaining;
            }
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
            // Even after broadening and (interactively) the repo picker, nothing resolved. Don't
            // dead-end: hand the user links to *browse* for it themselves and read the project's
            // own install docs (owner ask) — GitHub search finds the repo, Flathub a desktop app.
            // Skipped in JSON (machine output) and when a `--source` was pinned (the miss is about
            // that one source, not "where do I find this at all").
            if !renderer.is_json() && self.global.source.is_none() {
                let palette = renderer.palette();
                renderer.info(&crate::t!("install.browse_hint"));
                for name in &not_found {
                    let q = url_query_encode(name);
                    renderer.info(&palette.dim(&crate::t!(
                        "install.browse_github",
                        url = format!("https://github.com/search?q={q}&type=repositories")
                    )));
                    renderer.info(&palette.dim(&crate::t!(
                        "install.browse_flathub",
                        url = format!("https://flathub.org/apps/search?q={q}")
                    )));
                }
            }
        }
        if chosen.is_empty() {
            // Nothing at all to install. If that is because a name resolved nowhere, the run
            // failed — it printed a red ✗ — and must exit non-zero, so a script wrapping jii
            // can tell. The message is already on screen; `AlreadyReported` carries only the
            // status. An empty `chosen` with no misses is an ordinary "nothing to do".
            return if not_found.is_empty() {
                Ok(())
            } else {
                Err(crate::error::JiiError::AlreadyReported)
            };
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

        // Loud red warning for every suspicious pick (the junk heuristic downgraded it):
        // the name may only *look like* the well-known tool. Shown right under the preview
        // so it can't be missed; the untrusted confirm barrier below still applies.
        for c in batch.iter().flat_map(|b| b.candidates.iter()).filter(|c| c.suspicious) {
            renderer.error(&crate::t!(
                "install.suspicious",
                name = c.name.clone(),
                source = c.source_id.clone()
            ));
        }

        // For a single install, offer the candidate's web page — the user can open it to
        // confirm "yes, this is the one I meant" before answering (their ask: a link to eyeball
        // the app/repo, especially for a last-resort GitHub binary). A batch would be a wall of
        // links, so only single. Shown for dry-run too (it's read-only, and useful there).
        if single
            && let [b] = batch.as_slice()
            && let [c] = b.candidates.as_slice()
            && let Some(url) = engine.web_url(c)
        {
            renderer.info(&renderer.palette().dim(&crate::t!("install.web_url", url = url)));
        }

        if self.global.dry_run {
            renderer.info(&crate::t!("common.dry_run_not_installed"));
            self.grant_achievement("dry-runner", renderer);
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
        let skip_confirm = chose_interactively && least_trusted <= engine.config().install.default_yes_max_trust;
        if !skip_confirm
            && !prompt::confirm_install_batch(renderer, least_trusted, installed.len(), engine.config(), &flags)
        {
            renderer.info(&crate::t!("common.aborted"));
            return Ok(());
        }

        // 6. One escalation, one run; records are written as each plan succeeds.
        engine.install_batch(&batch, renderer).await?;
        renderer.success(&crate::t!("install.installed", names = installed.join(", ")));
        let pinned = specs.iter().any(|s| s.source.is_some());
        self.record_install(&batch, installed.len(), pinned, renderer);

        // 7. `--run`: start it. Last of all, because it replaces this process.
        if self.global.run
            && let [b] = batch.as_slice()
            && let [candidate] = b.candidates.as_slice()
        {
            self.launch(&engine, candidate, renderer).await;
        } else if self.global.run {
            renderer.warn(&crate::t!("install.run_single_only"));
        }
        Ok(())
    }

    /// `--run`: launch what was just installed, asking its source how (`flatpak run <id>` for a
    /// Flatpak, the program's own name everywhere else).
    ///
    /// The command is **verified to exist** before running: plenty of packages install no program
    /// at all (a font, a library, a plugin), and the honest answer there is to say so rather than
    /// to run something that isn't what the user meant. On success this `exec`s — JII is replaced
    /// by the app, so an interactive program (htop) owns the terminal outright and its exit code
    /// becomes JII's, exactly as if it had been typed. Nothing runs after, hence last.
    async fn launch(&self, engine: &Engine, candidate: &PackageCandidate, renderer: &Renderer) {
        let Some(argv) = engine.launch_command(candidate) else {
            renderer.warn(&crate::t!("install.run_unknown", name = candidate.name.clone()));
            return;
        };
        if !crate::provider::which(&argv[0]).await {
            renderer.warn(&crate::t!("install.run_unknown", name = candidate.name.clone()));
            return;
        }
        renderer.info(
            &renderer
                .palette()
                .dim(&crate::t!("install.run_starting", cmd = argv.join(" "))),
        );
        // exec(2) returns only on failure.
        use std::os::unix::process::CommandExt;
        let error = std::process::Command::new(&argv[0]).args(&argv[1..]).exec();
        renderer.error(&crate::t!(
            "install.run_failed",
            cmd = argv.join(" "),
            error = error.to_string()
        ));
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
    fn preview_batch_friendly(&self, batch: &[crate::engine::BatchPlan], engine: &Engine, renderer: &Renderer) {
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

    /// The GitHub-style by-name repo picker (ADR-0053): search the forges for `name`, show the
    /// top matches, and let the user pick one — with a "show more" entry that pages forever.
    /// Picking resolves the repo's latest release into an installable candidate (returned for
    /// the normal preview→confirm→install flow). Returns `None` on cancel, nothing found, or if
    /// every pick published no installable Linux binary. Only reached in an interactive session.
    async fn repo_picker(
        &self, engine: &Engine, name: &str, renderer: &Renderer,
    ) -> Option<crate::model::PackageCandidate> {
        let palette = renderer.palette();
        renderer.info(&crate::t!("install.gh_searching", name = name));

        // The query we actually page through. If the verbatim term finds nothing we may swap in
        // a typo-corrected variant below, and from then on paginate *that* corrected term.
        let mut query = name.to_string();
        let mut hits = engine.forge_repo_search(&query, 1).await;
        if hits.is_empty() {
            // Recover from a typo (`exeteragram` → `exteragram`): retry the forge with cheap
            // edit-distance-1 variants and take the first that finds anything.
            for variant in crate::engine::typo_variants(name) {
                let found = engine.forge_repo_search(&variant, 1).await;
                if !found.is_empty() {
                    renderer.info(&crate::t!("install.gh_corrected", name = name, fixed = variant.clone()));
                    query = variant;
                    hits = found;
                    break;
                }
            }
        }
        if hits.is_empty() {
            renderer.info(&crate::t!("install.gh_none", name = name));
            return None;
        }
        let mut page = 1u32;
        let mut maybe_more = hits.len() as u32 >= crate::provider::forge::REPO_SEARCH_PER_PAGE;

        loop {
            let mut labels: Vec<String> = hits.iter().map(|h| repo_label(h, palette)).collect();
            let more_index = maybe_more.then(|| {
                labels.push(palette.dim(&crate::t!("install.gh_show_more")));
                labels.len() - 1
            });
            let header = crate::t!("install.gh_picker_header", name = &query);

            match prompt::choose(renderer, &header, &labels, 0) {
                None => return None, // cancelled
                Some(i) if Some(i) == more_index => {
                    page += 1;
                    let more = engine.forge_repo_search(&query, page).await;
                    maybe_more = more.len() as u32 >= crate::provider::forge::REPO_SEARCH_PER_PAGE;
                    hits.extend(more);
                }
                Some(i) => {
                    let hit = hits[i].clone();
                    let resolved = engine.resolve_repo(&hit.source_id, &hit.slug).await;
                    match resolved.into_iter().next() {
                        Some(candidate) => return Some(candidate),
                        // The repo has no installable Linux asset — say so and let them re-pick.
                        None => renderer.warn(&crate::t!("install.gh_no_release", slug = hit.slug)),
                    }
                }
            }
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
            renderer.error(&crate::t!("parse.pin_unsupported", pin = r, name = spec.name.clone()));
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
    async fn remove(&self, packages: &[String], config: Config, renderer: &Renderer) -> crate::error::Result<()> {
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
            // The rejection (a bad `@ref`, an unknown `:source`) is already on screen as a red
            // ✗ — but it is still a refusal to do what was asked, so the run exits non-zero.
            None => return Err(crate::error::JiiError::AlreadyReported),
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
                    labels.push(crate::t!("remove.all_owners"));
                    let header = crate::t!("remove.multi_header", name = name);
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
        self.grant_achievement("cleaner", renderer);
        Ok(())
    }

    /// Batch preview for remove/update.
    ///
    /// Friendly mode gets one short line per package — `<name> (<version>) via <source>
    /// [needs sudo]` — mirroring the install preview (UX #6): the full Plan block for a plain
    /// `jii remove htop` was three times the size of the thing it described. `--dry-run` and
    /// Advanced still print every merged command, because inspecting them is the whole point
    /// there; JSON prints the plans as JSON.
    fn preview_record_batch(&self, batch: &[crate::engine::RecordBatchPlan], renderer: &Renderer) {
        if !renderer.is_friendly() || self.global.dry_run {
            for bp in batch {
                renderer.plan(&bp.plan);
            }
            return;
        }
        let palette = renderer.palette();
        for bp in batch {
            let sudo = if bp.plan.needs_root() {
                palette.dim(&crate::t!("common.needs_sudo"))
            } else {
                String::new()
            };
            for record in &bp.records {
                let version = record
                    .version
                    .as_ref()
                    .map(|v| format!(" {}", palette.version(&format!("({v})"))))
                    .unwrap_or_default();
                renderer.info(&format!(
                    "  {}{version} {} {}{sudo}",
                    record.name,
                    palette.dim(&crate::t!("common.via")),
                    palette.source(&record.source_id),
                ));
            }
        }
    }

    /// Update path (one, many, or all packages): for each recorded install, re-search its
    /// owning source for the latest version, skip provably-current packages, then let the
    /// engine group + merge same-source updates into one command where the source can
    /// (`dnf upgrade a b c`) and run them as **one** operation. No per-source branching —
    /// the engine resolves each record's provider (ADR-0004/0025). A named package that is
    /// not installed is reported; a package whose source can't update is warned and
    /// skipped, never cancelling the rest.
    async fn update(&self, packages: &[String], config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        // `jii update jii` (or `jii` among the names) updates JII **itself** — a self-managed
        // action, not a registry package. Handle it, then update any remaining names.
        let wants_self = packages.iter().any(|p| p == selfupdate::SELF_NAME);
        if wants_self {
            self.self_update(config.clone(), renderer, None).await?;
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
            // We already know our own version, so the "is there a newer JII?" GitHub check can
            // run **in parallel** with the (slower, interactive) system update — by the time the
            // system finishes, the release lookup is usually already done, so it feels instant.
            let prefetch = tokio::spawn(selfupdate::latest_release());
            self.update_system(config.clone(), renderer).await?;
            self.self_update(config, renderer, Some(prefetch)).await?;
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
                // Already reported; carry the non-zero status (see the install path).
                None => return Err(crate::error::JiiError::AlreadyReported),
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
        self.grant_achievement("fresh", renderer);
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
    async fn self_update(
        &self, config: Config, renderer: &Renderer,
        prefetch: Option<tokio::task::JoinHandle<crate::error::Result<selfupdate::Latest>>>,
    ) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let install = selfupdate::detect_install().await?;
        renderer.info(&crate::t!("selfupdate.checking"));
        // Use the release lookup started in parallel with the system update when available,
        // otherwise fetch it now (the direct `jii update jii` path).
        let fetched = match prefetch {
            Some(handle) => handle
                .await
                .unwrap_or_else(|e| Err(crate::error::JiiError::Other(anyhow::anyhow!(e.to_string())))),
            None => selfupdate::latest_release().await,
        };
        let latest = match fetched {
            Ok(l) => l,
            // A failed check is a failed command (exit ≠ 0) — scripts must be able to tell.
            Err(e) => {
                return Err(crate::error::JiiError::Other(anyhow::anyhow!(crate::t!(
                    "selfupdate.check_failed",
                    error = e.to_string()
                ))));
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
        // A pulled/rolled-back release reads as "different tag" too — say it's a downgrade.
        if selfupdate::looks_like_downgrade(&latest.tag) {
            renderer.warn(&crate::t!("selfupdate.maybe_downgrade"));
        }
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
        self.grant_achievement("self-made", renderer);
        // "Updated" alone doesn't tell you what you got — show the release notes (ADR-0079).
        self.show_update_changelog(install.exe(), selfupdate::current_version(), renderer)
            .await;
        Ok(())
    }

    /// After a successful self-update, print what the new version actually brought.
    ///
    /// The running binary only carries notes up to *its own* release, so it cannot describe
    /// the version it just installed. The new binary can — it is already on disk at the same
    /// path — so we ask it: `jii changelog --since <the version we were>`. Best-effort: if it
    /// can't be run, point at the command rather than ending on a bare "updated".
    async fn show_update_changelog(&self, exe: &std::path::Path, from: &str, renderer: &Renderer) {
        // JSON consumers get one document per command; a second one from a child process
        // would corrupt it.
        if renderer.is_json() {
            return;
        }
        let mut cmd = tokio::process::Command::new(exe);
        cmd.arg("changelog").arg("--since").arg(from);
        if self.global.no_color {
            cmd.arg("--no-color");
        }
        if let Some(lang) = &self.global.lang {
            cmd.arg("--lang").arg(lang);
        }
        renderer.info("");
        let shown = matches!(cmd.status().await, Ok(status) if status.success());
        if !shown {
            renderer.info(&crate::t!("changelog.after_update_hint"));
        }
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
        let covered: std::collections::HashSet<&str> = system.sources.iter().map(|s| s.as_str()).collect();

        // JII-tracked packages whose source offers no bulk update-all still get updated,
        // per-record, so a bare `jii update` misses nothing it installed.
        let fallback_records: Vec<InstalledRecord> = engine
            .registry()
            .installed()
            .iter()
            .filter(|r| !covered.contains(r.source_id.as_str()))
            .cloned()
            .collect();
        let (refreshed, transitions, up_to_date) = self.refresh_for_update(&engine, fallback_records).await;
        let fallback = engine
            .plan_record_batch(refreshed, crate::engine::RecordOp::Update)
            .await?;
        for (name, reason) in &fallback.unplannable {
            renderer.warn(&crate::t!(
                "update.cannot_plan",
                name = name.clone(),
                reason = reason.clone()
            ));
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
                let sudo = if plan.needs_root() {
                    crate::t!("common.needs_sudo")
                } else {
                    String::new()
                };
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
        self.grant_achievement("fresh", renderer);
        Ok(())
    }

    /// Re-search each record's owning source for its latest version, skip the provably-current
    /// ones (exact version-string match — conservative, so we only ever *skip* what is surely
    /// current), and return the post-update records (version set to the refreshed target),
    /// human `old → new` transition lines, and how many were already current. Shared by the
    /// named-package and system-fallback update paths; the engine stamps installed_at/
    /// verification on write.
    async fn refresh_for_update(
        &self, engine: &Engine, records: Vec<InstalledRecord>,
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
    async fn latest_from_source(&self, engine: &Engine, record: &InstalledRecord) -> Option<PackageCandidate> {
        let query = Query::name(&record.name);
        let mut ranked = engine.rank(&record.name, engine.search(&query).await.candidates);
        ranked.retain(|c| c.source_id == record.source_id);
        ranked.into_iter().next()
    }

    /// Search path: show ranked candidates for a query without installing anything.
    /// Read-only — same search→rank the install path uses, just rendered, not executed.
    async fn search(&self, terms: &[String], config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(self.apply_profile(config))?;
        if !self.ensure_usable_source(&engine, renderer).await {
            return Ok(());
        }
        // `search` is free-text discovery, not a package spec (ADR-0031) — the terms are the
        // query verbatim; `--source` still narrows the results.
        let name = terms.join(" ");
        let ranked = self
            .ranked_for(&engine, &name, self.global.source.as_ref(), renderer)
            .await;
        if ranked.is_empty() {
            renderer.error(&crate::t!("search.none", name = name));
            if let Some(msg) = engine.explain_miss(&name).await {
                renderer.info(&format!("  → {msg}"));
            }
            return Ok(());
        }
        // A search that actually surfaced something counts as exploring (unlocks silently in JSON).
        self.grant_achievement("explorer", renderer);
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
    async fn info(&self, package: &str, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(self.apply_profile(config))?;
        if !self.ensure_usable_source(&engine, renderer).await {
            return Ok(());
        }
        // Parse the argument as a package spec (ADR-0031): a `:source` narrows `info` to that
        // provider; `@ref` is rejected until version selection lands.
        let specs = match self.parse_specs(&[package.to_string()], renderer) {
            Some(specs) => specs,
            // The rejection (a bad `@ref`, an unknown `:source`) is already on screen as a red
            // ✗ — but it is still a refusal to do what was asked, so the run exits non-zero.
            None => return Err(crate::error::JiiError::AlreadyReported),
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
        let row = |label: &str, value: &str| format!("  {}{value}", palette.dim(&format!("{label:<11}")));
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
        renderer.info(&crate::t!("info.recommended", source = palette.source(&best.source_id)));
        let highlights = engine.candidate_highlights(best);
        let check = palette.good(palette.mark_ok());
        for reason in recommendation_reasons(best, highlights) {
            renderer.info(&format!("  {check} {reason}"));
        }
        Ok(())
    }

    /// Render an informational `Reference` card (ADR-0045): `jii info` for a name that isn't
    /// an installable program (e.g. an npm library). Shows what it is — description, links,
    /// and a clarifying note — with **no install phrasing**, keeping `info` purely a "show".
    fn render_reference(&self, card: &crate::model::Reference, renderer: &Renderer) -> crate::error::Result<()> {
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
        let row = |label: &str, value: &str| format!("  {}{value}", palette.dim(&format!("{label:<11}")));
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
            renderer.info(&format!("{} {note}", palette.mark_info()));
        }
        Ok(())
    }

    /// Sources path: list enabled providers and whether each is usable on this machine.
    /// Native managers for other distros (pacman on Fedora) are hidden unless `all` — a user
    /// shouldn't have to reason about a package manager their system doesn't have.
    async fn sources(&self, all: bool, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let full = engine.source_catalog().await;
        // A disabled source is dropped when the provider registry is built, so it is absent from
        // the catalog entirely — read the ids from config, or a source you turned off would be
        // invisible here and there'd be nothing to tell you how to turn it back on.
        let disabled = engine.config().sources.disabled.clone();
        // Which sources are ecosystem *managers* (bootstrappable/removable), by id → is it a
        // pure script install (brew/nix/AUR helper)? System repos aren't here, so they get no
        // add/remove hint — you don't uninstall the package manager your OS is built on.
        let managers: std::collections::HashMap<&str, bool> = engine
            .ecosystem_catalog()
            .await
            .into_iter()
            .map(|e| (e.id, matches!(e.bootstrap, crate::provider::Bootstrap::Script { .. })))
            .collect();
        let hidden = full.iter().filter(|e| !e.relevant).count();
        let shown: Vec<&crate::engine::SourceEntry> = full.iter().filter(|e| all || e.relevant).collect();

        if renderer.is_json() {
            let mut rows: Vec<_> = shown
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id, "trust": e.trust.label(),
                        "available": e.available, "relevant": e.relevant,
                        "enabled": true,
                        "manager": managers.contains_key(e.id),
                    })
                })
                .collect();
            // Disabled sources carry the same key set as enabled ones (a stable schema for
            // tooling); what a dropped provider can't report is an explicit null.
            rows.extend(disabled.iter().map(|id| {
                serde_json::json!({
                    "id": id, "trust": serde_json::Value::Null,
                    "available": false, "relevant": serde_json::Value::Null,
                    "enabled": false,
                    "manager": serde_json::Value::Null,
                })
            }));
            renderer.json_value(&serde_json::json!(rows));
            return Ok(());
        }

        let palette = renderer.palette();
        type Row<'a> = &'a crate::engine::SourceEntry;
        // A right-hand hint for an ecosystem manager: `[remove: …]` when installed,
        // `[add: …]` when not. Empty for system repos (not in `managers`).
        let hint = |e: &crate::engine::SourceEntry| -> String {
            if !managers.contains_key(e.id) {
                return String::new();
            }
            let key = if e.available {
                "sources.remove_hint"
            } else {
                "sources.add_hint2"
            };
            format!("  {}", palette.dim(&crate::t!(key, id = e.id)))
        };
        let (active, inactive): (Vec<Row>, Vec<Row>) = shown.into_iter().partition(|e| e.available);
        if !active.is_empty() {
            renderer.heading(&crate::t!("sources.active"));
            for e in &active {
                let mark = palette.good(palette.mark_ok());
                renderer.info(&format!(
                    "  {mark} {} ({}){}",
                    palette.source(&format!("{:8}", e.id)),
                    palette.trust(e.trust),
                    hint(e),
                ));
            }
        }
        if !inactive.is_empty() {
            renderer.heading(&crate::t!("sources.unavailable"));
            for e in &inactive {
                renderer.info(&format!(
                    "  {}{}",
                    palette.dim(&format!("{} {:8} ({})", palette.mark_bad(), e.id, e.trust.display())),
                    hint(e),
                ));
            }
        }
        // Sources you turned off yourself. Listed from config (they're absent from the catalog),
        // each with the exact command that brings it back.
        if !disabled.is_empty() {
            renderer.heading(&crate::t!("sources.disabled_header"));
            for id in &disabled {
                renderer.info(&format!(
                    "  {}  {}",
                    palette.dim(&format!("{} {id:8}", palette.mark_bad())),
                    palette.dim(&crate::t!("sources.enable_hint", id = id.clone())),
                ));
            }
        }
        // Nudge that some sources were hidden, so `--all` is discoverable.
        if !all && hidden > 0 {
            renderer.info("");
            renderer.info(&palette.dim(&crate::t!("sources.hidden", count = hidden)));
        }
        // Turning a source off is the one thing this view could never tell you about (the ask:
        // "how do I disable a repository?" — the answer existed, nothing pointed at it).
        renderer.info("");
        renderer.info(&palette.dim(&crate::t!("sources.toggle_hint")));
        Ok(())
    }

    /// `jii sources disable|enable <id>` — flip a source's `[sources] disabled` entry and save.
    /// A disabled source is dropped when the provider registry is built, so JII stops searching
    /// it everywhere at once. The id is validated so a typo fails loudly instead of silently.
    fn sources_set_enabled(
        &self, id: &str, enable: bool, mut config: Config, renderer: &Renderer,
    ) -> crate::error::Result<()> {
        if !crate::config::KNOWN_SOURCES.contains(&id) {
            renderer.error(&crate::t!("sources.unknown", id = id));
            renderer.info(&crate::t!(
                "sources.known",
                list = crate::config::KNOWN_SOURCES.join(", ")
            ));
            return Ok(());
        }
        let currently_enabled = config.is_enabled(id);
        if enable == currently_enabled {
            let key = if enable {
                "sources.already_enabled"
            } else {
                "sources.already_disabled"
            };
            renderer.info(&crate::t!(key, id = id));
            return Ok(());
        }
        if enable {
            config.sources.disabled.retain(|s| s != id);
        } else {
            config.sources.disabled.push(id.to_string());
        }
        config.save()?;
        let key = if enable {
            "sources.enabled"
        } else {
            "sources.disabled_ok"
        };
        renderer.success(&crate::t!(key, id = id));
        Ok(())
    }

    /// `jii sources add <id>` — bootstrap a missing ecosystem manager. Two honest paths, no
    /// magic: a manager that lives in the distro repos (npm, cargo, go, pipx, flatpak, snap) is
    /// resolved cross-distro (`nodejs-npm` on Fedora, `npm` elsewhere) and handed to the
    /// **normal install path** — same preview → confirm → execute → record as any package. A
    /// manager that bootstraps via its own upstream script (Homebrew, Nix, an AUR helper) is
    /// **shown, never run** — JII does not pipe an installer into your shell (ADR-0005/0006).
    async fn sources_add(&self, id: &str, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        // `jii sources add yay|paru` — an AUR helper, only meaningful on Arch.
        if matches!(id, "yay" | "paru") {
            return self.add_aur_helper(id, renderer).await;
        }
        let engine = Engine::new(config.clone())?;
        let catalog = engine.ecosystem_catalog().await;

        let Some(eco) = catalog.iter().find(|e| e.id == id) else {
            renderer.error(&crate::t!("providers.unknown", name = id));
            let known: Vec<_> = catalog.iter().map(|e| e.id).collect();
            renderer.info(&crate::t!("providers.known", names = known.join(", ")));
            return Ok(());
        };
        self.bootstrap_ecosystem(&engine, eco, config, renderer).await
    }

    /// `jii sources add yay|paru` — an AUR helper is built from a PKGBUILD via `makepkg`, which
    /// JII will **show, never run** (the same trust boundary as the brew/nix installer scripts).
    /// If a helper is already present, say so. Arch-only: refuse elsewhere with a clear note.
    async fn add_aur_helper(&self, helper: &str, renderer: &Renderer) -> crate::error::Result<()> {
        if !crate::platform::Platform::detect().arch_like {
            renderer.error(&crate::t!("aur.not_arch", helper = helper.to_string()));
            return Ok(());
        }
        if crate::provider::which(helper).await {
            renderer.success(&crate::t!("aur.helper_present", helper = helper.to_string()));
            return Ok(());
        }
        renderer.info(&crate::t!("aur.helper_intro", helper = helper.to_string()));
        renderer.info(&format!("  git clone https://aur.archlinux.org/{helper}-bin.git",));
        renderer.info(&format!("  cd {helper}-bin && makepkg -si"));
        Ok(())
    }

    /// `jii sources remove <id>` — uninstall an ecosystem manager from the system. Refuses the
    /// system package managers (dnf/apt/pacman…): removing them would break the OS. A
    /// script-installed manager (Homebrew, Nix) can't be auto-removed — its own uninstall is
    /// pointed to. A repo-provided manager (flatpak/snap/cargo/pipx/go) is removed through the
    /// host's system package manager, with the **exact elevated command shown first** and a
    /// default-no confirmation. AUR helpers (yay/paru) are removed via pacman.
    async fn sources_remove(&self, id: &str, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        // AUR helpers are ordinary pacman packages — remove them directly.
        if matches!(id, "yay" | "paru") {
            return self.remove_via_pacman(id, renderer).await;
        }
        let engine = Engine::new(config.clone())?;
        let catalog = engine.ecosystem_catalog().await;

        let Some(eco) = catalog.iter().find(|e| e.id == id) else {
            // Not an ecosystem manager. If it's a *known* source, it's a system manager we
            // refuse to uninstall; otherwise it's just an unknown id.
            if crate::config::KNOWN_SOURCES.contains(&id) {
                renderer.error(&crate::t!("sources.remove_system", id = id));
            } else {
                renderer.error(&crate::t!("sources.unknown", id = id));
                let known: Vec<_> = catalog.iter().map(|e| e.id).collect();
                renderer.info(&crate::t!("providers.known", names = known.join(", ")));
            }
            return Ok(());
        };
        if !eco.installed {
            renderer.info(&crate::t!("sources.remove_not_installed", label = eco.label));
            return Ok(());
        }
        let names = match eco.bootstrap {
            // A script-installed manager (brew/nix/AUR): JII can't cleanly uninstall it.
            crate::provider::Bootstrap::Script { .. } => {
                renderer.info(&crate::t!("sources.remove_script", label = eco.label));
                return Ok(());
            }
            crate::provider::Bootstrap::Packages(names) => names,
        };
        let Some(mgr) = detect_system_manager().await else {
            renderer.error(&crate::t!("sources.remove_no_system_mgr", label = eco.label));
            return Ok(());
        };
        // Only remove the package(s) actually installed on this host, so we never guess-remove
        // a wrong name (go is `golang` on Fedora, `go` on Arch, `golang-go` on Debian).
        let mut installed = Vec::new();
        for n in names {
            if mgr.pkg_installed(n).await {
                installed.push((*n).to_string());
            }
        }
        if installed.is_empty() {
            renderer.warn(&crate::t!(
                "sources.remove_unknown_pkg",
                label = eco.label,
                names = names.join(", ")
            ));
            return Ok(());
        }
        self.run_system_remove(&mgr, &installed, eco.label, config, renderer)
            .await
    }

    /// Remove an AUR helper (yay/paru) via pacman — the exact elevated command shown first,
    /// default-no confirmation. Arch-only; a no-op with a note if it isn't installed.
    async fn remove_via_pacman(&self, helper: &str, renderer: &Renderer) -> crate::error::Result<()> {
        if !crate::provider::which(helper).await {
            renderer.info(&crate::t!("sources.remove_not_installed", label = helper.to_string()));
            return Ok(());
        }
        let argv = vec![
            "pacman".to_string(),
            "-Rs".to_string(),
            "--noconfirm".to_string(),
            helper.to_string(),
        ];
        self.confirm_and_run_removal(&argv, true, helper, renderer).await
    }

    /// Show the elevated system-manager removal command, confirm (default no), then run it.
    async fn run_system_remove(
        &self, mgr: &SysManager, pkgs: &[String], label: &str, _config: Config, renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let (argv, needs_root) = mgr.remove_argv(pkgs);
        self.confirm_and_run_removal(&argv, needs_root, label, renderer).await
    }

    /// Shared tail of every manager removal: print the exact elevated command, honour
    /// `--dry-run`, ask a default-no confirmation, then run it through the privilege layer.
    async fn confirm_and_run_removal(
        &self, argv: &[String], needs_root: bool, label: &str, renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let privilege = crate::privilege::Privilege::detect();
        let shown = privilege.elevated_argv(argv, needs_root);
        renderer.warn(&crate::t!("sources.remove_confirm", label = label.to_string()));
        renderer.info(&format!("  {}", shown.join(" ")));
        if self.global.dry_run {
            renderer.info(&crate::t!("common.dry_run_unchanged"));
            return Ok(());
        }
        let flags = self.prompt_flags(false);
        if !prompt::confirm(renderer, &crate::t!("sources.remove_prompt"), false, &flags) {
            renderer.info(&crate::t!("common.aborted"));
            return Ok(());
        }
        privilege.prime().await?;
        match privilege.run(argv, needs_root).await {
            Ok(()) => renderer.success(&crate::t!("sources.removed", label = label.to_string())),
            Err(e) => renderer.error(&crate::t!(
                "sources.remove_failed",
                label = label.to_string(),
                error = e.to_string()
            )),
        }
        Ok(())
    }

    /// Split ecosystem-manager names out of an install request and bootstrap each (#4),
    /// returning the remaining ordinary packages. A name counts as a manager only when it is
    /// unpinned (no `:source`, no `--source`) and matches a known ecosystem id; a cheap pure
    /// id check means an ordinary `jii vlc` pays nothing (no catalog probe).
    async fn route_managers(
        &self, engine: &Engine, packages: &[String], config: Config, renderer: &Renderer,
    ) -> crate::error::Result<Vec<String>> {
        let ids = engine.ecosystem_ids();
        let pinned_globally = self.global.source.is_some();
        let bare_name = |p: &str| p.split([':', '@']).next().unwrap_or(p).to_string();
        // `yay`/`paru` are AUR helpers (Arch-only) — bare-name manager requests like any other.
        let arch_like = crate::platform::Platform::detect().arch_like;
        let is_helper = move |name: &str| arch_like && matches!(name, "yay" | "paru");
        // A version pin (`npm@1.2` — any `@` past a leading npm-scope one) opts out of the
        // manager route: swallowing the `@ref` here would silently install "latest" when the
        // user asked for a version. Left alone, the token reaches parse_specs, which rejects
        // pins with a clear "not supported yet" instead.
        let has_pin = |p: &str| p.chars().skip(1).any(|c| c == '@');
        let is_manager_name = |p: &str| {
            !pinned_globally
                && !p.contains(':')
                && !has_pin(p)
                && (ids.iter().any(|id| *id == bare_name(p)) || is_helper(&bare_name(p)))
        };

        // Common case (no manager among the names): return untouched, no catalog I/O.
        if !packages.iter().any(|p| is_manager_name(p)) {
            return Ok(packages.to_vec());
        }

        let catalog = engine.ecosystem_catalog().await;
        let mut rest = Vec::new();
        for p in packages {
            let name = bare_name(p);
            if is_helper(&name) {
                self.add_aur_helper(&name, renderer).await?;
            } else if is_manager_name(p)
                && let Some(eco) = catalog.iter().find(|e| e.id == name)
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
        &self, engine: &Engine, eco: &crate::engine::EcosystemStatus, config: Config, renderer: &Renderer,
    ) -> crate::error::Result<()> {
        if eco.installed {
            renderer.success(&crate::t!("providers.already_installed", label = eco.label));
            return Ok(());
        }
        let label = eco.label;
        match eco.bootstrap {
            Bootstrap::Packages(names) => {
                renderer.info(&crate::t!("providers.looking", label = label));
                match engine.first_bootstrap_package(names).await {
                    // The source is pinned (`pipx:apt`) because only a *usable* source can set a
                    // manager up — never the absent manager itself (ADR-0066). route_managers=
                    // false: the bootstrap package's name (e.g. `pipx`) may itself be a manager
                    // id — routing it would loop. Box::pin breaks the async recursion cycle.
                    Some((pkg, source)) => {
                        let spec = format!("{pkg}:{source}");
                        Box::pin(self.install_inner(&[spec], config, renderer, false, false)).await?;
                        if self.global.dry_run {
                            // Everything is previewable: show the finishing steps too, since
                            // they are part of what `jii sources add` would really do.
                            if let Some(plan) = engine.post_bootstrap_plan(eco.id).await? {
                                renderer.info(&crate::t!("providers.then_setup", label = label));
                                self.preview_self_plan(&plan, renderer);
                            }
                            return Ok(());
                        }
                        // Installing the package is not the same as having the manager (a
                        // Flatpak with no remote, a snapd whose socket is off) — finish the job
                        // (ADR-0080). If the user declined the install, say what that means
                        // instead of ending on a bare "Aborted."
                        if engine.source_available(eco.id).await {
                            engine.finish_bootstrap(eco.id, renderer).await?;
                            renderer.success(&crate::t!("install.bootstrap_ready", manager = label));
                            self.grant_achievement("bootstrapper", renderer);
                        } else {
                            renderer.info(&crate::t!("providers.not_set_up", label = label, id = eco.id));
                        }
                        Ok(())
                    }
                    None => {
                        renderer.error(&crate::t!("providers.not_found", label = label));
                        renderer.info(&crate::t!("providers.tried", names = names.join(", ")));
                        Ok(())
                    }
                }
            }
            // No distro package exists for this one — its own upstream script is the install
            // path. Shown in full, run only on an explicit answer (ADR-0066).
            Bootstrap::Script { cmd, shell } => {
                if self
                    .offer_script_bootstrap(engine, eco.id, label, cmd, shell, renderer)
                    .await
                {
                    self.grant_achievement("bootstrapper", renderer);
                }
                Ok(())
            }
        }
    }

    /// T6 (ADR-0065): set up any *uninstalled* ecosystem manager a chosen candidate depends on,
    /// then keep the candidate so it installs through the now-present manager. A `can_search`
    /// source (Flatpak, Snap, cargo, npm, pipx, go, brew) can answer a search without its CLI, so
    /// an uninstalled-Flatpak `obsidian` outranks the last-resort GitHub binary — but its install
    /// command would fail. Rather than fall through to GitHub, we bootstrap the manager first.
    ///
    /// Per distinct manager (asked once, not once per app): a `Packages` manager (flatpak/snap/
    /// cargo/…) is offered for setup (default yes) and installed via the normal path; a `Script`
    /// manager (brew/nix) is **shown, never run** (ADR-0005/0006), so its apps are skipped with a
    /// note. Candidates whose manager is already present, or isn't an ecosystem at all (github),
    /// pass through untouched. Returns the survivors.
    async fn bootstrap_missing_managers(
        &self, engine: &Engine, chosen: Vec<PackageCandidate>, config: &Config, renderer: &Renderer, assume_yes: bool,
    ) -> crate::error::Result<Vec<PackageCandidate>> {
        let eco = engine.ecosystem_catalog().await;
        let status_of = |id: &str| eco.iter().find(|e| e.id == id);

        // Fast path: nothing chosen needs an absent manager → leave the batch untouched.
        if !chosen
            .iter()
            .any(|c| status_of(&c.source_id).is_some_and(|e| !e.installed))
        {
            return Ok(chosen);
        }

        let effective_auto = self.global.auto || config.install.auto;
        let flags = self.prompt_flags(effective_auto).with_yes(assume_yes);
        // Decide once per manager (a batch may want several apps from the same one).
        let mut decided: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
        let mut survivors: Vec<PackageCandidate> = Vec::with_capacity(chosen.len());

        for cand in chosen {
            let Some(status) = status_of(&cand.source_id) else {
                survivors.push(cand); // not an ecosystem manager (e.g. github) — unaffected
                continue;
            };
            if status.installed {
                survivors.push(cand);
                continue;
            }
            if let Some(&ok) = decided.get(&cand.source_id) {
                if ok {
                    survivors.push(cand);
                }
                continue;
            }

            let manager = status.label;
            renderer.info("");
            renderer.info(&crate::t!(
                "install.bootstrap_needed",
                app = cand.name.clone(),
                manager = manager
            ));
            let ok = match status.bootstrap {
                // brew/nix: no distro package exists, so their own upstream script *is* the
                // install path. Shown in full and run only on an explicit answer (ADR-0066).
                Bootstrap::Script { cmd, shell } => {
                    self.offer_script_bootstrap(engine, &cand.source_id, manager, cmd, shell, renderer)
                        .await
                }
                // flatpak/snap/cargo/…: install the manager's OS package, then wire up any default
                // remote it needs. In dry-run we preview the setup and keep the app so its plan is
                // shown too, without touching the system.
                Bootstrap::Packages(names) => {
                    let question = crate::t!("install.bootstrap_confirm", manager = manager, app = cand.name.clone());
                    if !self.global.dry_run && !prompt::confirm(renderer, &question, true, &flags) {
                        false
                    } else {
                        self.set_up_manager(engine, &cand.source_id, manager, names, config.clone(), renderer)
                            .await?
                    }
                }
            };
            decided.insert(cand.source_id.clone(), ok);
            if ok && !self.global.dry_run {
                // JII just set up a manager that wasn't here before — the T6 bootstrap path.
                self.grant_achievement("bootstrapper", renderer);
            }
            if ok {
                survivors.push(cand);
            } else {
                renderer.info(&crate::t!("install.bootstrap_skipped_app", app = cand.name.clone()));
            }
        }
        Ok(survivors)
    }

    /// Offer to run an ecosystem manager's own upstream installer script (Homebrew, Nix).
    ///
    /// These managers ship no distro package, so the script **is** the only install path — refusing
    /// outright just dead-ends the user (ADR-0066). But it is remote code JII can neither preview
    /// nor verify, so it never runs on anything but an explicit human answer: the exact command is
    /// shown first, `--auto`/`--yes` deliberately do **not** stand in for consent (CLAUDE.md's
    /// "auto mode never installs untrusted automatically"), and a non-interactive session only ever
    /// prints it. The prompt itself defaults to yes — the user did ask for this manager.
    ///
    /// Returns whether the manager is usable afterwards (a fresh Homebrew often isn't on this
    /// shell's PATH yet, which is reported rather than silently failing the dependent install).
    async fn offer_script_bootstrap(
        &self, engine: &Engine, source_id: &str, manager: &str, cmd: &str, shell: Option<crate::provider::ShellSetup>,
        renderer: &Renderer,
    ) -> bool {
        renderer.info(&crate::t!("install.script_intro", manager = manager));
        renderer.info(&format!("  {cmd}"));
        if self.global.dry_run {
            renderer.info(&crate::t!("common.dry_run_unchanged"));
            return false;
        }
        // Remote code: an explicit human answer or nothing at all.
        if self.global.no || !self.interactive(renderer) {
            renderer.info(&crate::t!("install.script_manual", manager = manager));
            return false;
        }
        let flags = prompt::PromptFlags {
            auto: false,
            yes: false,
            no: false,
        };
        let question = crate::t!("install.script_confirm", manager = manager);
        if !prompt::confirm(renderer, &question, true, &flags) {
            renderer.info(&crate::t!("install.script_manual", manager = manager));
            return false;
        }
        // `bash -c` (not `sh`): the upstream one-liners use bash-isms — Nix's `<(curl …)` process
        // substitution, Homebrew's own `/bin/bash -c "$(…)"`. Never elevated: these installers ask
        // for sudo themselves, exactly as they would if the user pasted the line (privilege.rs owns
        // JII's own escalation, and this isn't JII's command).
        let argv = vec!["bash".to_string(), "-c".to_string(), cmd.to_string()];
        if let Err(e) = run_plain_command(&argv).await {
            renderer.error(&crate::t!(
                "install.script_failed",
                manager = manager,
                error = e.to_string()
            ));
            return false;
        }
        if !engine.source_available(source_id).await {
            renderer.warn(&crate::t!("install.script_no_path", manager = manager));
            return false;
        }
        renderer.success(&crate::t!("install.bootstrap_ready", manager = manager));
        // JII can drive the manager by its absolute path, but the *user's* shell still can't
        // see it — Homebrew ends by printing a line for them to paste. Offer to add it for
        // them instead of leaving homework behind (ADR-0080).
        if let Some(setup) = shell {
            self.offer_shell_line(manager, setup, renderer);
        }
        true
    }

    /// Offer to append a manager's shell line (`eval "$(brew shellenv)"`) to the user's shell
    /// rc, so `brew` works in their terminal too — not just inside JII. Shown in full first and
    /// written only on an explicit yes: this edits a file JII doesn't own. Silent when the line
    /// is already there, when the binary can't be located, or in a non-interactive session
    /// (where the line is printed to paste instead — never a dead end).
    fn offer_shell_line(&self, manager: &str, setup: crate::provider::ShellSetup, renderer: &Renderer) {
        let Some(bin) = crate::provider::first_existing(setup.bins) else {
            return;
        };
        let line = setup.rc_line.replace("{bin}", &bin);
        let Some(rc) = crate::shellrc::rc_file() else {
            // An unsupported shell (fish syntax differs): show the line, don't guess a file.
            renderer.info(&crate::t!("install.shell_manual", line = line.clone()));
            return;
        };
        if crate::shellrc::already_present(&rc, &line) {
            return;
        }
        renderer.info("");
        renderer.info(&crate::t!(
            "install.shell_intro",
            manager = manager,
            file = rc.display().to_string()
        ));
        renderer.info(&format!("  {line}"));
        let flags = self.prompt_flags(self.global.auto);
        if !self.interactive(renderer) || !prompt::confirm(renderer, &crate::t!("install.shell_confirm"), true, &flags)
        {
            renderer.info(&crate::t!("install.shell_manual", line = line.clone()));
            return;
        }
        match crate::shellrc::append_line(&rc, manager, &line) {
            Ok(()) => renderer.success(&crate::t!("install.shell_added", file = rc.display().to_string())),
            Err(e) => {
                renderer.warn(&crate::t!("install.shell_failed", error = e.to_string()));
                renderer.info(&crate::t!("install.shell_manual", line = line.clone()));
            }
        }
    }

    /// Install an ecosystem manager's OS package (the first of `names` that resolves on this host)
    /// through the normal install path, then wire up any default remote it needs, and report
    /// whether it is usable afterwards. Consent was already given by the T6 prompt, so the package
    /// install runs with `assume_yes`. In dry-run it only previews and returns `true` (so the app's
    /// own plan is previewed too). Returns `false` — dropping the dependent app — if no package
    /// resolves or the manager still isn't present after install.
    async fn set_up_manager(
        &self, engine: &Engine, source_id: &str, manager: &str, names: &[&'static str], config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<bool> {
        let Some((pkg, from)) = engine.first_bootstrap_package(names).await else {
            renderer.error(&crate::t!("install.bootstrap_no_pkg", manager = manager, app = manager));
            return Ok(false);
        };
        renderer.info(&crate::t!(
            "install.bootstrap_installing",
            manager = manager,
            source = from.clone()
        ));
        // The source is pinned (`pipx:apt`) so the manager is set up by a source that actually
        // works here — never by the absent manager itself, and never via a chooser the user
        // didn't ask for (ADR-0066). route_managers=false: `pkg` (e.g. "flatpak") is itself a
        // manager id; routing it would loop straight back into bootstrap. Box::pin breaks the
        // async-recursion cycle.
        let spec = format!("{pkg}:{from}");
        Box::pin(self.install_inner(&[spec], config.clone(), renderer, true, false)).await?;

        // Dry-run never actually installed anything — assume success so the app plan is previewed.
        if self.global.dry_run {
            return Ok(true);
        }
        if !engine.source_available(source_id).await {
            return Ok(false);
        }
        // The package alone rarely *is* the manager: Flatpak has no remote yet, snapd's socket
        // is off. Each source declares its own finishing steps, so this stays source-agnostic
        // (ADR-0080) instead of the `if source_id == "flatpak"` special case it replaces.
        engine.finish_bootstrap(source_id, renderer).await?;
        renderer.success(&crate::t!("install.bootstrap_ready", manager = manager));
        Ok(true)
    }

    /// `jii lang [code]` — show or persist the interface language. With no argument it prints
    /// the saved setting and the choices; with `en`/`ru`/`auto` it writes `[ui] locale` to the
    /// config so the choice sticks across runs (the global `--lang` stays a per-run override).
    fn lang(&self, code: Option<&str>, mut config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        const SUPPORTED: &[&str] = &["auto", "en", "ru"];
        match code {
            None => {
                renderer.info(&crate::t!("lang.current", lang = config.ui.locale.clone()));
                renderer.info(&crate::t!("lang.available", list = SUPPORTED.join(", ")));
            }
            Some(raw) => {
                let c = raw.trim().to_ascii_lowercase();
                if !SUPPORTED.contains(&c.as_str()) {
                    renderer.error(&crate::t!(
                        "lang.unknown",
                        code = c.clone(),
                        list = SUPPORTED.join(", ")
                    ));
                    return Ok(());
                }
                config.ui.locale = c.clone();
                config.save()?;
                // The active language is fixed for this process, so confirm in the language
                // just chosen (it takes effect for real on the next run).
                renderer.success(&crate::i18n::tr_in(&c, "lang.set", &[("lang", c.clone())]));
                self.grant_achievement("translator", renderer);
            }
        }
        Ok(())
    }

    /// `jii cache [clear]` — show the on-disk search-cache path, or delete it. Clearing just
    /// removes the file; JII rebuilds it on the next search, so it's always safe.
    fn cache(&self, action: Option<&CacheAction>, renderer: &Renderer) -> crate::error::Result<()> {
        match action {
            None => match crate::cache::Cache::path() {
                Some(p) => {
                    let mut line = crate::t!("cache.path", path = p.display().to_string());
                    if !p.exists() {
                        line.push_str(&format!(" ({})", crate::t!("cache.absent")));
                    }
                    renderer.info(&line);
                }
                None => renderer.info(&crate::t!("cache.no_path")),
            },
            Some(CacheAction::Clear) => match crate::cache::Cache::clear_disk() {
                Ok(Some(p)) => renderer.success(&crate::t!("cache.cleared", path = p.display().to_string())),
                Ok(None) => renderer.info(&crate::t!("cache.already_empty")),
                Err(e) => renderer.error(&crate::t!("cache.clear_failed", error = e.to_string())),
            },
        }
        Ok(())
    }

    /// The first-run wizard (and `jii setup`). Warm, short, jargon-free — written for someone
    /// who just opened a terminal. `first_run` is true when it fires automatically on the very
    /// first bare `jii`; then a decline is honored *and* still marks first-run done so it never
    /// nags again. It only asks and only changes the config it saves — it never touches the
    /// system without consent (the optional `doctor` it offers is read-only today; the
    /// system-helping doctor lands in U6).
    /// `offer_doctor` gates the "run a system check now?" step: it's `false` only when the
    /// invocation that triggered onboarding *is* `jii doctor`, so the real doctor runs once
    /// afterwards instead of being offered here and then again by the dispatched command.
    async fn setup(
        &self, mut config: Config, renderer: &Renderer, first_run: bool, offer_doctor: bool,
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
            &[crate::t!("setup.detail_friendly"), crate::t!("setup.detail_advanced")],
            0,
        ) {
            Some(1) => crate::config::OutputMode::Advanced,
            _ => crate::config::OutputMode::Friendly,
        };
        config.ui.mode = mode;

        // Step 2 — optional system check + setup. `doctor` is interactive: it diagnoses,
        // then offers to set up what's missing (each item a separate yes/no — the user stays
        // in control, and can skip every one with Enter). Skipped when the triggering command
        // is itself `jii doctor` — it will run right after, so we don't offer it twice.
        if offer_doctor && prompt::confirm(renderer, &crate::t!("setup.run_doctor_q"), true, &flags) {
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
        // Only a wizard run carried through to the end earns the hat — declining at the first
        // question returns above, before this point.
        self.grant_achievement("wizard", renderer);
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
        &self, engine: &Engine, name: &str, source: Option<&String>, renderer: &Renderer,
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

    /// Explain how a package was — **or would be** — installed. Three cases, in order:
    ///
    /// 1. JII's own ledger has it: every record (a package can be installed from more than one
    ///    source), with the date it went in.
    /// 2. The ledger has nothing, but the package *is* on the system — installed by hand, by the
    ///    distro, or before JII existed. Answer with what the system says: which manager owns it,
    ///    which version, how far that source is trusted. `jii remove` has always been able to
    ///    remove these, so refusing to explain them was both a dead end and a contradiction.
    /// 3. Not installed at all: explain how JII *would* install it — which is the other half of
    ///    what this command promises, and used to be missing entirely.
    async fn how(&self, package: &str, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let records = engine.registry().get_all(package);
        if records.is_empty() {
            return self.how_unrecorded(&engine, package, renderer).await;
        }
        for record in records {
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
            let ok = renderer.palette().mark_ok();
            renderer.info(&format!("  {ok} {}", crate::t!("how.version_line", version = version)));
            renderer.info(&format!("  {ok} {}", crate::t!("how.trust_line", trust = trust)));
        }
        self.grant_achievement("paper-trail", renderer);
        Ok(())
    }

    /// `how` for a package JII has no record of: either the system owns it, or nobody does.
    /// Never ends on "no record" alone — that was a dead end, and ADR-0080's rule is that a
    /// refusal must carry the next step.
    async fn how_unrecorded(&self, engine: &Engine, package: &str, renderer: &Renderer) -> crate::error::Result<()> {
        let palette = renderer.palette();
        let ok = palette.mark_ok();

        // Case 2 — the system has it, JII just didn't put it there.
        let owners = engine.resolve_all_installed(package).await;
        if !owners.is_empty() {
            for record in &owners {
                let trust = engine
                    .source_trust(&record.source_id)
                    .map(|t| t.display())
                    .unwrap_or_else(|| crate::t!("common.unknown"));
                renderer.info(&crate::t!(
                    "how.system_owned",
                    package = package,
                    source = record.source_id.clone()
                ));
                renderer.info(&format!(
                    "  {ok} {}",
                    crate::t!(
                        "how.version_line",
                        version = version_or_unknown(record.version.as_ref())
                    )
                ));
                renderer.info(&format!("  {ok} {}", crate::t!("how.trust_line", trust = trust)));
            }
            renderer.info(&palette.dim(&crate::t!("how.not_by_jii", package = package)));
            self.grant_achievement("paper-trail", renderer);
            return Ok(());
        }

        // Case 3 — not installed anywhere: say how it *would* go in.
        let ranked = self.ranked_for(engine, package, None, renderer).await;
        let Some(best) = ranked.first() else {
            renderer.warn(&crate::t!("how.nowhere", package = package));
            renderer.info(&palette.dim(&crate::t!("how.nowhere_hint", package = package)));
            return Ok(());
        };
        renderer.info(&crate::t!(
            "how.would_install",
            package = package,
            source = best.source_id.clone()
        ));
        renderer.info(&format!(
            "  {ok} {}",
            crate::t!("how.version_line", version = version_or_unknown(best.version.as_ref()))
        ));
        renderer.info(&format!(
            "  {ok} {}",
            crate::t!("how.trust_line", trust = best.trust.display())
        ));
        renderer.info(&palette.dim(&crate::t!("how.would_hint", package = package)));
        self.grant_achievement("paper-trail", renderer);
        Ok(())
    }

    /// List software installed via jii.
    fn list(&self, audit: bool, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        // `jii list --audit` is the security view (#5): the same ledger, but with trust,
        // verification, and concerns per install. Folded in from the former `jii audit`.
        if audit {
            let out = self.audit_view(&engine, renderer);
            self.grant_achievement("auditor", renderer);
            return out;
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
        self.grant_achievement("doctor", renderer);
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

        // Where the config lives — a common ask ("how do I change the language / defaults?").
        // Shown whether or not the file exists yet, so the user knows exactly what to create.
        if let Some(p) = crate::config::Config::default_path() {
            let mut line = crate::t!("doctor.config_line", path = p.display().to_string());
            if !p.exists() {
                line.push_str(&format!(" ({})", crate::t!("doctor.config_absent")));
            }
            renderer.info(&palette.dim(&line));
            renderer.info("");
        }

        renderer.heading(&crate::t!("doctor.sources_header"));
        for d in &diagnostics {
            let mark = if d.available {
                palette.good(palette.mark_ok())
            } else {
                palette.dim(palette.mark_bad())
            };
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

        // GitHub with no token is throttled to 60 req/h — the single biggest reliability
        // papercut for the last-resort GitHub source. Give the same token guidance `setup`
        // does (owner wanted it in `doctor` too), but only when it's actionable: GitHub is
        // actually in play here and no token is set (otherwise the source line already shows
        // the 5000-req budget, and repeating it would just be noise).
        let gh_present = diagnostics.iter().any(|d| d.id == "github");
        let has_token = std::env::var(&token_env).is_ok_and(|v| !v.is_empty());
        if gh_present && !has_token {
            renderer.info("");
            self.github_token_help(&config_for_fix, renderer);
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
    /// (RPM Fusion, codecs, fonts, …) — asks a plain yes/no (Enter = accept, default yes), and on
    /// "yes" applies it immediately. The single question is the consent, so installs don't ask
    /// twice (`with_yes`); the trust barrier (ADR-0006) still gates anything untrusted.
    /// `--dry-run` shows what each "yes" *would* do without changing anything.
    async fn doctor_offer(
        &self, engine: &Engine, checks: &[SystemCheck], config: Config, renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let fixes: Vec<(&SystemCheck, &Fix)> = checks
            .iter()
            .filter_map(|c| c.fix.as_ref().filter(|_| !c.ok).map(|f| (c, f)))
            .collect();

        let catalog = crate::recommend::Catalog::load().ok();
        let distro_id = crate::platform::Platform::detect().distro.id();
        let all_suggestions = catalog.as_ref().map(|c| c.for_distro(distro_id)).unwrap_or_default();

        // Analyse the system first (#1): drop suggestions the user has already done, so
        // doctor is real diagnostics — not a canned list. One installed-scan for the batch.
        let installed = engine.installed_index().await;
        // Keep `all_suggestions` alive: a chosen entry may name a prerequisite (`requires`)
        // that itself was already satisfied and filtered out of `suggestions`, so we look
        // prerequisites up in the full list (ADR-0055).
        let suggestions: Vec<&crate::recommend::Recommendation> = all_suggestions
            .iter()
            .copied()
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
            if !prompt::confirm(renderer, &format!("  {question}"), true, &flags) {
                continue;
            }
            self.apply_fix(fix, config.clone(), renderer).await?;
        }

        // B) Curated, distro-aware suggestions (the folded-in recommend catalog).
        //    A dependent entry (codecs/VLC) enables its prerequisite repo (RPM Fusion) first,
        //    so it never fails with a bare "not found" because the repo was skipped (ADR-0055).
        let mut enabled_repos: std::collections::HashSet<String> = std::collections::HashSet::new();
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
            if !prompt::confirm(renderer, &format!("    {question}"), true, &flags) {
                continue;
            }
            // Enable a prerequisite repo first (e.g. RPM Fusion for codecs/VLC). The exact
            // command is shown before it runs (apply_suggestion prints it); the "yes" to the
            // dependent is the consent for its prerequisite. Deduped within the run, and
            // skipped when the prerequisite is already present (pure decision in `recommend`).
            if let Some(prereq) = crate::recommend::prerequisite(r, &all_suggestions, &installed, &enabled_repos) {
                renderer.info(&format!("    {}", crate::t!("doctor.prereq", title = prereq.title)));
                if let Some(note) = &prereq.note {
                    renderer.info(&format!("        {}", crate::t!("common.note", note = note)));
                }
                self.apply_suggestion(prereq, config.clone(), renderer).await?;
                enabled_repos.insert(prereq.id.clone());
                // A just-enabled repo (RPM Fusion) has no local metadata yet, so the dependent's
                // install below would query a stale cache and wrongly report its packages "not
                // found" (the codecs bug: gstreamer1-plugins-ugly lives in rpmfusion-free). Refresh
                // the package metadata once, right after the repo is added, so the install that
                // follows actually sees them. Best-effort, non-root, and a no-op off Fedora
                // (guarded on dnf5); skipped in dry-run since nothing was really enabled.
                if !self.global.dry_run {
                    refresh_repo_metadata(renderer).await;
                }
            }
            self.apply_suggestion(r, config.clone(), renderer).await?;
            // Remember a repo the user enabled directly, so a later dependent doesn't re-run it.
            if r.manual.is_some() {
                enabled_repos.insert(r.id.clone());
            }
        }
        Ok(())
    }

    /// Apply one fixable system check. Installs route through the normal path with the
    /// questionnaire's "yes" carried through (`assume_yes`); a `Command` is shown then run;
    /// a `PathExport` appends the right line to the user's shell rc.
    async fn apply_fix(&self, fix: &Fix, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        match fix {
            Fix::Install(pkg) => {
                self.install_inner(&[pkg.to_string()], config, renderer, true, false)
                    .await?;
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
        &self, r: &crate::recommend::Recommendation, config: Config, renderer: &Renderer,
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

    /// Show the one-time "unlocked" toast for `id`. Silent in JSON mode.
    fn achievement_toast(&self, id: &str, renderer: &Renderer) {
        if renderer.is_json() {
            return;
        }
        if let Some(a) = crate::achievements::find(id) {
            let title = crate::i18n::tr(&format!("achieve.{}.title", a.id));
            renderer.success(&crate::t!("achieve.unlocked", icon = a.icon, title = title));
        }
    }

    /// If every non-secret achievement (bar `completionist` itself) is unlocked, unlock the crown.
    /// Pushes any newly-earned id onto `newly`. Secrets are excluded on purpose — the 👑 is for
    /// the visible set, so you never need the Sans easter egg to complete it.
    fn maybe_completionist(&self, store: &mut crate::achievements::Achievements, newly: &mut Vec<String>) {
        let all_visible_done = crate::achievements::CATALOG
            .iter()
            .filter(|a| !a.secret && a.id != "completionist")
            .all(|a| store.is_unlocked(a.id));
        if all_visible_done && store.unlock("completionist") {
            newly.push("completionist".to_string());
        }
        // Beating every boss is its own (secret) badge, so it never gates the crown.
        let all_bosses_down = crate::achievements::BOSSES.iter().all(|b| store.is_unlocked(b.id));
        if all_bosses_down && store.unlock("boss-slayer") {
            newly.push("boss-slayer".to_string());
        }
    }

    /// Grant an achievement as a side effect of a real action, showing a one-time toast the
    /// first time it's earned. Best-effort and cosmetic: any failure to load or persist the
    /// ledger is swallowed so it can never break the surrounding command. Silent in JSON mode.
    fn grant_achievement(&self, id: &str, renderer: &Renderer) {
        let Ok(mut store) = crate::achievements::Achievements::load() else {
            return;
        };
        let mut newly = Vec::new();
        if store.unlock(id) {
            newly.push(id.to_string());
            self.maybe_completionist(&mut store, &mut newly);
            let _ = store.save();
        }
        for id in &newly {
            self.achievement_toast(id, renderer);
        }
    }

    /// Grant the badges for winning a boss fight (`id`) with a given ending (`variant`). Three
    /// things can land at once: the boss's own badge, the badge for *that ending*, and — once
    /// you've seen every ending — the boss's "both ways" badge. Beating every boss additionally
    /// earns `boss-slayer` (via `maybe_completionist`). The toast carries a flavour line for the
    /// ending. Best-effort and cosmetic; silent in JSON mode.
    fn grant_boss(&self, id: &str, variant: &str, renderer: &Renderer) {
        let Ok(mut store) = crate::achievements::Achievements::load() else {
            return;
        };
        let mut newly = Vec::new();
        let boss_is_new = store.unlock(id);
        if boss_is_new {
            newly.push(id.to_string());
        }
        // Remember the path taken, and award the badge for it.
        store.bump(&format!("{id}-{variant}"), 1);
        let ending_id = format!("{id}-{variant}");
        if store.unlock(&ending_id) {
            newly.push(ending_id);
        }
        // Every ending seen at least once → the "both ways" badge. A fight with a single path
        // (Sans) has no endings listed and so never earns one.
        let endings = crate::achievements::boss(id).map(|b| b.endings).unwrap_or(&[]);
        let all_endings = !endings.is_empty() && endings.iter().all(|e| store.counter(&format!("{id}-{e}")) > 0);
        let both_id = format!("{id}-both");
        if all_endings && store.unlock(&both_id) {
            newly.push(both_id);
        }
        self.maybe_completionist(&mut store, &mut newly);
        let _ = store.save();
        if renderer.is_json() {
            return;
        }
        for new_id in &newly {
            self.achievement_toast(new_id, renderer);
        }
        // The flavour line belongs to the fight, not to any one badge — show it whenever the
        // ending itself was new, even if the boss badge was already earned.
        if newly.iter().any(|n| n == &format!("{id}-{variant}")) {
            let line = crate::i18n::tr(&format!("achieve.{id}.toast-{variant}"));
            renderer.info(&renderer.palette().dim(&line));
        }
    }

    /// Record a successful install against the achievement ledger: bump the lifetime install
    /// counter, remember which sources were used, and unlock whatever that newly earns
    /// (first-install, the 100/500 grinds, breadth, the night shift, the crown). One load/save,
    /// best-effort, silent in JSON mode. `count` is how many packages actually landed.
    fn record_install(&self, batch: &[crate::engine::BatchPlan], count: usize, pinned: bool, renderer: &Renderer) {
        use chrono::Timelike;
        let Ok(mut store) = crate::achievements::Achievements::load() else {
            return;
        };
        let mut newly = Vec::new();

        if store.unlock("first-install") {
            newly.push("first-install".to_string());
        }

        let total = store.bump("installs", count as u64);
        if total >= crate::achievements::CENTURION_AT && store.unlock("centurion") {
            newly.push("centurion".to_string());
        }
        if total >= crate::achievements::MILLENNIUM_AT && store.unlock("millennium") {
            newly.push("millennium".to_string());
        }

        for c in batch.iter().flat_map(|b| b.candidates.iter()) {
            store.add_source(&c.source_id);
        }
        if store.source_count() >= crate::achievements::POLYGLOT_SOURCES && store.unlock("polyglot") {
            newly.push("polyglot".to_string());
        }

        // A whole shopping list in one command.
        if count >= crate::achievements::HAUL_AT && store.unlock("haul") {
            newly.push("haul".to_string());
        }

        // An explicit `name:source` — you told JII exactly where to get it.
        if pinned && store.unlock("sniper") {
            newly.push("sniper".to_string());
        }

        // The two ends of the night: 00:00–03:59 and 05:00–07:59 local time.
        let hour = chrono::Local::now().hour();
        if hour < 4 && store.unlock("night-owl") {
            newly.push("night-owl".to_string());
        }
        if (5..8).contains(&hour) && store.unlock("early-bird") {
            newly.push("early-bird".to_string());
        }

        self.maybe_completionist(&mut store, &mut newly);
        let _ = store.save();
        for id in &newly {
            self.achievement_toast(id, renderer);
        }
    }

    /// `jii achievements` — the playful ledger: what you've unlocked and what's left to find.
    /// Secret-and-locked entries show as `???` with a teasing description; everything is
    /// read-only.
    fn achievements(&self, renderer: &Renderer) -> crate::error::Result<()> {
        let store = crate::achievements::Achievements::load()?;

        if renderer.is_json() {
            let rows: Vec<_> = crate::achievements::visible(&store)
                .map(|a| {
                    let unlocked = store.is_unlocked(a.id);
                    // A secret, still-locked achievement is not spoiled even in JSON — except
                    // an ending badge, which is only listed at all once its fight is won and
                    // is a named goal from then on (same rule as the friendly view).
                    let reveal = unlocked || !a.secret || a.revealed_by.is_some();
                    serde_json::json!({
                        "id": a.id,
                        "unlocked": unlocked,
                        "unlocked_at": store.unlocked_at(a.id),
                        "secret": a.secret,
                        "title": reveal.then(|| crate::i18n::tr(&format!("achieve.{}.title", a.id))),
                        "description": reveal
                            .then(|| crate::i18n::tr(&format!("achieve.{}.desc", a.id))),
                    })
                })
                .collect();
            renderer.json_value(&serde_json::json!(rows));
            return Ok(());
        }

        let palette = renderer.palette();
        // Count only what's on show: an ending badge you can't see yet must not make the
        // total jump around (or hint that something is missing).
        let total = crate::achievements::visible(&store).count();
        let earned = crate::achievements::visible(&store)
            .filter(|a| store.is_unlocked(a.id))
            .count();
        renderer.info(&palette.heading(&crate::t!("achieve.header", earned = earned, total = total)));
        renderer.info("");

        for a in crate::achievements::visible(&store) {
            let unlocked = store.is_unlocked(a.id);
            // Earned vs not is carried by a glyph, not by colour alone: colour is lost in a
            // pipe, a pasted log, `--no-color` and to plenty of eyes, and "did I get this one?"
            // is the only question this list answers.
            let state = if unlocked {
                palette.good(palette.mark_ok())
            } else {
                palette.dim(if renderer.unicode() { "·" } else { "-" })
            };
            // Keep a secret hidden until earned: show `???` and a teaser, not the real text.
            // An ending badge is exempt — it only appears once its fight is won, and then it
            // is a named goal ("now try sparing him"), not another anonymous row.
            if a.secret && !unlocked && a.revealed_by.is_none() {
                let title = palette.dim("???");
                renderer.info(&format!("  {state} {}  {}", a.icon, title));
                renderer.info(&format!("        {}", palette.dim(&crate::t!("achieve.hidden"))));
                continue;
            }
            let title = crate::i18n::tr(&format!("achieve.{}.title", a.id));
            let desc = crate::i18n::tr(&format!("achieve.{}.desc", a.id));
            let mark = if unlocked {
                palette.good(a.icon)
            } else {
                palette.dim(a.icon)
            };
            let title = if unlocked {
                palette.heading(&title)
            } else {
                palette.dim(&title)
            };
            renderer.info(&format!("  {state} {mark}  {title}"));
            renderer.info(&format!("        {}", palette.dim(&desc)));
        }
        Ok(())
    }

    /// `jii changelog` — what changed, in plain language (ADR-0079). The notes are embedded in
    /// the binary, so this works with no network: bare shows the running version, a version
    /// argument shows that release, `--all` the whole history, and `--since <ver>` everything
    /// newer than it (which is what `jii update jii` runs for you after an update).
    fn changelog(
        &self, version: Option<&str>, all: bool, since: Option<&str>, renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let running = crate::selfupdate::current_version();
        let picked: Vec<&crate::changelog::Release> = if all {
            crate::changelog::releases().iter().collect()
        } else if let Some(from) = since {
            crate::changelog::since(from)
        } else {
            let wanted = version.unwrap_or(running);
            let found = match version {
                Some(v) => crate::changelog::find(v),
                None => crate::changelog::current(),
            };
            match found {
                Some(r) => vec![r],
                None => {
                    // Never a dead end: say which versions we do have notes for.
                    let known: Vec<&str> = crate::changelog::releases()
                        .iter()
                        .map(|r| r.version.as_str())
                        .collect();
                    renderer.error(&crate::t!("changelog.unknown", version = wanted));
                    renderer.info(&crate::t!("changelog.known", list = known.join(", ")));
                    return Ok(());
                }
            }
        };

        if renderer.is_json() {
            let rows: Vec<_> = picked
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "version": r.version,
                        "date": r.date,
                        "current": r.version == running,
                        "notes": r.notes(),
                    })
                })
                .collect();
            renderer.json_value(&serde_json::json!(rows));
            return Ok(());
        }

        // `--since` after an update legitimately finds nothing (you were already current, or
        // this build's notes predate the file); say so in one line instead of printing nothing.
        if picked.is_empty() {
            renderer.info(&crate::t!("changelog.none"));
            return Ok(());
        }

        let palette = renderer.palette();
        renderer.info(&palette.heading(&crate::t!("changelog.header")));
        for r in &picked {
            renderer.info("");
            let mut head = format!("{} · {}", r.version, r.date);
            if r.version == running {
                head.push_str(&format!(" {}", crate::t!("changelog.this_one")));
            }
            renderer.info(&format!("  {}", palette.heading(&head)));
            for note in r.notes() {
                renderer.info(&format!("    • {note}"));
            }
        }
        // Only hint at more when there *is* more to see.
        if !all && crate::changelog::releases().len() > picked.len() {
            renderer.info("");
            renderer.info(&palette.dim(&crate::t!("changelog.hint_all")));
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
        let warn_mark = renderer.palette().mark_warn();
        let rows: Vec<Vec<String>> = entries
            .iter()
            .map(|e| {
                let trust = e.trust.map_or_else(|| crate::t!("common.unknown"), |t| t.display());
                let status = if e.concerns.is_empty() {
                    crate::t!("list.status_ok")
                } else {
                    flagged += 1;
                    let reasons: Vec<&str> = e.concerns.iter().map(|c| c.message()).collect();
                    format!("{warn_mark} {}", reasons.join(", "))
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
            renderer.warn(&crate::t!(
                "audit.need_attention",
                flagged = flagged,
                total = entries.len()
            ));
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

    /// Resolve the effective declarative preference for this run: the per-run flags
    /// `--nix-config` / `--nix-imperative` override the `[install] prefer_declarative` config.
    fn declarative_pref(&self, config: &Config) -> DeclarativePref {
        if self.global.nix_config {
            DeclarativePref::Always
        } else if self.global.nix_imperative {
            DeclarativePref::Never
        } else {
            config.install.prefer_declarative
        }
    }

    /// Show a declarative file-edit's diff, confirm (honouring `--yes/--auto/--no` and
    /// `default_yes`), back up to `<path>.jii-bak` and write — or, under `--dry-run`, show it and
    /// change nothing. Shared by the interactive strategy menu and the `always` auto-route (Nix
    /// Etap B/C, ADR-0056/0058). A user-owned file is written directly; a root-owned one
    /// (`needs_root`) is written via `privilege.rs`, with the exact `sudo`/`pkexec` commands
    /// shown first.
    async fn apply_edit_file(
        &self, config_auto: bool, kind: &crate::model::StrategyKind, assume_yes: bool, renderer: &Renderer,
    ) {
        let crate::model::StrategyKind::EditFile {
            path,
            new_content,
            diff,
            apply,
            needs_root,
        } = kind
        else {
            return; // only EditFile reaches here; other kinds are no-ops
        };
        renderer.info(&crate::t!("nix.edit_intro", file = path.display().to_string()));
        renderer.info(diff);
        // A root-owned config (e.g. /etc/nixos/configuration.nix) is written via the privilege
        // path: JII shows the exact elevated commands first, then runs them through privilege.rs.
        let privilege = needs_root.then(crate::privilege::Privilege::detect);
        let root_cmds = privilege.as_ref().map(|p| root_write_argv(p, path));
        if let Some((backup_cmd, write_cmd)) = &root_cmds {
            renderer.info(&crate::t!("nix.edit_root_cmds"));
            renderer.info(&format!("  {}", backup_cmd.join(" ")));
            renderer.info(&format!("  {}", write_cmd.join(" ")));
        }
        if self.global.dry_run {
            renderer.info(&crate::t!("nix.edit_dry_run"));
            return;
        }
        let flags = self.prompt_flags(config_auto).with_yes(assume_yes);
        if !prompt::confirm(renderer, &crate::t!("nix.edit_confirm"), true, &flags) {
            renderer.info(&crate::t!("common.aborted"));
            return;
        }
        let result = match (privilege, root_cmds) {
            (Some(priv_), Some((backup_cmd, write_cmd))) => {
                write_nix_config_root(&priv_, path, new_content, &backup_cmd, &write_cmd).await
            }
            // User-owned file: write it directly, no escalation.
            _ => write_nix_config(path, new_content).map_err(|e| e.to_string()),
        };
        match result {
            Ok(backup) => {
                renderer.success(&crate::t!("nix.edit_written", file = path.display().to_string()));
                renderer.info(&crate::t!("nix.edit_backup", backup = backup.display().to_string()));
                renderer.info(&crate::t!("nix.edit_apply", cmd = apply.to_string()));
            }
            Err(e) => renderer.error(&crate::t!("nix.edit_failed", error = e)),
        }
    }

    /// Under `prefer_declarative = always`, route one candidate to its declarative install if the
    /// owning source offers one: an auto-editable file (`EditFile`) is edited (diff → backup →
    /// write); a config that can only be shown (`Manual`, e.g. root-owned NixOS
    /// `configuration.nix`) prints its snippet. Returns `true` when handled declaratively (so the
    /// caller must NOT also install it imperatively); `false` to fall through to an imperative
    /// install (any non-Nix source, or Nix with no detected config → empty strategies).
    async fn route_declarative(
        &self, engine: &Engine, candidate: &PackageCandidate, assume_yes: bool, renderer: &Renderer,
    ) -> bool {
        let strategies = engine.install_strategies(&candidate.source_id, candidate).await;
        // Prefer a file we can actually edit; else a snippet we can only show.
        if let Some(strat) = strategies
            .iter()
            .find(|s| matches!(s.kind, crate::model::StrategyKind::EditFile { .. }))
        {
            self.apply_edit_file(engine.config().install.auto, &strat.kind, assume_yes, renderer)
                .await;
            return true;
        }
        if let Some(strat) = strategies
            .iter()
            .find(|s| matches!(s.kind, crate::model::StrategyKind::Manual { .. }))
            && let crate::model::StrategyKind::Manual { guidance } = &strat.kind
        {
            renderer.info(guidance);
            renderer.info(&crate::t!("nix.guidance_footer"));
            return true;
        }
        false
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
    version
        .map(|v| v.to_string())
        .unwrap_or_else(|| crate::t!("common.unknown"))
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
/// One line in the GitHub repo picker: `owner/repo — description  ★1.2k`. Kept to a single
/// terminal row (each menu item must be exactly one line so the chooser's per-row redraw and
/// mouse hit-testing stay correct): the description is budgeted against the slug length.
fn repo_label(hit: &crate::model::RepoHit, palette: crate::ui::Palette) -> String {
    let slug_len = hit.slug.chars().count().min(48);
    let budget = 64usize.saturating_sub(slug_len).max(12);
    let desc = hit
        .description
        .as_deref()
        .filter(|d| !d.is_empty())
        .map(|d| palette.dim(&format!(" — {}", one_line(d, budget))))
        .unwrap_or_default();
    let stars = palette.dim(&format!("★{}", humanize_count(hit.stars)));
    format!("{}{desc}  {stars}", hit.slug)
}

/// Compact star/download counts: `1234` → `1.2k`, `2_500_000` → `2.5M`.
fn humanize_count(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1_000.0),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

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
        SystemCheck {
            ok: true,
            critical: false,
            label: label.into(),
            advice: None,
            fix: None,
        }
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
    /// Whether Homebrew is present and, if so, whether a compiler is available: brew builds
    /// from source whenever no bottle matches, and its own installer only *suggests* the
    /// build tools in its closing notes.
    brew: bool,
    build_tools: bool,
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
        SystemCheck::warn(crate::t!("check.internet_missing"), crate::t!("check.internet_advice")).critical()
    });

    // Common tools JII and its sources lean on — and which JII can itself install.
    checks.push(if f.git {
        SystemCheck::pass(crate::t!("check.git_ok"))
    } else {
        SystemCheck::warn(crate::t!("check.git_missing"), crate::t!("check.git_advice")).fixable(Fix::Install("git"))
    });
    checks.push(if f.curl {
        SystemCheck::pass(crate::t!("check.curl_ok"))
    } else {
        SystemCheck::warn(crate::t!("check.curl_missing"), crate::t!("check.curl_advice")).fixable(Fix::Install("curl"))
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
        .fixable(Fix::PathExport {
            dir: f.local_bin.clone(),
        })
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
            .fixable(Fix::PathExport {
                dir: f.cargo_bin.clone(),
            })
        });
    }

    // A compiler for Homebrew — only meaningful once brew is installed. Homebrew pours a
    // prebuilt bottle when it has one and compiles when it doesn't, so a missing toolchain
    // isn't broken, just a formula away from failing. JII can install it; brew only mentions it.
    if f.brew {
        checks.push(if f.build_tools {
            SystemCheck::pass(crate::t!("check.build_ok"))
        } else {
            SystemCheck::warn(crate::t!("check.build_missing"), crate::t!("check.build_advice"))
                .fixable(Fix::Install("gcc"))
        });
    }

    // Flathub — only meaningful when Flatpak is installed.
    if f.flatpak {
        checks.push(if f.flathub {
            SystemCheck::pass(crate::t!("check.flathub_ok"))
        } else {
            SystemCheck::warn(crate::t!("check.flathub_missing"), crate::t!("check.flathub_advice")).fixable(
                Fix::Command {
                    // --user: a user-scope remote needs no root/polkit and matches how JII installs
                    // (`flatpak install --user`). It also avoids "Unable to connect to system bus"
                    // on minimal/live systems with no running system D-Bus (seen on Void live).
                    argv: vec![
                        "flatpak".into(),
                        "remote-add".into(),
                        "--user".into(),
                        "--if-not-exists".into(),
                        "flathub".into(),
                        "https://flathub.org/repo/flathub.flatpakrepo".into(),
                    ],
                    show: "flatpak remote-add --user --if-not-exists flathub \
                       https://flathub.org/repo/flathub.flatpakrepo"
                        .into(),
                },
            )
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
    let local_bin_on_path = home.as_ref().map(|_| platform.is_on_path(&local_bin)).unwrap_or(true); // can't resolve HOME → don't cry wolf
    let cargo_bin_on_path = platform.is_on_path(&cargo_bin);

    // Independent probes run concurrently.
    let (internet, git, curl, cargo, flatpak, cc, make) = tokio::join!(
        check_internet(),
        crate::provider::which("git"),
        crate::provider::which("curl"),
        crate::provider::which("cargo"),
        crate::provider::which("flatpak"),
        crate::provider::which("cc"),
        crate::provider::which("make"),
    );
    // brew may live outside PATH right after its own installer ran.
    let brew = crate::provider::which(&crate::provider::homebrew::brew_bin()).await;
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
        brew,
        build_tools: cc && make,
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

/// Percent-encode a search term for a URL query component. Keeps the RFC 3986 unreserved set
/// (`A-Z a-z 0-9 - _ . ~`) and encodes everything else as `%XX`, so a term with a slash, an `@`,
/// or spaces (`@angular/cli`) still forms a valid browse link. Small and dependency-free — we
/// only ever encode short package names for the "browse for it" hint.
fn url_query_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Refresh the package manager's metadata after a repo was just enabled in the `doctor`
/// questionnaire, so the dependent install that follows sees the new repo's packages instead
/// of a stale-cache "not found" (the RPM Fusion → codecs case). Best-effort and non-root:
/// a no-op where dnf5 is absent (the only distro with a repo prerequisite today is Fedora),
/// and a failure is swallowed — the transaction below may still refresh on its own.
async fn refresh_repo_metadata(renderer: &Renderer) {
    if !crate::provider::which("dnf5").await {
        return;
    }
    // A bare `dnf5 makecache` can sit silent for several seconds — indistinguishable from a
    // hang (the owner's ask: "put a spinner on waits like these"). Animate one while it runs,
    // and capture its output (run_capture) so the manager's chatter doesn't fight the spinner
    // line. Best-effort: a failure is swallowed — the transaction below may refresh on its own.
    let spinner = crate::ui::Spinner::start(renderer, &crate::t!("doctor.refreshing_meta"));
    let _ = crate::provider::run_capture(&["dnf5", "makecache"]).await;
    spinner.stop().await;
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
        .is_ok_and(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.trim() == "flathub")
        })
}

/// Back up `path` to `<path>.jii-bak` and overwrite it with `content`, returning the backup
/// path (Nix Etap B, ADR-0056). Used for a **user-owned** config (`needs_root == false`), so
/// this is plain user-space file IO — no privilege escalation (the root-owned case goes through
/// [`write_nix_config_root`]). The backup is written first, so a failed write always leaves a
/// recoverable copy.
fn write_nix_config(path: &std::path::Path, content: &str) -> std::io::Result<std::path::PathBuf> {
    let backup = jii_backup_path(path);
    std::fs::copy(path, &backup)?;
    std::fs::write(path, content)?;
    Ok(backup)
}

/// The `<path>.jii-bak` sibling of a config file (its pre-edit backup).
fn jii_backup_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut backup = path.as_os_str().to_owned();
    backup.push(".jii-bak");
    std::path::PathBuf::from(backup)
}

/// The two elevated commands that back up and then overwrite a root-owned config `path`, with
/// the session's `sudo`/`pkexec` prefix already applied (Nix Etap C, ADR-0058). Returned as
/// `(backup_cmd, write_cmd)` so the CLI can *show them verbatim* before running anything. The
/// write command copies from a placeholder temp path (`{tmp}`) that [`write_nix_config_root`]
/// fills in — the shown form uses the same real temp path it will run.
fn root_write_argv(privilege: &crate::privilege::Privilege, path: &std::path::Path) -> (Vec<String>, Vec<String>) {
    let dest = path.display().to_string();
    let backup = jii_backup_path(path).display().to_string();
    let tmp = root_tmp_path(path).display().to_string();
    let backup_cmd = privilege.elevated_argv(&["cp".into(), "-a".into(), "--".into(), dest.clone(), backup], true);
    let write_cmd = privilege.elevated_argv(&["cp".into(), "--".into(), tmp, dest], true);
    (backup_cmd, write_cmd)
}

/// A per-process temp path (in the system temp dir) used to stage a root config's new contents
/// before `sudo cp` moves it into place. Deterministic for one `path` within a run so
/// [`root_write_argv`] and [`write_nix_config_root`] agree; created with `O_EXCL` at write time.
fn root_tmp_path(path: &std::path::Path) -> std::path::PathBuf {
    let stem = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config".into());
    std::env::temp_dir().join(format!("jii-nixedit-{}-{stem}", std::process::id()))
}

/// Write a root-owned Nix config via the privilege path (Etap C, ADR-0058): stage `content` in a
/// user-owned temp file (`O_EXCL`, so an existing path/symlink can't be clobbered), prime the
/// escalation once, then run the pre-built `backup_cmd` and `write_cmd` (exactly what the CLI
/// already showed). Returns the backup path on success. JII never runs fully as root — only these
/// two concrete `cp` steps escalate, through `privilege.rs`.
async fn write_nix_config_root(
    privilege: &crate::privilege::Privilege, path: &std::path::Path, content: &str, backup_cmd: &[String],
    write_cmd: &[String],
) -> std::result::Result<std::path::PathBuf, String> {
    use std::io::Write;
    let tmp = root_tmp_path(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    let staged = file.write_all(content.as_bytes()).and_then(|()| file.sync_all());
    if let Err(e) = staged {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    drop(file);
    privilege.prime().await.map_err(|e| e.to_string())?;
    let run = async {
        privilege.run(backup_cmd, true).await.map_err(|e| e.to_string())?;
        privilege.run(write_cmd, true).await.map_err(|e| e.to_string())
    }
    .await;
    let _ = std::fs::remove_file(&tmp);
    run?;
    Ok(jii_backup_path(path))
}

/// The host's system package manager, used to **uninstall** an ecosystem manager that was
/// installed as a distro package (`jii sources remove flatpak`). Detected by binary presence;
/// each variant knows both how to remove a package and how to check one is installed, so we
/// never guess-remove a package name that doesn't exist on this distro.
enum SysManager {
    /// dnf/dnf5 (Fedora/RHEL) — RPM-based.
    Dnf(&'static str),
    /// apt-get (Debian/Ubuntu).
    Apt,
    /// pacman (Arch).
    Pacman,
    /// zypper (openSUSE) — RPM-based.
    Zypper,
    /// xbps (Void).
    Xbps,
    /// portage/emerge (Gentoo).
    Portage,
}

/// Detect the host's system package manager, in the order distros ship them. `xbps` is
/// detected via `xbps-install` (the remove tool `xbps-remove` ships in the same package).
async fn detect_system_manager() -> Option<SysManager> {
    use crate::provider::which;
    if which("dnf5").await {
        Some(SysManager::Dnf("dnf5"))
    } else if which("dnf").await {
        Some(SysManager::Dnf("dnf"))
    } else if which("apt-get").await {
        Some(SysManager::Apt)
    } else if which("pacman").await {
        Some(SysManager::Pacman)
    } else if which("zypper").await {
        Some(SysManager::Zypper)
    } else if which("xbps-install").await {
        Some(SysManager::Xbps)
    } else if which("emerge").await {
        Some(SysManager::Portage)
    } else {
        None
    }
}

impl SysManager {
    /// The elevated argv (and `needs_root`) to remove `pkgs` with this manager.
    fn remove_argv(&self, pkgs: &[String]) -> (Vec<String>, bool) {
        let mut argv: Vec<String> = match self {
            SysManager::Dnf(bin) => vec![bin.to_string(), "remove".into(), "-y".into()],
            SysManager::Apt => vec!["apt-get".into(), "remove".into(), "-y".into()],
            SysManager::Pacman => vec!["pacman".into(), "-Rs".into(), "--noconfirm".into()],
            SysManager::Zypper => vec!["zypper".into(), "--non-interactive".into(), "remove".into()],
            SysManager::Xbps => vec!["xbps-remove".into(), "-Ry".into()],
            SysManager::Portage => vec!["emerge".into(), "--unmerge".into()],
        };
        argv.extend(pkgs.iter().cloned());
        (argv, true)
    }

    /// Whether `name` is currently installed, checked with a cheap query so removal targets only
    /// packages that actually exist here. Portage has no cheap universal probe, so it optimistically
    /// includes the name (the shown `emerge --unmerge` no-ops harmlessly on an absent package).
    async fn pkg_installed(&self, name: &str) -> bool {
        let ok = |argv: &[&str]| {
            let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
            async move {
                tokio::process::Command::new(&argv[0])
                    .args(&argv[1..])
                    .output()
                    .await
                    .is_ok_and(|o| o.status.success())
            }
        };
        match self {
            SysManager::Dnf(_) | SysManager::Zypper => ok(&["rpm", "-q", name]).await,
            SysManager::Apt => tokio::process::Command::new("dpkg-query")
                .args(["-W", "-f=${Status}", name])
                .output()
                .await
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("install ok installed")),
            SysManager::Pacman => ok(&["pacman", "-Q", name]).await,
            SysManager::Xbps => ok(&["xbps-query", name]).await,
            SysManager::Portage => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PkgVersion, TrustLevel};

    #[test]
    fn url_query_encode_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(url_query_encode("firefox"), "firefox");
        assert_eq!(url_query_encode("gh-cli_2.0.tar~"), "gh-cli_2.0.tar~");
        // slash, @ and space must be percent-encoded so the browse link stays valid.
        assert_eq!(url_query_encode("@angular/cli"), "%40angular%2Fcli");
        assert_eq!(url_query_encode("a b"), "a%20b");
    }

    #[test]
    fn sys_manager_remove_argv_is_per_manager_and_root() {
        let pkgs = vec!["flatpak".to_string()];
        let (argv, root) = SysManager::Dnf("dnf5").remove_argv(&pkgs);
        assert_eq!(argv, ["dnf5", "remove", "-y", "flatpak"]);
        assert!(root);
        let (argv, _) = SysManager::Apt.remove_argv(&pkgs);
        assert_eq!(argv, ["apt-get", "remove", "-y", "flatpak"]);
        let (argv, _) = SysManager::Pacman.remove_argv(&pkgs);
        assert_eq!(argv, ["pacman", "-Rs", "--noconfirm", "flatpak"]);
        let (argv, _) = SysManager::Zypper.remove_argv(&pkgs);
        assert_eq!(argv, ["zypper", "--non-interactive", "remove", "flatpak"]);
        let (argv, _) = SysManager::Xbps.remove_argv(&pkgs);
        assert_eq!(argv, ["xbps-remove", "-Ry", "flatpak"]);
    }

    #[test]
    fn sys_manager_remove_argv_carries_all_packages() {
        let pkgs = vec!["cargo".to_string(), "rust".to_string()];
        let (argv, _) = SysManager::Dnf("dnf").remove_argv(&pkgs);
        assert_eq!(argv, ["dnf", "remove", "-y", "cargo", "rust"]);
    }

    #[test]
    fn write_nix_config_backs_up_then_overwrites() {
        // Nix Etap B: the original is preserved at <path>.jii-bak before the new content lands.
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("home.nix");
        std::fs::write(&cfg, "old\n").unwrap();
        let backup = write_nix_config(&cfg, "new\n").unwrap();
        assert_eq!(backup, cfg.with_file_name("home.nix.jii-bak"));
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "new\n");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "old\n");
    }

    #[test]
    fn declarative_pref_flags_override_config() {
        use clap::Parser;
        // The per-run flags win over the config; with neither, the config decides.
        let mut config = Config::default();

        config.install.prefer_declarative = DeclarativePref::Never;
        let cli = Cli::parse_from(["jii", "install", "foo"]);
        assert_eq!(cli.declarative_pref(&config), DeclarativePref::Never);

        let cli = Cli::parse_from(["jii", "--nix-config", "install", "foo"]);
        assert_eq!(cli.declarative_pref(&config), DeclarativePref::Always);

        config.install.prefer_declarative = DeclarativePref::Always;
        let cli = Cli::parse_from(["jii", "--nix-imperative", "install", "foo"]);
        assert_eq!(cli.declarative_pref(&config), DeclarativePref::Never);

        // No flags → the `always` config is honoured.
        let cli = Cli::parse_from(["jii", "install", "foo"]);
        assert_eq!(cli.declarative_pref(&config), DeclarativePref::Always);
    }

    #[tokio::test]
    async fn dry_run_edit_file_never_writes() {
        use clap::Parser;
        // The `always`/`--nix-config` route reaches apply_edit_file under `--dry-run`; it must
        // show the change and touch nothing (no overwrite, no backup).
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("home.nix");
        std::fs::write(&cfg, "original\n").unwrap();

        let kind = crate::model::StrategyKind::EditFile {
            path: cfg.clone(),
            new_content: "edited\n".into(),
            diff: "+ edited".into(),
            apply: "home-manager switch".into(),
            needs_root: false,
        };
        let cli = Cli::parse_from(["jii", "-d", "install", "foo"]);
        let renderer = Renderer::new(ColorChoice::Never, false, crate::config::OutputMode::Friendly);
        cli.apply_edit_file(false, &kind, false, &renderer).await;

        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "original\n");
        assert!(!cfg.with_file_name("home.nix.jii-bak").exists());
    }

    #[tokio::test]
    async fn dry_run_root_edit_never_writes_and_stages_nothing() {
        use clap::Parser;
        // Etap C: a root-owned config under `--dry-run` shows the sudo commands but must not
        // write, back up, or even stage a temp file.
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("configuration.nix");
        std::fs::write(&cfg, "original\n").unwrap();

        let kind = crate::model::StrategyKind::EditFile {
            path: cfg.clone(),
            new_content: "edited\n".into(),
            diff: "+ edited".into(),
            apply: "sudo nixos-rebuild switch".into(),
            needs_root: true,
        };
        let cli = Cli::parse_from(["jii", "-d", "install", "foo"]);
        let renderer = Renderer::new(ColorChoice::Never, false, crate::config::OutputMode::Friendly);
        cli.apply_edit_file(false, &kind, false, &renderer).await;

        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "original\n");
        assert!(!cfg.with_file_name("configuration.nix.jii-bak").exists());
        assert!(!root_tmp_path(&cfg).exists());
    }

    #[test]
    fn root_write_argv_shows_backup_then_write_with_elevation() {
        // The exact elevated commands JII prints (and later runs) for a root-owned config.
        let priv_ = crate::privilege::Privilege::detect();
        let path = std::path::Path::new("/etc/nixos/configuration.nix");
        let (backup_cmd, write_cmd) = root_write_argv(&priv_, path);
        // Both back up to <path>.jii-bak and copy the staged temp into place.
        assert!(backup_cmd.iter().any(|a| a == "/etc/nixos/configuration.nix.jii-bak"));
        assert!(backup_cmd.iter().any(|a| a == "cp"));
        assert!(write_cmd.iter().any(|a| a == "/etc/nixos/configuration.nix"));
        assert!(write_cmd.contains(&root_tmp_path(path).display().to_string()));
    }

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
            popularity: None,
            suspicious: false,
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
        let reasons = recommendation_reasons(&candidate(TrustLevel::Official, true, Some("1.2")), vec![]);
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
        assert_eq!(reasons, vec!["unverified source".to_string()]);
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
    fn humanize_count_compacts_large_numbers() {
        assert_eq!(humanize_count(0), "0");
        assert_eq!(humanize_count(999), "999");
        assert_eq!(humanize_count(1_200), "1.2k");
        assert_eq!(humanize_count(12_500), "12.5k");
        assert_eq!(humanize_count(2_500_000), "2.5M");
    }

    #[test]
    fn typo_variants_recover_common_slips() {
        use crate::engine::typo_variants;
        // Extra character: the corrected term is reachable by a single deletion.
        assert!(typo_variants("exeteragram").contains(&"exteragram".to_string()));
        // A mid-word doubled key (the owner's example): `pipix` → `pipx`.
        assert!(typo_variants("pipix").contains(&"pipx".to_string()));
        // Swapped neighbours: an adjacent transposition puts them back.
        assert!(typo_variants("gti").contains(&"git".to_string()));
        // Deduped, never echoes the input, and stays a bounded handful.
        let v = typo_variants("firefox");
        assert!(!v.contains(&"firefox".to_string()));
        assert!(v.len() <= 16);
        let mut sorted = v.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), v.len());
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
            brew: true,
            build_tools: true,
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
        let net = checks
            .iter()
            .find(|c| c.label.contains("internet") || c.label.contains("Internet"))
            .unwrap();
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
    fn build_tools_are_only_checked_once_brew_is_here_and_are_fixable() {
        // No Homebrew, no opinion — a compiler isn't JII's business otherwise.
        let mut f = facts_all_good();
        f.brew = false;
        f.build_tools = false;
        assert!(
            !system_checks(&f).iter().any(|c| c.label.contains("compiler")),
            "no brew, no compiler check"
        );

        // With Homebrew present, a missing compiler is a warning JII can fix itself —
        // rather than the "install the build tools yourself" note brew signs off with.
        f.brew = true;
        let checks = system_checks(&f);
        let build = checks
            .iter()
            .find(|c| c.label.contains("compiler"))
            .expect("brew hosts get a compiler check");
        assert!(!build.ok);
        assert!(matches!(build.fix, Some(Fix::Install("gcc"))));
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
