//! Notify-only update check.
//!
//! This deliberately does not download or install anything. It asks GitHub for
//! the latest published release, compares the tag against the version compiled
//! into this binary, and hands the frontend a link. The user decides what to do
//! with it.
//!
//! The request lives in Rust rather than the WebView for two reasons: the CSP
//! in `tauri.conf.json` only allows `api.openai.com`, and the GitHub API rejects
//! requests without a `User-Agent`.

use serde::Serialize;

const RELEASES_API: &str =
  "https://api.github.com/repos/exalsch/AiDesktopCompanion/releases/latest";

/// GitHub rejects unauthenticated requests that do not identify themselves.
const USER_AGENT: &str = concat!("AiDesktopCompanion/", env!("CARGO_PKG_VERSION"));

#[derive(Serialize)]
pub struct UpdateInfo {
  /// Version compiled into this binary.
  pub current: String,
  /// Latest published release, tag stripped of a leading `v`.
  pub latest: String,
  /// Release page to open in a browser.
  pub url: String,
  pub update_available: bool,
}

/// Split a version string into numeric components, ignoring a leading `v` and
/// anything after the first pre-release or build separator.
///
/// Returns None for anything that does not start with a number, so a tag that
/// is not a version cannot be mistaken for one.
fn parse_version(raw: &str) -> Option<Vec<u64>> {
  let trimmed = raw.trim().trim_start_matches(['v', 'V']);
  let core = trimmed
    .split(['-', '+'])
    .next()
    .unwrap_or("");
  let parts: Vec<u64> = core
    .split('.')
    .map(|p| p.parse::<u64>().ok())
    .collect::<Option<Vec<u64>>>()?;
  if parts.is_empty() { None } else { Some(parts) }
}

/// True when `latest` is strictly newer than `current`.
///
/// Compares component by component and treats a missing component as zero, so
/// `0.2` and `0.2.0` are equal rather than one being newer.
fn is_newer(latest: &str, current: &str) -> bool {
  let (Some(l), Some(c)) = (parse_version(latest), parse_version(current)) else {
    return false;
  };
  let len = l.len().max(c.len());
  for i in 0..len {
    let a = l.get(i).copied().unwrap_or(0);
    let b = c.get(i).copied().unwrap_or(0);
    if a != b { return a > b; }
  }
  false
}

/// Ask GitHub for the latest release.
///
/// Errors are returned rather than swallowed so the caller can log them, but the
/// frontend treats a failure as "no update known" - being offline is not
/// something to interrupt the user about.
#[tauri::command]
pub async fn check_for_update() -> Result<UpdateInfo, String> {
  let current = env!("CARGO_PKG_VERSION").to_string();
  let client = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(10))
    .connect_timeout(std::time::Duration::from_secs(5))
    .user_agent(USER_AGENT)
    .build()
    .map_err(|e| format!("client build failed: {e}"))?;

  let resp = client
    .get(RELEASES_API)
    .header("Accept", "application/vnd.github+json")
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;

  if !resp.status().is_success() {
    return Err(format!("GitHub returned {}", resp.status()));
  }

  let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
  let tag = v
    .get("tag_name")
    .and_then(|x| x.as_str())
    .ok_or_else(|| "release has no tag_name".to_string())?;
  let url = v
    .get("html_url")
    .and_then(|x| x.as_str())
    .unwrap_or("https://github.com/exalsch/AiDesktopCompanion/releases/latest")
    .to_string();

  let latest = tag.trim_start_matches(['v', 'V']).to_string();
  Ok(UpdateInfo {
    update_available: is_newer(&latest, &current),
    current,
    latest,
    url,
  })
}

/// Open a release page in the user's browser.
///
/// Restricted to this project's own GitHub pages. The URL reaches here from the
/// WebView, so treating it as an arbitrary string to hand to the shell would
/// turn a rendering bug into a way to launch anything.
#[tauri::command]
pub fn open_release_page(url: String) -> Result<(), String> {
  const ALLOWED_PREFIX: &str = "https://github.com/exalsch/AiDesktopCompanion/";
  if !url.starts_with(ALLOWED_PREFIX) {
    return Err(format!("refusing to open a URL outside {ALLOWED_PREFIX}"));
  }

  #[cfg(target_os = "windows")]
  {
    std::process::Command::new("explorer.exe")
      .arg(&url)
      .spawn()
      .map_err(|e| format!("failed to open browser: {e}"))?;
    Ok(())
  }

  #[cfg(not(target_os = "windows"))]
  {
    Err("Opening a browser is not implemented for this platform".to_string())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn detects_a_newer_release() {
    assert!(is_newer("0.1.17", "0.1.16"));
    assert!(is_newer("0.2.0", "0.1.16"));
    assert!(is_newer("1.0.0", "0.9.9"));
    // Tags carry a leading v; the comparison has to see through it.
    assert!(is_newer("v0.1.17", "0.1.16"));
  }

  #[test]
  fn ignores_same_or_older_releases() {
    assert!(!is_newer("0.1.16", "0.1.16"));
    assert!(!is_newer("0.1.15", "0.1.16"));
    // A shorter tag is not newer just because it has fewer components.
    assert!(!is_newer("0.2", "0.2.0"));
    assert!(is_newer("0.2.1", "0.2"));
  }

  #[test]
  fn compares_numerically_not_lexically() {
    // The bug this guards: "0.1.9" > "0.1.10" as strings.
    assert!(is_newer("0.1.10", "0.1.9"));
    assert!(!is_newer("0.1.9", "0.1.10"));
  }

  #[test]
  fn refuses_unparseable_tags() {
    assert!(!is_newer("nightly", "0.1.16"));
    assert!(!is_newer("0.1.17", "not-a-version"));
  }

  #[test]
  fn strips_prerelease_suffixes() {
    assert_eq!(parse_version("v1.2.3-beta.1"), Some(vec![1, 2, 3]));
    assert_eq!(parse_version("1.2.3+build5"), Some(vec![1, 2, 3]));
  }

  #[test]
  fn only_opens_this_project() {
    assert!(open_release_page("https://evil.example.com/x".into()).is_err());
    assert!(open_release_page("https://github.com/someone/else".into()).is_err());
  }
}
