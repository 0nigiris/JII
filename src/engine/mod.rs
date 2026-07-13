//! The engine orchestrates the pipeline: `search → rank → plan → execute`.
//!
//! It is the only component that holds providers, the privilege layer and the
//! registry, and the only place that executes anything or writes the registry.
//! It operates purely on the model.

pub mod ranking;

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::cache::Cache;
use crate::config::Config;
use crate::error::{JiiError, Result};
use crate::model::{
    Health, InstallPlan, InstallStrategy, InstalledRecord, MatchMode, PackageCandidate, PkgVersion,
    Query, RepoHit, TrustLevel,
};
use crate::privilege::Privilege;
use crate::provider::ProviderRegistry;
use crate::registry::Registry;
use crate::ui::Renderer;

/// Result of a search across providers: candidates plus any sources that failed
/// (so the UI can report them without failing the whole search).
pub struct SearchResult {
    pub candidates: Vec<PackageCandidate>,
    /// `(source_id, reason)` for each source that was unavailable or errored.
    pub failed: Vec<(String, String)>,
}

/// One unit of a batch install: a plan plus the candidates it installs. A source that
/// batches (`plan_install_many`) yields one `BatchPlan` per source (its plan installs
/// many candidates); a source that can't yields one `BatchPlan` per candidate. Grouping
/// candidates back with their plan lets `install_batch` record every install.
pub struct BatchPlan {
    pub plan: InstallPlan,
    pub candidates: Vec<PackageCandidate>,
}

/// One unit of a batch remove/update: a plan plus the records it covers. The record twin
/// of [`BatchPlan`] (which pairs a plan with the *candidates* it installs). A source that
/// batches yields one per source; one that can't yields one per record.
pub struct RecordBatchPlan {
    pub plan: InstallPlan,
    pub records: Vec<InstalledRecord>,
}

/// The result of planning a batch remove/update: the plans to run, plus the records that
/// could **not** be planned (e.g. github has no update path). Reporting them instead of
/// aborting keeps one un-actionable package from cancelling the rest — the same
/// facts-not-failures shape as [`SearchResult`].
pub struct RecordBatch {
    pub plans: Vec<RecordBatchPlan>,
    /// `(name, reason)` for each record whose plan could not be built.
    pub unplannable: Vec<(String, String)>,
}

/// The result of planning a system-wide update (D10): every willing provider's bulk
/// "update everything I own" plan, plus the ids of the sources that offered one — so the
/// caller can fall back to per-record updates for the sources that opted out.
pub struct SystemUpdate {
    pub plans: Vec<InstallPlan>,
    pub sources: Vec<String>,
}

/// Which record operation a batch planner is building. This selects the provider method
/// to call — it is the *operation*, never the source; the engine still never branches on
/// a concrete source id (ADR-0004).
#[derive(Clone, Copy)]
pub enum RecordOp {
    Remove,
    Update,
}

/// One row of `jii sources`: an enabled provider, its trust, and whether it is usable
/// on this machine right now. Cheaper than [`SourceHealth`] — availability only.
pub struct SourceEntry {
    pub id: &'static str,
    pub trust: TrustLevel,
    pub available: bool,
    /// Whether this source is relevant on this host: usable now, or usable after a bootstrap
    /// (network-searchable or a bootstrappable ecosystem). A native manager for another distro
    /// (pacman on Fedora) is none of these, so `jii sources` hides it unless `--all` (a Fedora
    /// user shouldn't have to reason about pacman). No source-id branch — pure capability.
    pub relevant: bool,
}

/// One row of `jii providers`: an installable *ecosystem* manager (npm, cargo, brew,
/// flatpak…), whether it is present on this host, and how to bootstrap it if not.
/// Built from each provider's [`ecosystem`](crate::provider::Provider::ecosystem) metadata.
pub struct EcosystemStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub installed: bool,
    pub bootstrap: crate::provider::Bootstrap,
}

/// Diagnostic for one source, produced by `diagnose` (backs `jii doctor`).
pub struct SourceHealth {
    pub id: String,
    pub available: bool,
    pub latency: Duration,
    pub health: Health,
    /// Optional human detail from the probe (e.g. remaining rate-limit budget).
    pub detail: Option<String>,
}

/// One row of `jii list --audit`: where an installed package came from, its trust, how it
/// was verified, and any concerns worth attention.
pub struct AuditEntry {
    pub name: String,
    pub source_id: String,
    pub version: Option<PkgVersion>,
    pub installed_at: DateTime<Utc>,
    /// Trust of the owning source, or `None` if that source is no longer enabled.
    pub trust: Option<TrustLevel>,
    pub verification: AuditVerification,
    pub concerns: Vec<AuditConcern>,
}

/// How an installed artifact was verified.
#[derive(Debug, PartialEq, Eq)]
pub enum AuditVerification {
    /// Verified by jii at download time with the named method (e.g. "sha256").
    Checksum(String),
    /// Installed via a self-verifying package manager (dnf/copr GPG, flatpak).
    ManagerSigned,
    /// Downloaded with no checksum/signature available.
    Unverified,
}

impl AuditVerification {
    /// Short human/JSON label.
    pub fn label(&self) -> String {
        match self {
            AuditVerification::Checksum(method) => method.clone(),
            AuditVerification::ManagerSigned => "manager-signed".to_string(),
            AuditVerification::Unverified => "unverified".to_string(),
        }
    }
}

/// Something about an installed package that may warrant attention.
#[derive(Debug, PartialEq, Eq)]
pub enum AuditConcern {
    /// From an untrusted source (an arbitrary third-party binary).
    UntrustedSource,
    /// Installed without any checksum/signature verification.
    Unverified,
    /// The owning source is no longer enabled, so jii can't manage/vouch for it.
    SourceUnavailable,
}

impl AuditConcern {
    /// Short human/JSON message.
    pub fn message(&self) -> &'static str {
        match self {
            AuditConcern::UntrustedSource => "untrusted source",
            AuditConcern::Unverified => "no checksum verification",
            AuditConcern::SourceUnavailable => "source no longer enabled",
        }
    }
}

