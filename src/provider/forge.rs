//! Forge-based release providers.
//!
//! A **forge** is a code host that publishes downloadable releases — GitHub, and (via the
//! [`Forge`] trait) Codeberg/Gitea, GitLab, and peers. The forge supplies only the
//! host-specific bits: its id, the API call that fetches a repo's latest release, the web
//! repo URL, and an optional rate-limit probe. Everything else — arch-aware asset selection,
//! checksum verification, and building the user-space `~/.local/bin` install plan (no root) —
//! is shared here. So GitHub is **one `Forge` among peers, not a hardcoded exception**
//! (ADR-0049); adding a forge is implementing this trait, with no core change.
//!
//! Scope: **raw executable assets, `.tar.gz`, and `.zip`** (other archives are skipped until
//! an `Extract` action covers them). Trust is always `untrusted` (an arbitrary third-party
//! binary), so installs are always confirmed explicitly — a verified sha256 raises confidence
//! but not the trust tier. The query must name the repo explicitly as `owner/repo`.

use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};

use super::{Probe, Provider, http_client};
use crate::error::{JiiError, Result};
use crate::model::{
    Action, InstallPlan, InstalledRecord, PackageCandidate, PackageInfo, PkgVersion, Query,
    TrustLevel, Verification,
};

/// A code-hosting forge that publishes releases. Implementors supply only host specifics; the
/// shared [`ForgeProvider`] machinery does asset selection, checksums and install planning.
#[async_trait]
pub trait Forge: Send + Sync {
    /// Source id, e.g. `"github"`, `"codeberg"`, `"gitlab"`.
    fn id(&self) -> &'static str;
    /// Human label for reasons/summaries, e.g. `"GitHub"`.
    fn label(&self) -> &'static str;
    /// The web URL of a repo, for the `info` card (e.g. `https://github.com/owner/repo`).
    fn repo_url(&self, owner: &str, repo: &str) -> String;
    /// Fetch a repo's latest installable release, **normalised** to [`Release`]. `token` is
    /// the optional API token (from the provider's configured env var).
    async fn latest_release(
        &self,
        client: &reqwest::Client,
        owner: &str,
        repo: &str,
        token: Option<&str>,
    ) -> Result<Release>;
    /// A health probe (rate-limit etc.) for `jii doctor`. Default: assume reachable — a forge
    /// with no cheap probe still works; per-request errors surface during search.
    async fn probe(&self, client: &reqwest::Client, token: Option<&str>) -> Probe {
        let _ = (client, token);
        Probe { reachable: true, rate_limited: false, detail: None }
    }
}

/// A release normalised across forges (whatever the host's native JSON shape).
pub struct Release {
    pub tag_name: String,
    pub assets: Vec<ForgeAsset>,
}

/// A downloadable release asset, normalised across forges.
#[derive(Clone)]
pub struct ForgeAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// A [`Provider`] backed by a [`Forge`]: resolves `owner/repo` → latest release → the
/// arch-matching asset → a verified user-space install into `~/.local/bin`.
pub struct ForgeProvider {
    forge: Box<dyn Forge>,
    /// Env var holding an API token (lifts anonymous rate limits).
    token_env: String,
    /// Host architecture used to filter release assets (e.g. `"x86_64"`).
    arch: &'static str,
}

impl ForgeProvider {
    pub fn new(forge: Box<dyn Forge>, token_env: String, arch: &'static str) -> Self {
        ForgeProvider { forge, token_env, arch }
    }

    /// An API token from the configured env var, if set and non-empty.
    fn token(&self) -> Option<String> {
        std::env::var(&self.token_env).ok().filter(|s| !s.is_empty())
    }
}

