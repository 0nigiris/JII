// SPDX-FileCopyrightText: 2026 0nigiris
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AUR provider (the Arch User Repository, via an AUR helper — `yay` or `paru`).
//!
//! The AUR is **Arch-family only**: it is meaningless on Fedora/Debian/openSUSE, so every
//! entry point self-gates on [`Platform::arch_like`](crate::platform::Platform::arch_like)
//! *and* the presence of an AUR helper. Search is a network call to the AUR's own RPC
//! (`aur.archlinux.org/rpc/v5`), but JII deliberately does **not** set `can_search`: without
//! a helper there is nothing to install it with, and surfacing AUR hits on a non-Arch host
//! would be wrong. So the source only participates where it actually applies.
//!
//! Installs run through the helper (`yay -S <pkg>` / `paru -S <pkg>`) with `needs_root =
//! false`: an AUR helper must **not** be run as root — it drops privileges to build and
//! escalates to `pacman` itself, exactly like Flatpak's own polkit path. Trust is
//! `community` (PKGBUILDs are user-submitted and built from source).

use async_trait::async_trait;
use serde::Deserialize;

use super::{Bootstrap, Ecosystem, Provider, command_plan, get_json_opt, run_capture_lax, which};
use crate::error::Result;
use crate::model::{InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel};
use crate::platform::Platform;

const ID: &str = "aur";
const RPC: &str = "https://aur.archlinux.org/rpc/v5";

/// The AUR installation source.
pub struct Aur;

impl Aur {
    pub fn new() -> Self {
        Aur
    }
}

impl Default for Aur {
    fn default() -> Self {
        Aur::new()
    }
}

/// The AUR helper to drive, in preference order — `paru` and `yay` are the two mainstream
/// helpers; whichever is present wins. `None` when neither is installed (or off Arch).
pub(crate) async fn aur_helper() -> Option<&'static str> {
    if !Platform::detect().arch_like {
        return None;
    }
    for bin in ["paru", "yay"] {
        if which(bin).await {
            return Some(bin);
        }
    }
    None
}

#[async_trait]
impl Provider for Aur {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // PKGBUILDs are user-submitted and built from source — not distro-official.
        TrustLevel::Community
    }

    async fn is_available(&self) -> bool {
        aur_helper().await.is_some()
    }

    /// The AUR *is* an ecosystem you bootstrap by installing a helper (yay/paru) — but only on
    /// Arch. Off Arch this returns `None`, so AUR never shows up in `jii sources`/`jii providers`
    /// on Fedora/Debian/etc. (reads only the cached `Platform`, no I/O). Bootstrapping is
    /// `Script`: a helper is built from a PKGBUILD via `makepkg`, which JII shows but never runs
    /// (the same trust boundary as brew/nix). `jii yay`/`jii paru` pick the helper explicitly.
    fn ecosystem(&self) -> Option<Ecosystem> {
        Platform::detect().arch_like.then_some(Ecosystem {
            label: "AUR helper (yay/paru)",
            // makepkg installs the helper as a normal pacman package — already on PATH.
            bootstrap: Bootstrap::Script {
                cmd: "git clone https://aur.archlinux.org/yay-bin.git && cd yay-bin && makepkg -si",
                shell: None,
            },
        })
    }

    fn web_url(&self, candidate: &PackageCandidate) -> Option<String> {
        Some(format!("https://aur.archlinux.org/packages/{}", candidate.name))
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        // Gate hard on Arch: never surface AUR results on a host that can't use them.
        if !Platform::detect().arch_like {
            return Ok(Vec::new());
        }
        let term = query.raw.trim();
        let url = format!("{RPC}/search/{term}?by=name");
        let Some(result) = get_json_opt::<RpcSearch>(ID, &url).await? else {
            return Ok(Vec::new());
        };
        Ok(result.results.into_iter().map(candidate).collect())
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let helper = self.helper_or_err().await?;
        let mut reasons = vec![crate::t!("reason.aur", helper = helper.to_string())];
        if let Some(v) = &candidate.version {
            reasons.push(crate::t!("reason.version", v = v.clone()));
        }
        Ok(helper_plan(
            helper,
            &["-S"],
            std::slice::from_ref(&candidate.name),
            reasons,
        ))
    }

    async fn plan_install_many(&self, candidates: &[&PackageCandidate]) -> Result<Option<InstallPlan>> {
        let helper = self.helper_or_err().await?;
        let names: Vec<String> = candidates.iter().map(|c| c.name.clone()).collect();
        let reasons = vec![crate::t!("reason.aur", helper = helper.to_string())];
        Ok(Some(helper_plan(helper, &["-S"], &names, reasons)))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let helper = self.helper_or_err().await?;
        let reasons = vec![crate::t!("reason.remove_one", name = record.name.clone(), mgr = helper)];
        Ok(helper_plan(
            helper,
            &["-Rs"],
            std::slice::from_ref(&record.name),
            reasons,
        ))
    }

    async fn plan_update(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let helper = self.helper_or_err().await?;
        let reasons = vec![crate::t!(
            "reason.update_one_reinstall",
            name = record.name.clone(),
            mgr = helper
        )];
        Ok(helper_plan(
            helper,
            &["-S"],
            std::slice::from_ref(&record.name),
            reasons,
        ))
    }

    async fn plan_update_all(&self) -> Result<Option<InstallPlan>> {
        let helper = self.helper_or_err().await?;
        // `-Sua` upgrades only the AUR packages (leaving the repo `-Syu` to the system update).
        let reasons = vec![crate::t!("reason.aur_upgrade_all", helper = helper.to_string())];
        Ok(Some(command_plan(
            ID,
            "all AUR packages",
            vec![helper.to_string(), "-Sua".to_string()],
            false,
            reasons,
        )))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        if aur_helper().await.is_none() {
            return Ok(Vec::new());
        }
        // `pacman -Qm` lists *foreign* packages — those not in any sync repo, i.e. AUR/manual.
        let out = run_capture_lax(&["pacman", "-Qm"]).await?;
        Ok(parse_qm(&out))
    }
}