/// The orchestrator.
pub struct Engine {
    config: Config,
    providers: ProviderRegistry,
    privilege: Privilege,
    registry: Registry,
    cache: Cache,
}

impl Engine {
    /// Build the engine from config, instantiating the enabled providers and
    /// loading the install registry and search cache.
    pub fn new(config: Config) -> Result<Self> {
        let providers = ProviderRegistry::from_config(&config);
        let registry = Registry::load()?;
        let cache = Cache::load(
            config.network.cache_ttl_secs,
            config.network.failure_cooldown_secs,
        );
        Ok(Engine {
            config,
            providers,
            privilege: Privilege::detect(),
            registry,
            cache,
        })
    }

    /// Whether any provider is enabled.
    pub fn has_providers(&self) -> bool {
        !self.providers.is_empty()
    }

    /// Whether at least one enabled provider is usable here — its backing tool is present.
    /// The honest, source-based replacement for the old Fedora-only wall (ADR-0029):
    /// "supported" means "JII has a working source", a question only the provider set can
    /// answer. Probes the same `is_available` fan-out `source_catalog` uses, short-circuiting
    /// on the first hit. The core never inspects the distro to decide this.
    pub async fn any_source_available(&self) -> bool {
        for provider in self.providers.iter() {
            if provider.is_available().await {
                return true;
            }
        }
        false
    }

    /// Whether any provider offers by-name repo search (a forge). Lets the CLI decide whether
    /// to offer the GitHub-style repo picker at all — no picker if no forge is enabled.
    pub fn has_repo_search(&self) -> bool {
        self.providers.iter().any(|p| p.supports_repo_search())
    }

    /// By-name repo search across every forge provider (ADR-0053): the `page`-th page (1-based)
    /// of repository matches, concatenated in provider order (each forge already returns its
    /// own relevance ranking). Only the interactive picker calls this, so it is a plain
    /// bounded, uncached fan-out; an erroring forge just contributes nothing.
    pub async fn forge_repo_search(&self, query: &str, page: u32) -> Vec<RepoHit> {
        let mut hits = Vec::new();
        for provider in self.providers.iter() {
            if let Ok(mut found) = provider.search_repos(query, page).await {
                hits.append(&mut found);
            }
        }
        hits
    }

    /// Resolve a picked `owner/repo` (from the forge that produced it) into installable
    /// candidate(s). Routes by `source_id` — this is dispatch, not a behavioural branch on the
    /// source. Empty if the repo publishes nothing installable for this arch.
    pub async fn resolve_repo(&self, source_id: &str, slug: &str) -> Vec<PackageCandidate> {
        for provider in self.providers.iter() {
            if provider.id() == source_id {
                return provider.resolve_repo(slug).await.unwrap_or_default();
            }
        }
        Vec::new()
    }

    /// Alternative install strategies the owning source offers for `candidate` (ADR-0054) —
    /// Nix's declarative config snippets alongside the imperative `nix profile install`.
    /// Routes by `source_id` (dispatch, not a behavioural source-branch). Empty unless the
    /// provider opts in *and* has something to offer (Nix only when a config is detected).
    /// Called only for a single-package interactive install.
    pub async fn install_strategies(
        &self,
        source_id: &str,
        candidate: &PackageCandidate,
    ) -> Vec<InstallStrategy> {
        for provider in self.providers.iter() {
            if provider.id() == source_id {
                return provider.install_strategies(candidate).await;
            }
        }
        Vec::new()
    }

    /// Search all providers concurrently, each bounded by the configured timeout.
    /// Unavailable/slow/erroring sources are collected into `failed` (tagged, e.g.
    /// "timeout") rather than aborting the search.
    pub async fn search(&self, query: &Query) -> SearchResult {
        let timeout = Duration::from_secs(self.config.network.timeout_secs);
        let results =
            futures::future::join_all(self.providers.iter().map(|p| self.search_one(p, query, timeout)))
                .await;

        let mut candidates = Vec::new();
        let mut failed = Vec::new();
        for result in results {
            match result {
                Ok(mut found) => candidates.append(&mut found),
                Err(failure) => failed.push(failure),
            }
        }
        self.cache.save();
        SearchResult { candidates, failed }
    }

    /// Search one provider with per-call timeouts, backed by the cache.
    ///
    /// A fresh cache hit skips the provider entirely; on failure/timeout a stale
    /// cache entry is used if present (offline resilience), otherwise the failure
    /// is returned as `(source_id, reason)`.
    async fn search_one(
        &self,
        provider: &dyn crate::provider::Provider,
        query: &Query,
        timeout: Duration,
    ) -> std::result::Result<Vec<PackageCandidate>, (String, String)> {
        let id = provider.id().to_string();
        let fail = |reason: &str| (id.clone(), reason.to_string());

        if let Some(cached) = self.cache.get_fresh(&id, &query.raw) {
            return Ok(cached);
        }

        // Circuit breaker (#2): a source that timed out / errored within the cooldown is
        // skipped *without waiting* — the single biggest search-latency win, since one slow
        // source (a ~9 s COPR against a 5 s timeout) otherwise makes every search pay that
        // timeout via the fan-out `join_all`. Serve a stale entry if we have one.
        if self.cache.recently_failed(&id) {
            return Ok(self.cache.get_stale(&id, &query.raw).unwrap_or_default());
        }

        // On any failure, record it (opens the circuit) and fall back to a stale entry.
        let or_stale = |failure: (String, String)| {
            self.cache.mark_failure(&id);
            match self.cache.get_stale(&id, &query.raw) {
                Some(stale) => Ok(stale),
                None => Err(failure),
            }
        };

        match tokio::time::timeout(timeout, provider.is_available()).await {
            Ok(true) => {}
            // Tool not installed here — the normal state for most sources on any given distro
            // (apt/pacman/zypper on Fedora, dnf on Arch, …). This is not a failure worth
            // reporting: surfacing it once per source per search is pure noise (UX #1), so we
            // contribute nothing silently (a stale cache entry still counts if we have one).
            // Not a circuit-breaker failure either — a missing local tool is instant, not slow.
            // `jii sources`/`jii doctor` remain the place to see what's unavailable.
            //
            // Exception (ADR-0061, part B): a source that can search over the network without its
            // CLI still runs, so an uninstalled manager can answer "do I have this?" — the hit is
            // later tagged needs_bootstrap so installing it first offers to bootstrap the manager.
            Ok(false) if !provider.can_search() => {
                return Ok(self.cache.get_stale(&id, &query.raw).unwrap_or_default());
            }
            Ok(false) => {}
            Err(_) => return or_stale(fail("timeout")),
        }
        match tokio::time::timeout(timeout, provider.search(query)).await {
            Ok(Ok(candidates)) => {
                self.cache.clear_failure(&id); // success closes the circuit
                self.cache.put(&id, &query.raw, candidates.clone());
                Ok(candidates)
            }
            Ok(Err(e)) => or_stale(fail(&e.to_string())),
            Err(_) => or_stale(fail("timeout")),
        }
    }

