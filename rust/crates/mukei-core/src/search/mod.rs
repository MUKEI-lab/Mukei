//! `mukei_core::search` — Adaptive Search Planner (NEW, v0.7.5 migration).
//!
//! This module replaces the legacy "Brave + Tavily + DDG fan-out" with
//! an LLM-guided, intent-aware planner that chooses ONE (or, when a
//! prompt splits into independent tasks, a SMALL SET of) search
//! engines per task. DuckDuckGo is permanently removed from the
//! production path — see Section 2 of the migration document.
//!
//! # Architecture (per migration §3)
//!
//! ```text
//! User Query
//!     ↓
//! IntentAnalyzer       — single TaskKind for the whole prompt
//!     ↓
//! TaskSplitter         — multi-question prompts → independent tasks
//!     ↓
//! TaskClassifier       — per-task: FACT | RESEARCH | COMPARE | NEWS | …
//!     ↓
//! SearchSelector       — picks engine(s) per task + execution shape
//!     ↓
//! Executor (engines/)  — Brave / Tavily with strict per-engine timeouts
//!     ↓
//! SearchResultRanker   — relevance × freshness × authority × citation
//!     ↓
//! ResponseBuilder      — sentinel-wrapped, citation-enforced output
//! ```
//!
//! # Invariants
//!
//! - **No DDG.** Compile-time `compile_error!` guards prevent any
//!   reintroduction. See `engines/mod.rs`.
//! - **No unconditional fan-out.** The selector decides per task; the
//!   executor MUST refuse to call every engine for every query.
//! - **Per-engine timeout policy.** Brave = 3 s, Tavily = 5 s
//!   (migration §13). A timeout never blocks the parent planner — the
//!   executor continues with whatever results have arrived.
//! - **Citation enforced.** Every factual claim returned to the LLM
//!   carries a `Citation` derived from the source `Url`. Outputs
//!   without citations are rejected at the response-builder stage.
//! - **Trust gating.** Sources classified as
//!   [`trust::SourceTrust::Unsafe`] are dropped BEFORE ranking.

pub mod cache;
pub mod engines;
pub mod intent;
pub mod planner;
pub mod policy;
pub mod ranker;
pub mod selector;
pub mod trust;

pub use cache::{CacheKind, SearchCache};
pub use engines::{SearchEngine, SearchEngineKind, SearchRequest};
pub use intent::{IntentAnalyzer, TaskClassifier, TaskKind, TaskSplitter};
pub use planner::{PlannedTask, SearchPlan, SearchPlanner};
pub use policy::{PlannerPolicy, TimeoutBudget};
pub use ranker::{RankedResult, SearchResultRanker};
pub use selector::SearchSelector;
pub use trust::{SourceTrust, TrustClassifier};

use serde::{Deserialize, Serialize};

/// One search hit, normalised across engines.
///
/// `engine` records which backend produced this hit so the ranker can
/// down-weight noisier sources, and so the response builder can render
/// per-engine attribution.
/// Note: omits `Eq` because [`SearchHit::engine_score`] carries an `f32`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchHit {
    /// Hit title.
    pub title: String,
    /// Canonical URL.
    pub url: String,
    /// Engine-supplied snippet / description.
    pub snippet: String,
    /// Originating engine.
    pub engine: SearchEngineKind,
    /// Recency hint when the engine provides one (ISO-8601). Drives
    /// `freshness_score` in the ranker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    /// Engine-native relevance score, if the API returned one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_score: Option<f32>,
}

impl SearchHit {
    /// Convenience constructor for tests and adapters.
    pub fn new(
        title: impl Into<String>,
        url: impl Into<String>,
        snippet: impl Into<String>,
        engine: SearchEngineKind,
    ) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            snippet: snippet.into(),
            engine,
            published: None,
            engine_score: None,
        }
    }
}

/// Citation handed back to the agent loop alongside any factual claim.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct Citation {
    /// Source URL.
    pub url: String,
    /// Display title.
    pub title: String,
    /// Trust level assigned by [`trust::TrustClassifier`].
    pub trust: SourceTrust,
}