#[async_trait]
impl Provider for ForgeProvider {
    fn id(&self) -> &'static str {
        self.forge.id()
    }

    fn trust(&self) -> TrustLevel {
        // A third-party binary from an arbitrary repo — always confirmed explicitly.
        TrustLevel::Untrusted
    }

    fn highlights(&self, _candidate: &PackageCandidate) -> Vec<String> {
        vec![crate::t!("reason.forge_thirdparty"), crate::t!("reason.forge_installs_local")]
    }

    async fn is_available(&self) -> bool {
        // A remote service with no local binary to probe. Reachability surfaces per-request
        // as a search error/timeout, which the engine handles gracefully.
        true
    }

    async fn describe(&self, candidate: &PackageCandidate) -> Option<PackageInfo> {
        // Cheap card from what search already captured (`owner/repo`), no extra API call.
        let slug = candidate.raw.get("slug")?.as_str()?;
        let (owner, repo) = slug.split_once('/')?;
        Some(PackageInfo {
            description: None,
            homepage: None,
            repository: Some(self.forge.repo_url(owner, repo)),
            license: None,
            author: (!owner.is_empty()).then(|| owner.to_string()),
        })
    }

    async fn search(&self, query: &Query) -> Result<Vec<PackageCandidate>> {
        let Some((owner, repo)) = parse_owner_repo(&query.raw) else {
            // Not an `owner/repo` query — this provider has nothing to offer.
            return Ok(Vec::new());
        };

        let client = http_client()?;
        let token = self.token();
        let release = self.forge.latest_release(&client, &owner, &repo, token.as_deref()).await?;

        let Some((asset, kind)) = select_asset(&release.assets, self.arch) else {
            return Ok(Vec::new());
        };

        // Resolve a checksum now (network stays in `search`) so the plan can enforce it.
        let sha256 = match find_checksums_asset(&release.assets) {
            Some(sums) => fetch_text(&client, &sums.url, token.as_deref())
                .await
                .ok()
                .and_then(|text| parse_checksums(&text, &asset.name)),
            None => None,
        };

        Ok(vec![candidate(
            self.id(),
            self.forge.label(),
            &owner,
            &repo,
            &release.tag_name,
            asset,
            kind,
            sha256,
        )])
    }

    async fn plan_install(&self, candidate: &PackageCandidate) -> Result<InstallPlan> {
        let url = raw_str(candidate, "url")?;
        let filename = raw_str(candidate, "filename")?;
        let slug = raw_str(candidate, "slug").unwrap_or_else(|_| candidate.name.clone());
        let sha256 = candidate.raw.get("sha256").and_then(|v| v.as_str()).map(str::to_string);
        let size = candidate.raw.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        let archive = candidate.raw.get("archive").and_then(|v| v.as_bool()).unwrap_or(false);

        Ok(build_install_plan(
            self.id(),
            &candidate.name,
            &slug,
            candidate.version.as_ref(),
            &url,
            &filename,
            archive,
            sha256,
            size,
            &bin_dir()?,
            &cache_dir()?,
        ))
    }

    async fn plan_remove(&self, record: &InstalledRecord) -> Result<InstallPlan> {
        let dest = bin_dir()?.join(&record.name);
        let reasons = vec![crate::t!("reason.forge_remove", name = record.name.clone(), dest = dest.display().to_string())];
        Ok(InstallPlan {
            candidate_ref: record.name.clone(),
            source_id: self.id().to_string(),
            actions: vec![Action::RemoveFile { path: dest }],
            download_size: None,
            reasons,
        })
    }

    async fn plan_update(&self, _record: &InstalledRecord) -> Result<InstallPlan> {
        Err(JiiError::Other(anyhow::anyhow!(
            "updating forge installs is not supported yet — reinstall with `jii <owner>/<repo>`"
        )))
    }

    async fn list_installed(&self) -> Result<Vec<InstalledRecord>> {
        // File-based source: it cannot enumerate its installs (no manifest of which
        // ~/.local/bin files came from a forge). The registry records what we installed;
        // `is_installed` verifies a specific record by file.
        Ok(Vec::new())
    }

    async fn is_installed(&self, record: &InstalledRecord) -> bool {
        bin_dir().is_ok_and(|d| is_placed(&d, &record.name))
    }

    async fn probe(&self) -> Probe {
        let Ok(client) = http_client() else {
            return Probe::unreachable();
        };
        self.forge.probe(&client, self.token().as_deref()).await
    }
}

/// GET a small text asset (e.g. a checksums file). Public asset URLs, so a bearer token is
/// only attached when present (private repos).
pub async fn fetch_text(client: &reqwest::Client, url: &str, token: Option<&str>) -> Result<String> {
    let mut req = client.get(url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| JiiError::Other(anyhow::anyhow!("forge: {e}")))?;
    resp.text()
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("forge: {e}")))
}

/// Whether the binary named `name` is present in `bin_dir` (a forge install).
fn is_placed(bin_dir: &Path, name: &str) -> bool {
    bin_dir.join(name).exists()
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

/// How a chosen asset is turned into an installed binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetKind {
    /// A directly-runnable executable — download then place.
    Binary,
    /// A gzip tarball — download then extract the binary.
    TarGz,
    /// A zip archive — download then extract the binary.
    Zip,
}