    /// The candidate's public web page, if its owning source exposes one (Flathub app page,
    /// crates.io / npm / PyPI page, GitHub repo…). Cheap and synchronous — used to let the
    /// user open it and confirm "this is the one I meant" before installing (ADR-0061 follow-up).
    pub fn web_url(&self, candidate: &PackageCandidate) -> Option<String> {
        self.provider(&candidate.source_id).ok()?.web_url(candidate)
    }

    /// Rank candidates, best first, for the name the user typed (`query`) — exact name
    /// matches sort above prefix/substring matches (ADR-0042).
    pub fn rank(&self, query: &str, candidates: Vec<PackageCandidate>) -> Vec<PackageCandidate> {
        ranking::rank(&self.config, query, candidates)
    }

    /// Broaden a search that found no exact match (ADR-0042), ranked best-first. Called by
    /// the install/search flows only after the exact search comes up empty, so common
    /// queries stay noise-free. Two stages:
    ///
    /// 1. **Prefix** — `<name>*`, so `ayugram` resolves to `ayugram-desktop`.
    /// 2. **Typo fallback** — trim up to two trailing characters and retry the prefix
    ///    search (a trailing slip like `ayugramm` still reaches `ayugram*`). Stops at the
    ///    first stage that yields anything; a stem shorter than four characters is not
    ///    tried (too little signal to be a useful guess).
    ///
    /// Returns candidates ranked against whatever term matched (empty if nothing did).
    pub async fn broaden_search(&self, name: &str) -> Vec<PackageCandidate> {
        let prefix_q = Query::name(name).with_match_mode(MatchMode::Prefix);
        let prefix = self.rank(name, self.search(&prefix_q).await.candidates);
        if !prefix.is_empty() {
            return prefix;
        }

        let chars: Vec<char> = name.chars().collect();
        for drop in 1..=2 {
            if chars.len().saturating_sub(drop) < 4 {
                break;
            }
            let stem: String = chars[..chars.len() - drop].iter().collect();
            let q = Query::name(&stem).with_match_mode(MatchMode::Prefix);
            let hit = self.rank(&stem, self.search(&q).await.candidates);
            if !hit.is_empty() {
                return hit;
            }
        }

        // 3. **Mid-word typo** — try cheap edit-distance-1 variants (a dropped/duplicated key or
        //    a swapped pair, so `pipix` → `pipx`), each as an *exact* search so we don't broaden
        //    into unrelated prefixes. Only for terms long enough to carry signal.
        if chars.len() >= 4 {
            for variant in typo_variants(name) {
                let q = Query::name(&variant);
                let hit = self.rank(&variant, self.search(&q).await.candidates);
                if !hit.is_empty() {
                    return hit;
                }
            }
        }
        Vec::new()
    }

    /// Group a batch of candidates by owning source and, for each group, ask the source
    /// for a single batched plan ([`Provider::plan_install_many`]); when it declines
    /// (`None`), fall back to one `plan_install` per candidate. A single-package install
    /// is just a batch of one, so this is the one install-planning entry point. Source
    /// groups keep their first-seen (ranked) order, so the preview reads sensibly.
    /// The engine never branches on the source — it only uses the returned plan or falls
    /// back (ADR-0004/0022).
    pub async fn plan_install_batch(
        &self,
        candidates: Vec<PackageCandidate>,
    ) -> Result<Vec<BatchPlan>> {
        // Group by source_id, preserving the order sources first appear in.
        let groups = group_by_source(candidates, |c| c.source_id.as_str());

        let mut plans = Vec::new();
        for (source_id, group) in groups {
            let provider = self.provider(&source_id)?;
            // A group of one is not worth merging (the command is identical) and the
            // per-package plan carries richer reasons, so keep single-package output
            // byte-identical to a plain `jii install <pkg>`. Only 2+ ask to batch.
            if group.len() == 1 {
                let candidate = group.into_iter().next().expect("len checked");
                let plan = provider.plan_install(&candidate).await?;
                plans.push(BatchPlan { plan, candidates: vec![candidate] });
                continue;
            }
            let refs: Vec<&PackageCandidate> = group.iter().collect();
            match provider.plan_install_many(&refs).await? {
                Some(plan) => plans.push(BatchPlan { plan, candidates: group }),
                None => {
                    // Source can't batch: one plan per candidate.
                    for candidate in group {
                        let plan = provider.plan_install(&candidate).await?;
                        plans.push(BatchPlan { plan, candidates: vec![candidate] });
                    }
                }
            }
        }
        Ok(plans)
    }

