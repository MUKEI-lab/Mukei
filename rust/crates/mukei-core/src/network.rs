//! Network client construction policy.
//!
//! Mobile network calls should not create ad-hoc `reqwest::Client`
//! instances with implicit timeout and redirect behavior. This module
//! centralizes the baseline HTTP policy while still letting callers keep
//! domain-specific total time budgets.

use std::time::Duration;

use crate::error::{MukeiError, Result};

/// Production-oriented SaaS cloud transport boundary.
pub mod saas;

pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_MAX_REDIRECTS: usize = 3;
pub const MODEL_DOWNLOAD_TOTAL_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NetworkClientPolicy {
    pub user_agent: &'static str,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub total_timeout: Duration,
    pub max_redirects: usize,
}

impl NetworkClientPolicy {
    pub fn model_download() -> Self {
        Self {
            user_agent: concat!("mukei-bridge/", env!("CARGO_PKG_VERSION")),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            read_timeout: DEFAULT_READ_TIMEOUT,
            total_timeout: MODEL_DOWNLOAD_TOTAL_TIMEOUT,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }

    pub fn search(total_timeout: Duration) -> Self {
        Self {
            user_agent: concat!("mukei-core/", env!("CARGO_PKG_VERSION")),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT.min(total_timeout),
            read_timeout: DEFAULT_READ_TIMEOUT.min(total_timeout),
            total_timeout,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }
}

pub fn build_network_client(policy: NetworkClientPolicy) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(policy.user_agent)
        .connect_timeout(policy.connect_timeout)
        .read_timeout(policy.read_timeout)
        .timeout(policy.total_timeout)
        .redirect(reqwest::redirect::Policy::limited(policy.max_redirects))
        .build()
        .map_err(|e| MukeiError::HttpClientFailed(e.to_string()))
}

pub fn map_reqwest_error(operation: &str, error: reqwest::Error) -> MukeiError {
    let operation = crate::diagnostics::sanitize_log_value(operation);
    if error.is_timeout() {
        return MukeiError::NetworkTimeout { operation };
    }
    if let Some(status) = error.status() {
        return http_status_error(&operation, status);
    }
    if error.is_decode() {
        return MukeiError::NetworkInvalidResponse { operation };
    }
    let lower = error.to_string().to_ascii_lowercase();
    if lower.contains("tls") || lower.contains("certificate") || lower.contains("cert") {
        return MukeiError::NetworkTls { operation };
    }
    if error.is_connect()
        || lower.contains("dns")
        || lower.contains("resolve")
        || lower.contains("connection")
    {
        return MukeiError::NetworkUnavailable { operation };
    }
    MukeiError::NetworkError(format!(
        "{operation}: {}",
        crate::diagnostics::sanitize_error_message(error.to_string())
    ))
}

pub fn http_status_error(operation: &str, status: reqwest::StatusCode) -> MukeiError {
    let operation = crate::diagnostics::sanitize_log_value(operation);
    match status.as_u16() {
        408 => MukeiError::NetworkTimeout { operation },
        429 => MukeiError::NetworkRateLimited { operation },
        500..=599 => MukeiError::NetworkServerError {
            status: status.as_u16(),
            operation,
        },
        _ => MukeiError::NetworkError(format!("HTTP {status} during {operation}")),
    }
}

/// Send an idempotent HTTP request with bounded exponential backoff.
///
/// The request builder is rebuilt for every attempt because reqwest
/// request bodies are consumed by `send`. `accept_status` lets callers
/// preserve protocol-specific statuses such as HTTP 416 for resumable
/// downloads while still retrying 408/429/5xx responses.
pub async fn send_request_with_retry<F, A>(
    operation: &str,
    policy: RetryPolicy,
    build_request: F,
    accept_status: A,
) -> Result<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
    A: Fn(reqwest::StatusCode) -> bool,
{
    send_request_with_retry_inner(operation, policy, build_request, accept_status, None).await
}

/// Cancellable variant used by long-running mobile downloads. A user
/// cancellation interrupts both an in-flight request and backoff sleep.
pub async fn send_request_with_retry_cancellable<F, A>(
    operation: &str,
    policy: RetryPolicy,
    build_request: F,
    accept_status: A,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
    A: Fn(reqwest::StatusCode) -> bool,
{
    send_request_with_retry_inner(
        operation,
        policy,
        build_request,
        accept_status,
        Some(cancel),
    )
    .await
}

async fn send_request_with_retry_inner<F, A>(
    operation: &str,
    policy: RetryPolicy,
    mut build_request: F,
    accept_status: A,
    cancel: Option<&tokio_util::sync::CancellationToken>,
) -> Result<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
    A: Fn(reqwest::StatusCode) -> bool,
{
    let mut retries_completed = 0u32;
    loop {
        let send = build_request().send();
        let outcome = if let Some(cancel) = cancel {
            tokio::select! {
                _ = cancel.cancelled() => return Err(MukeiError::Cancelled),
                response = send => response,
            }
        } else {
            send.await
        };

        let result = match outcome {
            Ok(response) if accept_status(response.status()) => Ok(response),
            Ok(response) => Err(http_status_error(operation, response.status())),
            Err(error) => Err(map_reqwest_error(operation, error)),
        };

        match result {
            Ok(response) => return Ok(response),
            Err(error)
                if retries_completed < policy.max_attempts && RetryPolicy::is_retryable(&error) =>
            {
                retries_completed = retries_completed.saturating_add(1);
                let delay = policy.next_delay(retries_completed);
                tracing::warn!(
                    operation = %crate::diagnostics::sanitize_log_value(operation),
                    attempt = retries_completed,
                    delay_ms = delay.as_millis(),
                    error_code = error.error_code(),
                    "retrying transient network request"
                );
                if let Some(cancel) = cancel {
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(MukeiError::Cancelled),
                        _ = tokio::time::sleep(delay) => {},
                    }
                } else {
                    tokio::time::sleep(delay).await;
                }
            }
            Err(error) => return Err(error),
        }
    }
}

// =====================================================================
// RetryPolicy — v0.8 review followup (issue #4: "Full retry/backoff/jitter
// helper missing").
//
// Design constraints:
//   * Pure data — no async, no clock, no side effects. Callers drive
//     the loop using `next_delay(attempt)` and `is_retryable(err)`.
//   * Backoff must be capped so a flap loop cannot burn an unbounded
//     amount of wall-clock time.
//   * Jitter must be bounded and decentralised enough that two
//     coordinated clients do not synchronise their storms. Full-jitter
//     exponential backoff per AWS' "exponential backoff and jitter".
//   * Decision logic for "can we retry this error?" must consult the
//     existing typed error taxonomy (rate-limit → yes, terminal auth
//     → no). The v0.8 review flagged "Retryable vs fatal behavior
//     not consistently modeled" — this is the single place that
//     classifies that for the network module.
// =====================================================================

/// Maximum number of retry attempts after the initial try.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::web_search()
    }
}

