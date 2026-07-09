//! Installation registry: JSON record of what JII installed and a history log.
//!
//! The registry stores *intentions* — it is a hint, not the source of truth. Before
//! acting on it (remove/update), the engine verifies against the real package
//! manager (see `Engine::resolve_installed`). It is written only after a successful
//! operation.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{JiiError, Result};
use crate::model::{InstalledRecord, PkgVersion};

/// A recorded action, kept in the history log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Install,
    Remove,
    Update,
}

impl Action {
    /// A localized past-tense verb for history output.
    pub fn display(&self) -> String {
        match self {
            Action::Install => crate::t!("action.installed"),
            Action::Remove => crate::t!("action.removed"),
            Action::Update => crate::t!("action.updated"),
        }
    }
}

/// One entry in the history log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub action: Action,
    pub name: String,
    pub source_id: String,
    pub version: Option<PkgVersion>,
    pub at: DateTime<Utc>,
}

/// The persisted state: currently-installed records plus an append-only history.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    installed: Vec<InstalledRecord>,
    history: Vec<HistoryEvent>,
    /// Where this registry is persisted. Not serialized.
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl Registry {
    /// Default path: `$XDG_STATE_HOME/jii/state.json` (falls back to data dir).
    pub fn default_path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "jii")?;
        let base = dirs.state_dir().unwrap_or_else(|| dirs.data_dir());
        Some(base.join("state.json"))
    }

    /// Load from `path`, or start empty (remembering the path) if it does not exist.
    pub fn load_from(path: &Path) -> Result<Registry> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut reg: Registry = serde_json::from_str(&text)
                    .map_err(|e| JiiError::Config(format!("{}: {e}", path.display())))?;
                reg.path = Some(path.to_path_buf());
                Ok(reg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry {
                path: Some(path.to_path_buf()),
                ..Default::default()
            }),
            Err(e) => Err(JiiError::io(path, e)),
        }
    }

    /// Load from the default path (empty, unpersisted, if none is resolvable).
    pub fn load() -> Result<Registry> {
        match Self::default_path() {
            Some(p) => Self::load_from(&p),
            None => Ok(Registry::default()),
        }
    }

    /// Persist to disk. A no-op if there is no path (e.g. no HOME).
    pub fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| JiiError::io(parent, e))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| JiiError::Other(anyhow::anyhow!("failed to serialize registry: {e}")))?;
        std::fs::write(path, text).map_err(|e| JiiError::io(path, e))
    }

    /// Record a successful install (replacing any prior record of the same package).
    pub fn record_install(&mut self, record: InstalledRecord) {
        self.upsert(Action::Install, record);
    }

    /// Record a successful update: replace the record with its new version/timestamp
    /// and log it as an `Update` (an update is a reinstall-to-newest, so the stored
    /// record is refreshed the same way — only the history verb differs).
    pub fn record_update(&mut self, record: InstalledRecord) {
        self.upsert(Action::Update, record);
    }

    /// Shared install/update write: replace any prior record of the same
    /// package+source, log the change, and store the new record. One place so the
    /// "replace + log + push" invariant can never drift between install and update.
    fn upsert(&mut self, action: Action, record: InstalledRecord) {
        self.installed
            .retain(|r| !(r.name == record.name && r.source_id == record.source_id));
        self.history.push(HistoryEvent {
            action,
            name: record.name.clone(),
            source_id: record.source_id.clone(),
            version: record.version.clone(),
            at: record.installed_at,
        });
        self.installed.push(record);
    }

    /// Record a successful removal.
    pub fn record_remove(&mut self, name: &str, source_id: &str) {
        let version = self
            .get(name)
            .and_then(|r| r.version.clone());
        self.installed
            .retain(|r| !(r.name == name && r.source_id == source_id));
        self.history.push(HistoryEvent {
            action: Action::Remove,
            name: name.to_string(),
            source_id: source_id.to_string(),
            version,
            at: Utc::now(),
        });
    }

    /// The recorded install for a package, if any.
    pub fn get(&self, name: &str) -> Option<&InstalledRecord> {
        self.installed.iter().find(|r| r.name == name)
    }

    /// All recorded installs.
    pub fn installed(&self) -> &[InstalledRecord] {
        &self.installed
    }

    /// The history log (oldest first).
    pub fn history(&self) -> &[HistoryEvent] {
        &self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, source: &str) -> InstalledRecord {
        InstalledRecord {
            name: name.into(),
            source_id: source.into(),
            version: Some(PkgVersion::new("1.0")),
            installed_at: Utc::now(),
            verification: None,
        }
    }

    #[test]
    fn install_then_get() {
        let mut reg = Registry::default();
        reg.record_install(record("fastfetch", "dnf"));
        assert_eq!(reg.get("fastfetch").unwrap().source_id, "dnf");
        assert_eq!(reg.installed().len(), 1);
        assert_eq!(reg.history().len(), 1);
    }

    #[test]
    fn reinstall_replaces_not_duplicates() {
        let mut reg = Registry::default();
        reg.record_install(record("fastfetch", "dnf"));
        reg.record_install(record("fastfetch", "dnf"));
        assert_eq!(reg.installed().len(), 1);
        assert_eq!(reg.history().len(), 2); // both installs logged
    }

    #[test]
    fn remove_clears_record_and_logs() {
        let mut reg = Registry::default();
        reg.record_install(record("fastfetch", "dnf"));
        reg.record_remove("fastfetch", "dnf");
        assert!(reg.get("fastfetch").is_none());
        assert_eq!(reg.installed().len(), 0);
        assert_eq!(reg.history().last().unwrap().action, Action::Remove);
    }

    #[test]
    fn update_refreshes_version_and_logs_as_update() {
        let mut reg = Registry::default();
        reg.record_install(record("ripgrep", "cargo")); // v1.0
        let mut newer = record("ripgrep", "cargo");
        newer.version = Some(PkgVersion::new("2.0"));
        reg.record_update(newer);
        // Still one record, now at the new version.
        assert_eq!(reg.installed().len(), 1);
        assert_eq!(reg.get("ripgrep").unwrap().version.as_ref().unwrap().0, "2.0");
        // History logged the update with the new version.
        let last = reg.history().last().unwrap();
        assert_eq!(last.action, Action::Update);
        assert_eq!(last.version.as_ref().unwrap().0, "2.0");
    }

    #[test]
    fn roundtrips_through_json() {
        let mut reg = Registry::default();
        reg.record_install(record("fastfetch", "dnf"));
        let text = serde_json::to_string(&reg).unwrap();
        let back: Registry = serde_json::from_str(&text).unwrap();
        assert_eq!(back.installed().len(), 1);
        assert_eq!(back.get("fastfetch").unwrap().name, "fastfetch");
    }
}
