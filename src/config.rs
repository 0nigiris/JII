//! Configuration: typed struct, sane defaults, TOML loading and validation.
//!
//! Precedence is CLI flag > env > config > default; this module owns the
//! config-file layer. The format is documented in `docs/ARCHITECTURE.md` §12.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{JiiError, Result};
use crate::model::TrustLevel;

/// Source ids JII knows about. Used to validate `priority` / `disabled`.
///
/// Providers are wired in over later phases; this list is the contract the config
/// validates against so a typo fails fast instead of silently doing nothing.
pub const KNOWN_SOURCES: &[&str] = &[
    "dnf", "copr", "apt", "pacman", "zypper", "void", "gentoo", "flatpak", "snap", "github",
    "cargo", "npm", "pipx", "go", "brew", "nix",
];

/// Top-level configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub sources: SourcesConfig,
    pub install: InstallConfig,
    pub trust: TrustConfig,
    pub network: NetworkConfig,
    pub ui: UiConfig,
    pub meta: MetaConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SourcesConfig {
    /// Source ids in priority order (index = rank). Sources absent here rank last.
    pub priority: Vec<String>,
    /// Sources to disable entirely.
    pub disabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InstallConfig {
    pub profile: Profile,
    pub default_yes: bool,
    /// Highest trust level that `default_yes` applies to; below it, JII still asks.
    pub default_yes_max_trust: TrustLevel,
    pub auto: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustConfig {
    /// Trust level at (and below) which a signature is required.
    pub require_signature: TrustLevel,
    /// Whether `--auto` may install untrusted sources without confirmation.
    pub allow_untrusted_auto: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub timeout_secs: u64,
    pub cache_ttl_secs: u64,
    /// How long (seconds) a source that timed out / errored is skipped on later searches
    /// before being retried — the circuit-breaker cooldown that keeps a slow source (e.g.
    /// COPR) from making every search wait out its timeout again.
    #[serde(default = "default_failure_cooldown_secs")]
    pub failure_cooldown_secs: u64,
    /// Env var name to read a token from (e.g. GITHUB_TOKEN) to lift rate limits.
    pub github_token_env: String,
}

/// Default circuit-breaker cooldown: two minutes is long enough to spare repeat searches
/// in a session, short enough that a recovered source comes back quickly.
fn default_failure_cooldown_secs() -> u64 {
    120
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub color: ColorChoice,
    pub locale: String,
    /// How much to say: `friendly` (short, human, the default) or `advanced` (full detail).
    pub mode: OutputMode,
}

/// State JII records about itself (not user preferences). Kept in its own section so a
/// hand-edited config never mixes it with tunables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MetaConfig {
    /// Set once the first-run wizard has been offered, so it never auto-runs again.
    pub first_run_completed: bool,
}

/// How much output JII produces. Drives the Friendly/Advanced split (UX U5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    /// Short, lively, jargon-free — for someone who just opened a terminal (default).
    #[default]
    Friendly,
    /// Full detail: per-source failures, complete plans, source rationale.
    Advanced,
}

/// Ranking presets. See `docs/ARCHITECTURE.md` §6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// Prefer distro repositories (default).
    Stable,
    /// Freshness beats priority.
    Latest,
    /// Prefer Flatpak.
    Sandbox,
    /// Prefer the smallest dependency footprint.
    Minimal,
}

/// When to emit ANSI colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl Default for SourcesConfig {
    fn default() -> Self {
        SourcesConfig {
            priority: [
                "dnf", "copr", "apt", "pacman", "zypper", "void", "gentoo", "flatpak", "snap",
                "github", "cargo", "npm", "pipx", "go", "brew", "nix",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            disabled: Vec::new(),
        }
    }
}

impl Default for InstallConfig {
    fn default() -> Self {
        InstallConfig {
            profile: Profile::Stable,
            default_yes: true,
            default_yes_max_trust: TrustLevel::Community,
            auto: false,
        }
    }
}