impl AssetKind {
    /// Whether the asset must be extracted (vs. placed directly).
    fn is_archive(self) -> bool {
        !matches!(self, AssetKind::Binary)
    }
}

/// Build a candidate from the selected asset. Name = repo (also the installed binary name);
/// everything the plan needs is stashed in `raw`.
#[allow(clippy::too_many_arguments)]
fn candidate(
    source_id: &str,
    label: &str,
    owner: &str,
    repo: &str,
    tag: &str,
    asset: &ForgeAsset,
    kind: AssetKind,
    sha256: Option<String>,
) -> PackageCandidate {
    PackageCandidate {
        name: repo.to_string(),
        source_id: source_id.to_string(),
        version: (!tag.is_empty()).then(|| PkgVersion::new(tag)),
        trust: TrustLevel::Untrusted,
        arch_ok: true,
        signed: sha256.is_some(),
        summary: Some(format!("{label} release {owner}/{repo}")),
        raw: json!({
            "slug": format!("{owner}/{repo}"),
            "url": asset.url,
            "filename": asset.name,
            "size": asset.size,
            "sha256": sha256,
            "archive": kind.is_archive(),
        }),
    }
}

/// Pick the best installable asset for `arch` (a raw binary, a `.tar.gz`, or a `.zip`), or
/// `None` if the release ships only unsupported archives / other-OS artifacts.
fn select_asset<'a>(assets: &'a [ForgeAsset], arch: &str) -> Option<(&'a ForgeAsset, AssetKind)> {
    let tokens = arch_tokens(arch);
    if tokens.is_empty() {
        return None;
    }
    assets
        .iter()
        .filter_map(|a| classify(&a.name, tokens).map(|kind| (a, kind)))
        .min_by_key(|(a, kind)| asset_score(&a.name, *kind))
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

/// Classify a Linux asset for this arch: a raw binary, a supported `.tar.gz` / `.zip`, or
/// `None` for other OSes/packages, unsupported archives, and metadata/signatures.
fn classify(name: &str, arch_tokens: &[&str]) -> Option<AssetKind> {
    const REJECT: &[&str] = &[
        // other OSes
        "windows", "win32", "win64", ".exe", ".msi", "darwin", "macos", "apple", ".dmg",
        "freebsd", "netbsd", "openbsd", "android",
        // distro packages
        ".deb", ".rpm", ".apk", ".pkg",
        // delta/patch artifacts (e.g. deno's `*.bsdiff` auto-update patches)
        ".bsdiff", ".patch", ".delta", ".zsync",
        // unsupported archives / compression (only .tar.gz/.tgz and .zip are handled)
        ".tar.xz", ".tar.bz2", ".tar.zst", ".7z", ".bz2", ".xz", ".zst",
        // metadata / signatures / checksums
        ".sha256", ".sig", ".asc", ".pem", ".txt", ".json", ".sbom", ".yml", ".yaml",
    ];
    let n = name.to_ascii_lowercase();
    if REJECT.iter().any(|tok| n.contains(tok)) {
        return None;
    }
    let arch_ok = arch_tokens.iter().any(|t| n.contains(t));
    // An AppImage is a self-contained, Linux-only executable, so accept it as a raw binary
    // even without an explicit "linux" token (still arch-gated). `.AppImage.zsync` update
    // files are already rejected above by the `.zsync` token.
    if n.ends_with(".appimage") {
        return arch_ok.then_some(AssetKind::Binary);
    }
    // Otherwise require an explicit linux marker and a matching arch token.
    if !n.contains("linux") || !arch_ok {
        return None;
    }
    if n.ends_with(".tar.gz") || n.ends_with(".tgz") {
        Some(AssetKind::TarGz)
    } else if n.ends_with(".zip") {
        Some(AssetKind::Zip)
    } else if is_bare_name(&n) {
        Some(AssetKind::Binary)
    } else {
        None
    }
}

/// Whether a name has no archive/compression extension left (i.e. a raw binary).
fn is_bare_name(lower_name: &str) -> bool {
    const ARCHIVE_EXT: &[&str] = &[".tar", ".gz", ".zip", ".xz", ".bz2", ".zst", ".7z"];
    !ARCHIVE_EXT.iter().any(|ext| lower_name.contains(ext))
}

/// Lower is better: prefer a raw binary (no extraction), then `.tar.gz` (preserves unix
/// modes) over `.zip`, then musl over gnu, then a shorter name (the plain binary over a
/// variant).
fn asset_score(name: &str, kind: AssetKind) -> (u8, u8, usize) {
    let kind_rank = match kind {
        AssetKind::Binary => 0,
        AssetKind::TarGz => 1,
        AssetKind::Zip => 2,
    };
    let n = name.to_ascii_lowercase();
    let libc = if n.contains("musl") {
        0
    } else if n.contains("gnu") {
        1
    } else {
        2
    };
    (kind_rank, libc, name.len())
}

/// Find a checksums asset (a shared `checksums.txt`/`SHA256SUMS`, or a per-asset `*.sha256`).
fn find_checksums_asset(assets: &[ForgeAsset]) -> Option<&ForgeAsset> {
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
        if let (Some(digest), Some(name)) = (fields.next(), fields.next())
            && name.trim_start_matches('*') == filename
            && is_sha256(digest)
        {
            return Some(digest.to_ascii_lowercase());
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

/// Assemble the install plan: download (verified) to the cache, then either place the raw
/// binary or extract it from the archive into `~/.local/bin`. Pure (unit-testable, no IO).
#[allow(clippy::too_many_arguments)]
fn build_install_plan(
    source_id: &str,
    name: &str,
    slug: &str,
    version: Option<&PkgVersion>,
    url: &str,
    filename: &str,
    archive: bool,
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

    let download = Action::Download {
        url: url.to_string(),
        dest: staged.clone(),
        verify,
    };
    let install = if archive {
        Action::Extract {
            archive: staged,
            member: name.to_string(),
            dest: dest.clone(),
            mode: 0o755,
        }
    } else {
        Action::Place {
            src: staged,
            dest: dest.clone(),
            mode: 0o755,
        }
    };

    let mut reasons = vec![crate::t!("reason.forge_release", slug = slug)];
    if let Some(v) = version {
        reasons.push(crate::t!("reason.version", v = v.clone()));
    }
    if archive {
        reasons.push(crate::t!("reason.forge_extracts", name = name));
    }
    reasons.push(match &sha256 {
        Some(_) => crate::t!("reason.forge_verified"),
        None => crate::t!("reason.forge_unverified"),
    });
    reasons.push(crate::t!("reason.forge_installs", dest = dest.display().to_string()));

    InstallPlan {
        candidate_ref: name.to_string(),
        source_id: source_id.to_string(),
        actions: vec![download, install],
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
        .ok_or_else(|| JiiError::Other(anyhow::anyhow!("forge candidate missing '{key}'")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> ForgeAsset {
        ForgeAsset { name: name.into(), url: "https://x/a".into(), size: 1 }
    }

    #[test]
    fn parses_owner_repo() {
        assert_eq!(parse_owner_repo("jqlang/jq"), Some(("jqlang".into(), "jq".into())));
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

    #[test]
    fn selects_linux_binary_for_arch_rejecting_other_os() {
        let a = vec![
            asset("jq-windows-amd64.exe"),
            asset("jq-macos-amd64"),
            asset("jq-linux-amd64"),
            asset("jq-linux-arm64"),
            asset("jq-1.7.1.tar.gz"),
        ];
        let (picked, kind) = select_asset(&a, "x86_64").unwrap();
        assert_eq!(picked.name, "jq-linux-amd64");
        assert_eq!(kind, AssetKind::Binary);
        assert_eq!(select_asset(&a, "aarch64").unwrap().0.name, "jq-linux-arm64");
        assert!(select_asset(&a, "riscv64").is_none());
    }

    #[test]
    fn prefers_raw_binary_over_tarball() {
        let a = vec![asset("tool-linux-x86_64.tar.gz"), asset("tool-linux-x86_64")];
        let (picked, kind) = select_asset(&a, "x86_64").unwrap();
        assert_eq!(picked.name, "tool-linux-x86_64");
        assert_eq!(kind, AssetKind::Binary);
    }

    #[test]
    fn selects_targz_when_no_raw_binary() {
        let a = vec![asset("ripgrep-14-x86_64-unknown-linux-musl.tar.gz")];
        let (_, kind) = select_asset(&a, "x86_64").unwrap();
        assert_eq!(kind, AssetKind::TarGz);
    }

    #[test]
    fn prefers_musl_over_gnu() {
        let a = vec![asset("tool-linux-x86_64-gnu"), asset("tool-linux-x86_64-musl")];
        assert_eq!(select_asset(&a, "x86_64").unwrap().0.name, "tool-linux-x86_64-musl");
    }

    #[test]
    fn rejects_unsupported_archives_and_packages() {
        let a = vec![
            asset("tool-linux-amd64.tar.xz"),
            asset("tool-linux-amd64.tar.zst"),
            asset("tool-linux-amd64.deb"),
        ];
        assert!(select_asset(&a, "x86_64").is_none());
    }

    #[test]
    fn selects_zip_when_no_binary_or_tarball() {
        let a = vec![asset("eza_x86_64-unknown-linux-gnu.zip")];
        let (_, kind) = select_asset(&a, "x86_64").unwrap();
        assert_eq!(kind, AssetKind::Zip);
    }

    #[test]
    fn installs_appimage_asset_without_linux_token() {
        let a = vec![asset("Inkscape-1.3-x86_64.AppImage")];
        let (picked, kind) = select_asset(&a, "x86_64").unwrap();
        assert_eq!(picked.name, "Inkscape-1.3-x86_64.AppImage");
        assert_eq!(kind, AssetKind::Binary);
    }

    #[test]
    fn rejects_wrong_arch_appimage_and_zsync_updater() {
        let a = vec![asset("App-aarch64.AppImage"), asset("App-x86_64.AppImage.zsync")];
        assert!(select_asset(&a, "x86_64").is_none());
    }

    #[test]
    fn prefers_targz_over_zip() {
        let a = vec![asset("tool-linux-x86_64.zip"), asset("tool-linux-x86_64.tar.gz")];
        let (picked, kind) = select_asset(&a, "x86_64").unwrap();
        assert_eq!(picked.name, "tool-linux-x86_64.tar.gz");
        assert_eq!(kind, AssetKind::TarGz);
    }

    #[test]
    fn zip_candidate_is_marked_archive() {
        let c = candidate(
            "github",
            "GitHub",
            "x",
            "tool",
            "v1",
            &asset("tool-linux-x86_64.zip"),
            AssetKind::Zip,
            None,
        );
        assert_eq!(c.raw.get("archive").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(c.source_id, "github");
        assert_eq!(c.summary.as_deref(), Some("GitHub release x/tool"));
    }

    #[test]
    fn finds_and_parses_checksums() {
        let sums = "\
deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  jq-linux-arm64
2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  jq-linux-amd64
";
        let digest = parse_checksums(sums, "jq-linux-amd64").unwrap();
        assert_eq!(digest, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        assert!(parse_checksums(sums, "absent").is_none());
    }

    #[test]
    fn parses_lone_digest_sha_file() {
        let d = "ABC123abc123ABC123abc123ABC123abc123ABC123abc123ABC123abc123abcd";
        assert_eq!(parse_checksums(d, "whatever").unwrap(), d.to_ascii_lowercase());
    }

    #[test]
    fn checksums_asset_detected() {
        let a = vec![asset("jq-linux-amd64"), asset("sha256sums.txt")];
        assert_eq!(find_checksums_asset(&a).unwrap().name, "sha256sums.txt");
    }

    #[test]
    fn build_plan_downloads_verified_then_places_executable() {
        let bin = Path::new("/home/u/.local/bin");
        let cache = Path::new("/home/u/.cache/jii/downloads");
        let plan = build_install_plan(
            "github",
            "jq",
            "jqlang/jq",
            Some(&PkgVersion::new("jq-1.7.1")),
            "https://x/lin",
            "jq-linux-amd64",
            false,
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
    fn build_plan_extracts_from_archive() {
        let bin = Path::new("/home/u/.local/bin");
        let cache = Path::new("/home/u/.cache/jii/downloads");
        let plan = build_install_plan(
            "github",
            "rg",
            "BurntSushi/ripgrep",
            Some(&PkgVersion::new("14.1.0")),
            "https://x/tgz",
            "ripgrep-14.1.0-x86_64-unknown-linux-musl.tar.gz",
            true,
            None,
            99,
            bin,
            cache,
        );
        match &plan.actions[1] {
            Action::Extract { member, dest, mode, .. } => {
                assert_eq!(member, "rg");
                assert_eq!(dest, &bin.join("rg"));
                assert_eq!(*mode, 0o755);
            }
            other => panic!("expected extract, got {other:?}"),
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
            "github",
            "tool",
            "o/tool",
            None,
            "https://x/bin",
            "tool-linux-amd64",
            false,
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
