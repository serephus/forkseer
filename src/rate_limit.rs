use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use tokio::sync::Mutex;
use tokio::time::Instant;

/// A cloneable token-bucket style limiter shared across all requests.
///
/// Each clone shares the same schedule, so the configured rate applies to the
/// whole process rather than per task.
#[derive(Clone)]
pub struct RateLimiter {
    next_allowed: Arc<Mutex<Instant>>,
    interval: Duration,
}

impl RateLimiter {
    /// Creates a limiter that allows `requests_per_second` evenly spaced calls.
    pub fn per_second(requests_per_second: f64) -> Result<Self> {
        if !requests_per_second.is_finite() || requests_per_second <= 0.0 {
            bail!("--rate-limit must be a finite number greater than 0");
        }

        Ok(Self {
            next_allowed: Arc::new(Mutex::new(Instant::now())),
            interval: Duration::from_secs_f64(1.0 / requests_per_second),
        })
    }

    /// Waits until the next request may be issued.
    pub async fn acquire(&self) {
        let mut next_allowed = self.next_allowed.lock().await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_positive_rate() {
        assert!(RateLimiter::per_second(4.0).is_ok());
        assert!(RateLimiter::per_second(0.5).is_ok());
    }

    #[test]
    fn rejects_non_positive_or_non_finite_rates() {
        assert!(RateLimiter::per_second(0.0).is_err());
        assert!(RateLimiter::per_second(-1.0).is_err());
        assert!(RateLimiter::per_second(f64::NAN).is_err());
        assert!(RateLimiter::per_second(f64::INFINITY).is_err());
    }
}
