//! Token-bucket rate limiter for namespace-level frequency limiting
//!
//! Mirrors the Haskell `FrequencyLimiter` / `ConfLimiter` feature.

use std::time::Instant;

/// A token-bucket rate limiter for a single namespace
///
/// `max_tokens` = burst capacity (= `refill_rate`, i.e., one second's worth).
/// `refill_rate` = tokens added per second = configured messages-per-second limit.
#[derive(Debug)]
pub struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    /// Create a new token bucket.
    ///
    /// `max_freq` is the maximum number of messages per second.
    pub fn new(max_freq: f64) -> Self {
        Self {
            tokens: max_freq,
            max_tokens: max_freq,
            refill_rate: max_freq,
            last_refill: Instant::now(),
        }
    }

    /// Attempt to consume one token.
    ///
    /// Returns `true` if the message should be allowed through, `false` if it
    /// should be dropped.
    pub fn try_acquire(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_basic() {
        let mut bucket = TokenBucket::new(2.0);

        // First two messages allowed (starting with full bucket)
        assert!(bucket.try_acquire());
        assert!(bucket.try_acquire());
        // Third message blocked
        assert!(!bucket.try_acquire());
    }

    #[test]
    fn test_token_bucket_refills() {
        let mut bucket = TokenBucket::new(100.0);
        // Drain
        for _ in 0..100 {
            bucket.try_acquire();
        }
        assert!(!bucket.try_acquire());

        // Simulate 1 second passing by directly advancing last_refill
        bucket.last_refill -= std::time::Duration::from_secs(1);
        // Should refill ~100 tokens
        assert!(bucket.try_acquire());
    }
}
