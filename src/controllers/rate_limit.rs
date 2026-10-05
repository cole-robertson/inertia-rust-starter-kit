//! In-memory per-IP rate limiting for the credential endpoints, the equivalent of Rails 8's
//! `rate_limit to: 10, within: 3.minutes, only: :create, with: -> { redirect_to ..., alert:
//! "Try again later." }`.
//!
//! A token bucket per `(action, client IP)`: [`LIMIT`] tokens, refilled continuously at
//! `LIMIT` per [`WINDOW`]. Each request takes one token; with none left the request is
//! redirected back with the alert. State is per process — with several app servers each keeps
//! its own buckets (like Rails' default `:memory_store` cache).

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{extract::FromRequestParts, http::request::Parts, response::IntoResponse};
use loco_rs::app::AppContext;

use crate::inertia::redirect::Redirect;

pub const LIMIT: u32 = 10;
pub const WINDOW: Duration = Duration::from_secs(3 * 60);
pub const ALERT: &str = "Try again later.";

/// Buckets beyond this many trigger a sweep of the full (idle) ones.
const SWEEP_AT: usize = 10_000;

#[derive(Clone, Copy)]
struct Bucket {
    tokens: f64,
    updated: Instant,
}

/// The shared bucket table, one per app (stored in `ctx.shared_store`).
#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<(&'static str, IpAddr), Bucket>>,
}

impl RateLimiter {
    /// Take a token for `action` from `client`'s bucket. `false` when the bucket is empty.
    pub fn check(&self, action: &'static str, ip: IpAddr, now: Instant) -> bool {
        let rate = f64::from(LIMIT) / WINDOW.as_secs_f64();
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if buckets.len() >= SWEEP_AT {
            buckets.retain(|_, b| {
                b.tokens + now.duration_since(b.updated).as_secs_f64() * rate < f64::from(LIMIT)
            });
        }
        let bucket = buckets.entry((action, ip)).or_insert(Bucket {
            tokens: f64::from(LIMIT),
            updated: now,
        });
        let refill = now.duration_since(bucket.updated).as_secs_f64() * rate;
        bucket.tokens = (bucket.tokens + refill).min(f64::from(LIMIT));
        bucket.updated = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Install the app's limiter (called once while building routes).
pub fn install(ctx: &AppContext) {
    if !ctx.shared_store.contains::<Arc<RateLimiter>>() {
        ctx.shared_store.insert(Arc::new(RateLimiter::default()));
    }
}

/// Which endpoint a [`RateLimited`] extractor guards, and where to send the browser when there
/// is no usable Referer.
pub trait Action {
    const NAME: &'static str;
    const FALLBACK: &'static str;
    /// Whether a Precognition request spends a token too. Off for forms whose live
    /// validation checks nothing secret (sign-up); on where it would answer "is this the
    /// current password?", which would otherwise be a free guess.
    const PRECOGNITION_SPENDS: bool = false;
}

/// Extractor that spends a token; rejects with a redirect back + "Try again later.".
pub struct RateLimited<A>(std::marker::PhantomData<A>);

impl<A: Action> FromRequestParts<AppContext> for RateLimited<A> {
    type Rejection = axum::response::Response;

    async fn from_request_parts(
        parts: &mut Parts,
        ctx: &AppContext,
    ) -> Result<Self, Self::Rejection> {
        // Precognition (live form validation) never writes, sends mail or signs anyone in,
        // so it must not spend the attempts a real submission needs, unless it checks a
        // secret (`PRECOGNITION_SPENDS`). Endpoints that don't support it reject it earlier
        // via `NoPrecognition`.
        if !A::PRECOGNITION_SPENDS && crate::inertia::precognition::is_precognition(&parts.headers)
        {
            return Ok(Self(std::marker::PhantomData));
        }
        let Some(limiter) = ctx.shared_store.get::<Arc<RateLimiter>>() else {
            tracing::error!("rate limiter not installed; refusing credential request");
            return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response());
        };
        // Without a known client address every such request shares one bucket.
        let ip = crate::auth::client_ip(parts)
            .await
            .unwrap_or(IpAddr::from([0, 0, 0, 0]));
        if limiter.check(A::NAME, ip, Instant::now()) {
            Ok(Self(std::marker::PhantomData))
        } else {
            tracing::warn!(action = A::NAME, %ip, "rate limited");
            Err(Redirect::back(&parts.headers, A::FALLBACK)
                .alert(ALERT)
                .into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_allows_limit_then_refills_over_the_window() {
        let limiter = RateLimiter::default();
        let ip = IpAddr::from([10, 0, 0, 1]);
        let t0 = Instant::now();
        for _ in 0..LIMIT {
            assert!(limiter.check("a", ip, t0));
        }
        assert!(!limiter.check("a", ip, t0));
        // Other actions and other IPs have their own buckets.
        assert!(limiter.check("b", ip, t0));
        assert!(limiter.check("a", IpAddr::from([10, 0, 0, 2]), t0));
        // One token comes back after WINDOW / LIMIT.
        let later = t0 + WINDOW / LIMIT;
        assert!(limiter.check("a", ip, later));
        assert!(!limiter.check("a", ip, later));
        // A full window refills the bucket, capped at LIMIT.
        let much_later = later + WINDOW * 5;
        for _ in 0..LIMIT {
            assert!(limiter.check("a", ip, much_later));
        }
        assert!(!limiter.check("a", ip, much_later));
    }
}
