//! GitHub — a [`Forge`] implementation for GitHub Releases.
//!
//! Supplies only the GitHub-specific bits: the API base, the "latest release" call and its
//! JSON shape, the web repo URL, and the `/rate_limit` health probe. All the shared work
//! (arch-aware asset selection, checksums, the user-space install plan) lives in the generic
//! [`super::forge::ForgeProvider`], so GitHub is one forge among peers, not a special case
//! (ADR-0049). The provider is built by wrapping `GithubForge` in a `ForgeProvider`.

use async_trait::async_trait;
use serde::Deserialize;

use super::Probe;
use super::forge::{Forge, ForgeAsset, Release};
use crate::error::{JiiError, Result};
use crate::model::RepoHit;

const API: &str = "https://api.github.com";

/// The GitHub forge (GitHub Releases).
pub struct GithubForge;

#[async_trait]
impl Forge for GithubForge {
    fn id(&self) -> &'static str {
        "github"
    }

    fn label(&self) -> &'static str {
        "GitHub"
    }

    fn repo_url(&self, owner: &str, repo: &str) -> String {
        format!("https://github.com/{owner}/{repo}")
    }

    async fn latest_release(
        &self,
        client: &reqwest::Client,
        owner: &str,
        repo: &str,
        token: Option<&str>,
    ) -> Result<Release> {
        // The release **list**, not `/releases/latest`: the latter 404s on a repo whose only
        // releases are pre-releases (deliberately excluded there), which made `jii owner/repo`
        // fail on perfectly installable projects — the same trap selfupdate and install.sh
        // already dodged. The list is newest-first; take the newest non-draft that actually
        // ships assets (a source-only release has nothing to install), else the newest non-draft.
        let url = format!("{API}/repos/{owner}/{repo}/releases?per_page=20");
        let mut req = client.get(&url).header("Accept", "application/vnd.github+json");
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let resp = req
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| JiiError::Other(anyhow::anyhow!("github: {e}")))?;
        let releases: Vec<GhRelease> = resp
            .json()
            .await
            .map_err(|e| JiiError::Other(anyhow::anyhow!("github: malformed release json: {e}")))?;
        pick_release(releases)
            .map(GhRelease::normalize)
            .ok_or_else(|| JiiError::Other(anyhow::anyhow!("github: {owner}/{repo} has no published release")))
    }

    async fn probe(&self, client: &reqwest::Client, token: Option<&str>) -> Probe {
        // GET /rate_limit — cheap, and it does not itself count against the limit.
        match fetch_rate_limit(client, token).await {
            Ok((remaining, limit)) => Probe {
                reachable: true,
                rate_limited: remaining == 0,
                detail: Some(format!("{remaining}/{limit} req left")),
            },
            Err(_) => Probe::unreachable(),
        }
    }

    async fn search_repos(
        &self,
        client: &reqwest::Client,
        query: &str,
        per_page: u32,
        page: u32,
        token: Option<&str>,
    ) -> Result<Vec<RepoHit>> {
        // GitHub's repo search: relevance-ranked ("best match") by default, which is what we
        // want for "find the repo I mean by name". `page` is 1-based.
        let url = format!("{API}/search/repositories");
        let mut req = client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .query(&[
                ("q", query),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
            ]);
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let resp = req
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| JiiError::Other(anyhow::anyhow!("github: {e}")))?;
        let body: GhSearch = resp
            .json()
            .await
            .map_err(|e| JiiError::Other(anyhow::anyhow!("github: malformed search json: {e}")))?;
        Ok(body.into_hits(self.id()))
    }
}

/// GitHub `/search/repositories` response (only the fields the picker needs).
#[derive(Debug, Deserialize)]
struct GhSearch {
    #[serde(default)]
    items: Vec<GhRepo>,
}

impl GhSearch {
    /// Normalise to [`RepoHit`]s, dropping archived (dead) repos.
    fn into_hits(self, source_id: &str) -> Vec<RepoHit> {
        self.items
            .into_iter()
            .filter(|r| !r.archived)
            .map(|r| RepoHit {
                source_id: source_id.to_string(),
                slug: r.full_name,
                description: r.description.filter(|d| !d.is_empty()),
                stars: r.stargazers_count,
            })
            .collect()
    }
}

/// One repository in a search response.
#[derive(Debug, Deserialize)]
struct GhRepo {
    full_name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    stargazers_count: u64,
    #[serde(default)]
    archived: bool,
}

/// Fetch `(remaining, limit)` from the GitHub rate-limit endpoint.
async fn fetch_rate_limit(client: &reqwest::Client, token: Option<&str>) -> Result<(u32, u32)> {
    let mut req = client
        .get(format!("{API}/rate_limit"))
        .header("Accept", "application/vnd.github+json");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| JiiError::Other(anyhow::anyhow!("github: {e}")))?;
    let body: RateLimit = resp
        .json()
        .await
        .map_err(|e| JiiError::Other(anyhow::anyhow!("github: {e}")))?;
    Ok((body.rate.remaining, body.rate.limit))
}

