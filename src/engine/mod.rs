//! The engine orchestrates the pipeline: `search → rank → plan → execute`.
//!
//! It is the only component that holds providers and the privilege layer, and the
//! only place that executes anything. It operates purely on the model.

pub mod ranking;

use crate::config::Config;
use crate::error::{JiiError, Result};
use crate::model::{InstallPlan, PackageCandidate, Query};
use crate::privilege::Privilege;
use crate::provider::ProviderRegistry;
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
}

impl Engine {
    /// Build the engine from config, instantiating the enabled providers.
    pub fn new(config: Config) -> Self {
        let providers = ProviderRegistry::from_config(&config);
        Engine {
            config,
            providers,
            privilege: Privilege::detect(),
        }
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
        let provider = self
            .providers
            .get(&candidate.source_id)
            .ok_or_else(|| JiiError::UnknownSource(candidate.source_id.clone()))?;
        provider.plan_install(candidate).await
    }

    /// Execute a plan. The single privileged entry point.
    pub async fn execute(&self, plan: &InstallPlan, renderer: &Renderer) -> Result<()> {
        self.privilege.execute_plan(plan, renderer).await
    }

    /// Access the effective configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }
}
