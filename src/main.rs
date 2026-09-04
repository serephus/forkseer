use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use futures::{StreamExt, stream};
use octocrab::{
    Error, Octocrab,
    models::{Repository, repos::Branch},
};
use palc::Parser;
use serde::Serialize;
use tokio::{sync::Mutex, time::Instant};

#[derive(Parser, Debug)]
#[command(version)]
struct Cli {
    /// Repository to inspect, in owner/repo form.
    repo: String,

    /// Absolute path from the repository root, without a leading slash.
    path: String,

    /// Maximum number of forks to inspect.
    #[arg(long)]
    limit: Option<usize>,

    /// Number of repositories scanned concurrently.
    #[arg(long, default_value_t = 8)]
    concurrency: usize,

    /// Maximum GitHub API requests per second across the process.
    #[arg(long, default_value_t = 4.0)]
    rate_limit: f64,

    /// Emit JSON instead of text.
    #[arg(long)]
    json: bool,

    /// Print progress and diagnostic information to stderr.
    #[arg(short, long)]
    verbose: bool,

    /// GitHub token.
    #[arg(long)]
    token: Option<String>,

    /// Include the source repository in addition to its forks.
    #[arg(long)]
    include_root: bool,
}

#[derive(Debug, Serialize)]
struct RepoMatch {
    full_name: String,
    branch: String,
    html_url: String,
    path: String,
}

#[derive(Clone)]
struct RateLimiter {
    state: Arc<Mutex<Instant>>,
    interval: Duration,
}

impl RateLimiter {
    fn new(requests_per_second: f64) -> Result<Self> {
        if !requests_per_second.is_finite() || requests_per_second <= 0.0 {
            bail!("--rate-limit must be greater than 0");
        }

        Ok(Self {
            state: Arc::new(Mutex::new(Instant::now())),
            interval: Duration::from_secs_f64(1.0 / requests_per_second),
        })
    }

