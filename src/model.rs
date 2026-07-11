//! Core data model shared across the whole pipeline: `search → rank → plan → execute`.
//!
//! These types are source-agnostic on purpose — the engine and UI operate only on
//! them, never on provider-specific details (see `docs/ARCHITECTURE.md` §11).

// The model is defined up front per the agreed architecture, so a few fields/variants
// are reserved for later phases. These are marked with a *targeted* `#[allow(dead_code)]`
// and a note at each site — rather than a module-wide silencer — so any *accidental* dead
// code added to this module in future is still caught (BETA_ROADMAP: "narrow or remove it").

use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A package version string as reported by a source.
///
/// Sources are heterogeneous — RPM uses EVR (`2.63.1-1.fc44`, with epoch/release),
/// GitHub uses tags (`v2.63.1`), Flatpak uses its own scheme — so we keep the raw
/// string for faithful display. Cross-source version comparison (the freshness
/// tie-breaker) is added in Phase 3 where it is actually needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PkgVersion(pub String);

impl PkgVersion {
    pub fn new(s: impl Into<String>) -> Self {
        PkgVersion(s.into())
    }
}

impl fmt::Display for PkgVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A user request, either a package name or a free-text description.
#[derive(Debug, Clone)]
pub struct Query {
    pub raw: String,
    /// Reserved: only `Name` is used today; `Description` lands with semantic/fuzzy
    /// search (Phase 6). Read then, so it's an intentional forward-looking field.
    #[allow(dead_code)]
    pub kind: QueryKind,
    /// How strictly a provider should match `raw` (ADR-0042). Defaults to `Exact`; the
    /// engine escalates to `Prefix` as a fallback only when an exact search finds nothing,
    /// so `jii git` stays noise-free while `jii ayugram` still finds `ayugram-desktop`.
    pub match_mode: MatchMode,
}

impl Query {
    /// Build an exact-name query (the default kind).
    pub fn name(raw: impl Into<String>) -> Self {
        Query {
            raw: raw.into(),
            kind: QueryKind::Name,
            match_mode: MatchMode::Exact,
        }
    }

    /// Same query, reinterpreted with a different match mode (the engine's broaden-on-miss step).
    pub fn with_match_mode(mut self, mode: MatchMode) -> Self {
        self.match_mode = mode;
        self
    }
}

/// How strictly a provider should match a [`Query`]'s `raw` term against package names.
/// A provider that can't express a mode (no glob support) may treat `Prefix` like its
/// natural search — the engine still filters/ranks by match quality afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// Match the name exactly (the noise-free default).
    Exact,
    /// Match names that start with the term (e.g. `ayugram` → `ayugram-desktop`).
    Prefix,
}

/// How a [`Query`] should be interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryKind {
    /// Match against package names (MVP).
    Name,
    /// Match against descriptions/metadata (future). Reserved for Phase 6 semantic search.
    #[allow(dead_code)]
    Description,
}

/// A parsed **package specification** — the language of JII (ADR-0031): `name[:source][@ref]`.
///
/// This is the single place a user package token is parsed. `name` is the only required part;
/// `:source` pins the owning provider (and, in the install flow, suppresses the source
/// chooser); `@ref` is a **source-interpreted** version/channel/branch reference the core only
/// *stores* — it never interprets it (ADR-0004; versions/refs are opaque to the core, ADR-0009).
/// Kept pure and unit-tested (ADR-0012); the CLI turns `name` into a [`Query`] and interprets
/// `source`/`reference`. It does **not** validate the source id against the known set — that
/// (with a did-you-mean) belongs to the CLI, which holds the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSpec {
    pub name: String,
    pub source: Option<String>,
    pub reference: Option<String>,
}

