//! Command-line surface: clap definitions, global flags, and dispatch.
//!
//! The command set is intentionally the full, stable surface from
//! `docs/ARCHITECTURE.md` §13. Commands not yet implemented return a clear
//! "not yet" message that names the phase, so the CLI shape never churns.

use clap::{Parser, Subcommand};

use crate::config::{ColorChoice, Config, DeclarativePref, Profile};
use crate::engine::Engine;
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, PackageSpec, Query};
use crate::selfupdate;
use crate::ui::Renderer;
use crate::ui::prompt::{self, PromptFlags};

/// The long form behind `jii --version`: what this is, what it was built for, and where
/// its two files live.
///
/// `-V` keeps clap's terse `jii 0.1.19-beta` — scripts read that, and it must not change
/// shape. `--version` is the one a person types, so it can afford to answer the questions
/// that usually follow it ("built for which arch?", "where is the config?").
///
/// Leaked on purpose: clap wants a `&'static str`, this is built once per process, and the
/// process is about to print it and exit.
fn long_version() -> &'static str {
    static TEXT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TEXT.get_or_init(|| {
        let config = crate::config::Config::default_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "~/.config/jii/config.toml".to_string());
        format!(
            "{}\n{}\n\n  built for   {} · linux\n  config      {}\n  docs        https://github.com/0nigiris/JII",
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_DESCRIPTION"),
            std::env::consts::ARCH,
            config,
        )
    })
    .as_str()
}

