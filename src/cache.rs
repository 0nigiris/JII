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

/// The on-disk shape: cached results plus a per-source recent-failure table (the
/// circuit-breaker state), so a slow/broken source (e.g. a timing-out COPR) is skipped
/// on the *next* search instead of making every search wait out its timeout again.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Persisted {
    #[serde(default)]
    entries: HashMap<String, Entry>,
    #[serde(default)]
    failures: HashMap<String, DateTime<Utc>>,
}

/// A TTL search cache persisted as a single JSON file, with a source-level failure
/// cooldown (circuit breaker).
pub struct Cache {
    ttl: Duration,
    /// How long a source stays "circuit-broken" after a failure before it's retried.
    failure_cooldown: Duration,
    path: Option<PathBuf>,
    entries: Mutex<HashMap<String, Entry>>,
    /// Source id → time of its last search failure (timeout/error).
    failures: Mutex<HashMap<String, DateTime<Utc>>>,
    dirty: Mutex<bool>,
}

impl Cache {
    /// Default path: `$XDG_CACHE_HOME/jii/search-cache.json`.
    fn default_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "jii")
            .map(|d| d.cache_dir().join("search-cache.json"))
    }

    /// The on-disk cache file path (backs `jii cache`).
    pub fn path() -> Option<PathBuf> {
        Self::default_path()
    }

    /// Delete the on-disk search cache (backs `jii cache clear`). Returns the removed path,
    /// or `None` if there was nothing to remove. In-memory state is untouched — this is a
    /// one-shot CLI action that exits right after.
    pub fn clear_disk() -> std::io::Result<Option<PathBuf>> {
        match Self::default_path() {
            Some(p) if p.exists() => {
                std::fs::remove_file(&p)?;
                Ok(Some(p))
            }
            _ => Ok(None),
        }
    }

    /// Load the cache from disk (empty if missing/corrupt), with the given TTL and
    /// per-source failure cooldown.
    pub fn load(ttl_secs: u64, failure_cooldown_secs: u64) -> Self {
        let path = Self::default_path();
        let persisted: Persisted = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Cache {
            ttl: Duration::from_secs(ttl_secs),
            failure_cooldown: Duration::from_secs(failure_cooldown_secs),
            path,
            entries: Mutex::new(persisted.entries),
            failures: Mutex::new(persisted.failures),
            dirty: Mutex::new(false),
        }
    }

    /// Whether `source` failed (timed out / errored) recently enough to still be inside
    /// its cooldown — i.e. the circuit is open, so skip it without waiting. After the
    /// cooldown lapses the source is retried (a self-healing, "half-open" retry).
    pub fn recently_failed(&self, source: &str) -> bool {
        let failures = self.failures.lock().unwrap();
        failures.get(source).is_some_and(|at| {
            Utc::now()
                .signed_duration_since(*at)
                .to_std()
                .map(|age| age < self.failure_cooldown)
                .unwrap_or(false)
        })
    }

    /// Record that `source` just failed a search (opens its circuit for the cooldown).
    pub fn mark_failure(&self, source: &str) {
        self.failures.lock().unwrap().insert(source.to_string(), Utc::now());
        *self.dirty.lock().unwrap() = true;
    }

    /// Clear a source's failure mark after it succeeds (closes the circuit).
    pub fn clear_failure(&self, source: &str) {
        if self.failures.lock().unwrap().remove(source).is_some() {
            *self.dirty.lock().unwrap() = true;
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
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }
        let persisted = Persisted {
            entries: self.entries.lock().unwrap().clone(),
            failures: self.failures.lock().unwrap().clone(),
        };
        if let Ok(text) = serde_json::to_string(&persisted) {
            let _ = std::fs::write(path, text);
        }
    }
}

/// Cache key for a `(source, query)` pair. The unit separator avoids collisions.
fn key(source: &str, query: &str) -> String {
    format!("{source}\u{1f}{query}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A disk-less cache for tests (no path → never reads/writes the real cache file).
    fn empty(ttl_secs: u64, cooldown_secs: u64) -> Cache {
        Cache {
            ttl: Duration::from_secs(ttl_secs),
            failure_cooldown: Duration::from_secs(cooldown_secs),
            path: None,
            entries: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
            dirty: Mutex::new(false),
        }
    }

    #[test]
    fn failure_opens_and_success_closes_the_circuit() {
        let cache = empty(3600, 3600);
        assert!(!cache.recently_failed("copr")); // never failed yet
        cache.mark_failure("copr");
        assert!(cache.recently_failed("copr")); // now circuit-broken
        cache.clear_failure("copr");
        assert!(!cache.recently_failed("copr")); // success reopened it
    }

    #[test]
    fn a_zero_cooldown_never_circuit_breaks() {
        // With no cooldown window, a past failure is already expired — the source is retried.
        let cache = empty(3600, 0);
        cache.mark_failure("copr");
        assert!(!cache.recently_failed("copr"));
    }

    #[test]
    fn failures_are_per_source() {
        let cache = empty(3600, 3600);
        cache.mark_failure("copr");
        assert!(cache.recently_failed("copr"));
        assert!(!cache.recently_failed("dnf")); // unrelated source unaffected
    }
}
