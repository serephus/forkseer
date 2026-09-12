use std::env;

use anyhow::{Result, anyhow, bail};
use palc::Parser;

/// Command-line arguments.
#[derive(Parser, Debug)]
#[command(version)]
pub struct Cli {
    /// Repository to inspect, in owner/repo form.
    pub repo: String,

    /// Absolute path from the repository root, without a leading slash.
    pub path: String,

    /// Maximum number of forks to inspect.
    #[arg(long)]
    pub limit: Option<usize>,

    /// Number of repositories scanned concurrently.
    #[arg(long, default_value_t = 8)]
    pub concurrency: usize,

    /// Maximum GitHub API requests per second across the process.
    #[arg(long, default_value_t = 4.0)]
    pub rate_limit: f64,

    /// Emit JSON instead of text.
    #[arg(long)]
    pub json: bool,

    /// Print progress and diagnostic information to stderr.
    #[arg(short, long)]
    pub verbose: bool,

    /// GitHub token. Falls back to the `GITHUB_TOKEN` environment variable.
    #[arg(long)]
    pub token: Option<String>,

    /// Include the source repository in addition to its forks.
    #[arg(long)]
    pub include_root: bool,
}

impl Cli {
    /// Validates the raw arguments and resolves environment fallbacks.
    pub fn into_parts(self) -> Result<(Target, Options)> {
        let target = Target::parse(&self.repo, &self.path)?;
        let token = self
            .token
            .or_else(|| env::var("GITHUB_TOKEN").ok())
            .filter(|token| !token.is_empty());

        Ok((
            target,
            Options {
                limit: self.limit,
                concurrency: self.concurrency,
                rate_limit: self.rate_limit,
                json: self.json,
                verbose: self.verbose,
                token,
                include_root: self.include_root,
            },
        ))
    }
}

/// A validated `owner/repo` pair and a normalized repository-relative path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub owner: String,
    pub repo: String,
    pub path: String,
}

impl Target {
    pub fn parse(repo: &str, path: &str) -> Result<Self> {
        let (owner, repo) = split_owner_repo(repo)?;
        Ok(Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            path: normalize_path(path)?,
        })
    }
}

/// Scan tuning options, independent of the command-line representation.
#[derive(Debug, Clone)]
pub struct Options {
    pub limit: Option<usize>,
    pub concurrency: usize,
    pub rate_limit: f64,
    pub json: bool,
    pub verbose: bool,
    pub token: Option<String>,
    pub include_root: bool,
}

/// Splits an `owner/repo` string, rejecting malformed input.
pub fn split_owner_repo(input: &str) -> Result<(&str, &str)> {
    let (owner, repo) = input
        .split_once('/')
        .ok_or_else(|| anyhow!("repository must be in owner/repo form"))?;
    if owner.is_empty() || repo.is_empty() {
        bail!("repository must be in owner/repo form");
    }
    Ok((owner, repo))
}

/// Normalizes and validates a repository-root-relative path.
pub fn normalize_path(input: &str) -> Result<String> {
    let path = input.trim();
    if path.is_empty() {
        bail!("path must not be empty");
    }
    if path.starts_with('/') {
        bail!("path must be repository-root-relative, without a leading slash");
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        bail!("path must be a normalized repository-root-relative path");
    }
    Ok(path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_owner_and_repo() {
        assert_eq!(
            split_owner_repo("rust-lang/rust").unwrap(),
            ("rust-lang", "rust")
        );
    }

    #[test]
    fn rejects_malformed_repository() {
        assert!(split_owner_repo("rust").is_err());
        assert!(split_owner_repo("/rust").is_err());
        assert!(split_owner_repo("rust/").is_err());
        assert!(split_owner_repo("").is_err());
    }

    #[test]
    fn normalizes_valid_paths() {
        assert_eq!(normalize_path(" flake.nix ").unwrap(), "flake.nix");
        assert_eq!(normalize_path("src/main.rs").unwrap(), "src/main.rs");
    }

    #[test]
    fn rejects_invalid_paths() {
        assert!(normalize_path("").is_err());
        assert!(normalize_path("/etc/passwd").is_err());
        assert!(normalize_path("a//b").is_err());
        assert!(normalize_path("a/./b").is_err());
        assert!(normalize_path("a/../b").is_err());
        assert!(normalize_path("a/").is_err());
    }

    #[test]
    fn parses_target() {
        let target = Target::parse("owner/repo", "src/lib.rs").unwrap();
        assert_eq!(target.owner, "owner");
        assert_eq!(target.repo, "repo");
        assert_eq!(target.path, "src/lib.rs");
    }
}