    /// Execute a batch of install plans as one operation: prime privilege **once**
    /// across all plans, run them in order, and record each plan's candidates as it
    /// succeeds — so the registry reflects reality even if a later plan fails. The
    /// single privileged + registry-write path for batch installs.
    pub async fn install_batch(&mut self, batch: &[BatchPlan], renderer: &Renderer) -> Result<()> {
        let plans: Vec<&InstallPlan> = batch.iter().map(|b| &b.plan).collect();
        crate::exec::prime_for(&plans, &self.privilege).await?;

        let mut outcome = Ok(());
        for bp in batch {
            if let Err(e) = crate::exec::run_actions(&bp.plan, &self.privilege, renderer).await {
                outcome = Err(e);
                break; // stop at the first failure; already-run plans stay recorded
            }
            let now = Utc::now();
            let verification = plan_verification(&bp.plan);
            for candidate in &bp.candidates {
                self.registry.record_install(InstalledRecord {
                    name: candidate.name.clone(),
                    source_id: candidate.source_id.clone(),
                    version: candidate.version.clone(),
                    installed_at: now,
                    verification: verification.clone(),
                });
            }
        }
        self.registry.save()?; // persist whatever succeeded before any failure
        outcome
    }

    /// Group records by owning source and, per group, ask the source for **one** batched
    /// plan (`plan_remove_many`/`plan_update_many`); on `None` fall back to one plan per
    /// record. A group of one keeps the richer single-record plan (identical to a plain
    /// `jii remove <pkg>` / `jii update <pkg>`). A record whose single plan cannot be built
    /// (e.g. github has no update path) is collected into `unplannable` rather than
    /// aborting the batch. Symmetric with [`plan_install_batch`]; the engine never branches
    /// on the source id — only on the *operation* (ADR-0004/0025).
    pub async fn plan_record_batch(
        &self,
        records: Vec<InstalledRecord>,
        op: RecordOp,
    ) -> Result<RecordBatch> {
        let groups = group_by_source(records, |r| r.source_id.as_str());
        let mut plans = Vec::new();
        let mut unplannable = Vec::new();
        for (source_id, group) in groups {
            let provider = self.provider(&source_id)?;

            // A group of one is not worth merging (identical command) and the per-record
            // plan carries richer reasons, so keep single-package output byte-identical.
            if group.len() == 1 {
                let record = group.into_iter().next().expect("len checked");
                match plan_one_record(provider, &record, op).await {
                    Ok(plan) => plans.push(RecordBatchPlan { plan, records: vec![record] }),
                    Err(e) => unplannable.push((record.name, e.to_string())),
                }
                continue;
            }

            let refs: Vec<&InstalledRecord> = group.iter().collect();
            let merged = match op {
                RecordOp::Remove => provider.plan_remove_many(&refs).await?,
                RecordOp::Update => provider.plan_update_many(&refs).await?,
            };
            match merged {
                Some(plan) => plans.push(RecordBatchPlan { plan, records: group }),
                None => {
                    // Source can't batch this op: one plan per record (skipping any that
                    // cannot be planned, so the rest still proceed).
                    for record in group {
                        match plan_one_record(provider, &record, op).await {
                            Ok(plan) => plans.push(RecordBatchPlan { plan, records: vec![record] }),
                            Err(e) => unplannable.push((record.name, e.to_string())),
                        }
                    }
                }
            }
        }
        Ok(RecordBatch { plans, unplannable })
    }

    /// Execute a batch of removal plans as one operation: prime privilege **once**, run
    /// each plan in order, and record each removal as its plan succeeds — so a mid-batch
    /// failure still leaves the registry accurate. Mirrors `install_batch`.
    pub async fn remove_batch(
        &mut self,
        batch: &[RecordBatchPlan],
        renderer: &Renderer,
    ) -> Result<()> {
        let plans: Vec<&InstallPlan> = batch.iter().map(|b| &b.plan).collect();
        crate::exec::prime_for(&plans, &self.privilege).await?;

        let mut outcome = Ok(());
        for bp in batch {
            if let Err(e) = crate::exec::run_actions(&bp.plan, &self.privilege, renderer).await {
                outcome = Err(e);
                break;
            }
            for record in &bp.records {
                self.registry.record_remove(&record.name, &record.source_id);
            }
        }
        self.registry.save()?;
        outcome
    }

    /// Execute a batch of update plans as one operation: prime **once**, run each in order,
    /// and record each update as its plan succeeds. Each carried record supplies the
    /// **post-update** coordinate — its `version` is the refreshed target the caller set;
    /// the engine stamps `installed_at`/verification from the plan (as `install_batch`
    /// does), so there is one place that shapes a written record. Mirrors `install_batch`.
    pub async fn update_batch(
        &mut self,
        batch: &[RecordBatchPlan],
        renderer: &Renderer,
    ) -> Result<()> {
        let plans: Vec<&InstallPlan> = batch.iter().map(|b| &b.plan).collect();
        crate::exec::prime_for(&plans, &self.privilege).await?;

        let mut outcome = Ok(());
        for bp in batch {
            if let Err(e) = crate::exec::run_actions(&bp.plan, &self.privilege, renderer).await {
                outcome = Err(e);
                break;
            }
            let now = Utc::now();
            let verification = plan_verification(&bp.plan);
            for record in &bp.records {
                self.registry.record_update(InstalledRecord {
                    name: record.name.clone(),
                    source_id: record.source_id.clone(),
                    version: record.version.clone(),
                    installed_at: now,
                    verification: verification.clone(),
                });
            }
        }
        self.registry.save()?;
        outcome
    }

    /// Gather every available provider's "update everything I own" plan (D10). Each willing
    /// provider (`plan_update_all` returns `Some`) contributes one plan; the returned
    /// `sources` names them so the caller can fall back to per-record updates for the sources
    /// that opted out (github, cargo, go). The engine never branches on the source id — it
    /// just aggregates whatever plans the providers offer (ADR-0022/0025).
    pub async fn plan_update_all(&self) -> Result<SystemUpdate> {
        let mut plans = Vec::new();
        let mut sources = Vec::new();
        for provider in self.providers.iter() {
            if !provider.is_available().await {
                continue;
            }
            if let Some(plan) = provider.plan_update_all().await? {
                sources.push(provider.id().to_string());
                plans.push(plan);
            }
        }
        Ok(SystemUpdate { plans, sources })
    }

