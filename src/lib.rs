//! `forkseer` checks whether a path exists in the forks of a GitHub repository.
//!
//! The crate is split into small pieces so the command-line surface, the
//! GitHub client, the scan orchestration and the output rendering can be
//! reasoned about (and tested) independently.

pub mod cli;
pub mod github;
pub mod output;
pub mod rate_limit;
pub mod scan;

use anyhow::Result;

use crate::cli::{Cli, Options, Target};
use crate::github::Github;
use crate::rate_limit::RateLimiter;
use crate::scan::{ScanOptions, ScanReport};

/// Runs a full scan for a parsed command line.
pub async fn run(cli: Cli) -> Result<ScanReport> {
    let (target, options) = cli.into_parts()?;
    scan_target(&target, &options).await
}

/// Runs a scan for an already validated target and options.
pub async fn scan_target(target: &Target, options: &Options) -> Result<ScanReport> {
    let limiter = RateLimiter::per_second(options.rate_limit)?;
    let github = Github::new(options.token.as_deref(), limiter)?;

    scan::scan(ScanOptions {
        github: &github,
        target,
        limit: options.limit,
        concurrency: options.concurrency,
        include_root: options.include_root,
        verbose: options.verbose,
    })
    .await
}
