//! GitHub Releases provider.
//!
//! Resolves a query of the form `owner/repo` to that repo's latest release, picks
//! the asset matching the current arch/OS, and plans a user-space install into
//! `~/.local/bin` — no root. All network access happens in `search`, which embeds
//! everything the plan needs (download URL, filename, checksum) into the candidate's
//! `raw`, so `plan_install` stays pure and testable (mirrors how dnf/flatpak work).
//!
//! Scope of this slice: **raw executable assets only**. GitHub releases that ship
//! only compressed archives (`.tar.gz`, `.zip`, …) are skipped until an `Extract`
//! action exists (see docs/TASKS.md Phase 4). Trust is always `untrusted` (an
//! arbitrary third-party binary), so installs are always confirmed explicitly — a
//! verified sha256 raises confidence but not the trust tier.
//!
//! Broad name→repo resolution (e.g. `jii ripgrep` → `BurntSushi/ripgrep`) is future
//! work; today the query must name the repo explicitly as `owner/repo`.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::Provider;
use crate::error::{JiiError, Result};
use crate::model::{
    Action, InstallPlan, InstalledRecord, PackageCandidate, PkgVersion, Query, TrustLevel,
    Verification,
};
use std::path::{Path, PathBuf};

const ID: &str = "github";
const API: &str = "https://api.github.com";

/// The GitHub Releases installation source.
pub struct Github {
    /// Env var holding an API token (lifts the 60/h anonymous rate limit).
    token_env: String,
    /// Host architecture used to filter release assets (e.g. "x86_64").
    arch: &'static str,
}

impl Github {
    pub fn new(token_env: String, arch: &'static str) -> Self {
        Github { token_env, arch }
    }

    /// An API token from the configured env var, if set and non-empty.
    fn token(&self) -> Option<String> {
        std::env::var(&self.token_env).ok().filter(|s| !s.is_empty())
    }

    /// HTTP client with the User-Agent the GitHub API requires.
    fn client(&self) -> Result<reqwest::Client> {
        reqwest::Client::builder()
            .user_agent(concat!("jii/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| JiiError::Other(anyhow::anyhow!("http client: {e}")))
    }
}

#[async_trait]
impl Provider for Github {
    fn id(&self) -> &'static str {
        ID
    }

    fn trust(&self) -> TrustLevel {
        // A third-party binary from an arbitrary repo — always confirmed explicitly.
        TrustLevel::Untrusted
    }

    async fn is_available(&self) -> bool {
        // GitHub is a remote service with no local binary to probe. Reachability is
        // surfaced per-request as a search error/timeout, which the engine handles
        // gracefully; a real rate-limit/health probe for `doctor` is a later slice.
        true
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        let Some((owner, repo)) = parse_owner_repo(&query.raw) else {
            // Not an `owner/repo` query — this provider has nothing to offer (yet).
            return Ok(Vec::new());
        };

        let client = self.client()?;
        let token = self.token();
        let release = fetch_release(&client, &owner, &repo, token.as_deref()).await?;

        let Some(asset) = select_asset(&release.assets, self.arch) else {
            return Ok(Vec::new());
        };

        // Resolve a checksum now (network stays in `search`) so the plan can enforce it.
        let sha256 = match find_checksums_asset(&release.assets) {
            Some(sums) => fetch_text(&client, &sums.browser_download_url, token.as_deref())
                .await
                .ok()
                .and_then(|text| parse_checksums(&text, &asset.name)),
            None => None,
        };

        Ok(vec![candidate(&owner, &repo, &release.tag_name, asset, sha256)])
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let url = raw_str(candidate, "url")?;
        let filename = raw_str(candidate, "filename")?;
        let slug = raw_str(candidate, "slug").unwrap_or_else(|_| candidate.name.clone());
        let sha256 = candidate
            .raw
            .get("sha256")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let size = candidate.raw.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

        Ok(build_install_plan(
            &candidate.name,
            &slug,
            candidate.version.as_ref(),
            &url,
            &filename,
            sha256,
            size,
            &bin_dir()?,
            &cache_dir()?,
        ))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let dest = bin_dir()?.join(&record.name);
        let reasons = vec![format!("Remove {} from {}", record.name, dest.display())];
        Ok(InstallPlan {
            candidate_ref: record.name.clone(),
            source_id: ID.to_string(),
            actions: vec![Action::RemoveFile { path: dest }],
            download_size: None,
            reasons,
        })
    }

    async fn plan_update(&self, _record: &InstalledRecord) -> Result<InstallPlan> {
        // Updating a GitHub binary means re-resolving the latest release; wire this
        // through the update command in Phase 5.
        Err(JiiError::Other(anyhow::anyhow!(
            "updating github installs is not supported yet — reinstall with `jii <owner>/<repo>`"
        )))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // File-based source: it cannot enumerate its installs (no manifest of which
        // ~/.local/bin files came from github). The JSON registry is the record of
        // what we installed; `is_installed` verifies a specific record by file.
        Ok(Vec::new())
    }

    async fn is_installed(&self, record: &InstalledRecord) -> bool {
        // Verify the registry hint by checking the placed binary still exists.
        bin_dir().map(|d| is_placed(&d, &record.name)).unwrap_or(false)
    }
}

/// Whether the binary named `name` is present in `bin_dir` (a github install).
fn is_placed(bin_dir: &Path, name: &str) -> bool {
    bin_dir.join(name).exists()
}

/// The subset of the GitHub release API we consume.
#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// One downloadable release asset.
#[derive(Debug, Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Parse an `owner/repo` slug. Rejects whitespace, empty halves, and extra slashes.
fn parse_owner_repo(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains(char::is_whitespace) {
        return None;
    }
    let mut parts = raw.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// GET the latest (non-draft, non-prerelease) release of a repo.
async fn fetch_release(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Release> {
    let url = format!("{API}/repos/{owner}/{repo}/releases/latest");
    let mut req = client.get(&url).header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| JiiError::Other(anyhow::anyhow!("github: {e}")))?;
    resp.json::<Release>()
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("github: malformed release json: {e}")))
}