    /// Execute a system-wide update: the providers' bulk update-all `system_plans` (which
    /// upgrade the whole system, not JII-tracked packages, so nothing is recorded), followed
    /// by the per-record `fallback` batch for sources without a bulk path (recorded as each
    /// succeeds, exactly like [`update_batch`]). Privilege is primed **once** across
    /// everything, so a mix of root (dnf) and user (flatpak) steps asks for elevation a single
    /// time. Stops at the first failure, leaving the registry accurate for what did run.
    pub async fn run_system_update(
        &mut self,
        system_plans: &[InstallPlan],
        fallback: &[RecordBatchPlan],
        renderer: &Renderer,
    ) -> Result<()> {
        let mut all: Vec<&InstallPlan> = system_plans.iter().collect();
        all.extend(fallback.iter().map(|b| &b.plan));
        crate::exec::prime_for(&all, &self.privilege).await?;

        // Bulk managers can be very chatty (npm/flatpak flood the terminal). Capture each one's
        // output and reduce it to a one-line, source-agnostic summary instead of streaming it.
        let palette = renderer.palette();
        for plan in system_plans {
            let (ok, out) = self.run_plan_captured(plan).await?;
            let summary = crate::exec::summarize_update(&out);
            let mark = if ok {
                palette.good(palette.mark_ok())
            } else {
                palette.mark_bad().to_string()
            };
            renderer.info(&format!(
                "  {:8} {mark} {}",
                palette.source(&plan.source_id),
                summary.headline
            ));
            for note in &summary.notes {
                renderer.info(&format!("      {}", palette.dim(note)));
            }
            // On failure, show a short tail of the captured output so the error isn't swallowed.
            if !ok {
                for line in out.lines().rev().take(4).collect::<Vec<_>>().into_iter().rev() {
                    renderer.info(&palette.dim(&format!("      {line}")));
                }
            }
        }

        let mut outcome = Ok(());
        for bp in fallback {
            if let Err(e) = crate::exec::run_actions(&bp.plan, &self.privilege, renderer).await {
                outcome = Err(e);
                break;
            }
            let now = Utc::now();
            let verification = plan_verification(&bp.plan);
            for record in &bp.records {
                self.registry.record_update(InstalledRecord {
                    name: record.name.clone(),
                    source_id: record.source_id.clone(),
                    version: record.version.clone(),
                    installed_at: now,
                    verification: verification.clone(),
                });
            }
        }
        self.registry.save()?;
        outcome
    }

    /// Run every action of a bulk-update `plan` capturing output (for the whole-system update
    /// summary). Update-all plans are `RunCommand`s (e.g. `apt-get update` then `apt-get
    /// upgrade`); their combined output is concatenated. Returns `(all_ok, combined_output)` —
    /// a spawn failure is still a hard error. Priming happens in the caller.
    async fn run_plan_captured(&self, plan: &InstallPlan) -> Result<(bool, String)> {
        let mut combined = String::new();
        let mut ok = true;
        for action in &plan.actions {
            if let crate::model::Action::RunCommand { argv, needs_root } = action {
                let (step_ok, out) = self.privilege.run_captured(argv, *needs_root).await?;
                combined.push_str(&out);
                if !step_ok {
                    ok = false;
                    break; // a failed step (e.g. `apt update`) aborts the rest of this plan
                }
            }
        }
        Ok((ok, combined))
    }

    /// Run a single self-management plan (jii updating or removing **itself**) through the
    /// same executor + privilege layer as any plan. Kept apart from the registry batch paths:
    /// it acts on jii's own binary/package, not on a tracked install.
    pub async fn run_self_plan(&self, plan: &InstallPlan, renderer: &Renderer) -> Result<()> {
        crate::exec::prime_for(std::slice::from_ref(&plan), &self.privilege).await?;
        crate::exec::run_actions(plan, &self.privilege, renderer).await
    }

    /// Resolve which source owns an installed package.
    ///
    /// Uses the registry as a hint but verifies against the real manager; if the
    /// registry is missing or stale, scans providers' installed lists. Errors if
    /// the package is not actually installed.
    pub async fn resolve_installed(&self, name: &str) -> Result<InstalledRecord> {
        // 1. Registry hint, verified against the owning provider.
        if let Some(record) = self.registry.get(name)
            && self.is_installed_via(record).await
        {
            return Ok(record.clone());
        }
        // 2. Registry absent or stale: scan providers for the package.
        for provider in self.providers.iter() {
            if !provider.is_available().await {
                continue;
            }
            if let Ok(installed) = provider.list_installed().await
                && let Some(found) = installed.into_iter().find(|r| r.name == name)
            {
                return Ok(found);
            }
        }
        Err(JiiError::Other(anyhow::anyhow!(
            "'{name}' is not installed"
        )))
    }

    /// Every enabled source that currently has `name` installed — a full fan-out, unlike
    /// [`resolve_installed`] which returns the first owner. Backs multi-owner `remove` (UX
    /// #11): a package present in several sources (e.g. ripgrep via dnf *and* cargo) must let
    /// the user choose which copy to remove. File-based sources that can't enumerate (github)
    /// are covered by folding in a verified registry record. Correctness over latency (remove
    /// is not the hot path), so the cost of scanning every provider is deliberate.
    pub async fn resolve_all_installed(&self, name: &str) -> Vec<InstalledRecord> {
        let mut owners: Vec<InstalledRecord> = Vec::new();
        for provider in self.providers.iter() {
            if !provider.is_available().await {
                continue;
            }
            if let Ok(installed) = provider.list_installed().await
                && let Some(found) = installed.into_iter().find(|r| r.name == name)
            {
                owners.push(found);
            }
        }
        // Fold in a recorded owner the scan missed (e.g. a github file-install), verified present.
        if let Some(record) = self.registry.get(name)
            && !owners.iter().any(|o| o.source_id == record.source_id)
            && self.is_installed_via(record).await
        {
            owners.push(record.clone());
        }
        owners
    }

