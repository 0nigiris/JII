//! Achievements — a small, playful ledger of milestones the user hits while using JII.
//!
//! Purely cosmetic: nothing here changes what JII installs, ranks, or recommends. Unlocking
//! is idempotent and cheap, and every failure is swallowed (an achievement must never break a
//! real command). The store lives next to the registry
//! (`$XDG_STATE_HOME/jii/achievements.json`) so the whole of JII's state sits together.
//!
//! Titles and descriptions are localized, not stored here (ADR-0050): the text lives in
//! `locales/*.toml` under `achieve.<id>.title` / `achieve.<id>.desc`, keyed by the stable id.
//!
//! Secret achievements render as `???` with a hidden description in `jii achievements` until
//! they are earned. One of them — `sans` — is granted only by the secret install path: that
//! installer drops a sentinel file which JII notices on its next run (see [`Achievements::
//! take_sentinel`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{JiiError, Result};

/// A static catalog entry. The persisted store holds only ids + timestamps; all the
/// presentational metadata (icon, secrecy) and text (via locale keys) live here / in the
/// locale files.
pub struct Achievement {
    /// Stable id — the persistence key *and* the locale-key stem. Never rename an existing id.
    pub id: &'static str,
    /// A decorative single-glyph icon.
    pub icon: &'static str,
    /// Secret achievements show as `???` (hidden title + description) until unlocked.
    pub secret: bool,
}

/// The full set JII knows about. The order here is the display order in `jii achievements`.
pub const CATALOG: &[Achievement] = &[
    Achievement { id: "first-install", icon: "🌱", secret: false },
    Achievement { id: "doctor", icon: "🩺", secret: false },
    Achievement { id: "sans", icon: "💀", secret: true },
];

/// Look up a catalog entry by id.
pub fn find(id: &str) -> Option<&'static Achievement> {
    CATALOG.iter().find(|a| a.id == id)
}

/// The persisted ledger: which achievements are unlocked, and when.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Achievements {
    /// id → unlock timestamp. A `BTreeMap` keeps the on-disk JSON stable and ordered.
    unlocked: BTreeMap<String, DateTime<Utc>>,
    /// Where this ledger is persisted. Not serialized.
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl Achievements {
    /// Default path: `$XDG_STATE_HOME/jii/achievements.json` (falls back to the data dir),
    /// mirroring [`crate::registry::Registry::default_path`].
    pub fn default_path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "jii")?;
        let base = dirs.state_dir().unwrap_or_else(|| dirs.data_dir());
        Some(base.join("achievements.json"))
    }

    /// The sentinel the secret installer drops (`$XDG_STATE_HOME/jii/secret-install`) to grant
    /// `sans` on JII's next run. Kept beside the ledger so a single state dir holds everything.
    pub fn sentinel_path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "jii")?;
        let base = dirs.state_dir().unwrap_or_else(|| dirs.data_dir());
        Some(base.join("secret-install"))
    }

    /// Load from `path`, or start empty (remembering the path) if it does not exist.
    pub fn load_from(path: &Path) -> Result<Achievements> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut a: Achievements = serde_json::from_str(&text)
                    .map_err(|e| JiiError::Config(format!("{}: {e}", path.display())))?;
                a.path = Some(path.to_path_buf());
                Ok(a)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Achievements {
                path: Some(path.to_path_buf()),
                ..Default::default()
            }),
            Err(e) => Err(JiiError::io(path, e)),
        }
    }

    /// Load from the default path (empty, unpersisted, if none is resolvable).
    pub fn load() -> Result<Achievements> {
        match Self::default_path() {
            Some(p) => Self::load_from(&p),
            None => Ok(Achievements::default()),
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
            .map_err(|e| JiiError::Other(anyhow::anyhow!("failed to serialize achievements: {e}")))?;
        std::fs::write(path, text).map_err(|e| JiiError::io(path, e))
    }

    /// Is `id` unlocked?
    pub fn is_unlocked(&self, id: &str) -> bool {
        self.unlocked.contains_key(id)
    }

    /// When `id` was unlocked, if it is.
    pub fn unlocked_at(&self, id: &str) -> Option<DateTime<Utc>> {
        self.unlocked.get(id).copied()
    }

    /// Unlock `id` if it isn't already. Returns `true` only when this call *newly* unlocked it,
    /// so the caller can show a one-time toast. An unknown id is ignored (returns `false`),
    /// keeping the store honest — it can only ever hold ids from the catalog.
    pub fn unlock(&mut self, id: &str) -> bool {
        if find(id).is_none() || self.unlocked.contains_key(id) {
            return false;
        }
        self.unlocked.insert(id.to_string(), Utc::now());
        true
    }

    /// If the secret-install sentinel exists, delete it and return `true` (the caller then
    /// unlocks `sans`). Best-effort: a sentinel that can't be removed is still consumed once —
    /// we report its presence at most a single time by unlinking before returning.
    pub fn take_sentinel() -> bool {
        let Some(path) = Self::sentinel_path() else {
            return false;
        };
        if path.exists() {
            let _ = std::fs::remove_file(&path);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_is_idempotent_and_reports_only_first_time() {
        let mut a = Achievements::default();
        assert!(a.unlock("first-install"), "first unlock is new");
        assert!(!a.unlock("first-install"), "second unlock is not new");
        assert!(a.is_unlocked("first-install"));
    }

    #[test]
    fn unknown_ids_are_never_stored() {
        let mut a = Achievements::default();
        assert!(!a.unlock("not-a-real-achievement"));
        assert!(!a.is_unlocked("not-a-real-achievement"));
    }

    #[test]
    fn every_catalog_id_is_unique() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|a| a.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "catalog ids must be unique");
    }

    #[test]
    fn round_trips_through_json() {
        let mut a = Achievements::default();
        a.unlock("doctor");
        let text = serde_json::to_string(&a).unwrap();
        let back: Achievements = serde_json::from_str(&text).unwrap();
        assert!(back.is_unlocked("doctor"));
        assert!(!back.is_unlocked("sans"));
    }
}
