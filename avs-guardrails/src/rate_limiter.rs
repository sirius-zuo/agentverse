// avs-guardrails/src/rate_limiter.rs
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::GuardrailError;

/// Per-user rate limiter.
pub struct RateLimiter {
    limits: Mutex<HashMap<String, RateLimitState>>,
    default_max_requests: usize,
    default_window_seconds: u64,
}

struct RateLimitState {
    requests: Vec<Instant>,
}

impl RateLimiter {
    pub fn new(default_max_requests: usize, default_window_seconds: u64) -> Self {
        Self {
            limits: Mutex::new(HashMap::new()),
            default_max_requests,
            default_window_seconds,
        }
    }

    /// Check if a user is within their rate limit.
    pub fn check(&self, user_id: &str) -> Result<(), GuardrailError> {
        let mut limits = self.limits.lock().unwrap();
        let state = limits
            .entry(user_id.to_string())
            .or_insert_with(|| RateLimitState {
                requests: Vec::new(),
            });

        let now = Instant::now();
        let window = std::time::Duration::from_secs(self.default_window_seconds);

        // Remove old requests outside the window
        state.requests.retain(|t| now.duration_since(*t) < window);

        if state.requests.len() >= self.default_max_requests {
            return Err(GuardrailError::RateLimited(format!(
                "User {} exceeded rate limit: {} requests per {}s",
                user_id, self.default_max_requests, self.default_window_seconds
            )));
        }

        state.requests.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_within_limit() {
        let limiter = RateLimiter::new(5, 60);
        for _ in 0..5 {
            assert!(limiter.check("user1").is_ok());
        }
    }

    #[test]
    fn test_exceed_limit() {
        let limiter = RateLimiter::new(3, 60);
        assert!(limiter.check("user1").is_ok());
        assert!(limiter.check("user1").is_ok());
        assert!(limiter.check("user1").is_ok());
        assert!(limiter.check("user1").is_err());
    }

    #[test]
    fn test_independent_users() {
        let limiter = RateLimiter::new(2, 60);
        assert!(limiter.check("user1").is_ok());
        assert!(limiter.check("user1").is_ok());
        assert!(limiter.check("user1").is_err());

        // user2 should still be within limit
        assert!(limiter.check("user2").is_ok());
    }
}