impl PackageSpec {
    /// Parse `name[:source][@ref]`, or an error string for a structurally invalid token.
    ///
    /// Splitting rules (ADR-0031):
    /// - `@ref` is the part after the **last, non-leading** `@` — an npm scoped name such as
    ///   `@angular/cli` starts with `@`, so a leading `@` is part of the name, never a ref;
    /// - `:source` is the part after the **last** `:` in what remains — source ids never contain
    ///   `:`, so the final `:`-segment is the source; a package name that itself contains a colon
    ///   is vanishingly rare and uses the `--source` flag as the escape hatch.
    pub fn parse(input: &str) -> Result<PackageSpec, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err("empty package name".to_string());
        }

        // 1. Peel off `@ref` on the last non-leading '@' (a leading '@' is an npm scope).
        let (left, reference) = match trimmed.rfind('@') {
            Some(at) if at > 0 => {
                let r = &trimmed[at + 1..];
                if r.is_empty() {
                    return Err(format!("'{trimmed}': empty version/channel after '@'"));
                }
                (&trimmed[..at], Some(r.to_string()))
            }
            _ => (trimmed, None),
        };

        // 2. Peel off `:source` on the last ':'.
        let (name, source) = match left.rfind(':') {
            Some(colon) => {
                let name = &left[..colon];
                let source = &left[colon + 1..];
                if name.is_empty() {
                    return Err(format!("'{trimmed}': empty package name before ':'"));
                }
                if source.is_empty() {
                    return Err(format!("'{trimmed}': empty source after ':'"));
                }
                (name.to_string(), Some(source.to_string()))
            }
            None => (left.to_string(), None),
        };

        Ok(PackageSpec { name, source, reference })
    }
}

/// Trust attached to a source or repository. Drives the confirmation barrier and
/// whether `--auto` may proceed silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    /// Official distro repos, verified Flathub, etc.
    Official,
    /// COPR, known third-party repos, crates.io, etc.
    Community,
    /// Arbitrary binaries / unknown URLs — always confirmed, even in `--auto`.
    Untrusted,
}

impl TrustLevel {
    /// Stable lowercase identifier for JSON output and config — never translated.
    pub fn label(&self) -> &'static str {
        match self {
            TrustLevel::Official => "official",
            TrustLevel::Community => "community",
            TrustLevel::Untrusted => "untrusted",
        }
    }

    /// Localized human label for prompts and previews (the identifier `label()` is kept
    /// stable for machine output; this one is translated).
    pub fn display(&self) -> String {
        match self {
            TrustLevel::Official => crate::t!("trust.official"),
            TrustLevel::Community => crate::t!("trust.community"),
            TrustLevel::Untrusted => crate::t!("trust.untrusted"),
        }
    }
}

/// Reported health of a source, used by `doctor` and as a ranking tie-breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Slow,
    Offline,
    RateLimited,
}

impl Health {
    /// Stable lowercase identifier for JSON output — never translated.
    pub fn label(&self) -> &'static str {
        match self {
            Health::Healthy => "healthy",
            Health::Slow => "slow",
            Health::Offline => "offline",
            Health::RateLimited => "rate-limited",
        }
    }

    /// Localized human label for the `doctor` sources table.
    pub fn display(&self) -> String {
        match self {
            Health::Healthy => crate::t!("health.healthy"),
            Health::Slow => crate::t!("health.slow"),
            Health::Offline => crate::t!("health.offline"),
            Health::RateLimited => crate::t!("health.rate_limited"),
        }
    }
}

/// How a downloaded artifact is verified before it is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// Expected lowercase hex SHA-256 digest.
    Sha256(String),
    /// GPG-signed artifact. Reserved: the verifier stubs it fail-closed until a source
    /// needs it (ADR-0016); kept so the trust model is complete and typed.
    #[allow(dead_code)]
    Gpg,
    /// Sigstore-signed artifact. Reserved, same as `Gpg`.
    #[allow(dead_code)]
    Sigstore,
    /// No verification available (the source provides none).
    None,
}

