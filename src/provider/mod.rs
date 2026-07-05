//! The provider abstraction: every installation source implements [`Provider`].
//!
//! The core operates only through this trait and the source-agnostic model — it
//! never branches on a concrete source id (see `docs/ARCHITECTURE.md` §5).

use async_trait::async_trait;
use tokio::process::Command;

use serde::de::DeserializeOwned;

use crate::config::Config;
use crate::error::{JiiError, Result};
use crate::model::{
    Action, InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
};

pub mod cargo;
pub mod copr;
pub mod dnf;
pub mod flatpak;
pub mod github;
pub mod go;
pub mod npm;
pub mod pipx;

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

    /// Whether `record` is still installed via this source. Default: look it up in
    /// `list_installed`. File-based sources (e.g. github) that cannot enumerate their
    /// installs override this to check the installed file directly.
    async fn is_installed(&self, record: &InstalledRecord) -> bool {
        self.list_installed()
            .await
            .map(|list| list.iter().any(|r| r.name == record.name))
            .unwrap_or(false)
    }

    /// Probe this source's live health for `jii doctor`. Default: local availability
    /// only. Network sources (github, copr) override this to check API reachability
    /// and, where relevant, rate limits. Providers report raw facts; the engine
    /// decides the [`Health`](crate::model::Health) category.
    async fn probe(&self) -> Probe {
        Probe {
            reachable: self.is_available().await,
            rate_limited: false,
            detail: None,
        }
    }
}

/// A raw health probe of a source (mapped to a `Health` category by the engine).
pub struct Probe {
    /// Whether the source responded / is usable right now.
    pub reachable: bool,
    /// Whether the source is currently rate-limited.
    pub rate_limited: bool,
    /// Optional human detail (e.g. remaining rate-limit budget).
    pub detail: Option<String>,
}

impl Probe {
    /// A probe for a source that did not respond.
    pub fn unreachable() -> Self {
        Probe {
            reachable: false,
            rate_limited: false,
            detail: None,
        }
    }
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

        // Register known providers; later phases add more here.
        if config.is_enabled("dnf") {
            providers.push(Box::new(dnf::Dnf::new()));
        }
        if config.is_enabled("copr") {
            providers.push(Box::new(copr::Copr::new(
                crate::platform::Platform::detect().arch,
            )));
        }
        if config.is_enabled("flatpak") {
            providers.push(Box::new(flatpak::Flatpak::new()));
        }
        if config.is_enabled("github") {
            providers.push(Box::new(github::Github::new(
                config.network.github_token_env.clone(),
                crate::platform::Platform::detect().arch,
            )));
        }
        if config.is_enabled("cargo") {
            providers.push(Box::new(cargo::Cargo::new()));
        }
        if config.is_enabled("npm") {
            providers.push(Box::new(npm::Npm::new()));
        }
        if config.is_enabled("pipx") {
            providers.push(Box::new(pipx::Pipx::new()));
        }
        if config.is_enabled("go") {
            providers.push(Box::new(go::Go::new()));
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

// ---- Shared helpers for network providers (crates.io, npm, COPR, GitHub…) ----

/// The HTTP client network providers use. Sends the User-Agent registries expect and
/// gives us one place to evolve UA / transport policy. Per-request auth/headers stay in
/// the provider (e.g. github's bearer token).
pub(crate) fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("jii/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| JiiError::Other(anyhow::anyhow!("http client: {e}")))
}

/// GET `url` and deserialize the body as `T`, treating **404 as "no such item"**
/// (`Ok(None)`) rather than an error. Shared by exact-name registry providers
/// (cargo/npm/pipx/go…), so the network + not-found + error-formatting policy lives in
/// one place; `source_id` prefixes error messages. Providers with a different request
/// shape (github's authed release fetch, copr's query search) use `http_client` directly.
pub(crate) async fn get_json_opt<T: DeserializeOwned>(
    source_id: &str,
    url: &str,
) -> Result<Option<T>> {
    let resp = http_client()?
        .get(url)
        .send()
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("{source_id}: {e}")))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let resp = resp
        .error_for_status()
        .map_err(|e| JiiError::Other(anyhow::anyhow!("{source_id}: {e}")))?;
    let body = resp
        .json::<T>()
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("{source_id}: malformed json: {e}")))?;
    Ok(Some(body))
}

/// Build a plan of **one** command action. Shared by the single-step providers
/// (dnf/cargo/npm/pipx/go…): each assembles its own `argv`, this centralises the
/// `InstallPlan` shape so a model change is a one-line edit, not a per-provider one.
/// (copr's two-step enable+install plan is genuinely different and stays local.)
pub(crate) fn command_plan(
    source_id: &str,
    name: &str,
    argv: Vec<String>,
    needs_root: bool,
    reasons: Vec<String>,
) -> InstallPlan {
    InstallPlan {
        candidate_ref: name.to_string(),
        source_id: source_id.to_string(),
        actions: vec![Action::RunCommand { argv, needs_root }],
        download_size: None,
        reasons,
    }
}

// ---- Shared helpers for CLI-backed providers (dnf, flatpak, …) ----

/// Non-blank lines of a command's output, in order.
pub(crate) fn nonempty_lines(stdout: &str) -> impl Iterator<Item = &str> {
    stdout.lines().filter(|l| !l.trim().is_empty())
}

/// Run a command and return its stdout as a string. Errors if the binary cannot be
/// spawned or exits non-zero.
pub(crate) async fn run_capture(argv: &[&str]) -> Result<String> {
    let output = Command::new(argv[0])
        .args(&argv[1..])
        .output()
        .await
        .map_err(|e| JiiError::spawn(argv[0], e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(JiiError::Other(anyhow::anyhow!(
            "{} failed: {}",
            argv.join(" "),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether an executable is runnable (used for `is_available`).
pub(crate) async fn which(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Parse tab-separated `name<TAB>version` lines into installed records. Shared by
/// providers whose "list installed" output has that shape (dnf, flatpak).
pub(crate) fn parse_installed_records(stdout: &str, source_id: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    nonempty_lines(stdout)
        .filter_map(|line| {
            let mut fields = line.splitn(2, '\t');
            let name = fields.next()?.trim();
            if name.is_empty() {
                return None;
            }
            let version = fields.next().unwrap_or("").trim();
            Some(InstalledRecord {
                name: name.to_string(),
                source_id: source_id.to_string(),
                version: (!version.is_empty()).then(|| PkgVersion::new(version)),
                installed_at: now,
                // Live-queried from the manager, not a jii install record.
                verification: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_installed_records() {
        let sample = "bash\t5.3.9-3.fc44\nfastfetch\t2.63.1-1.fc44\n";
        let recs = parse_installed_records(sample, "dnf");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "bash");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "5.3.9-3.fc44");
        assert_eq!(recs[1].name, "fastfetch");
    }

    #[test]
    fn skips_blank_and_nameless_lines() {
        let recs = parse_installed_records("\n\t1.0\nvalid\t2.0\n", "flatpak");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].name, "valid");
        assert_eq!(recs[0].source_id, "flatpak");
    }
}
