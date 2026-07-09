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
    Action, InstallPlan, InstalledRecord, PackageCandidate, PackageInfo, PkgVersion, Query,
    TrustLevel,
};

pub mod apt;
pub mod cargo;
pub mod copr;
pub mod dnf;
pub mod flatpak;
pub mod github;
pub mod go;
pub mod homebrew;
pub mod nix;
pub mod npm;
pub mod pacman;
pub mod pipx;
pub mod snap;
pub mod zypper;

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

    /// Build **one** plan that installs several candidates at once (e.g. `dnf install a
    /// b c`), or `None` if this source can't batch — the engine then falls back to one
    /// [`plan_install`] per candidate. Default: `None`. Overriding is the same
    /// optional-method growth as `is_installed`/`probe` (ADR-0022): a source opts into
    /// batching by assembling a single multi-package command; the engine never branches
    /// on the source id, it just uses the returned plan or falls back. `candidates` is
    /// non-empty and all share this provider's `source_id`.
    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        let _ = candidates;
        Ok(None)
    }

    /// Build a removal plan for a previously installed record.
    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan>;

    /// Build **one** plan that removes several records at once (e.g. `dnf remove a b c`),
    /// or `None` if this source can't batch — the engine then falls back to one
    /// [`plan_remove`] per record. Default `None`. The remove-side twin of
    /// [`plan_install_many`](Provider::plan_install_many): same optional-method growth
    /// (ADR-0022/0025), the engine never branches on the source. `records` is non-empty and
    /// all share this provider's `source_id`.
    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let _ = records;
        Ok(None)
    }

    /// Build an update plan for a previously installed record (drives `jii update`).
    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan>;

    /// Build **one** plan that updates several records at once (e.g. `dnf upgrade a b c`),
    /// or `None` if this source can't batch — the engine falls back to one [`plan_update`]
    /// per record. Default `None`. The update-side twin of `plan_install_many`.
    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let _ = records;
        Ok(None)
    }

    /// Build one plan that updates **everything this source owns** (`dnf upgrade`,
    /// `flatpak update`, `pipx upgrade-all`…), independent of JII's registry — the "update
    /// my whole system" path behind a bare `jii update` (D10). Default `None`: a source with
    /// no first-class bulk-upgrade (github, cargo, go) opts out, and the engine falls back to
    /// per-record updates for it. Pure ADR-0022/0025 growth — the engine aggregates the plans
    /// every willing provider offers and never branches on the source id.
    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        Ok(None)
    }

    /// What is installed via this source, to verify the registry.
    async fn list_installed(&self) -> Result<Vec<InstalledRecord>>;

    /// Whether `record` is still installed via this source. Default: look it up in
    /// `list_installed`. File-based sources (e.g. github) that cannot enumerate their
    /// installs override this to check the installed file directly.
    async fn is_installed(&self, record: &InstalledRecord) -> bool {
        self.list_installed()
            .await
            .is_ok_and(|list| list.iter().any(|r| r.name == record.name))
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

    /// Short, honest, **source-specific** reasons this candidate is a good pick — the
    /// "why" behind a recommendation that the source-agnostic core cannot phrase without
    /// branching on the source id (ADR-0004). Default: none. A provider — which *is*
    /// allowed to know it is dnf/flatpak/github — overrides this to add tags like "Official
    /// Fedora package" or "Sandboxed". The engine concatenates these with the model-derived
    /// facts (trust, signature, version) for `jii info` and the chooser; it stays optional,
    /// exactly the ADR-0022 growth shape. Synchronous and pure (no I/O) — it reads only the
    /// candidate it is given. (UX D5.)
    fn highlights(&self, candidate: &PackageCandidate) -> Vec<String> {
        let _ = candidate;
        Vec::new()
    }

    /// When a search for `query` yields **no candidate from this source**, optionally explain
    /// *why an exact name that exists still isn't installable* — e.g. a crates.io/npm library
    /// that ships no executable (JII installs programs, not libraries). Default `None`. The
    /// engine calls this **only on a total miss** (no source produced a candidate), so it is
    /// off the hot path and may do one lookup. ADR-0022 optional-method growth: it turns a bare
    /// "not found" into a helpful "that's a library, not a program" (#9) with no core knowledge
    /// of the source.
    async fn explain_miss(&self, query: &Query) -> Option<String> {
        let _ = query;
        None
    }

    /// Rich human metadata for `jii info`'s **app card** (#4): description, homepage,
    /// repository, license, author. Default `None` — a source that can't cheaply describe a
    /// candidate opts out and the card degrades to the basics it already has (version, trust,
    /// source). This is the ADR-0022 optional-method growth again; it *may* do I/O (dnf runs
    /// `dnf5 info`), so it is async, unlike the pure `highlights`. The engine calls it only for
    /// the recommended candidate on `jii info`, never on the hot search path.
    async fn describe(&self, candidate: &PackageCandidate) -> Option<PackageInfo> {
        let _ = candidate;
        None
    }

    /// An **informational** card for `jii info` resolved from a name, even when the package
    /// is not an installable program (ADR-0045) — e.g. an npm/cargo library. This is what
    /// separates `info` (show) from `search`/`install` (act): a source may describe something
    /// it would never offer to install. Default `None`. Off the hot path — the engine calls
    /// it only for `jii info`, and only when the normal (installable) resolve found nothing.
    async fn reference(&self, query: &Query) -> Option<crate::model::Reference> {
        let _ = query;
        None
    }

    /// If this source is an installable *ecosystem* manager (npm, cargo, brew, flatpak…),
    /// describe it for `jii providers` and for bootstrapping a missing manager (#7/#8).
    /// Default `None`: base system repos (dnf/copr/apt/pacman/zypper) and non-manager
    /// sources (github) are things JII *drives*, never *installs*. Pure metadata, no I/O —
    /// the same optional-method growth as `highlights`/`plan_update_all` (ADR-0022); the
    /// engine aggregates it and never branches on the source id.
    fn ecosystem(&self) -> Option<Ecosystem> {
        None
    }
}