impl Verification {
    /// Short human label for previews and audit output.
    pub fn label(&self) -> &'static str {
        match self {
            Verification::Sha256(_) => "sha256",
            Verification::Gpg => "gpg",
            Verification::Sigstore => "sigstore",
            Verification::None => "unverified",
        }
    }
}

/// A repository match from a forge's **by-name** search (GitHub, Codeberg, …), shown in the
/// interactive repo picker *before* a release is resolved (ADR-0053). A hit is only a
/// pointer — the actual installable candidate is resolved on pick via
/// [`resolve_repo`](crate::provider::Provider::resolve_repo).
#[derive(Debug, Clone)]
pub struct RepoHit {
    /// The forge that produced this hit (routes the resolve back to the right provider).
    pub source_id: String,
    /// `owner/repo`.
    pub slug: String,
    /// One-line repo description, when the forge provides one.
    pub description: Option<String>,
    /// Popularity signal (stars), used as a cross-forge tiebreaker in the picker.
    pub stars: u64,
}

/// An alternative *way* to install a candidate, offered interactively beyond the default
/// imperative `plan_install` (ADR-0054). Nix is the first producer: it detects the user's
/// declarative setup and offers the config-snippet paths alongside `nix profile install`.
/// The core never branches on the source — the CLI shows whatever a provider returns and
/// either runs the imperative plan or prints the manual guidance.
#[derive(Debug, Clone)]
pub struct InstallStrategy {
    /// Menu label, e.g. "Add to ~/.config/home-manager/home.nix".
    pub label: String,
    /// One-line hint of what this choice means / when to use it.
    pub hint: String,
    /// What choosing it does.
    pub kind: StrategyKind,
}

/// What an [`InstallStrategy`] does when chosen.
#[derive(Debug, Clone)]
pub enum StrategyKind {
    /// Run the source's normal imperative `plan_install` (execute as usual).
    Imperative,
    /// **Show** this guidance and change nothing — a ready snippet, the file it goes in, the
    /// apply command, and a backup note. JII never writes the file or runs a command here
    /// ("show, never run", ADR-0048/0054 Etap A).
    Manual { guidance: String },
    /// **Auto-edit** a declarative config (Nix Etap B/C, ADR-0056/0058): the provider has
    /// already parsed the file and spliced the package into the existing list. The CLI shows
    /// `diff`, asks to confirm, backs the file up, then writes `new_content` — after which the
    /// user runs `apply` to activate it. For a user-owned file (`needs_root == false`, e.g.
    /// home-manager `home.nix`) the CLI writes it directly. For a root-owned file
    /// (`needs_root == true`, e.g. `/etc/nixos/configuration.nix`) the CLI performs the backup
    /// and write via `privilege.rs` (batched `sudo`/`pkexec`, exact commands shown first). The
    /// provider only *plans* — it never escalates or writes; the CLI does, through the one
    /// escalation path.
    EditFile {
        /// The config file to rewrite (already confirmed to exist and parse).
        path: PathBuf,
        /// The full new file contents — the package spliced into its declarative list.
        new_content: String,
        /// A rendered diff (unchanged-context / `-` removed / `+` added lines) shown first.
        diff: String,
        /// The command that activates the change once written (`home-manager switch`).
        apply: String,
        /// Whether writing the file requires root (a system config like
        /// `/etc/nixos/configuration.nix`). Drives whether the CLI writes directly or via the
        /// privilege-escalation path. The core stays source-agnostic — it acts on this flag,
        /// not on the source name.
        needs_root: bool,
    },
}

/// A single installation candidate produced by a provider's `search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageCandidate {
    pub name: String,
    pub source_id: String,
    pub version: Option<PkgVersion>,
    pub trust: TrustLevel,
    /// Whether this candidate is compatible with the current arch/libc.
    pub arch_ok: bool,
    /// Whether the artifact carries a signature/checksum we can verify.
    pub signed: bool,
    pub summary: Option<String>,
    /// Source-specific payload, opaque to the core, consumed by `plan_install`.
    pub raw: serde_json::Value,
}

