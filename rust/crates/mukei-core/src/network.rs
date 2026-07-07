//! Network client construction policy.
//!
//! Mobile network calls should not create ad-hoc `reqwest::Client`
//! instances with implicit timeout and redirect behavior. This module
//! centralizes the baseline HTTP policy while still letting callers keep
//! domain-specific total time budgets.

use std::time::Duration;

use crate::error::{MukeiError, Result};

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
}