impl RetryPolicy {
    /// Conservative policy for web search engines. 3 retries with
    /// 500 ms base — short enough that a flaky cell tower does not
    /// cascade into visible UI lag.
    pub const fn web_search() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
        }
    }

    /// Patient policy for model downloads. 4 retries with 2 s base
    /// and a 30 s ceiling so a transient 502 can recover before we
    /// give up on a multi-gigabyte download.
    pub const fn model_download() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(30),
        }
    }

    /// Compute the delay BEFORE attempt `n` (1-indexed). Uses
    /// full-jitter exponential backoff capped at `max_delay`.
    /// `attempt == 0` collapses to 0.
    pub fn next_delay(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let shift = attempt.saturating_sub(1).min(20);
        let factor: u32 = 1u32.checked_shl(shift).unwrap_or(u32::MAX);
        let candidate = self
            .base_delay
            .checked_mul(factor)
            .unwrap_or(self.max_delay);
        let capped = candidate.min(self.max_delay);
        let nanos = capped.as_nanos().min(u64::MAX as u128) as u64;
        if nanos == 0 {
            return Duration::ZERO;
        }
        // Full-jitter: random in [0, capped]. Deterministic-quality
        // splitmix64 step — uniform-in-range is sufficient for de-
        // syncing retries and avoids the rand crate dependency here.
        let mut state = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0)
            ^ attempt as u64
            ^ nanos;
        state = state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        Duration::from_nanos(z % (nanos + 1))
    }

    /// Map a typed network error to a retryable Yes/No answer.
    /// Conservative by design: anything we are unsure about returns
    /// `false`. The caller can still re-classify manually when it has
    /// more context (e.g. offline / quota / maintenance screens).
    pub fn is_retryable(err: &MukeiError) -> bool {
        use MukeiError::*;
        match err {
            NetworkTimeout { .. }
            | NetworkUnavailable { .. }
            | NetworkTls { .. }
            | NetworkRateLimited { .. }
            | NetworkServerError { .. }
            | NetworkInvalidResponse { .. } => true,
            NetworkError(_) => false,
            _ => false,
        }
    }

    /// Convenience: returns `Ok(())` if the error is retryable and
    /// attempts remain, `Err(clone)` otherwise.
    pub fn check_attempt(&self, attempt: u32, err: &MukeiError) -> Result<()> {
        if attempt >= self.max_attempts || !Self::is_retryable(err) {
            return Err(clone_network_error(err));
        }
        Ok(())
    }
}