    async fn acquire(&self) {
        let mut next_allowed = self.state.lock().await;
        let now = Instant::now();
        let scheduled = (*next_allowed).max(now);
        *next_allowed = scheduled + self.interval;
        drop(next_allowed);

        let delay = scheduled.saturating_duration_since(now);
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let target_path = normalize_repo_path(&cli.path)?;
    let limiter = RateLimiter::new(cli.rate_limit)?;
    let (owner, repo) = parse_repo(&cli.repo)?;

    if cli.verbose {
        eprintln!("inspecting {}/{} for {}", owner, repo, target_path);
    }

    let mut builder = Octocrab::builder();
    if let Some(token) = cli.token.as_deref() {
        builder = builder.personal_token(token.to_owned());
    }
    let crab = builder.build()?;

    limiter.acquire().await;
    let root = crab
        .repos(owner, repo)
        .get()
        .await
        .with_context(|| format!("failed to fetch repository metadata for {}", cli.repo))?;

    let origin_branches = list_branches(&crab, &limiter, owner, repo).await?;
    let mut repos = list_forks(&crab, &limiter, owner, repo).await?;
    if cli.include_root {
        repos.push(root);
    }

    let limit = cli.limit.unwrap_or(repos.len());
    repos.truncate(limit);

    if cli.verbose {
        eprintln!(
            "found {} repositories to scan ({} source branches, concurrency {})",
            repos.len(),
            origin_branches.len(),
            cli.concurrency.max(1)
        );
    }

    let concurrency = cli.concurrency.max(1);
    let verbose = cli.verbose;
    let results = stream::iter(repos.into_iter().map(|repository| {
        let crab = crab.clone();
        let limiter = limiter.clone();
        let target_path = target_path.clone();
        let origin_branches = origin_branches.clone();
        async move {
            scan_repository(
                &crab,
                &limiter,
                repository,
                &target_path,
                &origin_branches,
                verbose,
            )
            .await
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>()
    .await;

    let mut matched = Vec::new();
    let mut failures = Vec::new();

    for result in results {
        match result {
            Ok(repo_matches) => matched.extend(repo_matches),
            Err(err) => failures.push(err.to_string()),
        }
    }

    if cli.verbose {
        eprintln!(
            "scan complete: {} matches, {} failures",
            matched.len(),
            failures.len()
        );
    }

    render_output(cli.json, &matched, &failures)?;

    if failures.is_empty() {
        Ok(())
    } else {
        bail!("completed with {} repository scan failures", failures.len())
    }
}

fn parse_repo(input: &str) -> Result<(&str, &str)> {
    let (owner, repo) = input
        .split_once('/')
        .ok_or_else(|| anyhow!("repository must be in owner/repo form"))?;
    if owner.is_empty() || repo.is_empty() {
        bail!("repository must be in owner/repo form");
    }
    Ok((owner, repo))
}

fn normalize_repo_path(input: &str) -> Result<String> {
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

async fn list_forks(
    crab: &Octocrab,
    limiter: &RateLimiter,
    owner: &str,
    repo: &str,
) -> Result<Vec<Repository>> {
    limiter.acquire().await;
    let mut page = crab
        .repos(owner, repo)
        .list_forks()
        .per_page(100)
        .send()
        .await
        .with_context(|| format!("failed to list forks for {owner}/{repo}"))?;

    let mut forks = page.take_items();
    while page.next.is_some() {
        limiter.acquire().await;
        let next_page = crab.get_page::<Repository>(&page.next).await?;
        match next_page {
            Some(next_page) => {
                page = next_page;
                forks.extend(page.take_items());
            }
            None => break,
        }
    }

    Ok(forks)
}

async fn list_branches(
    crab: &Octocrab,
    limiter: &RateLimiter,
    owner: &str,
    repo: &str,
) -> Result<std::collections::HashSet<String>> {
    limiter.acquire().await;
    let mut page = crab
        .repos(owner, repo)
        .list_branches()
        .per_page(100)
        .send()
        .await
        .with_context(|| format!("failed to list branches for {owner}/{repo}"))?;

    let mut branches = std::collections::HashSet::new();
    for branch in page.take_items() {
        branches.insert(branch.name);
    }

    while page.next.is_some() {
        limiter.acquire().await;
        let next_page = crab.get_page::<Branch>(&page.next).await?;
        match next_page {
            Some(mut next_page) => {
                for branch in next_page.take_items() {
                    branches.insert(branch.name);
                }
                page = next_page;
            }
            None => break,
        }
    }

    Ok(branches)
}

async fn scan_repository(
    crab: &Octocrab,
    limiter: &RateLimiter,
    repository: Repository,
    target_path: &str,
    origin_branches: &std::collections::HashSet<String>,
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
    let (owner, repo) = parse_repo(&full_name)?;

    if verbose {
        eprintln!("scanning {full_name}");
    }

    let fork_branches = list_branches(crab, limiter, owner, repo).await?;
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
        let exists = content_exists(crab, limiter, owner, repo, &branch, target_path)
            .await
            .with_context(|| {
                format!("failed to fetch content for {full_name}:{branch}:{target_path}")
            })?;
        if verbose {
            eprintln!(
                "  {}:{} -> {}",
                full_name,
                branch,
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

async fn content_exists(
    crab: &Octocrab,
    limiter: &RateLimiter,
    owner: &str,
    repo: &str,
    branch: &str,
    target_path: &str,
) -> Result<bool> {
    limiter.acquire().await;
    let repo_handler = crab.repos(owner, repo);
    let request = repo_handler.get_content().path(target_path).r#ref(branch);

    match request.send().await {
        Ok(items) => Ok(!items.items.is_empty()),
        Err(Error::GitHub { source, .. }) if source.status_code.as_u16() == 404 => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn render_output(json: bool, matched: &[RepoMatch], failures: &[String]) -> Result<()> {
    if json {
        #[derive(Serialize)]
        struct JsonOutput<'a> {
            matches: &'a [RepoMatch],
            failures: &'a [String],
        }

        println!(
            "{}",
            serde_json::to_string_pretty(&JsonOutput {
                matches: matched,
                failures,
            })?
        );
    } else {
        for repo_match in matched {
            println!("{} [{}]", repo_match.full_name, repo_match.branch);
            println!("  {}", repo_match.html_url);
            println!("  - {}", repo_match.path);
        }

        if !failures.is_empty() {
            eprintln!("failures:");
            for failure in failures {
                eprintln!("  - {failure}");
            }
        }
    }

    Ok(())
}