    /// The cheap "is this already here?" for the **install pre-check** (UX #3): the registry
    /// hint (covers jii's own installs) verified against its owning provider, else a single
    /// lookup in just the **recommended** source's installed set. Deliberately *not* a full
    /// provider fan-out like [`resolve_installed`] — the install path is hot and a package a
    /// source offers is, if installed at all, most likely installed via that source. Returns
    /// the owning record (with its version) if present. The cross-source scan stays in
    /// `resolve_installed`, used by remove/update where correctness beats latency.
    pub async fn installed_lookup(
        &self,
        name: &str,
        recommended_source: &str,
    ) -> Option<InstalledRecord> {
        // 1. Registry hint (jii installed it), verified still-present.
        if let Some(record) = self.registry.get(name)
            && self.is_installed_via(record).await
        {
            return Some(record.clone());
        }
        // 2. Otherwise check only the recommended source (one provider, not a fan-out).
        let provider = self.providers.get(recommended_source)?;
        if !provider.is_available().await {
            return None;
        }
        let installed = provider.list_installed().await.ok()?;
        installed.into_iter().find(|r| r.name == name)
    }

    /// The set of package names currently installed across every **available** source,
    /// gathered once (each provider's `list_installed` is called a single time). Backs
    /// `doctor`'s "analyse the system first" step (#1): a suggestion whose identifiers are
    /// all in this set is already done and must not be offered. Names are source-native
    /// (dnf package names, Flatpak app-ids, npm package names…), matched against a
    /// suggestion's `check`/`packages`. Best-effort — a provider that errors contributes
    /// nothing rather than failing the whole index.
    pub async fn installed_index(&self) -> std::collections::HashSet<String> {
        let mut names = std::collections::HashSet::new();
        for provider in self.providers.iter() {
            if !provider.is_available().await {
                continue;
            }
            if let Ok(installed) = provider.list_installed().await {
                names.extend(installed.into_iter().map(|r| r.name));
            }
        }
        names
    }

    /// Whether the recorded install is still present, asked of its owning provider
    /// (which decides how to verify — list lookup or file existence).
    async fn is_installed_via(&self, record: &InstalledRecord) -> bool {
        let Some(provider) = self.providers.get(&record.source_id) else {
            return false;
        };
        if !provider.is_available().await {
            return false;
        }
        provider.is_installed(record).await
    }

    /// Access the install registry (read-only).
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Trust level of a source, if it is enabled.
    pub fn source_trust(&self, source_id: &str) -> Option<crate::model::TrustLevel> {
        self.providers.get(source_id).map(|p| p.trust())
    }

    /// Source-specific recommendation highlights for a candidate (UX D5), asked of its
    /// owning provider (ADR-0022 optional `highlights`). Empty if the source is no longer
    /// enabled or offers none; the CLI concatenates these with the model-derived facts.
    pub fn candidate_highlights(&self, candidate: &PackageCandidate) -> Vec<String> {
        self.providers
            .get(&candidate.source_id)
            .map(|p| p.highlights(candidate))
            .unwrap_or_default()
    }

    /// Rich metadata for `jii info`'s app card (#4), asked of the candidate's owning
    /// provider (optional `describe`, ADR-0022). `None` if the source is no longer enabled
    /// or offers no card. May do I/O (dnf runs `dnf5 info`) — called only on `jii info`.
    pub async fn candidate_info(
        &self,
        candidate: &PackageCandidate,
    ) -> Option<crate::model::PackageInfo> {
        match self.providers.get(&candidate.source_id) {
            Some(p) => p.describe(candidate).await,
            None => None,
        }
    }

    /// An informational card for `jii info` resolved from a **name**, for the case where the
    /// normal (installable) resolve found nothing — e.g. an npm/cargo library (ADR-0045).
    /// Asks each **available** source and returns the first card offered, so `info` can show
    /// what a package *is* without the install path's "nothing to install". `None` if no
    /// source recognises the name.
    pub async fn reference(&self, name: &str) -> Option<crate::model::Reference> {
        let query = Query::name(name);
        for provider in self.providers.iter() {
            if provider.is_available().await
                && let Some(card) = provider.reference(&query).await
            {
                return Some(card);
            }
        }
        None
    }

    /// List the enabled providers with their trust and live availability (backs
    /// `jii sources`). Availability only — no network health probe (that is `doctor`).
    pub async fn source_catalog(&self) -> Vec<SourceEntry> {
        let mut out = Vec::new();
        for provider in self.providers.iter() {
            let available = provider.is_available().await;
            out.push(SourceEntry {
                id: provider.id(),
                trust: provider.trust(),
                available,
                relevant: source_relevant(available, provider),
            });
        }
        out
    }

    /// Whether a specific source's manager is usable right now (its CLI is present). Backs the
    /// T6 bootstrap flow — after installing a manager's package, confirm it actually landed
    /// before planning the app that needs it. Checked live (`which`), so it reflects a manager
    /// installed earlier in the same run. Unknown/absent id → false.
    pub async fn source_available(&self, id: &str) -> bool {
        match self.provider(id) {
            Ok(p) => p.is_available().await,
            Err(_) => false,
        }
    }

