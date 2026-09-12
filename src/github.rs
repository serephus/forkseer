use std::collections::HashSet;

use anyhow::{Context, Result};
use octocrab::models::{Repository, repos::Branch};
use octocrab::{Error, Octocrab, Page};
use serde::de::DeserializeOwned;

use crate::rate_limit::RateLimiter;

/// A cloneable GitHub API client that applies rate limiting to every request.
#[derive(Clone)]
pub struct Github {
    client: Octocrab,
    limiter: RateLimiter,
}

impl Github {
    pub fn new(token: Option<&str>, limiter: RateLimiter) -> Result<Self> {
        let mut builder = Octocrab::builder();
        if let Some(token) = token {
            builder = builder.personal_token(token.to_owned());
        }

        Ok(Self {
            client: builder.build()?,
            limiter,
        })
    }

    /// Fetches repository metadata.
    pub async fn repository(&self, owner: &str, repo: &str) -> Result<Repository> {
        self.limiter.acquire().await;
        self.client
            .repos(owner, repo)
            .get()
            .await
            .with_context(|| format!("failed to fetch repository metadata for {owner}/{repo}"))
    }

    /// Lists every fork of a repository, following pagination.
    pub async fn list_forks(&self, owner: &str, repo: &str) -> Result<Vec<Repository>> {
        self.limiter.acquire().await;
        let mut page = Some(
            self.client
                .repos(owner, repo)
                .list_forks()
                .per_page(100)
                .send()
                .await
                .with_context(|| format!("failed to list forks for {owner}/{repo}"))?,
        );

        let mut forks = Vec::new();
        while let Some(mut current) = page {
            forks.extend(current.take_items());
            page = self.next_page(&current).await?;
        }

        Ok(forks)
    }

    /// Lists the branch names of a repository, following pagination.
    pub async fn list_branches(&self, owner: &str, repo: &str) -> Result<HashSet<String>> {
        self.limiter.acquire().await;
        let mut page = Some(
            self.client
                .repos(owner, repo)
                .list_branches()
                .per_page(100)
                .send()
                .await
                .with_context(|| format!("failed to list branches for {owner}/{repo}"))?,
        );

        let mut branches = HashSet::new();
        while let Some(mut current) = page {
            branches.extend(
                current
                    .take_items()
                    .into_iter()
                    .map(|branch: Branch| branch.name),
            );
            page = self.next_page(&current).await?;
        }

        Ok(branches)
    }

    /// Returns whether `path` exists on `branch`. A `404` is reported as
    /// `false`; any other error is propagated.
    pub async fn content_exists(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        path: &str,
    ) -> Result<bool> {
        self.limiter.acquire().await;
        let repo_handler = self.client.repos(owner, repo);
        let request = repo_handler.get_content().path(path).r#ref(branch);

        match request.send().await {
            Ok(items) => Ok(!items.items.is_empty()),
            Err(Error::GitHub { source, .. }) if source.status_code.as_u16() == 404 => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Fetches the page after `current`, if any, applying the rate limiter.
    async fn next_page<T: DeserializeOwned>(&self, current: &Page<T>) -> Result<Option<Page<T>>> {
        if current.next.is_none() {
            return Ok(None);
        }

        self.limiter.acquire().await;
        Ok(self.client.get_page(&current.next).await?)
    }
}