/// Just Install It — a smart universal package installer for Linux.
#[derive(Debug, Parser)]
#[command(name = "jii", version, long_version = long_version(), about)]
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
    /// Find every way to install something, and offer to.
    Search {
        /// Query terms.
        #[arg(required = true)]
        query: Vec<String>,
        /// Show the hits that were set aside as name-squats too.
        #[arg(long)]
        all: bool,
        /// Treat the query as a package name only — never answer with a topic.
        #[arg(long)]
        exact: bool,
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
    /// Store, inspect or forget the GitHub token JII uses, so you never have to create the
    /// file by hand. With no argument it asks for the token and reads it without echoing —
    /// the safe way, since a token typed as an argument is kept in your shell history.
    /// `--show` says where the current token comes from (never what it is); `--forget`
    /// deletes the stored one.
    #[command(name = "ghtoken", alias = "gh-token")]
    GhToken {
        /// The token. Prefer omitting it and pasting at the prompt: an argument lands in
        /// your shell history, and JII will say so.
        token: Option<String>,
        /// Say where the token JII would use comes from — environment, file, or `gh` — and
        /// never print the token itself.
        #[arg(long, conflicts_with_all = ["token", "forget"])]
        show: bool,
        /// Delete the token file JII stores.
        #[arg(long, conflicts_with = "token")]
        forget: bool,
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

mod bootstrap;
mod doctor;
mod install;
mod sources;
use sources::{root_write_argv, write_nix_config, write_nix_config_root};
use doctor::{describe_origin, token_file_display};

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
            | Some(Commands::GhToken { .. })
            | Some(Commands::Lang { .. })
            | Some(Commands::Cache { .. })
            | Some(Commands::Doctor { .. })
            | Some(Commands::Uninstall)
            | Some(Commands::Completions { .. })
            | Some(Commands::Man)
            | Some(Commands::DevTest) => None,
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
            Some(Commands::Search { query, .. }) => Some(format!("jii search {}", query.join(" "))),
            Some(Commands::Info { package }) => Some(format!("jii info {package}")),
            Some(Commands::How { package }) => Some(format!("jii how {package}")),
            Some(Commands::List { audit }) => {
                Some(if *audit { "jii list --audit".to_string() } else { "jii list".to_string() })
            }
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
            self.grant_achievement("sans");
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
            Some(Commands::Install { packages }) => {
                self.install(packages, config, &renderer).await
            }
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

            Some(Commands::Search { query, all, exact }) => {
                self.search(query, *all, *exact, config, &renderer).await
            }
            Some(Commands::Info { package }) => self.info(package, config, &renderer).await,
            Some(Commands::Sources { all, action }) => match action {
                None => self.sources(*all, config, &renderer).await,
                Some(SourcesAction::Disable { id }) => {
                    self.sources_set_enabled(id, false, config, &renderer)
                }
                Some(SourcesAction::Enable { id }) => {
                    self.sources_set_enabled(id, true, config, &renderer)
                }
                Some(SourcesAction::Add { id }) => self.sources_add(id, config, &renderer).await,
                Some(SourcesAction::Remove { id }) => {
                    self.sources_remove(id, config, &renderer).await
                }
            },
            Some(Commands::Lang { code }) => self.lang(code.as_deref(), config, &renderer),
            Some(Commands::Cache { action }) => self.cache(action.as_ref(), &renderer),
            Some(Commands::GhToken { token, show, forget }) => {
                self.gh_token(token.as_deref(), *show, *forget, config, &renderer)
            }
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
        self.grant_achievement("cleaner");
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
                palette.dim(&root_label())
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
        self.grant_achievement("fresh");
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
        &self,
        config: Config,
        renderer: &Renderer,
        prefetch: Option<tokio::task::JoinHandle<crate::error::Result<selfupdate::Latest>>>,
    ) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        let install = selfupdate::detect_install().await?;
        renderer.info(&crate::t!("selfupdate.checking"));
        // Use the release lookup started in parallel with the system update when available,
        // otherwise fetch it now (the direct `jii update jii` path).
        let fetched = match prefetch {
            Some(handle) => handle.await.unwrap_or_else(|e| {
                Err(crate::error::JiiError::Other(anyhow::anyhow!(e.to_string())))
            }),
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
        self.grant_achievement("self-made");
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
    async fn show_update_changelog(
        &self,
        exe: &std::path::Path,
        from: &str,
        renderer: &Renderer,
    ) {
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
                let sudo = if plan.needs_root() { root_label() } else { String::new() };
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
        self.grant_achievement("fresh");
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
        show_all: bool,
        exact_only: bool,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let engine = Engine::new(self.apply_profile(config.clone()))?;
        if !self.ensure_usable_source(&engine, renderer).await {
            return Ok(());
        }
        // `search` is free-text discovery, not a package spec (ADR-0031) — the terms are the
        // query verbatim; `--source` still narrows the results.
        let name = terms.join(" ");
        let ranked = self.ranked_for(&engine, &name, self.global.source.as_ref(), renderer).await;

        // A concept, not a package name (ADR-0091). `jii search markdown` used to answer with
        // a library literally called `markdown`; what was wanted was an editor. And a Russian
        // word like «браузер» is not a package name anywhere, so without this the answer was
        // nothing at all.
        //
        // The topic answers unless the query *is* one of the programs it would name: `docker`
        // and `steam` are terms of the container and gaming topics, but someone typing them
        // named a program and must get that program, not its neighbours. Anything else — even
        // when a package happens to carry the word, as npm's `markdown` library does — is the
        // collision topics exist to route around.
        let topic = if exact_only || show_all {
            None
        } else {
            crate::topics::Topics::load()
                .ok()
                .and_then(|t| t.lookup(&name).cloned())
                .filter(|t| !t.picks.iter().any(|p| p.eq_ignore_ascii_case(&name)))
        };
        let topical: Vec<PackageCandidate> = match &topic {
            Some(t) => {
                let spinner = crate::ui::Spinner::start(renderer, &crate::t!("offer.topic_looking"));
                let found = engine.topic_candidates(&t.picks).await;
                drop(spinner);
                found
            }
            None => Vec::new(),
        };
        let answering_topic = !topical.is_empty();

        if ranked.is_empty() && !answering_topic {
            renderer.error(&crate::t!("search.none", name = name));
            if let Some(msg) = engine.explain_miss(&name).await {
                renderer.info(&format!("  → {msg}"));
            }
            return Ok(());
        }
        // A search that actually surfaced something counts as exploring (unlocks silently in JSON).
        self.grant_achievement("explorer");
        if renderer.is_json() {
            // Machine output says *why* these are the answer: a topic reply is a different
            // kind of result from a name match, and a script must be able to tell.
            renderer.json_value(&match (&topic, answering_topic) {
                (Some(t), true) => {
                    serde_json::json!({ "topic": t.id, "candidates": topical })
                }
                _ => serde_json::json!({ "topic": serde_json::Value::Null, "candidates": ranked }),
            });
            return Ok(());
        }

        // Split off the name-squats: a search that leads with "a lhk 1st training project"
        // buries the real answer. They stay one keystroke away via `--all` (rule 5).
        let pool: &[PackageCandidate] = if answering_topic { &topical } else { &ranked };
        let (offered, aside): (Vec<&PackageCandidate>, Vec<&PackageCandidate>) = if show_all {
            (pool.iter().collect(), Vec::new())
        } else {
            pool.iter().partition(|c| !c.suspicious)
        };
        // Everything looked like a squat: say so by showing them anyway rather than nothing.
        let offered = if offered.is_empty() { pool.iter().collect() } else { offered };
        let owned: Vec<PackageCandidate> = offered.iter().map(|c| (*c).clone()).collect();

        let shown: Vec<crate::ui::story::Alternative> = offered
            .iter()
            .take(crate::ui::story::MAX_NUMBERED)
            .map(|c| crate::ui::story::Alternative::of(c, engine.source_nature(&c.source_id)))
            .collect();
        // A curated topic answer keeps the curator's order — its first pick is the answer.
        let best = if answering_topic {
            0
        } else {
            crate::engine::ranking::recommended_index(&owned)
                .unwrap_or(0)
                .min(shown.len().saturating_sub(1))
        };

        let lead = match &topic {
            Some(t) if answering_topic => {
                crate::t!("offer.topic", query = name.clone(), topic = t.title())
            }
            _ => crate::tn!("offer.found", offered.len() as u64, name = name.clone()),
        };
        renderer.info(&crate::ui::story::wrap(&lead, 2));
        crate::ui::story::verdict(renderer, &shown, best);
        crate::ui::story::alternatives(renderer, &shown, best);

        if answering_topic && !ranked.is_empty() {
            // Never quietly discard what the person literally typed (rule 5).
            renderer.info("");
            renderer.info(&crate::ui::story::wrap(
                &crate::t!("offer.topic_literal", name = name.clone()),
                2,
            ));
        }

        let mut aside_sources: Vec<String> = aside.iter().map(|c| c.source_id.clone()).collect();
        aside_sources.dedup();
        crate::ui::story::set_aside(renderer, &aside_sources, &format!("jii search {name} --all"));

        // Rule 2: a search that found the answer must not make the user retype it as an
        // install. Only ever asked on a real terminal — a piped or scripted `jii search` is
        // a question, and answering it "yes" would install software nobody asked for.
        if !crate::platform::Platform::detect().is_tty || self.global.no {
            return Ok(());
        }
        renderer.info("");
        let pick = crate::ui::prompt::decide(
            renderer,
            &crate::t!("offer.install_q"),
            shown.len(),
            best,
            &self.prompt_flags(engine.config().install.auto),
        );
        let index = match pick {
            crate::ui::prompt::Pick::Best => best,
            crate::ui::prompt::Pick::Other(i) => i,
            crate::ui::prompt::Pick::None => {
                renderer.info(&crate::t!("offer.cancelled"));
                return Ok(());
            }
        };
        let chosen = offered[index];
        // Pin the exact line the user pointed at (`name:source`, ADR-0031) so the install
        // path installs *that* one and never re-decides.
        let spec = format!("{}:{}", chosen.name, chosen.source_id);
        self.install_inner(&[spec], config, renderer, false, false).await
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
        // The same numbered list the offer uses, so a source reads identically wherever it
        // appears — `info` is the detail view, not a second dialect (ADR-0089).
        let shown: Vec<crate::ui::story::Alternative> = ranked
            .iter()
            .take(crate::ui::story::MAX_NUMBERED)
            .map(|c| crate::ui::story::Alternative::of(c, engine.source_nature(&c.source_id)))
            .collect();
        crate::ui::story::alternatives(renderer, &shown, 0);
        renderer.info("");
        renderer.info(&crate::t!(
            "info.recommended",
            source = palette.source(&best.source_id)
        ));
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
            renderer.info(&format!("{} {note}", palette.mark_info()));
        }
        Ok(())
    }
















    /// `jii ghtoken` — put the GitHub token where JII reads it, without the user having to
    /// know about umasks and here-docs (the owner's ask).
    ///
    /// The default form takes **no argument** and reads the token from stdin without echoing
    /// it. That is deliberate: a token passed as an argument is written to the shell history
    /// file and shows up in `ps` while the command runs, which is the same class of exposure
    /// ADR-0083 removed from `~/.bashrc`. The argument form still works — the owner asked for
    /// it — but it says what it costs and tells the user how to clear it.
    ///
    /// The file is created 0600 under a 0700 directory; the token is never echoed, logged, or
    /// printed back.
    fn gh_token(
        &self,
        token: Option<&str>,
        show: bool,
        forget: bool,
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
        let env = config.network.github_token_env.clone();
        let Some(path) = crate::secret::token_path(&env) else {
            renderer.error(&crate::t!("ghtoken.no_config_dir"));
            return Err(crate::error::JiiError::AlreadyReported);
        };

        if show {
            // Ask the providers, exactly as `doctor` does, so the answer covers a token that
            // lives in the environment or in `gh` and not only JII's own file.
            let engine = Engine::new(config)?;
            match engine.credential_origins().first() {
                Some((_, origin)) => renderer.success(&describe_origin(origin)),
                None => {
                    renderer.warn(&crate::t!("ghtoken.none"));
                    renderer.info(&crate::t!("ghtoken.none_hint"));
                }
            }
            return Ok(());
        }

        if forget {
            match std::fs::remove_file(&path) {
                Ok(()) => renderer.success(&crate::t!("ghtoken.forgotten", path = path.display().to_string())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    renderer.info(&crate::t!("ghtoken.nothing_to_forget", path = path.display().to_string()));
                }
                Err(e) => return Err(crate::error::JiiError::io(path, e)),
            }
            return Ok(());
        }

        let value = match token {
            Some(t) => {
                // Said plainly, and only once: the token is already in the history file by the
                // time this runs, so this is a "here is how to clean up", not a refusal.
                renderer.warn(&crate::t!("ghtoken.argument_warning"));
                t.trim().to_string()
            }
            None => crate::ui::prompt::read_secret(renderer, &crate::t!("ghtoken.prompt"))
                .map_err(|e| crate::error::JiiError::io(path.clone(), e))?,
        };
        if value.is_empty() {
            renderer.error(&crate::t!("ghtoken.empty"));
            return Err(crate::error::JiiError::AlreadyReported);
        }

        crate::secret::store(&path, &value).map_err(|e| crate::error::JiiError::io(path.clone(), e))?;
        renderer.success(&crate::t!("ghtoken.saved", path = path.display().to_string()));
        Ok(())
    }

    /// `jii lang [code]` — show or persist the interface language. With no argument it prints
    /// the saved setting and the choices; with `en`/`ru`/`auto` it writes `[ui] locale` to the
    /// config so the choice sticks across runs (the global `--lang` stays a per-run override).
    fn lang(
        &self,
        code: Option<&str>,
        mut config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
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
                self.grant_achievement("translator");
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
                Ok(Some(p)) => {
                    renderer.success(&crate::t!("cache.cleared", path = p.display().to_string()))
                }
                Ok(None) => renderer.info(&crate::t!("cache.already_empty")),
                Err(e) => {
                    renderer.error(&crate::t!("cache.clear_failed", error = e.to_string()))
                }
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
        &self,
        mut config: Config,
        renderer: &Renderer,
        first_run: bool,
        offer_doctor: bool,
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
        self.grant_achievement("wizard");
        Ok(())
    }

    /// Explain the optional GitHub token: what it buys you (a 60→5000 requests/hour lift) and
    /// where to put it. Read-only guidance — JII never mints or stores a token.
    ///
    /// This used to say "add `export GITHUB_TOKEN=…` to your `~/.bashrc`", which is how most
    /// of the internet phrases it and is the wrong advice from a package installer: an
    /// exported variable is handed to every process the user starts, including the unverified
    /// binaries JII itself installs, and `~/.bashrc` is world-readable by default. So the
    /// recommended routes are now `gh auth login` (nothing lands on disk for us) or a 0600
    /// file only JII reads; the env var stays documented for CI and one-off runs (ADR-0083).
    fn github_token_help(&self, config: &Config, renderer: &Renderer) {
        let palette = renderer.palette();
        let env = &config.network.github_token_env;
        renderer.heading(&crate::t!("setup.gh_header"));
        renderer.info(&crate::t!("setup.gh_benefit"));

        // Already have one? Say where it's coming from and stop. Only the two places JII can
        // check without a provider (env var, token file) — `doctor` sees `gh` too, via the
        // providers, and skips this whole block when any of them found a credential.
        if let Some(token) = crate::secret::resolve(env, None) {
            renderer.success(&crate::t!(
                "setup.gh_already",
                origin = describe_origin(&token.origin)
            ));
            return;
        }

        renderer.info("");
        renderer.info(&crate::t!("setup.gh_step_create"));

        // Route A — the GitHub CLI, when it's already here. Zero secrets for JII to hold.
        renderer.info("");
        renderer.info(&crate::t!("setup.gh_route_cli"));
        renderer.info(&palette.dim("   gh auth login"));

        // Route B — hand it to JII and let it do the file work. This is the route most
        // people should take: it writes the same 0600 file as the recipe below, and reads
        // the token without echoing it, so nothing lands in shell history either.
        let path = token_file_display(env);
        renderer.info("");
        renderer.info(&crate::t!("setup.gh_route_jii"));
        renderer.info(&palette.dim("   jii ghtoken"));

        // Route C — the same file, written by hand. Kept because it is worth knowing what
        // `jii ghtoken` actually does, and it works from a script.
        renderer.info("");
        renderer.info(&crate::t!("setup.gh_route_file", path = path.clone()));
        renderer.info(&palette.dim(&format!("   (umask 077; cat > {path})")));
        renderer.info(&palette.dim(&crate::t!("setup.gh_route_file_hint")));

        // Route C — the environment, scoped to what it's good for and no longer recommended
        // as a place to *keep* a token.
        renderer.info("");
        renderer.info(&crate::t!("setup.gh_route_env", env = env.clone()));
        renderer.info(&palette.dim(&format!("   {env}=ghp_your_token_here jii install owner/repo")));
        renderer.info("");
        // The `{env}` here is the same variable named just above — it was going out
        // unsubstituted, so the tester's log reads "Deliberately not `export {env}`".
        renderer.info(&palette.dim(&crate::t!("setup.gh_why_not_rc", env = env.clone())));
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
        self.grant_achievement("paper-trail");
        Ok(())
    }

    /// `how` for a package JII has no record of: either the system owns it, or nobody does.
    /// Never ends on "no record" alone — that was a dead end, and ADR-0080's rule is that a
    /// refusal must carry the next step.
    async fn how_unrecorded(
        &self,
        engine: &Engine,
        package: &str,
        renderer: &Renderer,
    ) -> crate::error::Result<()> {
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
            self.grant_achievement("paper-trail");
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
        self.grant_achievement("paper-trail");
        Ok(())
    }

    /// List software installed via jii.
    fn list(&self, audit: bool, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        let engine = Engine::new(config)?;
        // `jii list --audit` is the security view (#5): the same ledger, but with trust,
        // verification, and concerns per install. Folded in from the former `jii audit`.
        if audit {
            let out = self.audit_view(&engine, renderer);
            self.grant_achievement("auditor");
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
        let all_bosses_down =
            crate::achievements::BOSSES.iter().all(|b| store.is_unlocked(b.id));
        if all_bosses_down && store.unlock("boss-slayer") {
            newly.push("boss-slayer".to_string());
        }
    }

    /// Grant an achievement as a side effect of a real action, showing a one-time toast the
    /// first time it's earned. Best-effort and cosmetic: any failure to load or persist the
    /// ledger is swallowed so it can never break the surrounding command. Silent in JSON mode.
    /// Earned **quietly**, on purpose. A "✓ Achievement unlocked" toast in the middle of a
    /// `doctor` or a `search` is noise in the one place the user is reading for an answer —
    /// the owner's words: it looks childish, and achievements belong in `jii achievements`.
    /// They are still awarded; that command is where they show (ADR-0087). The boss fights
    /// keep their toast, where the badge *is* the payoff.
    fn grant_achievement(&self, id: &str) {
        let Ok(mut store) = crate::achievements::Achievements::load() else {
            return;
        };
        if store.unlock(id) {
            let mut newly = Vec::new();
            self.maybe_completionist(&mut store, &mut newly);
            let _ = store.save();
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
        let all_endings =
            !endings.is_empty() && endings.iter().all(|e| store.counter(&format!("{id}-{e}")) > 0);
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
    fn record_install(
        &self,
        batch: &[crate::engine::BatchPlan],
        count: usize,
        pinned: bool,
        renderer: &Renderer,
    ) {
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
        renderer.info(&palette.heading(&crate::t!(
            "achieve.header",
            earned = earned,
            total = total
        )));
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
            let mark = if unlocked { palette.good(a.icon) } else { palette.dim(a.icon) };
            let title = if unlocked { palette.heading(&title) } else { palette.dim(&title) };
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
        &self,
        version: Option<&str>,
        all: bool,
        since: Option<&str>,
        renderer: &Renderer,
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
        &self,
        config_auto: bool,
        kind: &crate::model::StrategyKind,
        assume_yes: bool,
        renderer: &Renderer,
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
        &self,
        engine: &Engine,
        candidate: &PackageCandidate,
        assume_yes: bool,
        renderer: &Renderer,
    ) -> bool {
        let strategies = engine
            .install_strategies(&candidate.source_id, candidate)
            .await;
        // Prefer a file we can actually edit; else a snippet we can only show.
        if let Some(strat) = strategies
            .iter()
            .find(|s| matches!(s.kind, crate::model::StrategyKind::EditFile { .. }))
        {
            self.apply_edit_file(
                engine.config().install.auto,
                &strat.kind,
                assume_yes,
                renderer,
            )
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
/// How to label a step that needs root, in the words of *this* machine.
///
/// It used to be the fixed string "[needs sudo]", which was a lie on three kinds of
/// host at once: a root shell (nothing is needed), a `doas` distro, and a container
/// with no helper at all. Names the mechanism that will actually be used.
/// A size a person reads at a glance: "1.2 MB", not "1258291".
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["size.b", "size.kb", "size.mb", "size.gb"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    // Whole numbers below a kilobyte; one decimal above, because "1 MB" hides the difference
    // between 1.0 and 1.9 and that is exactly the range people are deciding in.
    let number =
        if unit == 0 { format!("{value:.0}") } else { format!("{value:.1}").trim_end_matches(".0").to_string() };
    format!("{number} {}", crate::t!(UNITS[unit]))
}

fn root_label() -> String {
    use crate::platform::ElevationKind;
    match crate::platform::Platform::detect().elevation_kind() {
        ElevationKind::AlreadyRoot => crate::t!("common.as_root"),
        ElevationKind::Missing => crate::t!("common.needs_root"),
        kind => crate::t!("common.needs_root_via", helper = kind.helper().unwrap_or("root")),
    }
}

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
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
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
        let renderer =
            Renderer::new(ColorChoice::Never, false, crate::config::OutputMode::Friendly);
        cli.apply_edit_file(false, &kind, false, &renderer).await;

        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "original\n");
        assert!(!cfg.with_file_name("home.nix.jii-bak").exists());
    }



    #[test]
    fn cli_definition_is_valid() {
        // clap's own validation of the whole command tree (conflicting args, bad flags,
        // duplicate subcommands…). Cheap insurance that the CLI surface always parses.
        use clap::CommandFactory;
        Cli::command().debug_assert();
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
















}