    /// List the installable *ecosystem* managers (npm, cargo, brew, flatpak…) with their
    /// presence on this host (backs `jii providers`). Only providers that declare an
    /// [`ecosystem`](crate::provider::Provider::ecosystem) appear — base system repos
    /// (dnf/apt) and non-managers (github) are omitted. `installed` is `is_available()`
    /// (the manager's binary is present); no network probe.
    /// The ids of every source that is an installable ecosystem manager (npm, cargo, pipx,
    /// flatpak, snap, go, brew, nix). Pure — no `is_available` I/O — so the install path can
    /// cheaply tell "is this name a manager?" before doing any work (#4). The full status
    /// (installed?/bootstrap) comes from [`ecosystem_catalog`].
    pub fn ecosystem_ids(&self) -> Vec<&'static str> {
        self.providers
            .iter()
            .filter_map(|p| p.ecosystem().map(|_| p.id()))
            .collect()
    }

    pub async fn ecosystem_catalog(&self) -> Vec<EcosystemStatus> {
        let mut out = Vec::new();
        for provider in self.providers.iter() {
            if let Some(eco) = provider.ecosystem() {
                out.push(EcosystemStatus {
                    id: provider.id(),
                    label: eco.label,
                    installed: provider.is_available().await,
                    bootstrap: eco.bootstrap,
                });
            }
        }
        out
    }

    /// Return the first of `names` that resolves to ≥1 candidate on this host, searched in
    /// order — the cross-distro bootstrap resolver for a missing ecosystem manager (npm is
    /// `nodejs-npm` on Fedora, `npm` on Debian/Arch). `None` if none resolve, so the caller
    /// can fall back to an honest "couldn't find it" instead of guessing. JII's own search
    /// does the per-distro work; no distro branch here (ADR-0004/0029).
    pub async fn first_available_package(&self, names: &[&str]) -> Option<String> {
        for name in names {
            let query = crate::model::Query::name(*name);
            let ranked = self.rank(name, self.search(&query).await.candidates);
            if !ranked.is_empty() {
                return Some((*name).to_string());
            }
        }
        None
    }

    /// On a total search miss, ask each **available** source to explain why an exact name
    /// might exist yet not be installable — e.g. an npm/cargo library that ships no program
    /// (#9). Returns the first explanation offered, else `None`. Off the hot path: called only
    /// when nothing was found, and gated on `is_available` so JII never probes a registry for
    /// an ecosystem the user doesn't even have. The core learns nothing source-specific — the
    /// message comes wholesale from the provider (ADR-0004).
    pub async fn explain_miss(&self, name: &str) -> Option<String> {
        let query = crate::model::Query::name(name);
        for provider in self.providers.iter() {
            if provider.is_available().await
                && let Some(msg) = provider.explain_miss(&query).await
            {
                return Some(msg);
            }
        }
        None
    }

    /// Probe each source's live health (backs `jii doctor`). Each provider reports
    /// raw facts (`reachable`, `rate_limited`, a human `detail`); the engine maps
    /// them — together with the measured latency — to a [`Health`] category. Network
    /// sources (github, copr) check API reachability and rate limits; local ones fall
    /// back to binary availability (see `Provider::probe`).
    pub async fn diagnose(&self) -> Vec<SourceHealth> {
        let timeout = Duration::from_secs(self.config.network.timeout_secs);
        let mut out = Vec::new();
        for provider in self.providers.iter() {
            // Only diagnose sources relevant to this host — skip a foreign distro's native
            // manager (pacman on Fedora) that can neither run here nor be bootstrapped. Same
            // predicate as `source_catalog`/`jii sources`, so `doctor` and `sources` never
            // disagree on what the user can actually use (ADR-0064): a Fedora user is never
            // shown apt/pacman/zypper/void/gentoo/aur. Capability, not a source-id branch.
            if !source_relevant(provider.is_available().await, provider) {
                continue;
            }
            let start = Instant::now();
            let probe = match tokio::time::timeout(timeout, provider.probe()).await {
                Ok(probe) => probe,
                Err(_) => crate::provider::Probe::unreachable(),
            };
            let latency = start.elapsed();
            out.push(SourceHealth {
                id: provider.id().to_string(),
                available: probe.reachable,
                latency,
                health: health_from(probe.reachable, probe.rate_limited, latency),
                detail: probe.detail,
            });
        }
        out
    }

    /// Audit every recorded install: provenance, trust, verification and concerns.
    /// Registry-based and fast (no live provider calls).
    pub fn audit(&self) -> Vec<AuditEntry> {
        self.registry
            .installed()
            .iter()
            .map(|record| self.audit_entry(record))
            .collect()
    }

    /// Build one audit row from a registry record.
    fn audit_entry(&self, record: &InstalledRecord) -> AuditEntry {
        let trust = self.source_trust(&record.source_id);
        let verification = resolve_verification(record.verification.as_deref());
        let concerns = audit_concerns(trust, &verification);
        AuditEntry {
            name: record.name.clone(),
            source_id: record.source_id.clone(),
            version: record.version.clone(),
            installed_at: record.installed_at,
            trust,
            verification,
            concerns,
        }
    }

    /// Access the effective configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Look up a provider by source id.
    fn provider(&self, source_id: &str) -> Result<&dyn crate::provider::Provider> {
        self.providers
            .get(source_id)
            .ok_or_else(|| JiiError::UnknownSource(source_id.to_string()))
    }
}

/// Whether a source is worth surfacing on this host (ADR-0064): usable now (`available`),
/// or bootstrappable — network-searchable (`can_search`) or a declarable ecosystem manager
/// (`ecosystem`). A foreign distro's native manager (pacman on Fedora, dnf on Arch) is none
/// of these. Shared by `source_catalog` (backs `jii sources`) and `diagnose` (backs
/// `jii doctor`) so the two never disagree about what the user can actually use. Pure
/// capability — no branch on a concrete source id. `available` is the caller's cached
/// `is_available()`, passed in so we don't probe twice.
fn source_relevant(available: bool, provider: &dyn crate::provider::Provider) -> bool {
    available || provider.can_search() || provider.ecosystem().is_some()
}

/// A source slower than this (but still reachable) is reported as [`Health::Slow`].
const SLOW_THRESHOLD: Duration = Duration::from_secs(2);

/// Map a source's raw probe facts + measured latency to a [`Health`] category. This
/// is the engine's decision (ADR-0015): providers report facts, the engine judges.
/// Precedence: unreachable → `Offline`; reachable but rate-limited → `RateLimited`
/// (usable now, but searches may soon fail); slow → `Slow`; otherwise `Healthy`.
fn health_from(reachable: bool, rate_limited: bool, latency: Duration) -> Health {
    if !reachable {
        Health::Offline
    } else if rate_limited {
        Health::RateLimited
    } else if latency > SLOW_THRESHOLD {
        Health::Slow
    } else {
        Health::Healthy
    }
}

/// The verification label to record for an install, taken from the plan's download
/// step. Command-based installs (no download) return `None` — their package manager
/// verifies the artifact itself (dnf/copr GPG, flatpak signatures).
fn plan_verification(plan: &InstallPlan) -> Option<String> {
    plan.actions.iter().find_map(|action| match action {
        crate::model::Action::Download { verify, .. } => Some(verify.label().to_string()),
        _ => None,
    })
}

