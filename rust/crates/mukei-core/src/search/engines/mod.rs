//! Pluggable search backends.
//!
//! # Invariants
//!
//! - **DuckDuckGo is permanently removed.** Any future PR that
//!   reintroduces a `ddg.rs` file or a `Ddg` variant on
//!   [`SearchEngineKind`] MUST be rejected. The `compile_error!`
//!   below catches the most direct regression at build time.
//! - **Closed engine set.** Only [`SearchEngineKind::Brave`] and
//!   [`SearchEngineKind::Tavily`] exist; adding a third engine requires
//!   touching the planner's selector matrix at the same time.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{MukeiError, Result};
use crate::search::SearchHit;

pub mod brave;
pub mod tavily;

pub use brave::BraveEngine;
pub use tavily::TavilyEngine;

// ---------------------------------------------------------------------
// Anti-regression tripwire — see invariant above.
// ---------------------------------------------------------------------
#[cfg(feature = "ddg")]
compile_error!("DuckDuckGo is permanently removed from the Mukei search architecture (v0.7.5 migration §2). Re-enabling it requires a new TRD amendment.");

/// Closed set of engine kinds. Stable JSON tag — persisted in cache
/// keys and per-hit attribution.
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngineKind {
    /// Brave Search API. Strong on factual / current-events queries.
    Brave,
    /// Tavily Search API. Strong on research / multi-source synthesis.
    Tavily,
}

impl SearchEngineKind {
    /// Stable identifier used in cache keys and `tracing` spans.
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Brave => "brave",
            Self::Tavily => "tavily",
        }
    }
}

/// One request issued to a backend. The planner constructs these; the
/// engine implementation must respect every field, especially `count`
/// and `max_age_days`, so the cost-aware policy holds.
#[derive(Clone, Debug)]
pub struct SearchRequest {
    /// Query string. Trimmed; non-empty by construction.
    pub query: String,
    /// Max number of hits to return.
    pub count: usize,
    /// If `Some`, ignore results older than this many days. Used by
    /// the `News` task class.
    pub max_age_days: Option<u32>,
}

impl SearchRequest {
    /// Build a request after trimming + non-empty check.
    pub fn new(query: impl Into<String>, count: usize) -> Result<Self> {
        let q = query.into();
        let trimmed = q.trim();
        if trimmed.is_empty() {
            return Err(MukeiError::ToolArgumentInvalid {
                field: "query",
                reason: "empty query".to_string(),
            });
        }
        Ok(Self {
            query: trimmed.to_string(),
            count: count.max(1),
            max_age_days: None,
        })
    }

    /// Fluent setter for `max_age_days`.
    pub fn with_max_age_days(mut self, days: u32) -> Self {
        self.max_age_days = Some(days);
        self
    }
}

/// Object-safe backend trait.
#[async_trait]
pub trait SearchEngine: Send + Sync {
    /// Which engine this is.
    fn kind(&self) -> SearchEngineKind;
    /// Execute a single search and return normalised hits.
    async fn search(&self, request: &SearchRequest) -> Result<Vec<SearchHit>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_kind_tags_are_stable_lowercase() {
        for k in [SearchEngineKind::Brave, SearchEngineKind::Tavily] {
            let t = k.as_tag();
            assert!(t.chars().all(|c| c.is_ascii_lowercase()));
        }
    }

    #[test]
    fn search_request_rejects_empty_query() {
        let err = SearchRequest::new("   ", 5).unwrap_err();
        assert!(matches!(err, MukeiError::ToolArgumentInvalid { .. }));
    }

    #[test]
    fn search_request_normalises_trim_and_count() {
        let req = SearchRequest::new("  hello  ", 0).unwrap();
        assert_eq!(req.query, "hello");
        assert_eq!(req.count, 1); // 0 → 1 minimum
    }
}
