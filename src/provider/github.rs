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
        let gh: GhRelease = resp
            .json()
            .await
            .map_err(|e| JiiError::Other(anyhow::anyhow!("github: malformed release json: {e}")))?;
        Ok(gh.normalize())
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

/// The subset of the GitHub release API we consume.
#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
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
    fn parses_rate_limit_json() {
        let body: RateLimit =
            serde_json::from_str(r#"{"rate":{"limit":60,"remaining":57}}"#).unwrap();
        assert_eq!(body.rate.limit, 60);
        assert_eq!(body.rate.remaining, 57);
    }
}