impl Default for TrustConfig {
    fn default() -> Self {
        TrustConfig {
            require_signature: TrustLevel::Untrusted,
            allow_untrusted_auto: false,
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            // Per-source search budget. Local managers answer in <100 ms; the fast network
            // sources (github/cargo/npm) in 1–3 s. The outlier is COPR's search API (~9 s on
            // a clean Fedora box), which blew the whole parallel search out to ~8 s while dnf
            // already had the answer. 5 s keeps the common case snappy; a source slower than
            // that is skipped (COPR is community-trust, ranked below dnf) — a deliberate
            // speed/coverage trade. A cached result still serves once obtained. See
            // docs/UX_EVALUATION.md (U0/U2).
            timeout_secs: 5,
            cache_ttl_secs: 3600,
            failure_cooldown_secs: default_failure_cooldown_secs(),
            github_token_env: "GITHUB_TOKEN".to_string(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig {
            color: ColorChoice::Auto,
            locale: "auto".to_string(),
            mode: OutputMode::Friendly,
        }
    }
}

impl Config {
    /// Default path: `$XDG_CONFIG_HOME/jii/config.toml`.
    pub fn default_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "jii")
            .map(|d| d.config_dir().join("config.toml"))
    }

    /// Load config from `path`, falling back to defaults if it does not exist.
    pub fn load_from(path: &Path) -> Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let cfg: Config = toml::from_str(&text)
                    .map_err(|e| JiiError::Config(format!("{}: {e}", path.display())))?;
                cfg.validate()?;
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(JiiError::io(path, e)),
        }
    }

    /// Load from the default path (or defaults if none is resolvable).
    pub fn load() -> Result<Config> {
        match Self::default_path() {
            Some(p) => Self::load_from(&p),
            None => Ok(Config::default()),
        }
    }

    /// Write the config back to the default path (creating the directory if needed). Used by
    /// the first-run wizard / `jii setup` to persist the chosen mode and the first-run flag.
    pub fn save(&self) -> Result<()> {
        let path = Self::default_path()
            .ok_or_else(|| JiiError::Config("cannot resolve a config path to save to".into()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| JiiError::io(&path, e))?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| JiiError::Config(format!("serializing config: {e}")))?;
        std::fs::write(&path, text).map_err(|e| JiiError::io(&path, e))?;
        Ok(())
    }

    /// Whether the first-run wizard has not yet been completed.
    pub fn is_first_run(&self) -> bool {
        !self.meta.first_run_completed
    }

    /// Reject unknown source ids so typos fail loudly.
    pub fn validate(&self) -> Result<()> {
        for id in self.sources.priority.iter().chain(&self.sources.disabled) {
            if !KNOWN_SOURCES.contains(&id.as_str()) {
                return Err(JiiError::UnknownSource(id.clone()));
            }
        }
        Ok(())
    }

    /// Rank of a source id: its index in `priority`, or a large number if absent.
    pub fn source_rank(&self, id: &str) -> usize {
        self.sources
            .priority
            .iter()
            .position(|s| s == id)
            .unwrap_or(usize::MAX)
    }

    /// Whether a source is enabled (not in `disabled`).
    pub fn is_enabled(&self, id: &str) -> bool {
        !self.sources.disabled.iter().any(|s| s == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn rejects_unknown_source() {
        let mut cfg = Config::default();
        cfg.sources.priority.push("totally-unknown".to_string());
        assert!(
            matches!(cfg.validate(), Err(JiiError::UnknownSource(s)) if s == "totally-unknown")
        );
    }

    #[test]
    fn parses_partial_toml_with_defaults() {
        let cfg: Config = toml::from_str("[install]\ndefault_yes = false\n").unwrap();
        assert!(!cfg.install.default_yes);
        // Untouched fields fall back to defaults.
        assert_eq!(cfg.install.profile, Profile::Stable);
        assert_eq!(cfg.network.timeout_secs, 5);
    }

    #[test]
    fn source_rank_orders_by_priority() {
        let cfg = Config::default();
        assert!(cfg.source_rank("dnf") < cfg.source_rank("flatpak"));
        assert_eq!(cfg.source_rank("nonexistent"), usize::MAX);
    }

    #[test]
    fn mode_defaults_to_friendly_and_first_run_is_true() {
        let cfg = Config::default();
        assert_eq!(cfg.ui.mode, OutputMode::Friendly);
        assert!(cfg.is_first_run());
    }

    #[test]
    fn round_trips_through_toml() {
        let mut cfg = Config::default();
        cfg.ui.mode = OutputMode::Advanced;
        cfg.meta.first_run_completed = true;
        // `save` writes to the real config dir, so exercise the same serialization directly.
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.ui.mode, OutputMode::Advanced);
        assert!(!back.is_first_run());
    }

    #[test]
    fn parses_mode_from_partial_toml() {
        let cfg: Config = toml::from_str("[ui]\nmode = \"advanced\"\n").unwrap();
        assert_eq!(cfg.ui.mode, OutputMode::Advanced);
        // Untouched sections still default.
        assert!(cfg.is_first_run());
    }
}
