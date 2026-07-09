//! Self-management: JII updating and removing **itself**.
//!
//! JII already knows how to install software the right way; it should treat itself the
//! same. `jii update jii` (and a bare `jii update`) check GitHub for the newest release
//! and, if it differs, build a previewable [`InstallPlan`] to upgrade this very binary;
//! `jii uninstall` / `jii remove jii` build one to remove it. The mechanism follows *how
//! jii was installed* (the same "cooperate, don't clobber" principle as the providers):
//!
//! - **user-space binary** (install.sh / tarball / `cargo install`) → download the matching
//!   static-musl tarball, verify its sha256, and **atomically swap** it in place — no root
//!   ([`Action::Replace`], which renames over the live binary without `ETXTBSY`);
//! - **package install** (`.rpm`/`.deb`, binary under `/usr/bin`) → download the matching
//!   package and install it through `dnf`/`apt` as a previewable **root** step, so the
//!   package database stays consistent (JII never clobbers a packaged file behind rpm's back).
//!
//! Everything is an `InstallPlan`, so `--dry-run` previews it and the trust/verification
//! path is the usual one. Version comparison is deliberately a plain "different tag →
//! offer" (versions are opaque, ADR-0009) — no fragile semver ordering.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{JiiError, Result};
use crate::model::{Action, InstallPlan, Verification};
use crate::platform::Platform;
use crate::provider::http_client;

/// The GitHub repository JII publishes releases from.
const REPO: &str = "0nigiris/JII";
/// The package name that means "JII itself" in `update`/`remove`.
pub const SELF_NAME: &str = "jii";

/// Which system package manager owns a packaged jii (decides the update/remove command).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Dnf,
    Apt,
}

/// How this running `jii` was installed — decides how it updates/removes itself.
#[derive(Debug, PartialEq, Eq)]
pub enum Install {
    /// A plain binary in a user-writable location (install.sh / tarball / `cargo install`).
    UserBinary(PathBuf),
    /// Managed by a system package manager.
    Package { manager: Manager, exe: PathBuf },
}

/// The version this binary was built as (`jii --version`).
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Strip a leading `v` from a release tag for comparison with [`current_version`].
pub fn normalize_tag(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// Whether `latest_tag` denotes a different release than the running binary. Deliberately a
/// plain string difference (versions are opaque, ADR-0009): different → offer to update.
pub fn update_available(latest_tag: &str) -> bool {
    normalize_tag(latest_tag) != current_version()
}

/// Detect how this jii was installed. A binary inside `$HOME` is always user-space; anything
/// else is checked against `rpm`/`dpkg` ownership, falling back to user-space.
pub async fn detect_install() -> Result<Install> {
    let exe = std::env::current_exe().map_err(|e| JiiError::io("current executable", e))?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);

    if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
        && exe.starts_with(&home)
    {
        return Ok(Install::UserBinary(exe));
    }
    if let Some(manager) = package_owner(&exe).await {
        return Ok(Install::Package { manager, exe });
    }
    Ok(Install::UserBinary(exe))
}

/// Which package manager (if any) owns `exe`. `rpm -qf`/`dpkg -S` exit 0 only when the file
/// belongs to a package.
async fn package_owner(exe: &Path) -> Option<Manager> {
    let path = exe.to_str()?;
    if run_ok("rpm", &["-qf", path]).await {
        return Some(Manager::Dnf);
    }
    if run_ok("dpkg", &["-S", path]).await {
        return Some(Manager::Apt);
    }
    None
}

/// Run a command and report whether it exited 0 (a missing binary counts as "no").
async fn run_ok(bin: &str, args: &[&str]) -> bool {
    tokio::process::Command::new(bin)
        .args(args)
        .output()
        .await
        .is_ok_and(|o| o.status.success())
}

/// One release asset (name + download URL).
#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseJson {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// The newest published release (tag + its assets).
#[derive(Debug)]
pub struct Latest {
    pub tag: String,
    pub assets: Vec<Asset>,
}