/// GET a small text asset (e.g. a checksums file).
async fn fetch_text(client: &reqwest::Client, url: &str, token: Option<&str>) -> Result<String> {
    let mut req = client.get(url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| JiiError::Other(anyhow::anyhow!("github: {e}")))?;
    resp.text()
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("github: {e}")))
}

/// Build a candidate from the selected asset. Name = repo (also the installed binary
/// name); everything the plan needs is stashed in `raw`.
fn candidate(
    owner: &str,
    repo: &str,
    tag: &str,
    asset: &Asset,
    sha256: Option<String>,
) -> PackageCandidate {
    PackageCandidate {
        name: repo.to_string(),
        source_id: ID.to_string(),
        version: (!tag.is_empty()).then(|| PkgVersion::new(tag)),
        trust: TrustLevel::Untrusted,
        arch_ok: true,
        signed: sha256.is_some(),
        summary: Some(format!("GitHub release {owner}/{repo}")),
        raw: json!({
            "slug": format!("{owner}/{repo}"),
            "url": asset.browser_download_url,
            "filename": asset.name,
            "size": asset.size,
            "sha256": sha256,
        }),
    }
}

/// Pick the best installable raw binary for `arch`, or `None` if the release ships
/// only archives / other-OS artifacts.
fn select_asset<'a>(assets: &'a [Asset], arch: &str) -> Option<&'a Asset> {
    let tokens = arch_tokens(arch);
    if tokens.is_empty() {
        return None;
    }
    assets
        .iter()
        .filter(|a| is_installable_binary(&a.name, tokens))
        .min_by_key(|a| asset_score(&a.name))
}

/// Asset-name tokens that indicate the given host architecture.
fn arch_tokens(arch: &str) -> &'static [&'static str] {
    match arch {
        "x86_64" => &["x86_64", "amd64", "x64"],
        "aarch64" => &["aarch64", "arm64"],
        "arm" => &["armv7", "armhf", "arm"],
        _ => &[],
    }
}

/// Whether an asset looks like a directly-runnable Linux binary for this arch.
/// Rejects other OSes, distro packages, archives, and metadata/signature files.
fn is_installable_binary(name: &str, arch_tokens: &[&str]) -> bool {
    const REJECT: &[&str] = &[
        // other OSes
        "windows", "win32", "win64", ".exe", ".msi", "darwin", "macos", "apple", ".dmg",
        "freebsd", "netbsd", "openbsd", "android",
        // distro packages
        ".deb", ".rpm", ".apk", ".pkg",
        // archives / compressed (no extractor yet)
        ".tar", ".tgz", ".zip", ".gz", ".bz2", ".xz", ".zst",
        // metadata / signatures / checksums
        ".sha256", ".sig", ".asc", ".pem", ".txt", ".json", ".sbom", ".yml", ".yaml",
    ];
    let n = name.to_ascii_lowercase();
    if REJECT.iter().any(|tok| n.contains(tok)) {
        return false;
    }
    // Require an explicit linux marker and a matching arch token, so we never place a
    // binary for the wrong OS/arch.
    n.contains("linux") && arch_tokens.iter().any(|t| n.contains(t))
}

/// Lower is better: prefer statically-linked musl, then gnu, then unknown; break ties
/// by shorter name (usually the plain binary over a variant).
fn asset_score(name: &str) -> (u8, usize) {
    let n = name.to_ascii_lowercase();
    let libc = if n.contains("musl") {
        0
    } else if n.contains("gnu") {
        1
    } else {
        2
    };
    (libc, name.len())
}

