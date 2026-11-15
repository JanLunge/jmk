use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;
use crate::SharedState;

/// Above this many tracked clients, expired entries are dropped before a new
/// one is added, which keeps the table from growing without bound.
const PRUNE_ABOVE: usize = 10_000;

struct Window {
    started: Instant,
    count: u32,
}

/// Fixed-window counter per client address.
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    clients: Mutex<HashMap<IpAddr, Window>>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self { limit, window, clients: Mutex::new(HashMap::new()) }
    }

    /// Counts one request made at `now`. When the client is over its limit,
    /// returns how long until its window ends.
    fn count(&self, client: IpAddr, now: Instant) -> Result<(), Duration> {
        let mut clients = self.clients.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if clients.len() > PRUNE_ABOVE {
            clients.retain(|_, window| now.duration_since(window.started) < self.window);
        }
        let window = clients.entry(client).or_insert(Window { started: now, count: 0 });
        let elapsed = now.duration_since(window.started);
        if elapsed >= self.window {
            *window = Window { started: now, count: 0 };
        } else if window.count >= self.limit {
            return Err(self.window - elapsed);
        }
        window.count += 1;
        Ok(())
    }
}

/// Middleware: limits every request that is not a read.
pub async fn limit_writes(State(state): State<SharedState>, request: Request, next: Next) -> Response {
    if matches!(*request.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(request).await;
    }
    let client = limiter_key(client_address(&request, state.config.trust_forwarded_for));
    match state.rate_limiter.count(client, Instant::now()) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            let mut response = ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many write requests from this address; try again later",
            )
            .into_response();
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(retry_after.as_secs().max(1)));
            response
        }
    }
}

fn client_address(request: &Request, trust_forwarded_for: bool) -> IpAddr {
    let forwarded = trust_forwarded_for
        .then(|| request.headers().get("x-forwarded-for"))
        .flatten()
        .and_then(|value| value.to_str().ok())
        // The last entry is the one the nearest proxy added; earlier ones
        // are whatever the client chose to send.
        .and_then(|value| value.rsplit(',').next())
        .and_then(|entry| entry.trim().parse().ok());
    forwarded
        .or_else(|| {
            request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(address)| address.ip())
        })
        // Without a peer address all such requests share one bucket.
        .unwrap_or(IpAddr::V6(Ipv6Addr::UNSPECIFIED))
}

/// An IPv6 client usually controls a whole /64, so that is what gets limited.
fn limiter_key(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V4(_) => address,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(Ipv6Addr::from(u128::from(v6) & !u128::from(u64::MAX))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_window_starts_after_the_old_one_ends() {
        let limiter = RateLimiter::new(2, Duration::from_secs(600));
        let client = IpAddr::from([203, 0, 113, 7]);
        let start = Instant::now();
        assert_eq!(limiter.count(client, start), Ok(()));
        assert_eq!(limiter.count(client, start + Duration::from_secs(100)), Ok(()));
        assert_eq!(
            limiter.count(client, start + Duration::from_secs(599)),
            Err(Duration::from_secs(1))
        );
        assert_eq!(limiter.count(client, start + Duration::from_secs(600)), Ok(()));
        assert_eq!(limiter.count(client, start + Duration::from_secs(601)), Ok(()));
        assert!(limiter.count(client, start + Duration::from_secs(602)).is_err());
    }

    #[test]
    fn ipv6_clients_are_limited_by_their_64_bit_prefix() {
        let key = |address: &str| limiter_key(address.parse().unwrap());
        assert_eq!(key("2001:db8:1:2:aaaa::1"), key("2001:db8:1:2:bbbb::2"));
        assert_ne!(key("2001:db8:1:2::1"), key("2001:db8:1:3::1"));
        assert_eq!(key("::ffff:203.0.113.7"), key("203.0.113.7"));
    }
}