/// Rich, human-facing metadata for `jii info`'s **app card** (#4) — description, links,
/// license, author. Every field is optional and filled only where a source cheaply knows
/// it; a provider assembles it in [`describe`](crate::provider::Provider::describe), and
/// the card renders just the fields that are present (graceful when a source offers none).
#[derive(Debug, Default, Serialize)]
pub struct PackageInfo {
    /// A longer description (falls back to the candidate's one-line summary).
    pub description: Option<String>,
    /// Project homepage / website.
    pub homepage: Option<String>,
    /// Source repository (e.g. the GitHub URL).
    pub repository: Option<String>,
    /// License identifier (e.g. `MIT`, `MPL-2.0`).
    pub license: Option<String>,
    /// Author / maintainer / vendor.
    pub author: Option<String>,
}

impl PackageInfo {
    /// Whether the card has anything worth a metadata block (any field present).
    pub fn is_empty(&self) -> bool {
        self.description.is_none()
            && self.homepage.is_none()
            && self.repository.is_none()
            && self.license.is_none()
            && self.author.is_none()
    }
}

/// An **informational** card for `jii info` resolved from a name, independent of whether the
/// package is an installable program (ADR-0045). This is how `info` (show) is separated from
/// `install`/`search` (act): a source can describe something it wouldn't offer to install —
/// e.g. an npm/cargo *library* — so `jii info lodash` shows what it is instead of an
/// install-flavoured "nothing to install".
#[derive(Debug, Serialize)]
pub struct Reference {
    /// The resolved package name (as the source spells it).
    pub name: String,
    /// The source that supplied this card (e.g. `npm`).
    pub source_id: String,
    /// The metadata card (description, homepage, …); may be sparse.
    pub info: PackageInfo,
    /// A one-line note about the package's nature, e.g. "npm library — a code dependency,
    /// not a runnable program". Present when the source has something clarifying to say.
    pub note: Option<String>,
    /// The package's version, when the source knows it cheaply.
    pub version: Option<PkgVersion>,
}

/// One action in a plan. Each variant has a single, clear responsibility; the plan
/// executor dispatches to a focused handler per variant. Providers describe actions
/// but never execute them.
#[derive(Debug, Clone)]
pub enum Action {
    /// Run an external command; `needs_root` requests privilege elevation.
    RunCommand { argv: Vec<String>, needs_root: bool },
    /// Download `url` to `dest`, enforcing `verify` before the file is used.
    Download {
        url: String,
        dest: PathBuf,
        verify: Verification,
    },
    /// Place a file at `dest` with the given unix `mode` (e.g. into ~/.local/bin).
    Place { src: PathBuf, dest: PathBuf, mode: u32 },
    /// Extract the binary named `member` from a (already-verified) archive to `dest`
    /// with the given unix `mode`. The handler locates the member inside the archive.
    Extract {
        archive: PathBuf,
        member: String,
        dest: PathBuf,
        mode: u32,
    },
    /// Remove a file (uninstall for file-based sources).
    RemoveFile { path: PathBuf },
    /// Atomically move `src` onto `dest` (rename within the same filesystem). Used to swap
    /// a freshly-downloaded binary over one that may be **running** — copying over a live
    /// executable fails with `ETXTBSY`, but a rename creates a new inode so the running
    /// process is undisturbed. Drives `jii update jii` for a user-space install.
    Replace { src: PathBuf, dest: PathBuf },
}

/// A previewable, executable plan. `Plan` is a first-class concept: every action
/// builds one before touching the system, and `--dry-run` renders it.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    /// Human-readable reference to the candidate this plan installs.
    pub candidate_ref: String,
    pub source_id: String,
    pub actions: Vec<Action>,
    pub download_size: Option<u64>,
    /// Why this candidate/source was recommended — shown to the user.
    pub reasons: Vec<String>,
}