impl Latest {
    /// The static-musl tarball for `arch` (`x86_64`/`aarch64`).
    fn tarball(&self, tag: &str, arch: &str) -> Option<&Asset> {
        let want = format!("jii-{tag}-{arch}-linux.tar.gz");
        self.assets.iter().find(|a| a.name == want)
    }

    /// The `.rpm` for `arch` (rpm names the arch `x86_64`/`aarch64`).
    fn rpm(&self, arch: &str) -> Option<&Asset> {
        self.assets
            .iter()
            .find(|a| a.name.ends_with(".rpm") && a.name.contains(arch))
    }

    /// The `.deb` for `arch` (deb names the arch `amd64`/`arm64`).
    fn deb(&self, arch: &str) -> Option<&Asset> {
        let deb_arch = deb_arch(arch);
        self.assets
            .iter()
            .find(|a| a.name.ends_with(".deb") && a.name.contains(deb_arch))
    }

    /// The companion `<asset>.sha256`, if published.
    fn checksum_of(&self, asset: &Asset) -> Option<&Asset> {
        let want = format!("{}.sha256", asset.name);
        self.assets.iter().find(|a| a.name == want)
    }
}

/// deb's arch spelling for a uname arch.
fn deb_arch(arch: &str) -> &str {
    match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// Fetch the newest release metadata from GitHub.
pub async fn latest_release() -> Result<Latest> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = http_client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("checking for updates: {e}")))?
        .error_for_status()
        .map_err(|e| JiiError::Other(anyhow::anyhow!("checking for updates: {e}")))?;
    let rel: ReleaseJson = resp
        .json()
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("release metadata: {e}")))?;
    Ok(Latest {
        tag: rel.tag_name,
        assets: rel.assets,
    })
}

/// Resolve the verification for `asset` — its published sha256 if present, else none.
async fn verify_for(latest: &Latest, asset: &Asset) -> Verification {
    let Some(sum) = latest.checksum_of(asset) else {
        return Verification::None;
    };
    match fetch_sha256(&sum.browser_download_url).await {
        Some(hex) => Verification::Sha256(hex),
        None => Verification::None,
    }
}

/// Download a `.sha256` file and pull the 64-hex digest out of it.
async fn fetch_sha256(url: &str) -> Option<String> {
    let text = http_client().ok()?.get(url).send().await.ok()?.text().await.ok()?;
    text.split_whitespace()
        .find(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_string)
}

/// Build the plan to update this jii to `latest`, per how it was installed. Selects the
/// right asset, resolves its checksum (network), then assembles the plan with a pure builder.
pub async fn plan_update(install: &Install, latest: &Latest) -> Result<InstallPlan> {
    let arch = Platform::detect().arch;
    match install {
        Install::UserBinary(exe) => {
            let asset = latest.tarball(&latest.tag, arch).ok_or_else(|| {
                JiiError::Other(anyhow::anyhow!(
                    "no {arch} tarball in release {} to update from",
                    latest.tag
                ))
            })?;
            let verify = verify_for(latest, asset).await;
            Ok(user_update_plan(exe, asset, &latest.tag, verify))
        }
        Install::Package { manager, .. } => {
            let asset = match manager {
                Manager::Dnf => latest.rpm(arch),
                Manager::Apt => latest.deb(arch),
            }
            .ok_or_else(|| {
                JiiError::Other(anyhow::anyhow!(
                    "no {arch} package in release {} to update from",
                    latest.tag
                ))
            })?;
            let verify = verify_for(latest, asset).await;
            Ok(package_update_plan(*manager, asset, &latest.tag, verify))
        }
    }
}

