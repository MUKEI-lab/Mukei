//! Tavily Search API backend (production search engine #2).
//!
//! # Invariants
//!
//! - Per-call timeout: [`crate::search::policy::PlannerPolicy::DEFAULT_TAVILY_TIMEOUT_SECS`] (5 s).
//! - API key comes from `TAVILY_API_KEY` or the bridge wrapped-secrets
//!   registry. Never from a plaintext `config.toml` field.
//! - The Tavily envelope returns a top-level `answer` field; we surface
//!   it as the first hit so the response builder can use it as a
//!   high-confidence summary.

use async_trait::async_trait;

#[cfg(feature = "network")]
use crate::error::MukeiError;
use crate::error::Result;
use crate::search::engines::{SearchEngine, SearchEngineKind, SearchRequest};
use crate::search::SearchHit;

/// Tavily Search engine.
#[cfg_attr(not(feature = "network"), allow(dead_code))]
pub struct TavilyEngine {
    api_key: String,
    base_url: String,
}

impl TavilyEngine {
    /// Standard production constructor.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.tavily.com/search".to_string(),
        }
    }

    /// Override the base URL for integration tests.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

#[async_trait]
impl SearchEngine for TavilyEngine {
    fn kind(&self) -> SearchEngineKind {
        SearchEngineKind::Tavily
    }

    async fn search(&self, request: &SearchRequest) -> Result<Vec<SearchHit>> {
        execute_tavily(self, request).await
    }
}

#[cfg(feature = "network")]
async fn execute_tavily(engine: &TavilyEngine, request: &SearchRequest) -> Result<Vec<SearchHit>> {
    use reqwest::Client;
    use serde::Deserialize;
    use serde_json::json;

    #[cfg(test)]
    if engine.base_url == "https://api.tavily.com/search" {
        return Ok(vec![SearchHit {
            title: format!("[stub:tavily] {}", request.query),
            url: "https://example.invalid/stub/tavily".to_string(),
            snippet: "Tavily Search stub result (network-enabled test build).".to_string(),
            engine: SearchEngineKind::Tavily,
            published: None,
            engine_score: Some(0.9),
        }]);
    }

    #[derive(Debug, Deserialize)]
    struct TavilyEnvelope {
        answer: Option<String>,
        results: Vec<TavilyResult>,
    }
    #[derive(Debug, Deserialize)]
    struct TavilyResult {
        title: Option<String>,
        url: Option<String>,
        content: Option<String>,
        score: Option<f32>,
        published_date: Option<String>,
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(
            crate::search::policy::PlannerPolicy::DEFAULT_TAVILY_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|e| MukeiError::HttpClientFailed(e.to_string()))?;

    let mut body = json!({
        "api_key": engine.api_key,
        "query": request.query,
        "max_results": request.count,
        "include_answer": true,
    });
    if let Some(days) = request.max_age_days {
        // Tavily supports `days` for date-bounded queries.
        body["days"] = json!(days);
    }

    let payload: TavilyEnvelope = client
        .post(&engine.base_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| MukeiError::WebSearchFailed(format!("tavily http: {e}")))?
        .json()
        .await
        .map_err(|e| MukeiError::WebSearchFailed(format!("tavily parse: {e}")))?;

    let mut out = Vec::new();
    // Surface the answer field as a synthetic high-confidence hit so
    // the ranker can give it priority.
    if let Some(answer) = payload.answer {
        if !answer.is_empty() {
            out.push(SearchHit {
                title: format!("Tavily answer: {}", request.query),
                url: "tavily://answer".to_string(),
                snippet: answer,
                engine: SearchEngineKind::Tavily,
                published: None,
                engine_score: Some(1.0),
            });
        }
    }
    for item in payload.results.into_iter().take(request.count) {
        let title = item.title.unwrap_or_default();
        let url = item.url.unwrap_or_default();
        let snippet = item.content.unwrap_or_default();
        if title.is_empty() && url.is_empty() {
            continue;
        }
        out.push(SearchHit {
            title,
            url,
            snippet,
            engine: SearchEngineKind::Tavily,
            published: item.published_date,
            engine_score: item.score,
        });
    }
    Ok(out)
}

#[cfg(not(feature = "network"))]
async fn execute_tavily(_engine: &TavilyEngine, request: &SearchRequest) -> Result<Vec<SearchHit>> {
    Ok(vec![SearchHit {
        title: format!("[stub:tavily] {}", request.query),
        url: "https://example.invalid/stub/tavily".to_string(),
        snippet: "Tavily Search stub result (network feature disabled).".to_string(),
        engine: SearchEngineKind::Tavily,
        published: None,
        engine_score: Some(0.0),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tavily_stub_returns_a_hit_in_sandbox() {
        let engine = TavilyEngine::new("test-key");
        let req = SearchRequest::new("hello", 3).unwrap();
        let hits = engine.search(&req).await.unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].engine, SearchEngineKind::Tavily);
    }

    #[cfg(feature = "network")]
    mod network_payload_tests {
        use super::*;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        fn request() -> SearchRequest {
            SearchRequest::new("android lifecycle", 3).unwrap()
        }

        #[tokio::test]
        async fn tavily_empty_payload_returns_no_hits() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/search"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "answer": null,
                    "results": []
                })))
                .mount(&server)
                .await;

            let engine =
                TavilyEngine::new("test-key").with_base_url(format!("{}/search", server.uri()));
            let hits = engine.search(&request()).await.unwrap();

            assert!(hits.is_empty());
        }

        #[tokio::test]
        async fn tavily_http_429_is_reported_as_search_failure() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/search"))
                .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
                .mount(&server)
                .await;

            let engine =
                TavilyEngine::new("test-key").with_base_url(format!("{}/search", server.uri()));
            let err = engine.search(&request()).await.unwrap_err();

            assert!(err.to_string().contains("tavily"));
        }

        #[tokio::test]
        async fn tavily_schema_drift_missing_results_is_error() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/search"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "answer": "summary without results field"
                })))
                .mount(&server)
                .await;

            let engine =
                TavilyEngine::new("test-key").with_base_url(format!("{}/search", server.uri()));
            let err = engine.search(&request()).await.unwrap_err();

            assert!(err.to_string().contains("tavily parse"));
        }
    }
}
