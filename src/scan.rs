use std::collections::HashSet;

use anyhow::{Context, Result, anyhow};
use futures::{StreamExt, stream};
use octocrab::models::Repository;
use serde::Serialize;

use crate::cli::Target;
use crate::github::Github;

/// A repository branch that contains the requested path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoMatch {
    pub full_name: String,
    pub branch: String,
    pub html_url: String,
    pub path: String,
}

/// Aggregated result of a scan.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub matches: Vec<RepoMatch>,
    pub failures: Vec<String>,
}

impl ScanReport {
    /// True when every repository was scanned without an error.
    pub fn is_success(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Inputs required to perform a scan.
pub struct ScanOptions<'a> {
    pub github: &'a Github,
    pub target: &'a Target,
    pub limit: Option<usize>,
    pub concurrency: usize,
    pub include_root: bool,
    pub verbose: bool,
}

/// Scans forks (and optionally the source repository) for the target path.
pub async fn scan(options: ScanOptions<'_>) -> Result<ScanReport> {
    let ScanOptions {
        github,
        target,
        limit,
        concurrency,
        include_root,
        verbose,
    } = options;

    let concurrency = concurrency.max(1);
    let owner = target.owner.as_str();
    let repo = target.repo.as_str();

    if verbose {
        eprintln!("inspecting {owner}/{repo} for {}", target.path);
    }

    let origin_branches = github.list_branches(owner, repo).await?;
    let mut repositories = github.list_forks(owner, repo).await?;
    if include_root {
        repositories.push(github.repository(owner, repo).await?);
    }
    repositories.truncate(limit.unwrap_or(repositories.len()));

    if verbose {
        eprintln!(
            "found {} repositories to scan ({} source branches, concurrency {concurrency})",
            repositories.len(),
            origin_branches.len(),
        );
    }

    let results = stream::iter(repositories.into_iter().map(|repository| {
        let github = github.clone();
        let target = target.clone();
        let origin_branches = origin_branches.clone();
        async move {
            scan_repository(&github, repository, &target.path, &origin_branches, verbose).await
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>()
    .await;

    let mut report = ScanReport::default();
    for result in results {
        match result {
            Ok(matches) => report.matches.extend(matches),
            Err(err) => report.failures.push(err.to_string()),
        }
    }

    if verbose {
        eprintln!(
            "scan complete: {} matches, {} failures",
            report.matches.len(),
            report.failures.len()
        );
    }

    Ok(report)
}

async fn scan_repository(
    github: &Github,
    repository: Repository,
    target_path: &str,
    origin_branches: &HashSet<String>,
    verbose: bool,
) -> Result<Vec<RepoMatch>> {
    let full_name = repository
        .full_name
        .clone()
        .ok_or_else(|| anyhow!("repository is missing full_name"))?;
    let html_url = repository
        .html_url
        .clone()
        .map(|url| url.to_string())
        .unwrap_or_else(|| format!("https://github.com/{full_name}"));
    let default_branch = repository
        .default_branch
        .clone()
        .unwrap_or_else(|| "HEAD".to_string());
    let (owner, repo) = split_full_name(&full_name)?;

    if verbose {
        eprintln!("scanning {full_name}");
    }

    let fork_branches = github.list_branches(owner, repo).await?;
    let mut branches = vec![default_branch.clone()];
    branches.extend(
        fork_branches
            .into_iter()
            .filter(|branch| !origin_branches.contains(branch) && branch != &default_branch),
    );

    if verbose {
        eprintln!("  checking {} branches", branches.len());
    }

    let mut matches = Vec::new();
    for branch in branches {
        let exists = github
            .content_exists(owner, repo, &branch, target_path)
            .await
            .with_context(|| {
                format!("failed to fetch content for {full_name}:{branch}:{target_path}")
            })?;

        if verbose {
            eprintln!(
                "  {full_name}:{branch} -> {}",
                if exists { "found" } else { "missing" }
            );
        }

        if exists {
            matches.push(RepoMatch {
                full_name: full_name.clone(),
                branch,
                html_url: html_url.clone(),
                path: target_path.to_string(),
            });
        }
    }

    if verbose {
        eprintln!("finished {full_name}: {} matches", matches.len());
    }

    Ok(matches)
}

fn split_full_name(full_name: &str) -> Result<(&str, &str)> {
    full_name
        .split_once('/')
        .filter(|(owner, repo)| !owner.is_empty() && !repo.is_empty())
        .ok_or_else(|| anyhow!("repository {full_name} is not in owner/repo form"))
}
