// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
//!
//! # Anti-tamper (ADR-0074)
//!
//! The ledger is a plain local file the user owns, so it can never be *truly* tamper-proof — any
//! key baked into the binary is extractable. What we can do is make casual hand-editing detectable
//! and unrewarding. Every save writes an HMAC-SHA256 signature over the ledger's content, keyed by
//! a constant in the binary and bound to this machine's `/etc/machine-id`. On load a bad signature
//! (a hand-edited JSON, or a ledger copied from another machine) is treated as tampering: the
//! ledger is wiped and JII reacts once, in-character. This is deterrence, not security — see the
//! ADR. Ledgers written before signing shipped (`sig` absent, no v2 fields) are grandfathered in
//! once and re-signed; a v2-shaped file with its `sig` stripped is treated as tampering.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{JiiError, Result};

/// A static catalog entry. The persisted store holds only ids, timestamps and a few counters;
/// all the presentational metadata (icon, secrecy) and text (via locale keys) live here / in the
/// locale files.
pub struct Achievement {
    /// Stable id — the persistence key *and* the locale-key stem. Never rename an existing id.
    pub id: &'static str,
    /// A decorative single-glyph icon.
    pub icon: &'static str,
    /// Secret achievements show as `???` (hidden title + description) until unlocked.
    pub secret: bool,
    /// When set, this entry stays **out of the list entirely** until the named achievement is
    /// unlocked. Used for the per-ending boss badges: you shouldn't see "spare Jevil" as a goal
    /// before you know Jevil exists — but once you've beaten him, both endings become named
    /// targets instead of another anonymous `???`.
    pub revealed_by: Option<&'static str>,
}

/// The full set JII knows about. The order here is the display order in `jii achievements`:
/// the everyday ones you stumble into first, then the ones you have to hunt for, the two
/// extreme grinds, and finally the secret.
pub const CATALOG: &[Achievement] = &[
    // Everyday — you bump into these just by using JII.
    Achievement {
        id: "first-install",
        icon: "🌱",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "doctor",
        icon: "🩺",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "explorer",
        icon: "🔍",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "cleaner",
        icon: "🧹",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "fresh",
        icon: "🔄",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "wizard",
        icon: "🧙",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "paper-trail",
        icon: "🔮",
        secret: false,
        revealed_by: None,
    },
    // Have to hunt for these.
    Achievement {
        id: "dry-runner",
        icon: "🧾",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "auditor",
        icon: "🛡️",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "sniper",
        icon: "🎯",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "haul",
        icon: "📦",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "translator",
        icon: "🌍",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "self-made",
        icon: "🧬",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "bootstrapper",
        icon: "🔧",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "night-owl",
        icon: "🌙",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "early-bird",
        icon: "🦉",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "polyglot",
        icon: "🗺️",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "centurion",
        icon: "🏆",
        secret: false,
        revealed_by: None,
    },
    // Extreme grinds.
    Achievement {
        id: "millennium",
        icon: "💯",
        secret: false,
        revealed_by: None,
    },
    Achievement {
        id: "completionist",
        icon: "👑",
        secret: false,
        revealed_by: None,
    },
    // Secret — the boss fights. Each fight's endings hang off its badge and only appear
    // once you've won it at least once.
    Achievement {
        id: "sans",
        icon: "💀",
        secret: true,
        revealed_by: None,
    },
    Achievement {
        id: "jevil",
        icon: "🃏",
        secret: true,
        revealed_by: None,
    },
    Achievement {
        id: "jevil-spare",
        icon: "😴",
        secret: true,
        revealed_by: Some("jevil"),
    },
    Achievement {
        id: "jevil-kill",
        icon: "⚔️",
        secret: true,
        revealed_by: Some("jevil"),
    },
    Achievement {
        id: "jevil-both",
        icon: "♠️",
        secret: true,
        revealed_by: Some("jevil"),
    },
    Achievement {
        id: "spamton",
        icon: "🎭",
        secret: true,
        revealed_by: None,
    },
    Achievement {
        id: "spamton-spare",
        icon: "🧵",
        secret: true,
        revealed_by: Some("spamton"),
    },
    Achievement {
        id: "spamton-kill",
        icon: "💥",
        secret: true,
        revealed_by: Some("spamton"),
    },
    Achievement {
        id: "spamton-both",
        icon: "📞",
        secret: true,
        revealed_by: Some("spamton"),
    },
    Achievement {
        id: "flowey",
        icon: "🌻",
        secret: true,
        revealed_by: None,
    },
    Achievement {
        id: "flowey-normal",
        icon: "🌼",
        secret: true,
        revealed_by: Some("flowey"),
    },
    Achievement {
        id: "flowey-hard",
        icon: "🥀",
        secret: true,
        revealed_by: Some("flowey"),
    },
    Achievement {
        id: "flowey-both",
        icon: "🌺",
        secret: true,
        revealed_by: Some("flowey"),
    },
    Achievement {
        id: "boss-slayer",
        icon: "👺",
        secret: true,
        revealed_by: None,
    },
];