/// How JII bootstraps a missing ecosystem manager (see [`Ecosystem`]). Holds only
/// `'static` metadata, so it is cheap to copy out of a catalog row.
#[derive(Debug, Clone, Copy)]
pub enum Bootstrap {
    /// Install one of these packages through JII's own install path — the **first that
    /// resolves** on this host wins (npm is `nodejs-npm` on Fedora, `npm` on Debian/Arch;
    /// go is `golang` on Fedora, `golang-go` on Debian). Cross-distro is handled by JII's
    /// own search, not a distro branch here.
    Packages(&'static [&'static str]),
    /// Bootstrapped by an upstream installer script JII will **show, never run** — piping
    /// a script into a shell is exactly the trust boundary JII refuses to cross (ADR-0005/0006).
    Script(&'static str),
}

/// An installable *ecosystem* manager (npm, cargo, brew, flatpak…), surfaced by
/// `jii providers`. Returned by [`Provider::ecosystem`]; base system repos and
/// non-managers return `None`. Pure metadata — no I/O, no per-host branching.
pub struct Ecosystem {
    /// Human label, e.g. "Node.js (npm)".
    pub label: &'static str,
    /// The command that proves the manager is present.
    pub binary: &'static str,
    /// How JII bootstraps it when it is missing.
    pub bootstrap: Bootstrap,
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
        if config.is_enabled("apt") {
            providers.push(Box::new(apt::Apt::new()));
        }
        if config.is_enabled("pacman") {
            providers.push(Box::new(pacman::Pacman::new()));
        }
        if config.is_enabled("zypper") {
            providers.push(Box::new(zypper::Zypper::new()));
        }
        if config.is_enabled("nix") {
            providers.push(Box::new(nix::Nix::new()));
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
        if config.is_enabled("brew") {
            providers.push(Box::new(homebrew::Homebrew::new()));
        }
        if config.is_enabled("snap") {
            providers.push(Box::new(snap::Snap::new()));
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

/// Like [`run_capture`], but returns stdout **even on a non-zero exit** (stderr ignored).
/// The lenient sibling for tools whose "no such package" is an error exit rather than empty
/// output — `apt-cache show` exits 100, `pacman -Si` exits 1. To JII that is "no candidate",
/// not a source failure, so an empty search result reads correctly instead of tagging the
/// source as broken. A spawn failure (the tool is absent) still errors.
pub(crate) async fn run_capture_lax(argv: &[&str]) -> Result<String> {
    let output = Command::new(argv[0])
        .args(&argv[1..])
        .output()
        .await
        .map_err(|e| JiiError::spawn(argv[0], e))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether an executable is runnable (used for `is_available`).
pub(crate) async fn which(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .await
        .is_ok_and(|o| o.status.success())
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

    #[test]
    fn ecosystems_declare_bootstrap_and_base_repos_do_not() {
        // Base system repos are the system, not something JII installs → no ecosystem.
        assert!(dnf::Dnf::new().ecosystem().is_none());
        assert!(
            github::Github::new("GITHUB_TOKEN".into(), "x86_64")
                .ecosystem()
                .is_none()
        );

        // Every ecosystem manager declares a non-empty binary and a usable bootstrap.
        let ecos: Vec<(&str, Option<Ecosystem>)> = vec![
            ("npm", npm::Npm::new().ecosystem()),
            ("cargo", cargo::Cargo::new().ecosystem()),
            ("pipx", pipx::Pipx::new().ecosystem()),
            ("go", go::Go::new().ecosystem()),
            ("flatpak", flatpak::Flatpak::new().ecosystem()),
            ("snap", snap::Snap::new().ecosystem()),
            ("brew", homebrew::Homebrew::new().ecosystem()),
            ("nix", nix::Nix::new().ecosystem()),
        ];
        for (id, eco) in ecos {
            let eco = eco.unwrap_or_else(|| panic!("{id} should declare an ecosystem"));
            assert!(!eco.binary.is_empty(), "{id} has an empty binary");
            match eco.bootstrap {
                Bootstrap::Packages(names) => {
                    assert!(!names.is_empty(), "{id} declares no bootstrap packages")
                }
                Bootstrap::Script(cmd) => assert!(!cmd.is_empty(), "{id} declares an empty script"),
            }
        }
    }
}