/// Interpret a recorded verification label into an audit category.
fn resolve_verification(recorded: Option<&str>) -> AuditVerification {
    match recorded {
        Some("unverified") => AuditVerification::Unverified,
        Some(method) => AuditVerification::Checksum(method.to_string()),
        // No download step was recorded: a self-verifying manager installed it.
        None => AuditVerification::ManagerSigned,
    }
}

/// Concerns implied by an install's trust and verification.
fn audit_concerns(trust: Option<TrustLevel>, verification: &AuditVerification) -> Vec<AuditConcern> {
    let mut concerns = Vec::new();
    if matches!(trust, Some(TrustLevel::Untrusted)) {
        concerns.push(AuditConcern::UntrustedSource);
    }
    if *verification == AuditVerification::Unverified {
        concerns.push(AuditConcern::Unverified);
    }
    if trust.is_none() {
        concerns.push(AuditConcern::SourceUnavailable);
    }
    concerns
}

/// Group items by their owning source id, preserving the order sources first appear in.
/// Shared by the batch planners (install/remove/update) so the "group, first-seen order"
/// invariant lives in exactly one place.
fn group_by_source<T>(items: Vec<T>, source_of: impl Fn(&T) -> &str) -> Vec<(String, Vec<T>)> {
    let mut groups: Vec<(String, Vec<T>)> = Vec::new();
    for item in items {
        let id = source_of(&item).to_string();
        match groups.iter_mut().find(|(gid, _)| gid == &id) {
            Some((_, group)) => group.push(item),
            None => groups.push((id, vec![item])),
        }
    }
    groups
}

/// Cheap edit-distance-1 variants of a search term, tried in order when the verbatim term finds
/// nothing — enough to recover from an everyday typo (`pipix` → `pipx`, `exeteragram` →
/// `exteragram`) without a dictionary. Covers single-character **deletions** first (an extra key
/// is the common slip) then **adjacent transpositions**; deduped, order-preserving, and capped so
/// a miss on a long term stays a handful of extra searches, not dozens. Shared by the by-name
/// broaden (`broaden_search`) and the GitHub repo picker.
pub(crate) fn typo_variants(query: &str) -> Vec<String> {
    let chars: Vec<char> = query.chars().collect();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut push = |v: String, out: &mut Vec<String>| {
        if v.chars().count() >= 2 && v != query && seen.insert(v.clone()) {
            out.push(v);
        }
    };
    // Deletions: drop one character.
    for i in 0..chars.len() {
        let v: String = chars[..i].iter().chain(&chars[i + 1..]).collect();
        push(v, &mut out);
    }
    // Adjacent transpositions: swap two neighbours.
    for i in 0..chars.len().saturating_sub(1) {
        let mut c = chars.clone();
        c.swap(i, i + 1);
        push(c.into_iter().collect(), &mut out);
    }
    out.truncate(16);
    out
}

/// Build a single-record plan for the given operation. The one spot that maps a
/// [`RecordOp`] to its `Provider` method — keeps `plan_record_batch` readable.
async fn plan_one_record(
    provider: &dyn crate::provider::Provider,
    record: &InstalledRecord,
    op: RecordOp,
) -> Result<InstallPlan> {
    match op {
        RecordOp::Remove => provider.plan_remove(record).await,
        RecordOp::Update => provider.plan_update(record).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_by_source_preserving_first_seen_order() {
        let items = vec![("dnf", 1), ("cargo", 2), ("dnf", 3), ("npm", 4), ("cargo", 5)];
        let groups = group_by_source(items, |(src, _)| src);
        let ids: Vec<&str> = groups.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["dnf", "cargo", "npm"]);
        assert_eq!(groups[0].1, vec![("dnf", 1), ("dnf", 3)]);
        assert_eq!(groups[1].1, vec![("cargo", 2), ("cargo", 5)]);
    }

    #[test]
    fn verification_resolution() {
        assert_eq!(resolve_verification(None), AuditVerification::ManagerSigned);
        assert_eq!(resolve_verification(Some("unverified")), AuditVerification::Unverified);
        assert_eq!(
            resolve_verification(Some("sha256")),
            AuditVerification::Checksum("sha256".to_string())
        );
    }

    #[test]
    fn official_verified_install_has_no_concerns() {
        let v = resolve_verification(None); // manager-signed
        assert!(audit_concerns(Some(TrustLevel::Official), &v).is_empty());
    }

    #[test]
    fn untrusted_unverified_flags_both() {
        let v = resolve_verification(Some("unverified"));
        let concerns = audit_concerns(Some(TrustLevel::Untrusted), &v);
        assert!(concerns.contains(&AuditConcern::UntrustedSource));
        assert!(concerns.contains(&AuditConcern::Unverified));
    }

    #[test]
    fn untrusted_but_checksum_verified_flags_only_source() {
        let v = resolve_verification(Some("sha256"));
        assert_eq!(
            audit_concerns(Some(TrustLevel::Untrusted), &v),
            vec![AuditConcern::UntrustedSource]
        );
    }

    #[test]
    fn disabled_source_is_flagged() {
        let v = resolve_verification(None);
        assert_eq!(audit_concerns(None, &v), vec![AuditConcern::SourceUnavailable]);
    }

    #[test]
    fn health_mapping_covers_each_category() {
        let fast = Duration::from_millis(50);
        let slow = SLOW_THRESHOLD + Duration::from_millis(1);
        // Unreachable always wins, regardless of the other facts.
        assert_eq!(health_from(false, false, fast), Health::Offline);
        assert_eq!(health_from(false, true, slow), Health::Offline);
        // Reachable but rate-limited outranks a slow/fast reading.
        assert_eq!(health_from(true, true, fast), Health::RateLimited);
        assert_eq!(health_from(true, true, slow), Health::RateLimited);
        // Reachable, not limited: latency decides.
        assert_eq!(health_from(true, false, slow), Health::Slow);
        assert_eq!(health_from(true, false, fast), Health::Healthy);
        // Exactly at the threshold is still healthy (strictly-greater is slow).
        assert_eq!(health_from(true, false, SLOW_THRESHOLD), Health::Healthy);
    }
}
