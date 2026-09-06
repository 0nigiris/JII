// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The install flow: search → rank → offer → plan → one confirmation → one execution.
//!
//! Everything a `jii <name>` does, in one place. The shape is fixed by three commitments:
//! a batch is planned and run as **one** operation (one preview, one confirmation, one root
//! escalation), a package that resolves to nothing never cancels the rest of the batch, and
//! nothing is executed that was not previewable first (`--dry-run` shows exactly it).
//!
//! Presentation is the house voice (ADR-0089): what was found is stated in a sentence, the
//! alternatives are numbered, and the prompt takes a number — so the user can always take a
//! different source without re-running the command.

use super::{Cli, DeclarativePref, human_size, repo_label, root_label,
            url_query_encode, version_or_unknown};
use crate::config::Config;
use crate::engine::Engine;
use crate::model::{PackageCandidate, Query};
use crate::ui::{Renderer, prompt};

impl Cli {
    /// Install path (one or many packages): for each package search → rank → pick best,
    /// then let the engine group + optimize the chosen candidates into batched plans, and
    /// run them as **one** operation (one preview, one confirmation, one root escalation,
    /// one execution). A not-found package never cancels the rest (requirement: it is
    /// reported and the user is offered to continue).
    pub(super) async fn install(
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
    pub(super) async fn install_inner(
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
        // Index-aligned with `chosen`: the runner-up candidates for each app, best first.
        let mut alternates: Vec<Vec<PackageCandidate>> = Vec::new();
        let mut not_found: Vec<String> = Vec::new();
        let mut chose_interactively = false;
        // Whether the offer above already said what will be installed and why — the
        // friendly one-line preview would then repeat it back, which rule 4 forbids.
        let mut told_story = false;
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
                // A second full round of every source, and it used to run in total silence —
                // the terminal sat there with nothing on it. On a Gentoo container that
                // silence was the whole visible symptom of a "hang" (ADR-0086): say what is
                // happening, and say that it is a *wider* look, not a repeat of the first.
                let spinner = crate::ui::Spinner::start(renderer, &crate::t!("install.searching_wider"));
                ranked = engine.broaden_search(name).await;
                spinner.stop().await;
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
                    // `jii htop --run` on an already-installed htop should still start it —
                    // "install and run" with the install already done is just "run" (this
                    // execs, so it doesn't come back).
                    if self.global.run
                        && single
                        && !self.global.dry_run
                        && let Some(candidate) =
                            ranked.iter().find(|c| c.source_id == record.source_id)
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
                let rec = crate::engine::ranking::recommended_index(&ranked);
                if rec.is_none() {
                    renderer.warn(&crate::t!("install.no_trusted_match", name = name));
                }
                // The house voice (ADR-0089): say what was found and why this one wins, then
                // number the alternatives and let one keypress take any of them. This replaces
                // the arrow-key chooser — a menu answers "which line", prose answers "why".
                let shown: Vec<crate::ui::story::Alternative> = ranked
                    .iter()
                    .take(crate::ui::story::MAX_NUMBERED)
                    .map(|c| crate::ui::story::Alternative::of(c, engine.source_nature(&c.source_id)))
                    .collect();
                let rec = rec.unwrap_or(0).min(shown.len().saturating_sub(1));
                renderer.info(&crate::ui::story::wrap(
                    &crate::tn!("offer.found", ranked.len() as u64, name = name.clone()),
                    2,
                ));
                crate::ui::story::verdict(renderer, &shown, rec);
                crate::ui::story::alternatives(renderer, &shown, rec);
                renderer.info("");
                told_story = true;
                let index = match prompt::decide(
                    renderer,
                    &format!("  {}", crate::t!("offer.install_q")),
                    shown.len(),
                    rec,
                    &self.prompt_flags(effective_auto),
                ) {
                    prompt::Pick::Best => rec,
                    prompt::Pick::Other(i) => i,
                    prompt::Pick::None => {
                        renderer.info(&format!("  {}", crate::t!("offer.cancelled")));
                        return Ok(());
                    }
                };
                chose_interactively = true;
                ranked.remove(index)
            } else {
                // No question to ask — a pinned source, `--auto`/`--yes`, or no terminal — but
                // the explanation is still owed. A scripted run gets the same sentence and the
                // same list, just without the prompt at the end (rule 2 is about never *hiding*
                // the alternatives; it can't conjure someone to answer).
                if single && ranked.len() > 1 && !renderer.is_json() {
                    let shown: Vec<crate::ui::story::Alternative> = ranked
                        .iter()
                        .take(crate::ui::story::MAX_NUMBERED)
                        .map(|c| {
                            crate::ui::story::Alternative::of(c, engine.source_nature(&c.source_id))
                        })
                        .collect();
                    renderer.info(&crate::ui::story::wrap(
                        &crate::tn!("offer.found", ranked.len() as u64, name = name.clone()),
                        2,
                    ));
                    // The pointer marks what will actually be installed: the top rank.
                    crate::ui::story::verdict(renderer, &shown, 0);
                    crate::ui::story::alternatives(renderer, &shown, 0);
                    told_story = true;
                }
                ranked.remove(0)
            };
            chosen.push(best);
            // Keep what came second, third… If the winner turns out to need a manager this
            // machine can't set up, these are the way out of the dead end (ADR-0088).
            alternates.push(ranked);
        }