impl InstallPlan {
    /// True if any action requires root (drives preview and elevation).
    pub fn needs_root(&self) -> bool {
        self.actions
            .iter()
            .any(|a| matches!(a, Action::RunCommand { needs_root: true, .. }))
    }
}

/// A record of software JII installed, persisted in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledRecord {
    pub name: String,
    pub source_id: String,
    pub version: Option<PkgVersion>,
    pub installed_at: DateTime<Utc>,
    /// How the artifact was verified at install time, as a [`Verification`] label
    /// (e.g. `"sha256"`, `"unverified"`). `None` means the install ran through a
    /// package manager that verifies itself (dnf/copr GPG, flatpak signatures).
    /// Recorded so `jii list --audit` can report provenance faithfully.
    #[serde(default)]
    pub verification: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::PackageSpec;

    fn spec(name: &str, source: Option<&str>, reference: Option<&str>) -> PackageSpec {
        PackageSpec {
            name: name.to_string(),
            source: source.map(str::to_string),
            reference: reference.map(str::to_string),
        }
    }

    #[test]
    fn plain_name() {
        assert_eq!(PackageSpec::parse("firefox").unwrap(), spec("firefox", None, None));
    }

    #[test]
    fn name_with_source() {
        assert_eq!(
            PackageSpec::parse("firefox:flatpak").unwrap(),
            spec("firefox", Some("flatpak"), None)
        );
    }

    #[test]
    fn name_with_ref() {
        assert_eq!(PackageSpec::parse("firefox@120").unwrap(), spec("firefox", None, Some("120")));
    }

    #[test]
    fn name_with_source_and_ref() {
        assert_eq!(
            PackageSpec::parse("node:brew@22").unwrap(),
            spec("node", Some("brew"), Some("22"))
        );
        // A ref may be a channel/branch, not a number — the core doesn't care (ADR-0031).
        assert_eq!(
            PackageSpec::parse("firefox:flatpak@stable").unwrap(),
            spec("firefox", Some("flatpak"), Some("stable"))
        );
    }

    #[test]
    fn leading_at_is_an_npm_scope_not_a_ref() {
        assert_eq!(PackageSpec::parse("@angular/cli").unwrap(), spec("@angular/cli", None, None));
    }

    #[test]
    fn npm_scope_with_ref_splits_on_the_last_at() {
        assert_eq!(
            PackageSpec::parse("@angular/cli@18").unwrap(),
            spec("@angular/cli", None, Some("18"))
        );
    }

    #[test]
    fn npm_scope_with_source_and_ref() {
        assert_eq!(
            PackageSpec::parse("@vue/cli:npm@5").unwrap(),
            spec("@vue/cli", Some("npm"), Some("5"))
        );
    }

    #[test]
    fn github_owner_repo_is_untouched() {
        assert_eq!(PackageSpec::parse("BurntSushi/ripgrep").unwrap(), spec("BurntSushi/ripgrep", None, None));
        assert_eq!(
            PackageSpec::parse("BurntSushi/ripgrep:github").unwrap(),
            spec("BurntSushi/ripgrep", Some("github"), None)
        );
    }

    #[test]
    fn source_splits_on_the_last_colon() {
        // A (rare) name containing a colon keeps everything before the final colon.
        assert_eq!(PackageSpec::parse("a:b:dnf").unwrap(), spec("a:b", Some("dnf"), None));
    }

    #[test]
    fn whitespace_is_trimmed() {
        assert_eq!(PackageSpec::parse("  firefox:flatpak  ").unwrap(), spec("firefox", Some("flatpak"), None));
    }

    #[test]
    fn structural_errors() {
        assert!(PackageSpec::parse("").is_err());
        assert!(PackageSpec::parse("   ").is_err());
        assert!(PackageSpec::parse(":flatpak").is_err()); // empty name before ':'
        assert!(PackageSpec::parse("firefox:").is_err()); // empty source after ':'
        assert!(PackageSpec::parse("firefox@").is_err()); // empty ref after '@'
    }
}