/// Find a checksums asset (a shared `checksums.txt`/`SHA256SUMS`, or a per-asset
/// `*.sha256`).
fn find_checksums_asset(assets: &[Asset]) -> Option<&Asset> {
    assets.iter().find(|a| {
        let n = a.name.to_ascii_lowercase();
        n.contains("checksum") || n.contains("sha256sum") || n.ends_with(".sha256")
    })
}

/// Extract the sha256 digest for `filename` from a checksums file. Supports both the
/// `<digest>␠␠<filename>` line format and a lone-digest `*.sha256` file.
fn parse_checksums(text: &str, filename: &str) -> Option<String> {
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if let (Some(digest), Some(name)) = (fields.next(), fields.next()) {
            // Some tools prefix the name with `*` (binary mode).
            if name.trim_start_matches('*') == filename && is_sha256(digest) {
                return Some(digest.to_ascii_lowercase());
            }
        }
    }
    // Lone-digest file (a per-asset `.sha256` with no filename column).
    let mut tokens = text.split_whitespace();
    match (tokens.next(), tokens.next()) {
        (Some(only), None) if is_sha256(only) => Some(only.to_ascii_lowercase()),
        _ => None,
    }
}

/// Whether `s` is a 64-char hex sha256 digest.
fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Assemble the install plan: download (verified) to the cache, then place as an
/// executable in `~/.local/bin`. Pure so it can be unit-tested without IO.
#[allow(clippy::too_many_arguments)]
fn build_install_plan(
    name: &str,
    slug: &str,
    version: Option<&PkgVersion>,
    url: &str,
    filename: &str,
    sha256: Option<String>,
    size: u64,
    bin_dir: &Path,
    cache_dir: &Path,
) -> InstallPlan {
    let staged = cache_dir.join(filename);
    let dest = bin_dir.join(name);
    let verify = match &sha256 {
        Some(digest) => Verification::Sha256(digest.clone()),
        None => Verification::None,
    };

    let mut reasons = vec![format!("GitHub release ({slug})")];
    if let Some(v) = version {
        reasons.push(format!("Version {v}"));
    }
    reasons.push(match &sha256 {
        Some(_) => "Verified sha256 checksum".to_string(),
        None => "⚠ no checksum published — unverified".to_string(),
    });
    reasons.push(format!("Installs to {} (no root)", dest.display()));

    InstallPlan {
        candidate_ref: name.to_string(),
        source_id: ID.to_string(),
        actions: vec![
            Action::Download {
                url: url.to_string(),
                dest: staged.clone(),
                verify,
            },
            Action::Place {
                src: staged,
                dest,
                mode: 0o755,
            },
        ],
        download_size: (size > 0).then_some(size),
        reasons,
    }
}

/// `~/.local/bin`, where user-space binaries go.
fn bin_dir() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".local").join("bin"))
        .ok_or_else(|| JiiError::Other(anyhow::anyhow!("cannot resolve home directory")))
}

/// `~/.cache/jii/downloads`, the staging area for downloaded artifacts.
fn cache_dir() -> Result<PathBuf> {
    directories::ProjectDirs::from("", "", "jii")
        .map(|d| d.cache_dir().join("downloads"))
        .ok_or_else(|| JiiError::Other(anyhow::anyhow!("cannot resolve cache directory")))
}