impl Aur {
    /// The active helper, or a clear error if none is installed (so a plan built without a
    /// helper fails loudly instead of emitting a command that can't run).
    async fn helper_or_err(&self) -> Result<&'static str> {
        aur_helper()
            .await
            .ok_or_else(|| crate::error::JiiError::Other(anyhow::anyhow!(crate::t!("aur.no_helper"))))
    }
}

/// One unprivileged AUR-helper command (`yay -S a b`, `paru -Rs x`). The helper handles its
/// own `pacman` escalation, so `needs_root = false` — JII must never run a helper as root.
fn helper_plan(helper: &str, flags: &[&str], names: &[String], reasons: Vec<String>) -> InstallPlan {
    let mut argv = vec![helper.to_string()];
    argv.extend(flags.iter().map(|s| s.to_string()));
    argv.extend(names.iter().cloned());
    command_plan(ID, &names.join(", "), argv, false, reasons)
}

/// Build a candidate from an AUR RPC search result.
fn candidate(pkg: RpcPackage) -> PackageCandidate {
    PackageCandidate {
        name: pkg.name,
        source_id: ID.to_string(),
        version: pkg.version.filter(|v| !v.is_empty()).map(PkgVersion::new),
        trust: TrustLevel::Community,
        arch_ok: true,
        // AUR packages are built from source PKGBUILDs — no upstream signature JII verifies.
        signed: false,
        summary: pkg.description.filter(|d| !d.is_empty()),
        popularity: None,
        suspicious: false,
        raw: serde_json::json!({}),
    }
}

/// Parse `pacman -Qm` output: `name version` per line.
fn parse_qm(stdout: &str) -> Vec<InstalledRecord> {
    let now = chrono::Utc::now();
    super::nonempty_lines(stdout)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?.to_string();
            let version = parts.next().map(PkgVersion::new);
            Some(InstalledRecord {
                name,
                source_id: ID.to_string(),
                version,
                installed_at: now,
                verification: None,
            })
        })
        .collect()
}

/// The AUR RPC v5 `search` response (only the fields we use).
#[derive(Debug, Deserialize)]
struct RpcSearch {
    #[serde(default)]
    results: Vec<RpcPackage>,
}

/// One AUR package in an RPC response.
#[derive(Debug, Deserialize)]
struct RpcPackage {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Version", default)]
    version: Option<String>,
    #[serde(rename = "Description", default)]
    description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_qm_lines() {
        let out = "yay 12.4.2-1\nspotify 1.2.31-1\n\n";
        let recs = parse_qm(out);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "yay");
        assert_eq!(recs[0].version.as_ref().unwrap().0, "12.4.2-1");
        assert_eq!(recs[1].name, "spotify");
    }

    #[test]
    fn candidate_carries_community_trust_and_unsigned() {
        let c = candidate(RpcPackage {
            name: "spotify".into(),
            version: Some("1.2.31-1".into()),
            description: Some("Music streaming".into()),
        });
        assert_eq!(c.source_id, "aur");
        assert_eq!(c.trust, TrustLevel::Community);
        assert!(!c.signed);
        assert_eq!(c.summary.as_deref(), Some("Music streaming"));
    }
}
