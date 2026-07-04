//! The engine orchestrates the pipeline: `search → rank → plan → execute`.
//!
//! It is the only component that holds providers, the privilege layer and the
//! registry, and the only place that executes anything or writes the registry.
//! It operates purely on the model.

pub mod ranking;

use chrono::Utc;

use crate::config::Config;
use crate::error::{JiiError, Result};
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, Query};
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

/// The orchestrator.
pub struct Engine {
    config: Config,
    providers: ProviderRegistry,
    privilege: Privilege,
    registry: Registry,
}

impl Engine {
    /// Build the engine from config, instantiating the enabled providers and
    /// loading the install registry.
    pub fn new(config: Config) -> Result<Self> {
        let providers = ProviderRegistry::from_config(&config);
        let registry = Registry::load()?;
        Ok(Engine {
            config,
            providers,
            privilege: Privilege::detect(),
            registry,
        })
    }

    /// Whether any provider is enabled.
    pub fn has_providers(&self) -> bool {
        !self.providers.is_empty()
    }

    /// Search all available providers. Unavailable/erroring sources are collected
    /// into `failed` rather than aborting the search.
    pub async fn search(&self, query: &Query) -> SearchResult {
        let mut candidates = Vec::new();
        let mut failed = Vec::new();

        for provider in self.providers.iter() {
            if !provider.is_available().await {
                failed.push((provider.id().to_string(), "unavailable".to_string()));
                continue;
            }
            match provider.search(query).await {
                Ok(mut found) => candidates.append(&mut found),
                Err(e) => failed.push((provider.id().to_string(), e.to_string())),
            }
        }

        SearchResult { candidates, failed }
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

    /// Execute an install plan, then record it. The single privileged + registry
    /// write path for installs.
    pub async fn install(
        &mut self,
        plan: &InstallPlan,
        candidate: &PackageCandidate,
        renderer: &Renderer,
    ) -> Result<()> {
        self.privilege.execute_plan(plan, renderer).await?;
        self.registry.record_install(InstalledRecord {
            name: candidate.name.clone(),
            source_id: candidate.source_id.clone(),
            version: candidate.version.clone(),
            installed_at: Utc::now(),
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
        self.privilege.execute_plan(plan, renderer).await?;
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
        if let Some(record) = self.registry.get(name) {
            if self.is_installed_via(&record.source_id, name).await {
                return Ok(record.clone());
            }
        }
        // 2. Registry absent or stale: scan providers for the package.
        for provider in self.providers.iter() {
            if !provider.is_available().await {
                continue;
            }
            if let Ok(installed) = provider.list_installed().await {
                if let Some(found) = installed.into_iter().find(|r| r.name == name) {
                    return Ok(found);
                }
            }
        }
        Err(JiiError::Other(anyhow::anyhow!(
            "'{name}' is not installed"
        )))
    }

    /// Whether `name` is currently installed via the given source.
    async fn is_installed_via(&self, source_id: &str, name: &str) -> bool {
        let Some(provider) = self.providers.get(source_id) else {
            return false;
        };
        if !provider.is_available().await {
            return false;
        }
        provider
            .list_installed()
            .await
            .map(|list| list.iter().any(|r| r.name == name))
            .unwrap_or(false)
    }

    /// Access the install registry (read-only).
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Trust level of a source, if it is enabled.
    pub fn source_trust(&self, source_id: &str) -> Option<crate::model::TrustLevel> {
        self.providers.get(source_id).map(|p| p.trust())
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
