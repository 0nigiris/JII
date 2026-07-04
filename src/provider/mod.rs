//! The provider abstraction: every installation source implements [`Provider`].
//!
//! The core operates only through this trait and the source-agnostic model — it
//! never branches on a concrete source id (see `docs/ARCHITECTURE.md` §5).

use async_trait::async_trait;

use crate::config::Config;
use crate::error::Result;
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, Query, TrustLevel};

pub mod dnf;

/// A source of installable software (a package manager, a repo, a registry).
///
/// Providers **plan but never execute privileged actions** — they return steps
/// flagged `needs_root`; the engine's privilege layer performs elevation.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable id, e.g. "dnf", "flatpak", "github".
    fn id(&self) -> &'static str;

    /// Base trust level of this source.
    fn trust(&self) -> TrustLevel;

    /// Whether this source is usable on the current machine (binary present, etc.).
    async fn is_available(&self) -> bool;

    /// Find candidates for a query. Must not panic on failure — a returned `Err`
    /// lets the engine tag the source and continue with the rest.
    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>>;

    /// Build an install plan without executing it.
    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan>;

    /// Build a removal plan for a previously installed record.
    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan>;

    /// Build an update plan for a previously installed record. (Called from Phase 5.)
    #[allow(dead_code)]
    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan>;

    /// What is installed via this source, to verify the registry.
    async fn list_installed(&self) -> Result<Vec<InstalledRecord>>;
}

/// The set of providers enabled for this run, in configured priority order.
pub struct ProviderRegistry {
    providers: Vec<Box<dyn Provider>>,
}

impl ProviderRegistry {
    /// Build the registry from config: instantiate known providers, drop disabled
    /// ones, and order them by the configured source priority.
    pub fn from_config(config: &Config) -> Self {
        let mut providers: Vec<Box<dyn Provider>> = Vec::new();

        // Phase 1: DNF only. Later phases register more here.
        if config.is_enabled("dnf") {
            providers.push(Box::new(dnf::Dnf::new()));
        }

        providers.sort_by_key(|p| config.source_rank(p.id()));
        ProviderRegistry { providers }
    }

    /// Iterate over the enabled providers in priority order.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Provider> {
        self.providers.iter().map(|p| p.as_ref())
    }

    /// Look up a provider by id.
    pub fn get(&self, id: &str) -> Option<&dyn Provider> {
        self.iter().find(|p| p.id() == id)
    }

    /// Number of enabled providers (used by `doctor` in Phase 3).
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Whether no providers are enabled.
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}
