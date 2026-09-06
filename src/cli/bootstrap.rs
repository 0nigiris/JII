// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Setting up a package manager that isn't here yet.
//!
//! A `can_search` source answers over the network, so Flatpak can rank first on a machine
//! where Flatpak isn't installed. Rather than build a command that would fail, JII offers to
//! set the manager up and then install the app through it (ADR-0061/0065). A bare manager
//! name (`jii flatpak`) means the same thing and routes here directly.
//!
//! Two rules shape everything below. A manager JII cannot package is installed only by its
//! own upstream script, whose contents JII cannot vouch for — so that is said plainly and
//! asked, never assumed (ADR-0062). And a manager that cannot be set up must not take the
//! package down with it: the flow walks down the ranking to a source that works here, never
//! promoting an unverified one (ADR-0088).

use super::{Cli, run_plain_command};
use crate::provider::Bootstrap;
use crate::config::Config;
use crate::engine::Engine;
use crate::model::PackageCandidate;
use crate::ui::{Renderer, prompt};

impl Cli {
    /// Split ecosystem-manager names out of an install request and bootstrap each (#4),
    /// returning the remaining ordinary packages. A name counts as a manager only when it is
    /// unpinned (no `:source`, no `--source`) and matches a known ecosystem id; a cheap pure
    /// id check means an ordinary `jii vlc` pays nothing (no catalog probe).
    pub(super) async fn route_managers(
        &self,
        engine: &Engine,
        packages: &[String],
        config: Config,
        renderer: &Renderer,
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
    pub(super) async fn bootstrap_ecosystem(
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
                match engine.first_bootstrap_package(names).await {
                    // The source is pinned (`pipx:apt`) because only a *usable* source can set a
                    // manager up — never the absent manager itself (ADR-0066). route_managers=
                    // false: the bootstrap package's name (e.g. `pipx`) may itself be a manager
                    // id — routing it would loop. Box::pin breaks the async recursion cycle.
                    Some((pkg, source)) => {
                        let spec = format!("{pkg}:{source}");
                        Box::pin(self.install_inner(&[spec], config, renderer, false, false))
                            .await?;
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
                            self.grant_achievement("bootstrapper");
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
                if self.offer_script_bootstrap(engine, eco.id, label, cmd, shell, renderer).await {
                    self.grant_achievement("bootstrapper");
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
    pub(super) async fn bootstrap_missing_managers(
        &self,
        engine: &Engine,
        chosen: Vec<PackageCandidate>,
        alternates: Vec<Vec<PackageCandidate>>,
        config: &Config,
        renderer: &Renderer,
        assume_yes: bool,
    ) -> crate::error::Result<(Vec<PackageCandidate>, bool)> {
        let eco = engine.ecosystem_catalog().await;
        let status_of = |id: &str| eco.iter().find(|e| e.id == id);

        // Fast path: nothing chosen needs an absent manager → leave the batch untouched.
        if !chosen
            .iter()
            .any(|c| status_of(&c.source_id).is_some_and(|e| !e.installed))
        {
            return Ok((chosen, false));
        }

        let effective_auto = self.global.auto || config.install.auto;
        let flags = self.prompt_flags(effective_auto).with_yes(assume_yes);
        // Decide once per manager (a batch may want several apps from the same one).
        let mut decided: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
        let mut survivors: Vec<PackageCandidate> = Vec::with_capacity(chosen.len());
        let mut alternates = alternates.into_iter();
        // Set when JII offered to set a manager up and could not — as opposed to the user
        // declining, which is an answer, not a failure.
        let mut blocked = false;

        for cand in chosen {
            let others = alternates.next().unwrap_or_default();
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
                    let question =
                        crate::t!("install.bootstrap_confirm", manager = manager, app = cand.name.clone());
                    if !self.global.dry_run && !prompt::confirm(renderer, &question, true, &flags) {
                        false
                    } else {
                        // A "no" above is the user's decision; a `false` here is JII failing to
                        // do what it offered. Only the second makes the run a failure.
                        let done = self
                            .set_up_manager(engine, &cand.source_id, manager, names, config.clone(), renderer)
                            .await?;
                        blocked |= !done;
                        done
                    }
                }
            };
            decided.insert(cand.source_id.clone(), ok);
            if ok && !self.global.dry_run {
                // JII just set up a manager that wasn't here before — the T6 bootstrap path.
                self.grant_achievement("bootstrapper");
            }
            if ok {
                survivors.push(cand);
                continue;
            }
            // The manager isn't here and couldn't be set up. Don't stop at "skipped" — that is
            // the dead end the tester hit and could not read ("Я нихуя не понял что тут
            // произошло"). Look down the ranking for a source that *works on this machine*.
            //
            // Unverified sources are not eligible: falling back to them silently is exactly
            // what "auto never installs untrusted" forbids, and on the tester's box the
            // unverified `htop` on crates.io is an HTML-to-PDF converter. If nothing
            // qualifies, say so with the command that would set the manager up by hand.
            match self.first_usable_alternate(engine, &others).await {
                Some(next) => {
                    renderer.info(&crate::t!(
                        "install.bootstrap_fallback",
                        app = cand.name.clone(),
                        manager = manager,
                        source = next.source_id.clone()
                    ));
                    survivors.push(next);
                }
                None => {
                    renderer.info(&crate::t!("install.bootstrap_skipped_app", app = cand.name.clone()));
                    renderer.info(&crate::t!(
                        "install.bootstrap_by_hand",
                        manager = manager,
                        id = cand.source_id.clone()
                    ));
                }
            }
        }
        Ok((survivors, blocked))
    }

    /// The best runner-up that could actually install here: its manager is present, and its
    /// trust is not "unverified".
    ///
    /// Both conditions matter. The first because offering a source whose CLI is missing just
    /// moves the dead end one step along; the second because an automatic fall-back to an
    /// unverified name-squat is precisely what the trust barrier exists to prevent — the
    /// user asked for `htop`, not for whatever a stranger published under that name.
    async fn first_usable_alternate(
        &self,
        engine: &Engine,
        others: &[PackageCandidate],
    ) -> Option<PackageCandidate> {
        for candidate in others {
            if candidate.trust == crate::model::TrustLevel::Untrusted {
                continue;
            }
            if engine.source_available(&candidate.source_id).await {
                return Some(candidate.clone());
            }
        }
        None
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
        &self,
        engine: &Engine,
        source_id: &str,
        manager: &str,
        cmd: &str,
        shell: Option<crate::provider::ShellSetup>,
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
        let flags = prompt::PromptFlags { auto: false, yes: false, no: false };
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
        // The installer just put a new binary on disk; forget what we knew about it.
        crate::provider::forget_availability();
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
    fn offer_shell_line(
        &self,
        manager: &str,
        setup: crate::provider::ShellSetup,
        renderer: &Renderer,
    ) {
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
        if !self.interactive(renderer)
            || !prompt::confirm(renderer, &crate::t!("install.shell_confirm"), true, &flags)
        {
            renderer.info(&crate::t!("install.shell_manual", line = line.clone()));
            return;
        }
        match crate::shellrc::append_line(&rc, manager, &line) {
            Ok(()) => renderer.success(&crate::t!(
                "install.shell_added",
                file = rc.display().to_string()
            )),
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
        &self,
        engine: &Engine,
        source_id: &str,
        manager: &str,
        names: &[&'static str],
        config: Config,
        renderer: &Renderer,
    ) -> crate::error::Result<bool> {
        let Some((pkg, from)) = engine.first_bootstrap_package(names).await else {
            // Was `app = manager`, which printed "skipping Snap" where the app's own name
            // belonged — and then "Skipped htop." underneath it. The message no longer names
            // an app at all; the caller says what happened to it.
            renderer.error(&crate::t!("install.bootstrap_no_pkg", manager = manager));
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
        // The manager was just installed, so anything remembered about "is it here?" is now
        // out of date — including the answer we are about to ask for.
        crate::provider::forget_availability();
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
}
