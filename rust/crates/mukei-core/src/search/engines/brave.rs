//! Brave Search API backend (production search engine #1).
//!
//! # Invariants
//!
//! - Per-call timeout: [`crate::search::policy::PlannerPolicy::DEFAULT_BRAVE_TIMEOUT_SECS`] (3 s).
//! - API key comes from `BRAVE_API_KEY` or the bridge crate's
//!   wrapped-secrets registry — NEVER from a plaintext field in
//!   `config.toml`.
//! - All requests must include `X-Subscription-Token`.

use async_trait::async_trait;

#[cfg(feature = "network")]
use crate::error::MukeiError;
use crate::error::Result;
use crate::search::engines::{SearchEngine, SearchEngineKind, SearchRequest};
use crate::search::SearchHit;

/// Brave Search engine. Construct with [`Self::new`] and pass the API
/// key as an opaque string (the executor never logs it).
#[cfg_attr(not(feature = "network"), allow(dead_code))]
pub struct BraveEngine {
    api_key: String,
    base_url: String,
}

impl BraveEngine {
    /// Standard production constructor.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.search.brave.com/res/v1/web/search".to_string(),
        }
    }

    /// Override the base URL — used by integration tests against a
    /// local mock server.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

#[async_trait]
impl SearchEngine for BraveEngine {
    fn kind(&self) -> SearchEngineKind {
        SearchEngineKind::Brave
    }

    async fn search(&self, request: &SearchRequest) -> Result<Vec<SearchHit>> {
        execute_brave(self, request).await
    }
}

#[cfg(feature = "network")]
async fn execute_brave(engine: &BraveEngine, request: &SearchRequest) -> Result<Vec<SearchHit>> {
    use reqwest::Client;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct BraveEnvelope {
        web: Option<BraveWeb>,
    }
    #[derive(Debug, Deserialize)]
    struct BraveWeb {
        results: Vec<BraveResult>,
    }
    #[derive(Debug, Deserialize)]
    struct BraveResult {
        title: Option<String>,
        url: Option<String>,
        description: Option<String>,
        age: Option<String>,
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(
            crate::search::policy::PlannerPolicy::DEFAULT_BRAVE_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|e| MukeiError::HttpClientFailed(e.to_string()))?;

    let mut params = vec![("q", request.query.as_str()), ("count", "5")];
    let count_str = request.count.to_string();
    params[1] = ("count", count_str.as_str());
    let max_age_str;
    if let Some(days) = request.max_age_days {
        max_age_str = format!("pd{}d", days);
        params.push(("freshness", max_age_str.as_str()));
    }

    let payload: BraveEnvelope = client
        .get(&engine.base_url)
        .query(&params)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", &engine.api_key)
        .send()
        .await
        .map_err(|e| MukeiError::WebSearchFailed(format!("brave http: {e}")))?
        .json()
        .await
        .map_err(|e| MukeiError::WebSearchFailed(format!("brave parse: {e}")))?;

    let mut out = Vec::new();
    if let Some(web) = payload.web {
        for item in web.results.into_iter().take(request.count) {
            let title = item.title.unwrap_or_default();
            let url = item.url.unwrap_or_default();
            let snippet = item.description.unwrap_or_default();
            if title.is_empty() && url.is_empty() {
                continue;
            }
            out.push(SearchHit {
                title,
                url,
                snippet,
                engine: SearchEngineKind::Brave,
                published: item.age,
                engine_score: None,
            });
        }
    }
    Ok(out)
}

#[cfg(not(feature = "network"))]
async fn execute_brave(_engine: &BraveEngine, request: &SearchRequest) -> Result<Vec<SearchHit>> {
    // Without the `network` feature the engine returns an explicit
    // stub hit so end-to-end planner tests can run in the sandbox.
    Ok(vec![SearchHit {
        title: format!("[stub:brave] {}", request.query),
        url: "https://example.invalid/stub/brave".to_string(),
        snippet: "Brave Search stub result (network feature disabled).".to_string(),
        engine: SearchEngineKind::Brave,
        published: None,
        engine_score: Some(0.0),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn brave_stub_returns_a_hit_in_sandbox() {
        let engine = BraveEngine::new("test-key");
        let req = SearchRequest::new("hello", 3).unwrap();
        let hits = engine.search(&req).await.unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].engine, SearchEngineKind::Brave);
    }
}