/// Read a required string field from a candidate's `raw` payload.
fn raw_str(candidate: &PackageCandidate, key: &str) -> Result<String> {
    candidate
        .raw
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| JiiError::Other(anyhow::anyhow!("github candidate missing '{key}'")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_owner_repo() {
        assert_eq!(
            parse_owner_repo("jqlang/jq"),
            Some(("jqlang".into(), "jq".into()))
        );
        assert_eq!(parse_owner_repo("  cli/cli  "), Some(("cli".into(), "cli".into())));
    }

    #[test]
    fn rejects_non_slug_queries() {
        assert!(parse_owner_repo("fastfetch").is_none());
        assert!(parse_owner_repo("a/b/c").is_none());
        assert!(parse_owner_repo("/jq").is_none());
        assert!(parse_owner_repo("jqlang/").is_none());
        assert!(parse_owner_repo("two words").is_none());
    }

    const SAMPLE_RELEASE: &str = r#"{
        "tag_name": "jq-1.7.1",
        "assets": [
            {"name": "jq-windows-amd64.exe", "browser_download_url": "https://x/win", "size": 1},
            {"name": "jq-macos-amd64", "browser_download_url": "https://x/mac", "size": 2},
            {"name": "jq-linux-amd64", "browser_download_url": "https://x/lin", "size": 3},
            {"name": "jq-linux-arm64", "browser_download_url": "https://x/arm", "size": 4},
            {"name": "jq-1.7.1.tar.gz", "browser_download_url": "https://x/src", "size": 5},
            {"name": "sha256sums.txt", "browser_download_url": "https://x/sums", "size": 6}
        ]
    }"#;

    fn assets() -> Vec<Asset> {
        serde_json::from_str::<Release>(SAMPLE_RELEASE).unwrap().assets
    }

    #[test]
    fn parses_release_json() {
        let release: Release = serde_json::from_str(SAMPLE_RELEASE).unwrap();
        assert_eq!(release.tag_name, "jq-1.7.1");
        assert_eq!(release.assets.len(), 6);
    }

    #[test]
    fn selects_linux_binary_for_arch_rejecting_other_os_and_archives() {
        let a = assets();
        let picked = select_asset(&a, "x86_64").unwrap();
        assert_eq!(picked.name, "jq-linux-amd64");

        let picked = select_asset(&a, "aarch64").unwrap();
        assert_eq!(picked.name, "jq-linux-arm64");

        // Unknown arch → nothing selectable.
        assert!(select_asset(&a, "riscv64").is_none());
    }

    #[test]
    fn prefers_musl_over_gnu() {
        let a = vec![
            Asset {
                name: "tool-linux-x86_64-gnu".into(),
                browser_download_url: "https://x/gnu".into(),
                size: 1,
            },
            Asset {
                name: "tool-linux-x86_64-musl".into(),
                browser_download_url: "https://x/musl".into(),
                size: 1,
            },
        ];
        assert_eq!(select_asset(&a, "x86_64").unwrap().name, "tool-linux-x86_64-musl");
    }

    #[test]
    fn no_asset_when_only_archives() {
        let a = vec![Asset {
            name: "ripgrep-14-x86_64-unknown-linux-musl.tar.gz".into(),
            browser_download_url: "https://x/tgz".into(),
            size: 1,
        }];
        assert!(select_asset(&a, "x86_64").is_none());
    }

    #[test]
    fn finds_and_parses_checksums() {
        let sums = "\
deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  jq-linux-arm64
2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  jq-linux-amd64
";
        let digest = parse_checksums(sums, "jq-linux-amd64").unwrap();
        assert_eq!(
            digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert!(parse_checksums(sums, "absent").is_none());
    }

    #[test]
    fn parses_lone_digest_sha_file() {
        let d = "ABC123abc123ABC123abc123ABC123abc123ABC123abc123ABC123abc123abcd";
        assert_eq!(parse_checksums(d, "whatever").unwrap(), d.to_ascii_lowercase());
    }

    #[test]
    fn checksums_asset_detected() {
        assert_eq!(find_checksums_asset(&assets()).unwrap().name, "sha256sums.txt");
    }

    #[test]
    fn build_plan_downloads_verified_then_places_executable() {
        let bin = Path::new("/home/u/.local/bin");
        let cache = Path::new("/home/u/.cache/jii/downloads");
        let plan = build_install_plan(
            "jq",
            "jqlang/jq",
            Some(&PkgVersion::new("jq-1.7.1")),
            "https://x/lin",
            "jq-linux-amd64",
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into()),
            42,
            bin,
            cache,
        );

        assert_eq!(plan.source_id, "github");
        assert!(!plan.needs_root());
        assert_eq!(plan.download_size, Some(42));
        match &plan.actions[0] {
            Action::Download { url, dest, verify } => {
                assert_eq!(url, "https://x/lin");
                assert_eq!(dest, &cache.join("jq-linux-amd64"));
                assert!(matches!(verify, Verification::Sha256(_)));
            }
            other => panic!("expected download, got {other:?}"),
        }
        match &plan.actions[1] {
            Action::Place { dest, mode, .. } => {
                assert_eq!(dest, &bin.join("jq"));
                assert_eq!(*mode, 0o755);
            }
            other => panic!("expected place, got {other:?}"),
        }
    }

    #[test]
    fn is_placed_reflects_file_existence() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_placed(dir.path(), "jq"));
        std::fs::write(dir.path().join("jq"), b"bin").unwrap();
        assert!(is_placed(dir.path(), "jq"));
    }

    #[test]
    fn build_plan_marks_unverified_without_checksum() {
        let plan = build_install_plan(
            "tool",
            "o/tool",
            None,
            "https://x/bin",
            "tool-linux-amd64",
            None,
            0,
            Path::new("/b"),
            Path::new("/c"),
        );
        assert!(matches!(plan.actions[0], Action::Download { verify: Verification::None, .. }));
        assert_eq!(plan.download_size, None);
        assert!(plan.reasons.iter().any(|r| r.contains("unverified")));
    }
}