/// A boss fight, as the ledger sees it: the badge it grants, the paths it can end on, and the
/// sentinel file its installer drops beside the ledger for JII to find on its next run.
///
/// Adding a fight is adding a row here (plus its badges above and its locale keys) — nothing
/// else in JII needs to learn the new boss's name.
pub struct Boss {
    /// The badge unlocked by winning at all — also the stem of every `<id>-<ending>` badge.
    pub id: &'static str,
    /// The paths the fight can end on. Empty for a single-path fight (Sans), which therefore
    /// has no per-ending badges and no `<id>-both`.
    pub endings: &'static [&'static str],
    /// The sentinel this fight's installer writes the ending into. `None` for Sans, whose
    /// older installer drops a contentless marker handled by [`Achievements::take_sentinel`].
    pub sentinel: Option<&'static str>,
}

/// The two ways a fight you can *choose* to end: spare the boss, or don't.
pub const MERCY_ENDINGS: &[&str] = &["spare", "kill"];
/// Omega Flowey offers no mercy — only a difficulty. Beating him on both counts as both ways.
pub const FLOWEY_ENDINGS: &[&str] = &["normal", "hard"];

/// Sentinel file names a boss-fight installer drops beside the ledger. Both Jevil fights (the
/// Chaos Simulator and the handheld-style VGB one) share `chaos-install` because they share the
/// 🃏 achievement; Spamton NEO and Omega Flowey have their own.
pub const JEVIL_SENTINEL: &str = "chaos-install";
pub const SPAMTON_SENTINEL: &str = "spamton-install";
pub const FLOWEY_SENTINEL: &str = "flowey-install";

/// Every boss, in fight order. `boss-slayer` is earned by unlocking all of them.
pub const BOSSES: &[Boss] = &[
    Boss {
        id: "sans",
        endings: &[],
        sentinel: None,
    },
    Boss {
        id: "jevil",
        endings: MERCY_ENDINGS,
        sentinel: Some(JEVIL_SENTINEL),
    },
    Boss {
        id: "spamton",
        endings: MERCY_ENDINGS,
        sentinel: Some(SPAMTON_SENTINEL),
    },
    Boss {
        id: "flowey",
        endings: FLOWEY_ENDINGS,
        sentinel: Some(FLOWEY_SENTINEL),
    },
];

/// Look up a boss by its badge id.
pub fn boss(id: &str) -> Option<&'static Boss> {
    BOSSES.iter().find(|b| b.id == id)
}

/// How many packages must land in a single command to earn `haul`.
pub const HAUL_AT: usize = 5;

/// Install-count milestones (the `installs` counter).
pub const CENTURION_AT: u64 = 100;
pub const MILLENNIUM_AT: u64 = 500;
/// How many *distinct* sources you must have installed from to earn `polyglot`.
pub const POLYGLOT_SOURCES: usize = 5;

