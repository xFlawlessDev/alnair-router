//! Release update checks against the project's GitHub releases.
//!
//! The dashboard asks `/api/update` whether a newer release exists. GitHub is
//! only ever contacted from here, never from the browser: an outbound request
//! keeps the repository's CORS and rate-limit behaviour out of the picture and
//! lets the answer be cached for every dashboard client at once.
//!
//! Every failure is reported in the payload rather than raised as an HTTP
//! error. An update check is decoration — a machine that is offline, behind a
//! proxy, or rate-limited by GitHub must still get its version back so the
//! dashboard can show what it is running.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::UpdateConfig;
use crate::error::Result;

const GITHUB_TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = "alnair-router";

/// The version this binary was built from.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Update availability plus the release the answer came from.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub name: String,
    pub version: String,
    /// Newest release GitHub reports, or null when the lookup was skipped or
    /// failed. Present does not imply newer — compare with `version`.
    pub latest_version: Option<String>,
    /// True only when `latest_version` parses and is strictly greater.
    pub update_available: bool,
    /// Release page to open; null when nothing was fetched.
    pub release_url: Option<String>,
    pub release_notes: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    /// When GitHub was last asked, i.e. when this answer was cached.
    pub checked_at: Option<DateTime<Utc>>,
    /// False when checks are disabled by configuration.
    pub check_enabled: bool,
    /// Why the lookup produced no `latest_version`, when it did not.
    pub error: Option<String>,
}

/// One GitHub release, narrowed to the fields the dashboard shows.
#[derive(Debug, Clone, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    published_at: Option<DateTime<Utc>>,
    #[serde(default)]
    prerelease: bool,
}

/// Strips a leading `v` so `v1.2.3` and `1.2.3` both parse.
fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.trim().trim_start_matches(['v', 'V'])).ok()
}

/// Picks the highest version among the releases, skipping pre-releases unless
/// asked. Falls back to whatever order GitHub returned when nothing parses.
fn newest_release(
    releases: Vec<GithubRelease>,
    include_prereleases: bool,
) -> Option<GithubRelease> {
    let mut best: Option<(Version, GithubRelease)> = None;
    let mut fallback: Option<GithubRelease> = None;

    for release in releases {
        if release.prerelease && !include_prereleases {
            continue;
        }
        fallback.get_or_insert_with(|| release.clone());

        let Some(version) = parse_tag(&release.tag_name) else {
            continue;
        };
        if best.as_ref().is_none_or(|(current, _)| version > *current) {
            best = Some((version, release));
        }
    }

    best.map(|(_, release)| release).or(fallback)
}

/// TTL-cached release lookup, shared by every dashboard session.
pub struct UpdateChecker {
    client: reqwest::Client,
    cache: RwLock<Option<CachedStatus>>,
}

struct CachedStatus {
    status: UpdateStatus,
    fetched_at: std::time::Instant,
}

