//! Per-tenant token-bucket rate limiting (dependency-free).
//!
//! Each tenant gets a bucket that refills at `rate_per_sec` up to `burst`. A
//! request consumes one token; if the bucket is empty the request is rejected
//! (HTTP 429). Keying on the *authenticated* tenant means one noisy tenant can't
//! starve others. Brief `std::sync::Mutex` critical sections only (no `.await`
//! held), so it's safe on the async path.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

pub struct RateLimiter {
    rate_per_sec: f64,
    burst: f64,
    buckets: Mutex<HashMap<u64, Bucket>>,
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    pub fn new(rate_per_sec: f64, burst: f64) -> Self {
        Self {
            rate_per_sec,
            burst: burst.max(1.0),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Returns true if the request is allowed (and consumes a token).
    pub fn allow(&self, tenant: u64) -> bool {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let bucket = buckets.entry(tenant).or_insert(Bucket {
            tokens: self.burst,
            last: now,
        });
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.last = now;
        bucket.tokens = (bucket.tokens + elapsed * self.rate_per_sec).min(self.burst);
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_then_throttle_then_refill() {
        // 10/sec sustained, burst of 3.
        let rl = RateLimiter::new(10.0, 3.0);
        assert!(rl.allow(1)); // 3 -> 2
        assert!(rl.allow(1)); // 2 -> 1
        assert!(rl.allow(1)); // 1 -> 0
        assert!(!rl.allow(1), "4th immediate request is throttled");
        // A different tenant is unaffected.
        assert!(rl.allow(2));
        // After ~150ms, ~1.5 tokens refilled.
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(rl.allow(1), "token refilled");
    }
}
