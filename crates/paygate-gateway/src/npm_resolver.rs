use rusqlite::params;
use serde::Deserialize;
use tracing::{info, warn};

use crate::db::DbReader;

/// Result of resolving an npm package to a GitHub owner.
#[derive(Debug, Clone)]
pub struct NpmResolution {
    pub github_owner: String,
    pub github_repo: Option<String>,
    pub cached: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("package not found on npm registry")]
    PackageNotFound,
    #[error("package has no repository field")]
    NoRepository,
    #[error("repository is not hosted on GitHub")]
    NotGitHub,
    #[error("npm registry request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("failed to parse npm registry response")]
    ParseError,
    #[error("database error: {0}")]
    DbError(String),
}

/// Subset of npm registry package metadata we need.
#[derive(Deserialize)]
struct NpmPackage {
    repository: Option<NpmRepository>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NpmRepository {
    Object { url: Option<String>, #[serde(rename = "type")] _type: Option<String> },
    String(String),
}

impl NpmRepository {
    fn url(&self) -> Option<&str> {
        match self {
            NpmRepository::Object { url, .. } => url.as_deref(),
            NpmRepository::String(s) => Some(s.as_str()),
        }
    }
}

/// Parse a GitHub owner and repo from a repository URL.
///
/// Handles formats like:
///   https://github.com/owner/repo
///   https://github.com/owner/repo.git
///   git+https://github.com/owner/repo.git
///   git://github.com/owner/repo.git
///   github:owner/repo
fn parse_github_url(url: &str) -> Option<(String, String)> {
    let url = url.trim();

    // Handle github:owner/repo shorthand
    if let Some(rest) = url.strip_prefix("github:") {
        let parts: Vec<&str> = rest.splitn(2, '/').collect();
        if parts.len() == 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }

    // Handle full URLs
    let url = url
        .strip_prefix("git+")
        .unwrap_or(url)
        .strip_prefix("git://")
        .unwrap_or(url)
        .strip_prefix("ssh://git@")
        .unwrap_or(url);

    // Find github.com in the URL
    let gh_idx = url.find("github.com")?;
    let after = &url[gh_idx + "github.com".len()..];
    let after = after.strip_prefix('/').or_else(|| after.strip_prefix(':'))?;

    let parts: Vec<&str> = after.split('/').collect();
    if parts.len() >= 2 {
        let owner = parts[0].to_string();
        let repo = parts[1]
            .strip_suffix(".git")
            .unwrap_or(parts[1])
            .to_string();
        if !owner.is_empty() && !repo.is_empty() {
            return Some((owner, repo));
        }
    }
    None
}

/// Check the npm_cache table for a cached resolution.
fn check_cache(db_reader: &DbReader, package_name: &str) -> Option<NpmResolution> {
    let conn = db_reader.conn_raw().ok()?;
    let mut stmt = conn
        .prepare(
            "SELECT github_owner, github_repo, resolved_at FROM npm_cache
             WHERE package_name = ? AND resolved_at > datetime('now', '-1 day')",
        )
        .ok()?;
    stmt.query_row(params![package_name], |row| {
        Ok(NpmResolution {
            github_owner: row.get(0)?,
            github_repo: row.get(1)?,
            cached: true,
        })
    })
    .ok()
}

/// Store a resolution in the npm_cache table.
fn store_cache(db_reader: &DbReader, package_name: &str, resolution: &NpmResolution) {
    if let Ok(conn) = db_reader.conn_raw() {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO npm_cache (package_name, github_owner, github_repo, resolved_at)
             VALUES (?, ?, ?, datetime('now'))",
            params![package_name, resolution.github_owner, resolution.github_repo],
        );
    }
}

/// Resolve an npm package name to a GitHub owner.
///
/// Checks cache first (TTL 24h), then fetches from the npm registry.
pub async fn resolve_package(
    http_client: &reqwest::Client,
    db_reader: &DbReader,
    package_name: &str,
) -> Result<NpmResolution, ResolveError> {
    // Check cache first
    if let Some(cached) = check_cache(db_reader, package_name) {
        info!(package = package_name, owner = %cached.github_owner, "npm cache hit");
        return Ok(cached);
    }

    // Fetch from npm registry (full metadata — abbreviated doesn't include repository)
    // In production (Railway), use NPM_REGISTRY_PROXY to route through the Node.js proxy
    // which can reach registry.npmjs.org (same connectivity fix as the RPC proxy).
    let url = match std::env::var("NPM_REGISTRY_PROXY") {
        Ok(proxy) => format!("{}/{}", proxy.trim_end_matches('/'), package_name),
        Err(_) => format!("https://registry.npmjs.org/{package_name}"),
    };
    let resp = http_client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(ResolveError::PackageNotFound);
    }
    if !resp.status().is_success() {
        warn!(package = package_name, status = %resp.status(), "npm registry error");
        return Err(ResolveError::ParseError);
    }

    let pkg: NpmPackage = resp.json().await.map_err(|_| ResolveError::ParseError)?;

    let repo = pkg.repository.ok_or(ResolveError::NoRepository)?;
    let repo_url = repo.url().ok_or(ResolveError::NoRepository)?;

    let (owner, repo_name) = parse_github_url(repo_url).ok_or(ResolveError::NotGitHub)?;

    let resolution = NpmResolution {
        github_owner: owner,
        github_repo: Some(repo_name),
        cached: false,
    };

    // Cache the result
    store_cache(db_reader, package_name, &resolution);
    info!(package = package_name, owner = %resolution.github_owner, "npm resolved");

    Ok(resolution)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_url_https() {
        let (owner, repo) = parse_github_url("https://github.com/chalk/chalk").unwrap();
        assert_eq!(owner, "chalk");
        assert_eq!(repo, "chalk");
    }

    #[test]
    fn test_parse_github_url_git_suffix() {
        let (owner, repo) =
            parse_github_url("https://github.com/sindresorhus/execa.git").unwrap();
        assert_eq!(owner, "sindresorhus");
        assert_eq!(repo, "execa");
    }

    #[test]
    fn test_parse_github_url_git_plus_prefix() {
        let (owner, repo) =
            parse_github_url("git+https://github.com/lodash/lodash.git").unwrap();
        assert_eq!(owner, "lodash");
        assert_eq!(repo, "lodash");
    }

    #[test]
    fn test_parse_github_url_shorthand() {
        let (owner, repo) = parse_github_url("github:visionmedia/debug").unwrap();
        assert_eq!(owner, "visionmedia");
        assert_eq!(repo, "debug");
    }

    #[test]
    fn test_parse_github_url_ssh() {
        let (owner, repo) =
            parse_github_url("git://github.com/isaacs/node-glob.git").unwrap();
        assert_eq!(owner, "isaacs");
        assert_eq!(repo, "node-glob");
    }

    #[test]
    fn test_parse_github_url_not_github() {
        assert!(parse_github_url("https://gitlab.com/foo/bar").is_none());
    }

    #[test]
    fn test_parse_github_url_empty() {
        assert!(parse_github_url("").is_none());
    }
}