/// Look up a catalog entry by id.
pub fn find(id: &str) -> Option<&'static Achievement> {
    CATALOG.iter().find(|a| a.id == id)
}

/// The entries `jii achievements` should show at all, in catalog order. An entry gated behind
/// `revealed_by` is omitted until that achievement is unlocked — it isn't even a `???` row, so
/// the list never hints at a fight you haven't found.
pub fn visible(store: &Achievements) -> impl Iterator<Item = &'static Achievement> + use<'_> {
    CATALOG
        .iter()
        .filter(|a| a.revealed_by.is_none_or(|parent| store.is_unlocked(parent)))
}

/// The signed content of the ledger — everything an HMAC must cover. Split out so we can
/// serialize *exactly this* (in stable `BTreeMap`/`BTreeSet` order) to sign and verify it,
/// with the signature living outside in [`StoredOut`].
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
struct Ledger {
    /// id → unlock timestamp.
    unlocked: BTreeMap<String, DateTime<Utc>>,
    /// Named running totals (e.g. `installs` → 137).
    counters: BTreeMap<String, u64>,
    /// Distinct source ids ever installed from (drives `polyglot`).
    sources: BTreeSet<String>,
}

/// On-disk shape when reading: every field optional so we can tell a pre-signing legacy file
/// (only `unlocked`) from a v2 file whose `sig` was stripped (has `counters`/`sources`).
#[derive(Debug, Default, Deserialize)]
struct RawStored {
    #[serde(default)]
    unlocked: Option<BTreeMap<String, DateTime<Utc>>>,
    #[serde(default)]
    counters: Option<BTreeMap<String, u64>>,
    #[serde(default)]
    sources: Option<BTreeSet<String>>,
    #[serde(default)]
    sig: Option<String>,
}

/// On-disk shape when writing: the signed content, flattened, plus its signature.
#[derive(Debug, Serialize)]
struct StoredOut<'a> {
    #[serde(flatten)]
    ledger: &'a Ledger,
    sig: String,
}

/// The in-memory ledger: unlocked achievements, counters, seen sources, and whether the file
/// we loaded had been tampered with.
#[derive(Debug, Default)]
pub struct Achievements {
    ledger: Ledger,
    /// Where this ledger is persisted. Not serialized.
    path: Option<PathBuf>,
    /// Set at load time when the on-disk signature didn't verify (hand-edited or copied from
    /// another machine). When true, `ledger` has been wiped; the caller reacts once and saves.
    tampered: bool,
}

