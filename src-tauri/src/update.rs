//! GitHub release update checking
//!
//! This module implements a lightweight "notify and open" update flow: it asks
//! the GitHub Releases API for the latest published release, compares it against
//! the running version, and reports whether a newer one exists. It does **not**
//! download or install anything - the caller is expected to point the user at the
//! release page so they can grab the new build themselves.
//!
//! The HTTP request is made by shelling out to `curl` via `tokio::process`, which
//! keeps the dependency tree small and mirrors how `CliManager` already runs the
//! Filen CLI as a subprocess.

use serde::Deserialize;

/// The GitHub repository to check for releases, in `owner/repo` form.
const GITHUB_REPO: &str = "philippgerard/filen-menubar";

/// Errors that can occur while checking for updates.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("failed to run curl (is it installed?): {0}")]
    Spawn(#[source] std::io::Error),

    #[error("network request failed (curl exit {code:?}): {stderr}")]
    Http { code: Option<i32>, stderr: String },

    #[error("failed to parse GitHub response: {0}")]
    Parse(#[source] serde_json::Error),

    #[error("could not parse version string: {0:?}")]
    ParseVersion(String),
}

/// Subset of the GitHub Releases API response we care about.
#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    /// Git tag for the release, e.g. "v0.1.23".
    tag_name: String,
    /// Human-facing release page URL (specific tag, includes changelog).
    html_url: String,
}

/// Information about an available update.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// Version string without a leading `v`, e.g. "0.1.23".
    pub version: String,
    /// URL of the release page to open in the browser.
    pub url: String,
}

/// Parse a version string like "v1.2.3" or "1.2.3" into `(major, minor, patch)`.
///
/// A leading `v`/`V` is ignored, and any pre-release/build suffix on the patch
/// component (e.g. "3-beta.1" or "3+build") is stripped. Returns `None` if the
/// string is not at least `major.minor.patch` with numeric components.
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim();
    let s = s
        .strip_prefix('v')
        .or_else(|| s.strip_prefix('V'))
        .unwrap_or(s);

    let mut parts = s.split('.');
    let major = parts.next()?.trim().parse().ok()?;
    let minor = parts.next()?.trim().parse().ok()?;

    // The patch component may carry a pre-release/build suffix; keep the leading digits.
    let patch_raw = parts.next()?.trim();
    let digits: String = patch_raw
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let patch = digits.parse().ok()?;

    Some((major, minor, patch))
}

/// Returns true if `latest` is strictly newer than `current`.
///
/// Comparison is lexicographic over `(major, minor, patch)`, which matches
/// semantic-versioning precedence for plain `X.Y.Z` versions.
fn is_newer(latest: (u32, u32, u32), current: (u32, u32, u32)) -> bool {
    latest > current
}

/// Fetch the latest published release from the GitHub API via `curl`.
async fn fetch_latest_release() -> Result<ReleaseResponse, UpdateError> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");

    // -f: fail on HTTP errors, -sS: quiet but show errors, -L: follow redirects.
    let output = tokio::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "20",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            // GitHub rejects API requests without a User-Agent.
            "User-Agent: filen-menubar",
            &url,
        ])
        .output()
        .await
        .map_err(UpdateError::Spawn)?;

    if !output.status.success() {
        return Err(UpdateError::Http {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    serde_json::from_slice(&output.stdout).map_err(UpdateError::Parse)
}

/// Check GitHub for a newer release than the currently running version.
///
/// Returns `Ok(Some(info))` if a newer published release exists, `Ok(None)` if
/// we are already up to date, or `Err` if the check could not be completed
/// (network failure, malformed response, etc.).
pub async fn check_for_update() -> Result<Option<UpdateInfo>, UpdateError> {
    let release = fetch_latest_release().await?;

    let latest = parse_version(&release.tag_name)
        .ok_or_else(|| UpdateError::ParseVersion(release.tag_name.clone()))?;

    let current_str = env!("CARGO_PKG_VERSION");
    let current = parse_version(current_str)
        .ok_or_else(|| UpdateError::ParseVersion(current_str.to_string()))?;

    if is_newer(latest, current) {
        let version = release
            .tag_name
            .trim()
            .trim_start_matches(['v', 'V'])
            .to_string();
        Ok(Some(UpdateInfo {
            version,
            url: release.html_url,
        }))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version_plain() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.1.22"), Some((0, 1, 22)));
    }

    #[test]
    fn test_parse_version_with_v_prefix() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("V0.1.23"), Some((0, 1, 23)));
    }

    #[test]
    fn test_parse_version_with_whitespace() {
        assert_eq!(parse_version("  v1.2.3  "), Some((1, 2, 3)));
    }

    #[test]
    fn test_parse_version_strips_prerelease_suffix() {
        assert_eq!(parse_version("v1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3+build5"), Some((1, 2, 3)));
    }

    #[test]
    fn test_parse_version_invalid() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("not-a-version"), None);
        assert_eq!(parse_version("1.x.3"), None);
    }

    #[test]
    fn test_is_newer_patch() {
        assert!(is_newer((0, 1, 23), (0, 1, 22)));
        assert!(!is_newer((0, 1, 22), (0, 1, 22)));
        assert!(!is_newer((0, 1, 21), (0, 1, 22)));
    }

    #[test]
    fn test_is_newer_minor_and_major() {
        assert!(is_newer((0, 2, 0), (0, 1, 99)));
        assert!(is_newer((1, 0, 0), (0, 99, 99)));
        assert!(!is_newer((0, 9, 9), (1, 0, 0)));
    }

    #[test]
    fn test_current_crate_version_parses() {
        // The running version must always be parseable, otherwise an update
        // check can never succeed.
        assert!(parse_version(env!("CARGO_PKG_VERSION")).is_some());
    }

    #[test]
    fn test_release_response_deserializes() {
        // A trimmed-down sample of the GitHub /releases/latest payload.
        let json = r#"{
            "tag_name": "v0.1.23",
            "html_url": "https://github.com/philippgerard/filen-menubar/releases/tag/v0.1.23",
            "name": "v0.1.23",
            "draft": false
        }"#;
        let release: ReleaseResponse = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v0.1.23");
        assert!(release.html_url.ends_with("v0.1.23"));
    }
}
