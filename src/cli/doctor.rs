//! `jii doctor` — diagnose the host, then offer to fix what it found.
//!
//! Three layers, in order: each source's live health (asked of the providers), the host
//! environment (pure verdicts over [`SystemFacts`] gathered once with I/O), and the curated
//! distro suggestions from `data/recommend/catalog.toml`. Only the last two are actionable,
//! and both are offered as one walk-through in the house voice — a numbered step at a time,
//! each answerable with yes, no, or "all the rest" (ADR-0089).
//!
//! The checks themselves are deliberately pure: [`gather_system_facts`] does every probe and
//! [`system_checks`] turns facts into wording, so the verdicts are unit-tested without a host.

use super::{Cli, run_plain_command, run_shell_command};
use crate::config::Config;
use crate::engine::Engine;
use crate::ui::{Renderer, prompt};

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
    /// The env var that names the token (and, lowercased, its file — see `crate::secret`).
    token_env: String,
    /// Where a token was found, already rendered for display, or `None` if there is none.
    /// Provenance only: the value itself never reaches `doctor`.
    token_origin: Option<String>,
    /// A token file that other users on this machine can read. `Some(path)` is a finding —
    /// the whole point of moving off `~/.bashrc` is that the secret stops being readable
    /// by everything, and a 0644 file gives that back (ADR-0083).
    token_file_exposed: Option<std::path::PathBuf>,
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

    // A compiler for Homebrew — only meaningful once brew is installed. Homebrew pours a
    // prebuilt bottle when it has one and compiles when it doesn't, so a missing toolchain
    // isn't broken, just a formula away from failing. JII can install it; brew only mentions it.
    if f.brew {
        checks.push(if f.build_tools {
            SystemCheck::pass(crate::t!("check.build_ok"))
        } else {
            SystemCheck::warn(
                crate::t!("check.build_missing"),
                crate::t!("check.build_advice"),
            )
            .fixable(Fix::Install("gcc"))
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
            })
        });
    }

    // GitHub token — a rate-limit papercut, never a blocker. Report *where* it came from:
    // "a token is set" used to be the whole answer, which left a user with two of them
    // (a stale export and a fresh file) guessing which one was in play.
    checks.push(match &f.token_origin {
        Some(origin) => SystemCheck::pass(crate::t!("check.token_ok", origin = origin.clone())),
        None => SystemCheck::warn(
            crate::t!("check.token_missing", env = f.token_env),
            crate::t!("check.token_advice", path = token_file_display(&f.token_env)),
        ),
    });

    // A token file the rest of the machine can read is worth saying out loud, and is one
    // `chmod` away from fixed — so doctor offers to run it.
    if let Some(path) = &f.token_file_exposed {
        let shown = path.display().to_string();
        checks.push(SystemCheck {
            ok: false,
            critical: false,
            label: crate::t!("check.token_perms", path = shown.clone()),
            advice: Some(crate::t!("check.token_perms_advice", path = shown.clone())),
            fix: Some(Fix::Command {
                argv: vec!["chmod".into(), "600".into(), shown.clone()],
                show: format!("chmod 600 {shown}"),
            }),
        });
    }

    checks
}
/// The path a token for `env` is read from, for advice text. Falls back to the literal
/// `~/.config/jii/...` when no config dir resolves — advice must still be copy-pasteable.
pub(super) fn token_file_display(env: &str) -> String {
    match crate::secret::token_path(env) {
        Some(p) => p.display().to_string(),
        None => format!("~/.config/jii/{}", env.to_ascii_lowercase()),
    }
}
/// Probe host facts for `doctor` (the one place these environment I/O calls live). Runs
/// the independent tool/network probes concurrently so `doctor` stays snappy.
async fn gather_system_facts(token_env: &str, token_origin: Option<String>) -> SystemFacts {
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
    // Check the file's mode whether or not it is the token actually in use: an exposed
    // secret sitting next to config.toml is worth reporting even when an env var wins.
    let token_file_exposed = crate::secret::token_path(token_env)
        .filter(|p| p.exists() && crate::secret::is_world_readable(p));

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
        token_origin,
        token_file_exposed,
    }
}
/// Render a credential's provenance for `doctor` — the *place*, never the secret.
pub(super) fn describe_origin(origin: &crate::secret::Origin) -> String {
    match origin {
        crate::secret::Origin::Env(var) => crate::t!("check.token_from_env", env = var.clone()),
        crate::secret::Origin::File(path) => {
            crate::t!("check.token_from_file", path = path.display().to_string())
        }
        crate::secret::Origin::Helper(cmd) => {
            crate::t!("check.token_from_helper", cmd = cmd.clone())
        }
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

impl Cli {
    /// Report source availability, latency and health (per-source), then a set of **system
    /// checks** about the host (network, common tools, `PATH`, Flathub, GitHub token). In an
    /// interactive terminal `doctor` then becomes a **setup questionnaire** (ADR-0041): each
    /// fixable check and each distro-appropriate suggestion (RPM Fusion, codecs, fonts…) is
    /// offered as a yes/no question and, on "yes", applied on the spot. It stays read-only in
    /// `--json`, under `-n/--no`, or with no TTY (Analyze → Explain → Ask → Apply).
    pub(super) async fn doctor(&self, config: Config, renderer: &Renderer) -> crate::error::Result<()> {
        self.grant_achievement("doctor");
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
            let mark =
                if d.available { palette.good(palette.mark_ok()) } else { palette.dim(palette.mark_bad()) };
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
        // Ask the providers where their credential came from rather than reading the env var
        // here: a token in JII's own file or in `gh` counts just as much (ADR-0083), and the
        // core stays out of the business of knowing which source that is.
        let token_origin = engine.credential_origins().first().map(|(_, o)| describe_origin(o));
        if gh_present && token_origin.is_none() {
            renderer.info("");
            self.github_token_help(&config_for_fix, renderer);
        }

        // System checks: probe the host environment (network, common tools, PATH, Flathub).
        let facts = gather_system_facts(&token_env, token_origin).await;
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
        let distro_ids = &crate::platform::Platform::detect().distro_ids;
        let all_suggestions = catalog
            .as_ref()
            .map(|c| c.for_distro(distro_ids))
            .unwrap_or_default();

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
        // One walk-through, numbered: the old form was a wall where every line looked
        // equally important and the command to act on it hid at the end of it (ADR-0089).
        let total = fixes.len() + suggestions.len();
        let mut nth = 0usize;
        // Set by answering `a`: the rest is applied without asking again.
        let mut take_all = false;
        renderer.info("");
        renderer.info(&crate::ui::story::wrap(&crate::tn!("doctor.found", total as u64), 2));

        // A) Fixable system checks.
        for (check, fix) in fixes {
            nth += 1;
            let (headline, detail) = match fix {
                Fix::Install(pkg) => {
                    (crate::t!("doctor.q_install", pkg = pkg), check.label.clone())
                }
                Fix::PathExport { dir } => (
                    crate::t!("doctor.q_add_path", dir = dir.display()),
                    check.advice.clone().unwrap_or_default(),
                ),
                Fix::Command { show, .. } => {
                    (crate::t!("doctor.q_fix", label = check.label.clone()), show.clone())
                }
            };
            crate::ui::story::step_header(
                renderer,
                nth,
                total,
                &crate::t!("doctor.cat_system"),
                &headline,
                &[detail],
            );
            if !take_all {
                match prompt::step(renderer, &format!("  {}", crate::t!("doctor.apply_q")), &flags) {
                    prompt::Step::Skip => continue,
                    prompt::Step::All => take_all = true,
                    prompt::Step::Yes => {}
                }
            }
            self.apply_fix(fix, config.clone(), renderer).await?;
        }

        // B) Curated, distro-aware suggestions (the folded-in recommend catalog).
        //    A dependent entry (codecs/VLC) enables its prerequisite repo (RPM Fusion) first,
        //    so it never fails with a bare "not found" because the repo was skipped (ADR-0055).
        let mut enabled_repos: std::collections::HashSet<String> = std::collections::HashSet::new();
        for r in &suggestions {
            nth += 1;
            let mut lines = vec![r.why().to_string()];
            if !r.packages.is_empty() {
                lines.push(crate::t!(
                    "doctor.will_install",
                    packages = crate::ui::story::join_and(&r.packages)
                ));
            }
            if let Some(note) = r.note() {
                lines.push(note.to_string());
            }
            crate::ui::story::step_header(
                renderer,
                nth,
                total,
                &crate::t!(&format!("doctor.cat_{}", r.category)),
                r.title(),
                &lines,
            );
            if !take_all {
                match prompt::step(renderer, &format!("  {}", crate::t!("doctor.apply_q")), &flags) {
                    prompt::Step::Skip => continue,
                    prompt::Step::All => take_all = true,
                    prompt::Step::Yes => {}
                }
            }
            // Enable a prerequisite repo first (e.g. RPM Fusion for codecs/VLC). The exact
            // command is shown before it runs (apply_suggestion prints it); the "yes" to the
            // dependent is the consent for its prerequisite. Deduped within the run, and
            // skipped when the prerequisite is already present (pure decision in `recommend`).
            if let Some(prereq) =
                crate::recommend::prerequisite(r, &all_suggestions, &installed, &enabled_repos)
            {
                renderer.info(&format!("  {}", crate::t!("doctor.prereq", title = prereq.title())));
                if let Some(note) = prereq.note() {
                    renderer.info(&crate::ui::story::wrap(&crate::t!("common.note", note = note), 4));
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
        let distro_ids = &crate::platform::Platform::detect().distro_ids;
        let entries = catalog.for_distro(distro_ids);
        if entries.is_empty() {
            return;
        }

        // The read-only twin of the walk-through: nobody is here to answer, so each entry is
        // stated once with the command that does it, instead of the old one-line-per-entry
        // wall where the command hid behind a middle dot at the end of a 200-column line.
        let palette = renderer.palette();
        renderer.info("");
        renderer.info(&crate::ui::story::wrap(
            &crate::tn!("doctor.found", entries.len() as u64),
            2,
        ));
        let mut last_category: Option<&str> = None;
        for (i, r) in entries.iter().enumerate() {
            if last_category != Some(r.category.as_str()) {
                renderer.info("");
                renderer.info(&format!(
                    "  {}",
                    palette.heading(&crate::t!(&format!("doctor.cat_{}", r.category)))
                ));
                last_category = Some(r.category.as_str());
            }
            let how = if !r.packages.is_empty() {
                format!("jii {}", r.packages.join(" "))
            } else if let Some(manual) = &r.manual {
                crate::t!("how.run", cmd = manual)
            } else {
                String::new()
            };
            renderer.info("");
            renderer.info(&format!("  {} {}", palette.dim(&format!("{}", i + 1)), r.title()));
            renderer.info(&crate::ui::story::wrap(r.why(), 5));
            if let Some(note) = r.note() {
                renderer.info(&crate::ui::story::wrap(
                    &palette.dim(&crate::t!("common.note", note = note)),
                    5,
                ));
            }
            if !how.is_empty() {
                renderer.info(&format!("     {}", palette.dim(&how)));
            }
        }
        renderer.info("");
        renderer.info(&crate::ui::story::wrap(&crate::t!("doctor.suggestions_info"), 2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            token_origin: Some("from the GITHUB_TOKEN environment variable".to_string()),
            token_file_exposed: None,
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
    fn a_missing_token_points_at_the_file_to_create_not_at_bashrc() {
        let mut f = facts_all_good();
        f.token_env = "GH_PAT".to_string();
        f.token_origin = None;
        let checks = system_checks(&f);
        let token_check = checks.iter().find(|c| !c.ok && c.label.contains("token")).unwrap();
        let advice = token_check.advice.as_deref().unwrap();
        // The file to create is named, and the old "export it in your shell profile"
        // advice is gone for good (ADR-0083).
        assert!(advice.contains("gh_pat"), "names the token file: {advice}");
        assert!(!advice.contains("export"), "no shell-profile export: {advice}");
        assert!(!advice.contains("bashrc"), "no shell-profile export: {advice}");
    }

    #[test]
    fn a_found_token_is_reported_by_provenance_and_never_by_value() {
        let f = facts_all_good();
        let checks = system_checks(&f);
        let token_check = checks.iter().find(|c| c.label.contains("token")).unwrap();
        assert!(token_check.ok);
        assert!(token_check.label.contains("GITHUB_TOKEN"), "says where: {}", token_check.label);
    }

    #[test]
    fn a_world_readable_token_file_is_flagged_and_fixable() {
        let mut f = facts_all_good();
        f.token_file_exposed = Some(std::path::PathBuf::from("/home/x/.config/jii/github_token"));
        let checks = system_checks(&f);
        let perms = checks.iter().find(|c| c.label.contains("github_token")).unwrap();
        assert!(!perms.ok);
        match &perms.fix {
            Some(Fix::Command { show, .. }) => assert!(show.starts_with("chmod 600 ")),
            other => panic!("expected a chmod fix, got {other:?}"),
        }
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
