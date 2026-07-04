//! On-disk search cache with TTL and stale-on-error fallback.
//!
//! Search results are cached per `(source, query)`. A fresh entry (younger than the
//! configured TTL) is served without hitting the provider; when a provider fails or
//! times out, a stale entry is used if present so JII degrades gracefully offline.
//!
//! Access is guarded by a `Mutex`; critical sections never await, so the standard
//! mutex is sufficient for the concurrent (single-task) search fan-out.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::PackageCandidate;

/// One cached search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    fetched_at: DateTime<Utc>,
    candidates: Vec<PackageCandidate>,
}

/// A TTL search cache persisted as a single JSON file.
pub struct Cache {
    ttl: Duration,
    path: Option<PathBuf>,
    entries: Mutex<HashMap<String, Entry>>,
    dirty: Mutex<bool>,
}

impl Cache {
    /// Default path: `$XDG_CACHE_HOME/jii/search-cache.json`.
    fn default_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "jii")
            .map(|d| d.cache_dir().join("search-cache.json"))
    }

    /// Load the cache from disk (empty if missing/corrupt), with the given TTL.
    pub fn load(ttl_secs: u64) -> Self {
        let path = Self::default_path();
        let entries = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Cache {
            ttl: Duration::from_secs(ttl_secs),
            path,
            entries: Mutex::new(entries),
            dirty: Mutex::new(false),
        }
    }

    /// A fresh (within TTL) cached result, if any.
    pub fn get_fresh(&self, source: &str, query: &str) -> Option<Vec<PackageCandidate>> {
        let entries = self.entries.lock().unwrap();
        let entry = entries.get(&key(source, query))?;
        let age = Utc::now().signed_duration_since(entry.fetched_at);
        match age.to_std() {
            Ok(age) if age < self.ttl => Some(entry.candidates.clone()),
            _ => None,
        }
    }

    /// Any cached result regardless of age (used as an offline fallback).
    pub fn get_stale(&self, source: &str, query: &str) -> Option<Vec<PackageCandidate>> {
        let entries = self.entries.lock().unwrap();
        entries.get(&key(source, query)).map(|e| e.candidates.clone())
    }

    /// Store a fresh result.
    pub fn put(&self, source: &str, query: &str, candidates: Vec<PackageCandidate>) {
        self.entries.lock().unwrap().insert(
            key(source, query),
            Entry {
                fetched_at: Utc::now(),
                candidates,
            },
        );
        *self.dirty.lock().unwrap() = true;
    }

    /// Persist to disk if anything changed. Best-effort: cache write errors are
    /// non-fatal and swallowed.
    pub fn save(&self) {
        if !*self.dirty.lock().unwrap() {
            return;
        }
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let entries = self.entries.lock().unwrap();
        if let Ok(text) = serde_json::to_string(&*entries) {
            let _ = std::fs::write(path, text);
        }
    }
}

/// Cache key for a `(source, query)` pair. The unit separator avoids collisions.
fn key(source: &str, query: &str) -> String {
    format!("{source}\u{1f}{query}")
}