/// The newest installable release from a newest-first list: the first non-draft **with
/// assets** (a source-only release has nothing to install), else the first non-draft.
fn pick_release(releases: Vec<GhRelease>) -> Option<GhRelease> {
    let mut first_non_draft: Option<GhRelease> = None;
    for r in releases.into_iter().filter(|r| !r.draft) {
        if !r.assets.is_empty() {
            return Some(r);
        }
        first_non_draft.get_or_insert(r);
    }
    first_non_draft
}

/// The subset of the GitHub release API we consume.
#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

impl GhRelease {
    /// Map GitHub's JSON onto the forge-neutral [`Release`].
    fn normalize(self) -> Release {
        Release {
            tag_name: self.tag_name,
            assets: self
                .assets
                .into_iter()
                .map(|a| ForgeAsset {
                    name: a.name,
                    url: a.browser_download_url,
                    size: a.size,
                })
                .collect(),
        }
    }
}

/// One downloadable release asset (GitHub shape).
#[derive(Debug, Clone, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// The GitHub `/rate_limit` response (only the core `rate` block).
#[derive(Debug, Deserialize)]
struct RateLimit {
    rate: RateCore,
}

#[derive(Debug, Deserialize)]
struct RateCore {
    limit: u32,
    remaining: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RELEASE: &str = r#"{
        "tag_name": "jq-1.7.1",
        "assets": [
            {"name": "jq-linux-amd64", "browser_download_url": "https://x/lin", "size": 3},
            {"name": "jq-linux-arm64", "browser_download_url": "https://x/arm", "size": 4},
            {"name": "sha256sums.txt", "browser_download_url": "https://x/sums", "size": 6}
        ]
    }"#;

    #[test]
    fn parses_and_normalizes_github_release_json() {
        let gh: GhRelease = serde_json::from_str(SAMPLE_RELEASE).unwrap();
        let release = gh.normalize();
        assert_eq!(release.tag_name, "jq-1.7.1");
        assert_eq!(release.assets.len(), 3);
        // browser_download_url is normalised to `url`.
        assert_eq!(release.assets[0].name, "jq-linux-amd64");
        assert_eq!(release.assets[0].url, "https://x/lin");
        assert_eq!(release.assets[0].size, 3);
    }

    #[test]
    fn forge_identity() {
        let f = GithubForge;
        assert_eq!(f.id(), "github");
        assert_eq!(f.label(), "GitHub");
        assert_eq!(f.repo_url("jqlang", "jq"), "https://github.com/jqlang/jq");
    }

    #[test]
    fn pick_release_prefers_newest_non_draft_with_assets() {
        let parse = |s: &str| serde_json::from_str::<Vec<GhRelease>>(s).unwrap();
        // A prerelease-only repo (the /releases/latest 404 trap) still resolves.
        let only_pre = parse(
            r#"[{"tag_name":"v0.2-beta","assets":[{"name":"a","browser_download_url":"u","size":1}]}]"#,
        );
        assert_eq!(pick_release(only_pre).unwrap().tag_name, "v0.2-beta");
        // A newer source-only release is skipped in favour of one that ships assets.
        let source_only_first = parse(
            r#"[{"tag_name":"v2"},
                {"tag_name":"v1","assets":[{"name":"a","browser_download_url":"u","size":1}]}]"#,
        );
        assert_eq!(pick_release(source_only_first).unwrap().tag_name, "v1");
        // Drafts never win; with no asset-bearing release the first non-draft is returned.
        let drafts_and_bare = parse(r#"[{"tag_name":"vd","draft":true},{"tag_name":"v3"}]"#);
        assert_eq!(pick_release(drafts_and_bare).unwrap().tag_name, "v3");
        assert!(pick_release(vec![]).is_none());
    }

    #[test]
    fn parses_rate_limit_json() {
        let body: RateLimit =
            serde_json::from_str(r#"{"rate":{"limit":60,"remaining":57}}"#).unwrap();
        assert_eq!(body.rate.limit, 60);
        assert_eq!(body.rate.remaining, 57);
    }

    #[test]
    fn search_json_normalizes_and_drops_archived() {
        let body: GhSearch = serde_json::from_str(
            r#"{"items":[
                {"full_name":"exteragram/ExteraGram","description":"A Telegram client","stargazers_count":1200,"archived":false},
                {"full_name":"old/dead","description":"","stargazers_count":5,"archived":true},
                {"full_name":"no/desc","stargazers_count":0}
            ]}"#,
        )
        .unwrap();
        let hits = body.into_hits("github");
        // The archived repo is dropped; the empty description becomes None.
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].slug, "exteragram/ExteraGram");
        assert_eq!(hits[0].source_id, "github");
        assert_eq!(hits[0].stars, 1200);
        assert_eq!(hits[0].description.as_deref(), Some("A Telegram client"));
        assert_eq!(hits[1].slug, "no/desc");
        assert_eq!(hits[1].description, None);
    }
}