/// The HMAC key baked into the binary. This is obfuscation, not a secret — a determined user can
/// extract it. It only has to defeat a text editor (ADR-0074).
const SIGN_KEY: &[u8] = b"jii/ach/v2:d0e0-4000-4ba8-99ae-fa57e1e57e14/keep-your-sins-off-the-ledger";

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

    /// The sentinel a *boss-fight* installer drops beside the ledger, named by `file` (see
    /// [`JEVIL_SENTINEL`] / [`SPAMTON_SENTINEL`]). Its contents record how the fight ended —
    /// `spare` or `kill` — so the achievement can show which path you took.
    pub fn boss_sentinel_path(file: &str) -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "jii")?;
        let base = dirs.state_dir().unwrap_or_else(|| dirs.data_dir());
        Some(base.join(file))
    }

    /// This machine's stable id, so a valid ledger can't simply be copied to another machine.
    /// Best-effort: if nothing is readable we fall back to a constant, which still defeats a
    /// plain hand-edit (the signature just won't be machine-specific).
    fn machine_id() -> Vec<u8> {
        for p in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
            if let Ok(s) = std::fs::read_to_string(p) {
                let t = s.trim();
                if !t.is_empty() {
                    return t.as_bytes().to_vec();
                }
            }
        }
        b"jii-no-machine-id".to_vec()
    }

    /// HMAC-SHA256, hand-rolled on the `sha2` we already depend on (no new crate for ~15 lines).
    fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        const BLOCK: usize = 64;

        let mut k = [0u8; BLOCK];

        if key.len() > BLOCK {
            k[..32].copy_from_slice(&Sha256::digest(key));
        } else {
            k[..key.len()].copy_from_slice(key);
        }
        let mut ipad = [0x36u8; BLOCK];
        let mut opad = [0x5cu8; BLOCK];
        for i in 0..BLOCK {
            ipad[i] ^= k[i];
            opad[i] ^= k[i];
        }
        let mut inner = Sha256::new();
        inner.update(ipad);
        inner.update(msg);
        let ih = inner.finalize();
        let mut outer = Sha256::new();
        outer.update(opad);
        outer.update(ih);
        outer.finalize().into()
    }

    /// The signature string for a ledger on this machine: HMAC over `machine-id \0 canonical-json`,
    /// rendered as lowercase hex. Canonical because `BTreeMap`/`BTreeSet` serialize in a fixed order.
    fn sign(ledger: &Ledger) -> String {
        let body = serde_json::to_vec(ledger).unwrap_or_default();
        let mut msg = Self::machine_id();
        msg.push(0);
        msg.extend_from_slice(&body);
        let mac = Self::hmac_sha256(SIGN_KEY, &msg);
        let mut hex = String::with_capacity(64);
        for b in mac {
            hex.push_str(&format!("{b:02x}"));
        }
        hex
    }

    /// Load from `path`, or start empty (remembering the path) if it does not exist.
    ///
    /// Verifies the signature. A file whose `sig` doesn't check out — or a v2-shaped file with its
    /// `sig` stripped — is treated as tampering: the returned ledger is wiped and [`tampered`](
    /// Self::tampered) is `true`. A pre-signing legacy file (only `unlocked`, no `sig`) is
    /// grandfathered in and will be re-signed on the next save.
    pub fn load_from(path: &Path) -> Result<Achievements> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Achievements {
                    path: Some(path.to_path_buf()),
                    ..Default::default()
                });
            }
            Err(e) => return Err(JiiError::io(path, e)),
        };
        let raw: RawStored =
            serde_json::from_str(&text).map_err(|e| JiiError::Config(format!("{}: {e}", path.display())))?;

        let has_v2_fields = raw.counters.is_some() || raw.sources.is_some();
        let ledger = Ledger {
            unlocked: raw.unlocked.unwrap_or_default(),
            counters: raw.counters.unwrap_or_default(),
            sources: raw.sources.unwrap_or_default(),
        };

        let tampered = match raw.sig {
            // Signed: trust it only if the signature verifies for this machine.
            Some(sig) => sig != Self::sign(&ledger),
            // Unsigned but v2-shaped → the signature was stripped. Tamper.
            None if has_v2_fields => true,
            // Unsigned, only `unlocked` → genuine pre-signing legacy. Grandfather it in.
            None => false,
        };

        if tampered {
            Ok(Achievements {
                ledger: Ledger::default(),
                path: Some(path.to_path_buf()),
                tampered: true,
            })
        } else {
            Ok(Achievements {
                ledger,
                path: Some(path.to_path_buf()),
                tampered: false,
            })
        }
    }

    /// Load from the default path (empty, unpersisted, if none is resolvable).
    pub fn load() -> Result<Achievements> {
        match Self::default_path() {
            Some(p) => Self::load_from(&p),
            None => Ok(Achievements::default()),
        }
    }

    /// Persist to disk, signing the content. A no-op if there is no path (e.g. no HOME).
    pub fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| JiiError::io(parent, e))?;
        }
        let out = StoredOut {
            ledger: &self.ledger,
            sig: Self::sign(&self.ledger),
        };
        let text = serde_json::to_string_pretty(&out)
            .map_err(|e| JiiError::Other(anyhow::anyhow!("failed to serialize achievements: {e}")))?;
        std::fs::write(path, text).map_err(|e| JiiError::io(path, e))
    }

    /// Was the loaded ledger tampered with? When true its content has been wiped; the caller
    /// should react once and [`save`](Self::save) the clean, freshly-signed ledger.
    pub fn tampered(&self) -> bool {
        self.tampered
    }

    /// Is `id` unlocked?
    pub fn is_unlocked(&self, id: &str) -> bool {
        self.ledger.unlocked.contains_key(id)
    }

    /// When `id` was unlocked, if it is.
    pub fn unlocked_at(&self, id: &str) -> Option<DateTime<Utc>> {
        self.ledger.unlocked.get(id).copied()
    }

    /// Unlock `id` if it isn't already. Returns `true` only when this call *newly* unlocked it,
    /// so the caller can show a one-time toast. An unknown id is ignored (returns `false`),
    /// keeping the store honest — it can only ever hold ids from the catalog.
    pub fn unlock(&mut self, id: &str) -> bool {
        if find(id).is_none() || self.ledger.unlocked.contains_key(id) {
            return false;
        }
        self.ledger.unlocked.insert(id.to_string(), Utc::now());
        true
    }

    /// The current value of a named counter (0 if never bumped).
    pub fn counter(&self, key: &str) -> u64 {
        self.ledger.counters.get(key).copied().unwrap_or(0)
    }

    /// Add `by` to a named counter and return its new value (saturating).
    pub fn bump(&mut self, key: &str, by: u64) -> u64 {
        let e = self.ledger.counters.entry(key.to_string()).or_insert(0);
        *e = e.saturating_add(by);
        *e
    }

    /// Record that we installed from `source_id`. Returns `true` if it was newly seen.
    pub fn add_source(&mut self, source_id: &str) -> bool {
        self.ledger.sources.insert(source_id.to_string())
    }

    /// How many distinct sources we've ever installed from.
    pub fn source_count(&self) -> usize {
        self.ledger.sources.len()
    }

    /// If this boss's sentinel exists, delete it and return how the fight ended — one of the
    /// boss's own [`Boss::endings`]. A missing, empty or unrecognized body is normalized to the
    /// first ending, so a half-written marker still grants the fight rather than nothing.
    /// Returns `None` when there's no sentinel (or the boss doesn't use one).
    pub fn take_boss_sentinel(boss: &Boss) -> Option<String> {
        let path = Self::boss_sentinel_path(boss.sentinel?)?;
        let body = std::fs::read_to_string(&path).ok()?;
        let _ = std::fs::remove_file(&path);
        let written = body.trim().to_ascii_lowercase();
        let variant = boss
            .endings
            .iter()
            .find(|ending| **ending == written)
            .or_else(|| boss.endings.first())
            .copied()?;
        Some(variant.to_string())
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

    fn temp_ledger() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("achievements.json");
        (dir, path)
    }

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
    fn secret_achievements_are_marked_secret() {
        for id in ["sans", "jevil", "spamton", "flowey"] {
            let a = find(id).unwrap_or_else(|| panic!("{id} missing from catalog"));
            assert!(a.secret, "{id} must be secret");
        }
    }

    #[test]
    fn every_boss_has_a_badge_per_ending_plus_both() {
        for boss in BOSSES {
            assert!(find(boss.id).is_some(), "{} missing from catalog", boss.id);
            // Sans has a single path — only the multi-ending fights carry ending badges.
            if boss.endings.is_empty() {
                assert!(
                    find(&format!("{}-both", boss.id)).is_none(),
                    "{} has no endings, so it must not have a -both badge",
                    boss.id
                );
                continue;
            }
            for ending in boss.endings {
                let id = format!("{}-{ending}", boss.id);
                let a = find(&id).unwrap_or_else(|| panic!("{id} missing from catalog"));
                assert!(a.secret, "{id} must be secret");
                assert_eq!(a.revealed_by, Some(boss.id), "{id} must hang off its boss");
            }
            let both = format!("{}-both", boss.id);
            assert!(find(&both).is_some(), "{both} missing from catalog");
        }
    }

    #[test]
    fn ending_badges_stay_hidden_until_the_boss_is_beaten() {
        let mut store = Achievements::default();
        let hidden_at_first = visible(&store).any(|a| a.id == "jevil-spare");
        assert!(
            !hidden_at_first,
            "an ending badge must not show before the fight is found"
        );

        store.unlock("jevil");
        let shown_now = visible(&store).any(|a| a.id == "jevil-spare");
        assert!(shown_now, "beating the boss reveals its endings as named goals");
        // A different boss's endings stay hidden.
        assert!(!visible(&store).any(|a| a.id == "spamton-kill"));
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
    fn counters_and_sources_accumulate() {
        let mut a = Achievements::default();
        assert_eq!(a.bump("installs", 3), 3);
        assert_eq!(a.bump("installs", 2), 5, "counter accumulates across bumps");
        assert!(a.add_source("dnf"));
        assert!(!a.add_source("dnf"), "same source not counted twice");
        assert!(a.add_source("flatpak"));
        assert_eq!(a.source_count(), 2);
    }

    #[test]
    fn signed_ledger_round_trips_and_loads_clean() {
        let (_d, path) = temp_ledger();
        let mut a = Achievements::load_from(&path).unwrap();
        a.unlock("doctor");
        a.bump("installs", 7);
        a.add_source("cargo");
        a.save().unwrap();

        let mut back = Achievements::load_from(&path).unwrap();
        assert!(!back.tampered(), "our own save must verify");
        assert!(back.is_unlocked("doctor"));
        assert_eq!(back.bump("installs", 0), 7, "counter survives the round trip");
        assert_eq!(back.source_count(), 1);
    }

    #[test]
    fn hand_edited_signed_ledger_is_flagged_and_wiped() {
        let (_d, path) = temp_ledger();
        let mut a = Achievements::load_from(&path).unwrap();
        a.unlock("doctor");
        a.save().unwrap();

        // Forge `sans` into the JSON, leaving the (now-stale) signature in place.
        let text = std::fs::read_to_string(&path).unwrap();
        let doctored = text.replacen("\"doctor\":", "\"sans\": \"2020-01-01T00:00:00Z\",\n    \"doctor\":", 1);
        std::fs::write(&path, doctored).unwrap();

        let back = Achievements::load_from(&path).unwrap();
        assert!(back.tampered(), "a bad signature must be caught");
        assert!(!back.is_unlocked("sans"), "forged unlock is wiped");
        assert!(!back.is_unlocked("doctor"), "the whole ledger is reset on tamper");
    }

    #[test]
    fn stripped_signature_on_v2_file_is_tamper() {
        // A v2-shaped file (has counters/sources) with no `sig` = the signature was removed.
        let (_d, path) = temp_ledger();
        std::fs::write(
            &path,
            r#"{"unlocked":{"sans":"2020-01-01T00:00:00Z"},"counters":{},"sources":[]}"#,
        )
        .unwrap();
        let back = Achievements::load_from(&path).unwrap();
        assert!(back.tampered());
        assert!(!back.is_unlocked("sans"));
    }

    #[test]
    fn legacy_unsigned_ledger_is_grandfathered() {
        // A pre-signing file has only `unlocked` and no `sig`: keep it, don't punish it.
        let (_d, path) = temp_ledger();
        std::fs::write(&path, r#"{"unlocked":{"sans":"2020-01-01T00:00:00Z"}}"#).unwrap();
        let back = Achievements::load_from(&path).unwrap();
        assert!(!back.tampered(), "genuine legacy file is trusted");
        assert!(back.is_unlocked("sans"), "hard-won legacy unlock is kept");
    }
}