fn clone_network_error(err: &MukeiError) -> MukeiError {
    use MukeiError::*;
    match err {
        NetworkError(s) => NetworkError(s.clone()),
        NetworkTimeout { operation } => NetworkTimeout {
            operation: operation.clone(),
        },
        NetworkUnavailable { operation } => NetworkUnavailable {
            operation: operation.clone(),
        },
        NetworkTls { operation } => NetworkTls {
            operation: operation.clone(),
        },
        NetworkInvalidResponse { operation } => NetworkInvalidResponse {
            operation: operation.clone(),
        },
        NetworkRateLimited { operation } => NetworkRateLimited {
            operation: operation.clone(),
        },
        NetworkServerError { status, operation } => NetworkServerError {
            status: *status,
            operation: operation.clone(),
        },
        other => {
            MukeiError::NetworkError(crate::diagnostics::sanitize_log_value(other.error_code()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_download_policy_has_mobile_timeouts() {
        let policy = NetworkClientPolicy::model_download();
        assert_eq!(policy.connect_timeout, DEFAULT_CONNECT_TIMEOUT);
        assert_eq!(policy.read_timeout, DEFAULT_READ_TIMEOUT);
        assert_eq!(policy.total_timeout, MODEL_DOWNLOAD_TOTAL_TIMEOUT);
        assert_eq!(policy.max_redirects, DEFAULT_MAX_REDIRECTS);
        assert!(policy.user_agent.starts_with("mukei-bridge/"));
    }

    #[test]
    fn search_policy_preserves_domain_total_budget() {
        let policy = NetworkClientPolicy::search(Duration::from_secs(3));
        assert_eq!(policy.total_timeout, Duration::from_secs(3));
        assert_eq!(policy.connect_timeout, Duration::from_secs(3));
        assert_eq!(policy.read_timeout, Duration::from_secs(3));
        assert!(policy.user_agent.starts_with("mukei-core/"));
    }

    #[test]
    fn http_status_mapping_is_typed_and_redacted() {
        let rate_limited = http_status_error(
            "/sdcard/private/model.gguf",
            reqwest::StatusCode::TOO_MANY_REQUESTS,
        );
        assert_eq!(rate_limited.error_code(), "ERR_NETWORK_RATE_LIMITED");
        assert!(rate_limited.to_string().contains("[redacted-path]"));

        let server = http_status_error("model download", reqwest::StatusCode::BAD_GATEWAY);
        assert_eq!(server.error_code(), "ERR_NETWORK_SERVER");
    }

    // ---- RetryPolicy tests (v0.8 review followup, issue #4) ----

    #[test]
    fn retry_policy_default_is_web_search_shaped() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_attempts, RetryPolicy::web_search().max_attempts);
        assert_eq!(p.base_delay, RetryPolicy::web_search().base_delay);
    }

    #[test]
    fn retry_policy_zero_attempt_is_zero_delay() {
        let p = RetryPolicy::model_download();
        assert_eq!(p.next_delay(0), Duration::ZERO);
    }

    #[test]
    fn retry_policy_caps_at_max_delay() {
        let p = RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(4),
        };
        // attempt=6 would want 32s — must cap at 4s and jitter within it.
        let d = p.next_delay(6);
        assert!(d <= p.max_delay);
    }

    #[test]
    fn retry_policy_classifies_typed_errors() {
        assert!(RetryPolicy::is_retryable(&MukeiError::NetworkTimeout {
            operation: "dl".into()
        }));
        assert!(RetryPolicy::is_retryable(&MukeiError::NetworkRateLimited {
            operation: "search".into()
        }));
        assert!(RetryPolicy::is_retryable(&MukeiError::NetworkServerError {
            status: 502,
            operation: "dl".into()
        }));
        assert!(!RetryPolicy::is_retryable(&MukeiError::NetworkError(
            "dns".into()
        )));
        assert!(!RetryPolicy::is_retryable(&MukeiError::Internal(
            "weird".into()
        )));
    }

    #[test]
    fn retry_policy_check_attempt_short_circuits_on_exhaustion() {
        let p = RetryPolicy {
            max_attempts: 2,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
        };
        let timeout_err = MukeiError::NetworkTimeout {
            operation: "dl".into(),
        };
        assert!(p.check_attempt(0, &timeout_err).is_ok());
        assert!(p.check_attempt(1, &timeout_err).is_ok());
        assert!(p.check_attempt(2, &timeout_err).is_err());
    }
}
