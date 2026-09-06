// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Snap provider (snapd — the Snap Store).
//!
//! Unlike the registry-user-space providers (cargo/npm/pipx/go/brew), snap installs
//! **system-wide**, so its plans need root (`sudo snap install …`) — like dnf/copr.
//! Search resolves an exact snap via the Snap Store info API (which requires a
//! `Snap-Device-Series` header, so it uses `http_client` directly rather than the
//! shared `get_json_opt`, like github/copr).
//!
//! Trust is `community`: the Snap Store is a third-party app store (like Flathub), and
//! snaps are store-signed. Some snaps use **classic** confinement (full system access);
//! those must be installed with `--classic`, which JII adds and surfaces in the plan.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{
    Bootstrap, Ecosystem, Provider, command_plan, http_client, nonempty_lines, run_capture, which,
};
use crate::error::{JiiError, Result};
use crate::model::{
    Action, InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
};

const ID: &str = "snap";
const BIN: &str = "snap";
const API: &str = "https://api.snapcraft.io/v2/snaps/info";

/// The Snap (snapd) installation source.
pub struct Snap;

impl Snap {
    pub fn new() -> Self {
        Snap
    }
}

impl Default for Snap {
    fn default() -> Self {
        Snap::new()
    }
}

#[async_trait]
impl Provider for Snap {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // The Snap Store is a third-party app store (like Flathub); snaps are store-signed.
        TrustLevel::Community
    }

    /// A sandboxed bundle with its own runtime.
    fn nature(&self) -> super::SourceNature {
        super::SourceNature::Sandboxed
    }

    async fn is_available(&self) -> bool {
        which(BIN).await
    }

    /// The Snap Store info API is a network call — no snapd needed to find a match, so a host
    /// without snap can still be offered "install snap, then this" (ADR-0061 part B).
    fn can_search(&self) -> bool {
        true
    }

    fn web_url(&self, candidate: &PackageCandidate) -> Option<String> {
        Some(format!("https://snapcraft.io/{}", candidate.name))
    }

    fn ecosystem(&self) -> Option<Ecosystem> {
        Some(Ecosystem {
            label: "Snap",
            bootstrap: Bootstrap::Packages(&["snapd"]),
        })
    }

    /// Installing the `snapd` package is not the same as having Snap: outside Ubuntu the
    /// socket ships disabled, and `/snap` — which classic snaps hard-code — doesn't exist.
    /// Both are documented, one-time, root steps, so JII plans them (shown, then escalated
    /// like any privileged step) instead of leaving the user with a snapd that refuses every
    /// install. `enable --now` is idempotent; the symlink is only planned when absent, and
    /// never on the distro that manages `/snap` itself (Ubuntu ships it as a real directory).
    async fn plan_post_bootstrap(&self) -> Result<Option<InstallPlan>> {
        let mut actions = vec![Action::RunCommand {
            argv: ["systemctl", "enable", "--now", "snapd.socket"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            needs_root: true,
        }];
        let mut reasons = vec![crate::t!("reason.snap_socket")];
        if !std::path::Path::new("/snap").exists() {
            actions.push(Action::RunCommand {
                argv: ["ln", "-s", "/var/lib/snapd/snap", "/snap"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                needs_root: true,
            });
            reasons.push(crate::t!("reason.snap_symlink"));
        }
        Ok(Some(InstallPlan {
            candidate_ref: "snapd".to_string(),
            source_id: ID.to_string(),
            actions,
            download_size: None,
            reasons,
        }))
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // `confinement` (and the rest) are omitted unless explicitly requested via
        // `fields`, and `fields` restricts the response to exactly what is listed — so we
        // must enumerate every field we read (verified against the live API).
        let url = format!(
            "{API}/{}?fields=version,confinement,summary,title",
            query.raw.trim()
        );
        // The Snap Store info API requires the device-series header (else 400).
        let resp = http_client()?
            .get(&url)
            .header("Snap-Device-Series", "16")
            .send()
            .await
            .map_err(|e| JiiError::Other(anyhow::anyhow!("snap: {e}")))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new()); // no such snap
        }
        let resp = resp
            .error_for_status()
            .map_err(|e| JiiError::Other(anyhow::anyhow!("snap: {e}")))?;
        let info: SnapInfo = resp
            .json()
            .await
            .map_err(|e| JiiError::Other(anyhow::anyhow!("snap: malformed json: {e}")))?;
        Ok(vec![candidate(&info)])
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let classic = is_classic(candidate);
        let mut argv = vec![BIN.to_string(), "install".to_string(), candidate.name.clone()];
        if classic {
            argv.push("--classic".to_string());
        }
        let mut reasons = vec![crate::t!("reason.snap_store")];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }
        if classic {
            reasons.push(crate::t!("reason.snap_classic"));
        }
        Ok(command_plan(ID, &candidate.name, argv, true, reasons))
    }

    async fn plan_install_many(
        &self,
        candidates: &[&PackageCandidate],
    ) -> Result<Option<InstallPlan>> {
        // `--classic` can't be applied selectively in one command, so only merge when no
        // candidate needs it; otherwise decline (`None`) so each gets its own correct plan.
        if candidates.iter().any(|c| is_classic(c)) {
            return Ok(None);
        }
        let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
        let reasons = vec![crate::t!("reason.snap_store_many", names = names.join(", "))];
        Ok(Some(snap_many("install", &names, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.remove_one", name = record.name.clone(), mgr = "snap")];
        Ok(snap_plan(&record.name, "remove", reasons))
    }

    async fn plan_remove_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<String> = records.iter().map(|r| r.name.clone()).collect();
        let reasons = vec![crate::t!("reason.remove_many", mgr = "snap", names = names.join(", "))];
        Ok(Some(snap_many("remove", &names, reasons)))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let reasons = vec![crate::t!("reason.update_one", name = record.name.clone(), mgr = "snap")];
        Ok(snap_plan(&record.name, "refresh", reasons))
    }

    async fn plan_update_many(&self, records: &[&InstalledRecord]) -> Result<Option<InstallPlan>> {
        let names: Vec<String> = records.iter().map(|r| r.name.clone()).collect();
        let reasons = vec![crate::t!("reason.update_many", mgr = "snap", names = names.join(", "))];
        Ok(Some(snap_many("refresh", &names, reasons)))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        // `snap refresh` with no names refreshes every installed snap (D10).
        let reasons = vec![crate::t!("reason.snap_refresh_all")];
        let argv = vec![BIN.to_string(), "refresh".to_string()];
        Ok(Some(command_plan(ID, "all snaps", argv, true, reasons)))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        let out = run_capture(&[BIN, "list"]).await?;
        Ok(parse_list(&out, ID))
    }
}

/// The Snap Store `v2/snaps/info/<name>` response (only the fields we use).
#[derive(Debug, Deserialize)]
struct SnapInfo {
    name: String,
    #[serde(default)]
    snap: SnapMeta,
    #[serde(rename = "channel-map", default)]
    channel_map: Vec<ChannelEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct SnapMeta {
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelEntry {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    confinement: Option<String>,
    #[serde(default)]
    channel: Channel,
}

#[derive(Debug, Default, Deserialize)]
struct Channel {
    #[serde(default)]
    risk: Option<String>,
    #[serde(default)]
    track: Option<String>,
}

/// Build a candidate from a snap lookup, taking the version + confinement from the
/// default channel (`latest/stable`, falling back to the first entry). No program filter
/// (the store has no reliable "is an app" signal beyond publishing — ADR-0023 spirit).
fn candidate(info: &SnapInfo) -> PackageCandidate {
    let default_channel = info
        .channel_map
        .iter()
        .find(|e| {
            e.channel.risk.as_deref() == Some("stable")
                && e.channel.track.as_deref() == Some("latest")
        })
        .or_else(|| info.channel_map.first());
    let version = default_channel
        .and_then(|e| e.version.clone())
        .filter(|s| !s.is_empty());
    let classic =
        default_channel.is_some_and(|e| e.confinement.as_deref() == Some("classic"));
    let summary = info
        .snap
        .summary
        .clone()
        .or_else(|| info.snap.title.clone())
        .filter(|s| !s.is_empty());
    PackageCandidate {
        name: info.name.clone(),
        source_id: ID.to_string(),
        version: version.map(PkgVersion::new),
        trust: TrustLevel::Community,
        // snapd fetches the right arch and snaps are store-signed.
        arch_ok: true,
        signed: true,
        summary,
        popularity: None,
        suspicious: false,
        raw: json!({ "classic": classic }),
    }
}

/// Whether a candidate is a classic-confinement snap (needs `--classic` to install).
fn is_classic(candidate: &PackageCandidate) -> bool {
    candidate
        .raw
        .get("classic")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// A single root `snap <verb> <name>` plan (install/remove/refresh).
fn snap_plan(name: &str, verb: &str, reasons: Vec<String>) -> InstallPlan {
    let argv = vec![BIN.to_string(), verb.to_string(), name.to_string()];
    command_plan(ID, name, argv, true, reasons)
}

/// One root `snap <verb> a b c` for a whole group.
fn snap_many(verb: &str, names: &[String], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![BIN.to_string(), verb.to_string()];
    argv.extend(names.iter().cloned());
    command_plan(ID, &names.join(", "), argv, true, reasons)
}

/// Parse `snap list` into installed records. The first line is a header
/// (`Name Version Rev …`); each data line is `name version …`.
fn parse_list(stdout: &str, source_id: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    nonempty_lines(stdout)
        .filter(|line| line.split_whitespace().next() != Some("Name"))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            if name.is_empty() {
                return None;
            }
            let version = fields.next();
            Some(InstalledRecord {
                name: name.to_string(),
                source_id: source_id.to_string(),
                version: version.map(PkgVersion::new),
                installed_at: now,
                // snaps are store-signed — a self-verifying manager.
                verification: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = r#"{
        "name": "hello",
        "snap": {"title": "GNU Hello", "summary": "GNU Hello, the hello world snap"},
        "channel-map": [
            {"version": "2.10", "confinement": "strict",
             "channel": {"risk": "stable", "track": "latest"}}
        ]
    }"#;

    const CODE_CLASSIC: &str = r#"{
        "name": "code",
        "snap": {"summary": "Code editing. Redefined."},
        "channel-map": [
            {"version": "1.96.0", "confinement": "classic",
             "channel": {"risk": "stable", "track": "latest"}}
        ]
    }"#;

    fn parse(sample: &str) -> SnapInfo {
        serde_json::from_str(sample).unwrap()
    }

    #[test]
    fn snap_becomes_a_community_candidate() {
        let c = candidate(&parse(HELLO));
        assert_eq!(c.name, "hello");
        assert_eq!(c.source_id, "snap");
        assert_eq!(c.version.as_ref().unwrap().0, "2.10");
        assert_eq!(c.trust, TrustLevel::Community);
        assert_eq!(c.summary.as_deref(), Some("GNU Hello, the hello world snap"));
        assert!(!is_classic(&c));
    }

    #[test]
    fn classic_confinement_is_recorded() {
        let c = candidate(&parse(CODE_CLASSIC));
        assert!(is_classic(&c));
    }

    #[tokio::test]
    async fn install_plan_is_one_root_snap_command() {
        let c = candidate(&parse(HELLO));
        let plan = Snap::new().plan_install(&c).await.unwrap();
        assert_eq!(plan.source_id, "snap");
        assert!(plan.needs_root());
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, needs_root } => {
                assert_eq!(argv, &["snap", "install", "hello"]);
                assert!(needs_root);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn classic_snap_install_adds_classic_flag() {
        let c = candidate(&parse(CODE_CLASSIC));
        let plan = Snap::new().plan_install(&c).await.unwrap();
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["snap", "install", "code", "--classic"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn batch_install_merges_non_classic_but_declines_when_any_is_classic() {
        let strict = candidate(&parse(HELLO));
        let mut strict2 = strict.clone();
        strict2.name = "htop".into();
        let plan = Snap::new()
            .plan_install_many(&[&strict, &strict2])
            .await
            .unwrap()
            .expect("all-strict batches");
        match &plan.actions[0] {
            crate::model::Action::RunCommand { argv, .. } => {
                assert_eq!(argv, &["snap", "install", "hello", "htop"]);
            }
            other => panic!("expected run, got {other:?}"),
        }

        // A classic snap in the group declines the merge (fall back to per-snap plans).
        let classic = candidate(&parse(CODE_CLASSIC));
        assert!(
            Snap::new()
                .plan_install_many(&[&strict, &classic])
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn parses_snap_list_skipping_header() {
        let sample = "\
Name    Version   Rev   Tracking       Publisher   Notes
core22  20240111  1122  latest/stable  canonical   base
hello   2.10      42    latest/stable  canonical   -
";
        let recs = parse_list(sample, "snap");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "core22");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "20240111");
        assert_eq!(recs[1].name, "hello");
        assert_eq!(recs[1].version.as_ref().unwrap().0, "2.10");
    }
}
