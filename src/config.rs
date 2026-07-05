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
    "dnf", "copr", "apt", "pacman", "zypper", "flatpak", "snap", "github", "cargo", "npm",
    "pipx", "go", "brew", "nix",
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
    /// Env var name to read a token from (e.g. GITHUB_TOKEN) to lift rate limits.
    pub github_token_env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub color: ColorChoice,
    pub locale: String,
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
                "dnf", "copr", "apt", "pacman", "zypper", "flatpak", "snap", "github", "cargo",
                "npm", "pipx", "go", "brew", "nix",
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
            timeout_secs: 8,
            cache_ttl_secs: 3600,
            github_token_env: "GITHUB_TOKEN".to_string(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig {
            color: ColorChoice::Auto,
            locale: "auto".to_string(),
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
        assert_eq!(cfg.network.timeout_secs, 8);
    }

    #[test]
    fn source_rank_orders_by_priority() {
        let cfg = Config::default();
        assert!(cfg.source_rank("dnf") < cfg.source_rank("flatpak"));
        assert_eq!(cfg.source_rank("nonexistent"), usize::MAX);
    }
}
