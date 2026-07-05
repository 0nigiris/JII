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
    Health, InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
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

/// Diagnostic for one source, produced by `diagnose` (backs `jii doctor`).
pub struct SourceHealth {
    pub id: String,
    pub available: bool,
    pub latency: Duration,
    pub health: Health,
    /// Optional human detail from the probe (e.g. remaining rate-limit budget).
    pub detail: Option<String>,
}

/// One row of `jii audit`: where an installed package came from, its trust, how it
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
        let cache = Cache::load(config.network.cache_ttl_secs);
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

        // On any failure, fall back to a stale cache entry if we have one.
        let or_stale = |failure: (String, String)| match self.cache.get_stale(&id, &query.raw) {
            Some(stale) => Ok(stale),
            None => Err(failure),
        };

        match tokio::time::timeout(timeout, provider.is_available()).await {
            Ok(true) => {}
            Ok(false) => return or_stale(fail("unavailable")),
            Err(_) => return or_stale(fail("timeout")),
        }
        match tokio::time::timeout(timeout, provider.search(query)).await {
            Ok(Ok(candidates)) => {
                self.cache.put(&id, &query.raw, candidates.clone());
                Ok(candidates)
            }
            Ok(Err(e)) => or_stale(fail(&e.to_string())),
            Err(_) => or_stale(fail("timeout")),
        }
    }

    /// Rank candidates, best first.
    pub fn rank(&self, candidates: Vec<PackageCandidate>) -> Vec<PackageCandidate> {
        ranking::rank(&self.config, candidates)
    }

    /// Build an install plan for a candidate via its owning provider.
    pub async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        self.provider(&candidate.source_id)?.plan_install(candidate).await
    }

    /// Build a removal plan for a recorded install via its owning provider.
    pub async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        self.provider(&record.source_id)?.plan_remove(record).await
    }

    /// Build an update plan for a recorded install via its owning provider.
    pub async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        self.provider(&record.source_id)?.plan_update(record).await
    }

    /// Execute an install plan, then record it. The single privileged + registry
    /// write path for installs.
    pub async fn install(
        &mut self,
        plan: &InstallPlan,
        candidate: &PackageCandidate,
        renderer: &Renderer,
    ) -> Result<()> {
        crate::exec::run_plan(plan, &self.privilege, renderer).await?;
        self.registry.record_install(InstalledRecord {
            name: candidate.name.clone(),
            source_id: candidate.source_id.clone(),
            version: candidate.version.clone(),
            installed_at: Utc::now(),
            verification: plan_verification(plan),
        });
        self.registry.save()
    }

    /// Execute an update plan, then refresh the registry record. Mirrors [`install`]
    /// (same execute-then-write path) but logs an update and carries the refreshed
    /// version: `new_version` is the just-installed latest (from a re-search), falling
    /// back to the prior recorded version when the owning source no longer reports one.
    pub async fn update(
        &mut self,
        plan: &InstallPlan,
        record: &InstalledRecord,
        new_version: Option<PkgVersion>,
        renderer: &Renderer,
    ) -> Result<()> {
        crate::exec::run_plan(plan, &self.privilege, renderer).await?;
        self.registry.record_update(InstalledRecord {
            name: record.name.clone(),
            source_id: record.source_id.clone(),
            version: new_version.or_else(|| record.version.clone()),
            installed_at: Utc::now(),
            verification: plan_verification(plan),
        });
        self.registry.save()
    }

    /// Execute a removal plan, then update the registry.
    pub async fn remove(
        &mut self,
        plan: &InstallPlan,
        record: &InstalledRecord,
        renderer: &Renderer,
    ) -> Result<()> {
        crate::exec::run_plan(plan, &self.privilege, renderer).await?;
        self.registry.record_remove(&record.name, &record.source_id);
        self.registry.save()
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

    /// Probe each source's live health (backs `jii doctor`). Each provider reports
    /// raw facts (`reachable`, `rate_limited`, a human `detail`); the engine maps
    /// them — together with the measured latency — to a [`Health`] category. Network
    /// sources (github, copr) check API reachability and rate limits; local ones fall
    /// back to binary availability (see `Provider::probe`).
    pub async fn diagnose(&self) -> Vec<SourceHealth> {
        let timeout = Duration::from_secs(self.config.network.timeout_secs);
        let mut out = Vec::new();
        for provider in self.providers.iter() {
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

#[cfg(test)]
mod tests {
    use super::*;

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