/// Pure assembly of the user-space update plan: download the tarball, extract `jii` to a
/// staging file beside the current binary, then atomically swap it in. No root, no network.
fn user_update_plan(exe: &Path, asset: &Asset, tag: &str, verify: Verification) -> InstallPlan {
    let archive = std::env::temp_dir().join(format!("jii-update-{tag}.tar.gz"));
    let exe_dir = exe.parent().unwrap_or_else(|| Path::new("."));
    let staged = exe_dir.join(".jii.update");
    let actions = vec![
        Action::Download {
            url: asset.browser_download_url.clone(),
            dest: archive.clone(),
            verify,
        },
        Action::Extract {
            archive,
            member: "jii".to_string(),
            dest: staged.clone(),
            mode: 0o755,
        },
        Action::Replace {
            src: staged,
            dest: exe.to_path_buf(),
        },
    ];
    self_plan(
        actions,
        vec![
            format!("Update JII {} → {}", current_version(), normalize_tag(tag)),
            format!("Replace {} in place (no root)", exe.display()),
        ],
    )
}

/// Pure assembly of the package update plan: download the `.rpm`/`.deb`, install via the
/// owning manager as a root step. No network.
fn package_update_plan(manager: Manager, asset: &Asset, tag: &str, verify: Verification) -> InstallPlan {
    let pkg = std::env::temp_dir().join(&asset.name);
    let pkg_str = pkg.to_string_lossy().to_string();
    let argv = match manager {
        Manager::Dnf => vec!["dnf".into(), "install".into(), "-y".into(), pkg_str],
        Manager::Apt => vec!["apt-get".into(), "install".into(), "-y".into(), pkg_str],
    };
    let actions = vec![
        Action::Download {
            url: asset.browser_download_url.clone(),
            dest: pkg,
            verify,
        },
        Action::RunCommand {
            argv,
            needs_root: true,
        },
    ];
    self_plan(
        actions,
        vec![format!(
            "Update JII {} → {} via the system package manager",
            current_version(),
            normalize_tag(tag)
        )],
    )
}

/// Build the plan to remove this jii, per how it was installed.
pub fn plan_uninstall(install: &Install) -> InstallPlan {
    match install {
        Install::UserBinary(exe) => self_plan(
            vec![Action::RemoveFile { path: exe.clone() }],
            vec![format!("Remove JII ({})", exe.display())],
        ),
        Install::Package { manager, .. } => {
            let argv = match manager {
                Manager::Dnf => vec!["dnf".into(), "remove".into(), "-y".into(), "jii".into()],
                Manager::Apt => vec![
                    "apt-get".into(),
                    "remove".into(),
                    "-y".into(),
                    "jii".into(),
                ],
            };
            self_plan(
                vec![Action::RunCommand {
                    argv,
                    needs_root: true,
                }],
                vec!["Remove JII via the system package manager".to_string()],
            )
        }
    }
}