impl UpdateChecker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            client: reqwest::Client::builder()
                .timeout(GITHUB_TIMEOUT)
                .build()
                .unwrap_or_default(),
            cache: RwLock::new(None),
        })
    }

    /// Returns the update status, reusing a cached answer until its TTL lapses.
    ///
    /// `force` bypasses the cache, which is what the dashboard's refresh button
    /// sends. A cached answer keeps its original `checked_at`.
    pub async fn status(&self, config: &UpdateConfig, force: bool) -> UpdateStatus {
        let current = current_version().to_string();

        if !config.check_enabled {
            return UpdateStatus {
                name: env!("CARGO_PKG_NAME").to_string(),
                version: current,
                latest_version: None,
                update_available: false,
                release_url: None,
                release_notes: None,
                published_at: None,
                checked_at: None,
                check_enabled: false,
                error: None,
            };
        }

        if !force {
            let cached = self.cache.read().await;
            if let Some(entry) = cached.as_ref()
                && entry.fetched_at.elapsed() < Duration::from_secs(config.cache_ttl_secs)
            {
                return entry.status.clone();
            }
        }

        let fetched = match self.fetch(config).await {
            Ok(release) => release,
            Err(error) => {
                // A failed refresh must not discard the last good answer: a
                // transient outage should not blank the update banner.
                if let Some(entry) = self.cache.read().await.as_ref() {
                    let mut status = entry.status.clone();
                    status.error = Some(error);
                    return status;
                }

                return UpdateStatus {
                    name: env!("CARGO_PKG_NAME").to_string(),
                    version: current,
                    latest_version: None,
                    update_available: false,
                    release_url: None,
                    release_notes: None,
                    published_at: None,
                    checked_at: None,
                    check_enabled: true,
                    error: Some(error),
                };
            }
        };

        let latest = parse_tag(&fetched.tag_name);
        let status = UpdateStatus {
            name: env!("CARGO_PKG_NAME").to_string(),
            version: current,
            latest_version: latest.as_ref().map(|version| version.to_string()),
            update_available: latest.is_some_and(|version| {
                parse_tag(current_version()).is_some_and(|running| version > running)
            }),
            release_url: fetched.html_url.clone(),
            release_notes: fetched.body.as_deref().map(truncate_notes),
            published_at: fetched.published_at,
            checked_at: Some(Utc::now()),
            check_enabled: true,
            error: None,
        };

        *self.cache.write().await = Some(CachedStatus {
            status: status.clone(),
            fetched_at: std::time::Instant::now(),
        });

        status
    }

    /// Asks GitHub for the newest release.
    async fn fetch(&self, config: &UpdateConfig) -> Result<GithubRelease, String> {
        let repo = config.repo.trim();
        if !repo.contains('/') {
            return Err(format!("update.repo must be 'owner/name' (got '{repo}')"));
        }
        let base = config.api_url.trim().trim_end_matches('/');
        if base.is_empty() {
            return Err("update.api_url must not be blank".to_string());
        }

        let url = if config.include_prereleases {
            format!("{base}/repos/{repo}/releases?per_page=20")
        } else {
            format!("{base}/repos/{repo}/releases/latest")
        };

        let response = self
            .client
            .get(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .map_err(|error| format!("cannot reach GitHub for '{repo}': {error}"))?;

        let status = response.status();
        if !status.is_success() {
            // 404 means "no releases yet", which is not a fault worth alarming
            // about on a fresh repository.
            return Err(if status == reqwest::StatusCode::NOT_FOUND {
                format!("'{repo}' has no published releases")
            } else {
                format!("GitHub returned {status} for '{repo}'")
            });
        }

        if config.include_prereleases {
            let releases = response
                .json::<Vec<GithubRelease>>()
                .await
                .map_err(|error| format!("cannot read the GitHub release list: {error}"))?;
            newest_release(releases, true)
                .ok_or_else(|| format!("'{repo}' has no published releases"))
        } else {
            response
                .json::<GithubRelease>()
                .await
                .map_err(|error| format!("cannot read the GitHub release payload: {error}"))
        }
    }
}

/// Keeps release notes to a size the dashboard can render without a scroller
/// the size of the changelog.
fn truncate_notes(body: &str) -> String {
    const MAX: usize = 4_000;
    if body.chars().count() <= MAX {
        return body.to_string();
    }
    let mut truncated: String = body.chars().take(MAX).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool) -> GithubRelease {
        GithubRelease {
            tag_name: tag.to_string(),
            html_url: Some(format!("https://example.test/{tag}")),
            body: None,
            published_at: None,
            prerelease,
        }
    }

    #[test]
    fn tags_parse_with_or_without_the_v_prefix() {
        assert_eq!(parse_tag("v1.2.3"), Version::parse("1.2.3").ok());
        assert_eq!(parse_tag("1.2.3"), Version::parse("1.2.3").ok());
        assert_eq!(parse_tag("not-a-version"), None);
    }

    #[test]
    fn newest_release_picks_the_highest_version_not_the_first() {
        let picked = newest_release(
            vec![release("v0.9.0", false), release("v0.10.0", false)],
            false,
        );
        assert_eq!(
            picked.map(|release| release.tag_name),
            Some("v0.10.0".into())
        );
    }

    #[test]
    fn prereleases_are_skipped_unless_requested() {
        let releases = vec![release("v0.2.0", true), release("v0.1.0", false)];

        let stable = newest_release(releases.clone(), false).expect("release");
        assert_eq!(stable.tag_name, "v0.1.0");

        let any = newest_release(releases, true).expect("release");
        assert_eq!(any.tag_name, "v0.2.0");
    }

    #[test]
    fn unparsable_tags_fall_back_to_the_listing_order() {
        let picked = newest_release(vec![release("nightly", false)], false);
        assert_eq!(
            picked.map(|release| release.tag_name),
            Some("nightly".into())
        );
    }

    #[tokio::test]
    async fn disabled_checks_report_the_running_version_only() {
        let checker = UpdateChecker::new();
        let config = UpdateConfig {
            check_enabled: false,
            ..UpdateConfig::default()
        };

        let status = checker.status(&config, false).await;
        assert!(!status.check_enabled);
        assert_eq!(status.version, current_version());
        assert_eq!(status.latest_version, None);
        assert!(!status.update_available);
    }

    #[tokio::test]
    async fn a_bad_repo_slug_reports_an_error_instead_of_failing() {
        let checker = UpdateChecker::new();
        let config = UpdateConfig {
            repo: "no-slash-here".to_string(),
            ..UpdateConfig::default()
        };

        let status = checker.status(&config, true).await;
        assert!(status.error.is_some(), "expected an error: {status:?}");
        assert_eq!(status.latest_version, None);
        assert_eq!(status.version, current_version());
    }
}