        // 1a-bis. T6 (ADR-0065): a chosen candidate may come from an ecosystem manager that isn't
        //   installed — a `can_search` source (Flatpak, Snap, cargo, npm, pipx, go, brew) answered
        //   over the network, so an uninstalled-Flatpak `obsidian` outranks the last-resort GitHub
        //   binary. Set the manager up first (then the app installs through it) instead of building
        //   a command that would fail; a declined or script-only manager drops its apps with a note.
        //   github has no `ecosystem()`, so it never routes here — it stays the plain binary.
        let blocked;
        (chosen, blocked) = self
            .bootstrap_missing_managers(&engine, chosen, alternates, &config, renderer, assume_yes)
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
                    let strategies = engine
                        .install_strategies(&candidate.source_id, candidate)
                        .await;
                    if !strategies.is_empty() {
                        let palette = renderer.palette();
                        let labels: Vec<String> = strategies
                            .iter()
                            .map(|s| format!("{}  —  {}", s.label, palette.dim(&s.hint)))
                            .collect();
                        let header =
                            crate::t!("nix.strategy_header", name = candidate.name.clone());
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
                                    self.apply_edit_file(
                                        engine.config().install.auto,
                                        kind,
                                        assume_yes,
                                        renderer,
                                    )
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
                    if self
                        .route_declarative(&engine, &candidate, assume_yes, renderer)
                        .await
                    {
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
            // Before the browse links: the name may have been a *concept* all along. If the
            // topic catalog knows the word, point at the search that answers it — a curated
            // answer beats two search-engine URLs (ADR-0091).
            let mut answered_by_topic: Vec<&String> = Vec::new();
            if !renderer.is_json()
                && let Ok(topics) = crate::topics::Topics::load()
            {
                for name in &not_found {
                    if let Some(topic) = topics.lookup(name) {
                        renderer.info(&crate::ui::story::wrap(
                            &crate::t!(
                                "install.try_topic",
                                name = name.clone(),
                                topic = topic.title()
                            ),
                            2,
                        ));
                        answered_by_topic.push(name);
                    }
                }
            }
            // Even after broadening and (interactively) the repo picker, nothing resolved. Don't
            // dead-end: hand the user links to *browse* for it themselves and read the project's
            // own install docs (owner ask) — GitHub search finds the repo, Flathub a desktop app.
            // Skipped in JSON (machine output) and when a `--source` was pinned (the miss is about
            // that one source, not "where do I find this at all").
            if !renderer.is_json() && self.global.source.is_none() {
                let palette = renderer.palette();
                // A name the topic catalog answered already has a better next step than two
                // search-engine URLs; only the genuinely unknown ones need those.
                let unknown: Vec<&String> =
                    not_found.iter().filter(|n| !answered_by_topic.contains(n)).collect();
                if unknown.is_empty() {
                    return Ok(());
                }
                renderer.info(&crate::t!("install.browse_hint"));
                for name in unknown {
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
            // `blocked` covers the third case: the package *was* found, but its only source
            // needed a manager JII could not set up here (the tester's openSUSE run, which
            // said "Skipped htop." and then exited 0 as if all were well).
            return if not_found.is_empty() && !blocked {
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
        // The offer ends on a list; whatever follows needs air between it and them.
        if told_story {
            renderer.info("");
        }
        if renderer.is_friendly() && !self.global.dry_run {
            if !told_story {
                self.preview_batch_friendly(&batch, &engine, renderer);
            }
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
            self.grant_achievement("dry-runner");
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
        //    When the offer told the story, the only thing left to say beforehand is what it
        //    costs — the password and the download — on one dim line (rule 3).
        if told_story {
            let mut facts: Vec<String> = Vec::new();
            if batch.iter().any(|b| b.plan.needs_root()) {
                facts.push(root_label());
            }
            if let Some(bytes) = batch.iter().filter_map(|b| b.plan.download_size).reduce(|a, b| a + b) {
                facts.push(human_size(bytes));
            }
            if !facts.is_empty() {
                renderer.info(&renderer.palette().dim(&format!("  {}", facts.join(" · "))));
            }
        }
        engine.install_batch(&batch, renderer).await?;
        if let Some(line) = self.finish_line(&engine, &batch, told_story) {
            renderer.info("");
            renderer.info(&crate::ui::story::wrap(&line, 2));
        } else {
            renderer.success(&crate::t!("install.installed", names = installed.join(", ")));
        }
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
        renderer.info(&renderer.palette().dim(&crate::t!("install.run_starting", cmd = argv.join(" "))));
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
    fn preview_batch_friendly(
        &self,
        batch: &[crate::engine::BatchPlan],
        engine: &Engine,
        renderer: &Renderer,
    ) {
        let palette = renderer.palette();
        for bp in batch {
            let sudo = if bp.plan.needs_root() {
                palette.dim(&root_label())
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
/// The last line of an install told in the house voice: what landed, and how to start it.
    ///
    /// `None` whenever the offer wasn't told (a batch, `--auto`, a pinned source) — the old
    /// "Installed a, b, c." is right there and repeating a name is not an improvement. Rule 5
    /// puts a way forward on screen: the launch command when the source knows one, and
    /// otherwise the spec that would install the runner-up instead.
    fn finish_line(
        &self,
        engine: &Engine,
        batch: &[crate::engine::BatchPlan],
        told_story: bool,
    ) -> Option<String> {
        if !told_story {
            return None;
        }
        let [bp] = batch else { return None };
        let [candidate] = bp.candidates.as_slice() else { return None };
        let version = candidate.version.as_ref().map(|v| v.0.clone()).unwrap_or_default();
        Some(match engine.launch_command(candidate) {
            Some(argv) => crate::t!(
                "offer.done_run",
                name = candidate.name.clone(),
                version = version,
                cmd = argv.join(" ")
            ),
            None => crate::t!("offer.done", name = candidate.name.clone(), version = version),
        })
    }

    /// The GitHub-style by-name repo picker (ADR-0053): search the forges for `name`, show the
    /// top matches, and let the user pick one — with a "show more" entry that pages forever.
    /// Picking resolves the repo's latest release into an installable candidate (returned for
    /// the normal preview→confirm→install flow). Returns `None` on cancel, nothing found, or if
    /// every pick published no installable Linux binary. Only reached in an interactive session.
    async fn repo_picker(
        &self,
        engine: &Engine,
        name: &str,
        renderer: &Renderer,
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
                    renderer.info(&crate::t!(
                        "install.gh_corrected",
                        name = name,
                        fixed = variant.clone()
                    ));
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
}