/// An `InstallPlan` for a self-management action (source id `self`).
fn self_plan(actions: Vec<Action>, reasons: Vec<String>) -> InstallPlan {
    InstallPlan {
        candidate_ref: SELF_NAME.to_string(),
        source_id: "self".to_string(),
        actions,
        download_size: None,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_string(),
            browser_download_url: format!("https://example/{name}"),
        }
    }

    fn latest() -> Latest {
        Latest {
            tag: "v0.1.1-beta".to_string(),
            assets: [
                "jii-v0.1.1-beta-x86_64-linux.tar.gz",
                "jii-v0.1.1-beta-x86_64-linux.tar.gz.sha256",
                "jii-v0.1.1-beta-aarch64-linux.tar.gz",
                "jii-0.1.1.beta-1.x86_64.rpm",
                "jii-0.1.1.beta-1.x86_64.rpm.sha256",
                "jii-0.1.1.beta-1.aarch64.rpm",
                "jii_0.1.1.beta_amd64.deb",
                "jii_0.1.1.beta_arm64.deb",
            ]
            .iter()
            .map(|n| asset(n))
            .collect(),
        }
    }

    #[test]
    fn normalize_and_update_available() {
        assert_eq!(normalize_tag("v1.2.3"), "1.2.3");
        assert_eq!(normalize_tag("1.2.3"), "1.2.3");
        // A tag equal to the current version (after stripping v) is not an update.
        let same = format!("v{}", current_version());
        assert!(!update_available(&same));
        assert!(update_available("v99.0.0"));
    }

    #[test]
    fn selects_tarball_rpm_and_deb_per_arch() {
        let l = latest();
        assert_eq!(
            l.tarball(&l.tag, "x86_64").unwrap().name,
            "jii-v0.1.1-beta-x86_64-linux.tar.gz"
        );
        assert_eq!(
            l.tarball(&l.tag, "aarch64").unwrap().name,
            "jii-v0.1.1-beta-aarch64-linux.tar.gz"
        );
        assert_eq!(l.rpm("x86_64").unwrap().name, "jii-0.1.1.beta-1.x86_64.rpm");
        assert_eq!(l.rpm("aarch64").unwrap().name, "jii-0.1.1.beta-1.aarch64.rpm");
        // deb uses amd64/arm64, not x86_64/aarch64.
        assert_eq!(l.deb("x86_64").unwrap().name, "jii_0.1.1.beta_amd64.deb");
        assert_eq!(l.deb("aarch64").unwrap().name, "jii_0.1.1.beta_arm64.deb");
    }

    #[test]
    fn checksum_companion_is_matched_not_the_binary() {
        let l = latest();
        let tar = l.tarball(&l.tag, "x86_64").unwrap();
        assert_eq!(
            l.checksum_of(tar).unwrap().name,
            "jii-v0.1.1-beta-x86_64-linux.tar.gz.sha256"
        );
        // The rpm's sha256 exists; the deb's does not in this fixture → None.
        assert!(l.checksum_of(l.deb("x86_64").unwrap()).is_none());
    }

    #[test]
    fn user_binary_plan_downloads_extracts_and_replaces() {
        let exe = PathBuf::from("/home/u/.local/bin/jii");
        let l = latest();
        let asset = l.tarball(&l.tag, "x86_64").unwrap();
        let plan = user_update_plan(&exe, asset, &l.tag, Verification::None);
        assert_eq!(plan.source_id, "self");
        assert!(!plan.needs_root());
        // Download → Extract → Replace(dest = the running exe).
        assert!(matches!(plan.actions[0], Action::Download { .. }));
        assert!(matches!(plan.actions[1], Action::Extract { .. }));
        match &plan.actions[2] {
            Action::Replace { dest, .. } => assert_eq!(dest, &exe),
            other => panic!("expected Replace, got {other:?}"),
        }
    }

    #[test]
    fn package_update_uses_root_manager_install() {
        let l = latest();
        let asset = l.rpm("x86_64").unwrap();
        let plan = package_update_plan(Manager::Dnf, asset, &l.tag, Verification::None);
        assert!(plan.needs_root());
        assert!(matches!(plan.actions[0], Action::Download { .. }));
        match &plan.actions[1] {
            Action::RunCommand { argv, needs_root } => {
                assert!(*needs_root);
                assert_eq!(argv[0], "dnf");
                assert_eq!(argv[1], "install");
            }
            other => panic!("expected RunCommand, got {other:?}"),
        }
    }

    #[test]
    fn package_uninstall_uses_root_manager_command() {
        let plan = plan_uninstall(&Install::Package {
            manager: Manager::Dnf,
            exe: PathBuf::from("/usr/bin/jii"),
        });
        assert!(plan.needs_root());
        match &plan.actions[0] {
            Action::RunCommand { argv, needs_root } => {
                assert!(*needs_root);
                assert_eq!(argv, &["dnf", "remove", "-y", "jii"]);
            }
            other => panic!("expected RunCommand, got {other:?}"),
        }
    }
}
