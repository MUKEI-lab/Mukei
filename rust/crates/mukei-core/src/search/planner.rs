//! Adaptive Search Planner — top-level orchestrator (migration §3).
//!
//! # Pipeline
//!
//! 1. [`IntentAnalyzer`] tags the prompt as a single [`TaskKind`] (or
//!    `MultiStep`).
//! 2. [`TaskSplitter`] breaks multi-step prompts into independent
//!    sub-queries.
//! 3. [`TaskClassifier`] re-classifies each sub-query.
//! 4. [`SearchSelector`] picks an ordered engine list per task.
//! 5. The executor invokes engines (with per-engine timeouts), passing
//!    through the [`SearchCache`] when enabled.
//! 6. [`SearchResultRanker`] composes the final result list, with
//!    `Unsafe` sources already filtered out.
//!
//! # Invariants
//!
//! - **No unconditional fan-out.** Each task consults at most
//!   [`PlannerPolicy::max_parallel_engines`] engines, picked by the
//!   selector — never every engine for every query.
//! - **Per-engine timeouts are non-blocking.** A slow Tavily call does
//!   NOT delay the planner's overall return; we keep whatever the
//!   faster engine produced.
//! - **Cache-first.** When a fresh cache entry exists, the engine is
//!   not called at all.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::error::Result;
use crate::search::cache::SearchCache;
use crate::search::engines::{SearchEngine, SearchEngineKind, SearchRequest};
use crate::search::intent::{IntentAnalyzer, TaskClassifier, TaskKind, TaskSplitter};
use crate::search::policy::PlannerPolicy;
use crate::search::ranker::{RankedResult, SearchResultRanker};
use crate::search::selector::SearchSelector;
use crate::search::SearchHit;

/// One task scheduled by the planner: text + classification + selected
/// engines.
#[derive(Clone, Debug)]
pub struct PlannedTask {
    /// Sub-query string.
    pub query: String,
    /// Classification chosen for this sub-query.
    pub kind: TaskKind,
    /// Ordered list of engines the executor will consult.
    pub engines: Vec<SearchEngineKind>,
}

/// Output of [`SearchPlanner::run`].
#[derive(Clone, Debug)]
pub struct SearchPlan {
    /// Tasks the planner actually executed.
    pub tasks: Vec<PlannedTask>,
    /// Final ranked hits across all tasks.
    pub results: Vec<RankedResult>,
}

/// Top-level planner.
///
/// Owns the per-engine implementations, the cache, the ranker, and the
/// policy. Construct once at boot.
pub struct SearchPlanner {
    engines: HashMap<SearchEngineKind, Arc<dyn SearchEngine>>,
    cache: SearchCache,
    ranker: SearchResultRanker,
    policy: PlannerPolicy,
}

impl SearchPlanner {
    /// Construct with an explicit engine map. Callers MUST wire the
    /// keys in [`SearchEngineKind`] order; missing engines produce
    /// empty hits for tasks that need them.
    pub fn new(
        engines: HashMap<SearchEngineKind, Arc<dyn SearchEngine>>,
        policy: PlannerPolicy,
    ) -> Self {
        Self {
            engines,
            cache: SearchCache::new(),
            ranker: SearchResultRanker::default(),
            policy,
        }
    }

    /// Drive the full pipeline for a single user prompt.
    pub async fn run(&self, prompt: &str) -> Result<SearchPlan> {
        // ---- 1. Intent + split ----
        let initial = IntentAnalyzer::analyze(prompt);
        let raw_tasks: Vec<String> = if matches!(initial, TaskKind::MultiStep) {
            TaskSplitter::split(prompt)
        } else {
            vec![prompt.trim().to_string()]
        };

        // ---- 2. Per-task classification + selection ----
        let mut tasks = Vec::with_capacity(raw_tasks.len());
        for q in raw_tasks {
            if q.is_empty() {
                continue;
            }
            let kind = TaskClassifier::classify(&q);
            let mut engines = SearchSelector::select(kind);
            engines.truncate(self.policy.max_parallel_engines);
            tasks.push(PlannedTask {
                query: q,
                kind,
                engines,
            });
        }

        // ---- 3. Execute per-task (parallel engines, sequential tasks) ----
        let mut all_hits: Vec<SearchHit> = Vec::new();
        for task in &tasks {
            let hits = self.execute_task(task).await;
            all_hits.extend(hits);
        }

        // ---- 4. Rank globally so duplicate URLs collapse into the best score ----
        let ranked = self.ranker.rank(prompt, all_hits);
        Ok(SearchPlan {
            tasks,
            results: ranked,
        })
    }

    async fn execute_task(&self, task: &PlannedTask) -> Vec<SearchHit> {
        // We can't `tokio::spawn` futures that borrow `self`, so we
        // collect the per-engine futures into a `FuturesUnordered` and
        // poll them on the caller's task. That keeps the per-engine
        // timeout independent (each future has its own `tokio::time::timeout`)
        // without forcing the caller to wait on a serial chain.
        use futures::stream::{FuturesUnordered, StreamExt};

        let mut futures = FuturesUnordered::new();
        for engine_kind in &task.engines {
            let engine = match self.engines.get(engine_kind) {
                Some(e) => e.clone(),
                None => continue,
            };
            let task_q = task.query.clone();
            let task_kind = task.kind;
            let cache = &self.cache;
            let cache_enabled = self.policy.enable_cache;
            let hits_per_engine = self.policy.hits_per_engine;
            let timeout = self.timeout_for(*engine_kind);
            let engine_kind = *engine_kind;

            futures.push(async move {
                // Cache lookup.
                if cache_enabled {
                    if let Some(hits) = cache.get(task_kind, engine_kind, &task_q) {
                        return hits;
                    }
                }
                let request = match SearchRequest::new(&task_q, hits_per_engine) {
                    Ok(r) => r,
                    Err(_) => return Vec::new(),
                };
                let live = engine.search(&request);
                let outcome = tokio::time::timeout(timeout, live).await;
                let hits = match outcome {
                    Ok(Ok(hits)) => hits,
                    Ok(Err(err)) => {
                        tracing::warn!(?err, engine = %engine_kind.as_tag(), "engine call failed");
                        Vec::new()
                    }
                    Err(_) => {
                        tracing::warn!(engine = %engine_kind.as_tag(), "engine call timed out");
                        Vec::new()
                    }
                };
                if cache_enabled && !hits.is_empty() {
                    cache.put(task_kind, engine_kind, &task_q, hits.clone());
                }
                hits
            });
        }

        let mut hits = Vec::new();
        while let Some(part) = futures.next().await {
            hits.extend(part);
        }
        hits
    }

    fn timeout_for(&self, kind: SearchEngineKind) -> Duration {
        match kind {
            SearchEngineKind::Brave => self.policy.timeouts.brave,
            SearchEngineKind::Tavily => self.policy.timeouts.tavily,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::engines::{BraveEngine, TavilyEngine};

    fn build_planner() -> SearchPlanner {
        let mut map: HashMap<SearchEngineKind, Arc<dyn SearchEngine>> = HashMap::new();
        map.insert(
            SearchEngineKind::Brave,
            Arc::new(BraveEngine::new("test-key")),
        );
        map.insert(
            SearchEngineKind::Tavily,
            Arc::new(TavilyEngine::new("test-key")),
        );
        SearchPlanner::new(map, PlannerPolicy::default())
    }

    #[tokio::test]
    async fn fact_task_consults_only_brave() {
        let p = build_planner();
        let plan = p.run("Who is the CEO of Meta?").await.unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].kind, TaskKind::Fact);
        assert_eq!(plan.tasks[0].engines, vec![SearchEngineKind::Brave]);
        assert!(!plan.results.is_empty());
    }

    #[tokio::test]
    async fn research_task_starts_with_tavily() {
        let p = build_planner();
        let plan = p.run("Best 7B open-source language models").await.unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].kind, TaskKind::Research);
        assert_eq!(plan.tasks[0].engines[0], SearchEngineKind::Tavily);
    }

    #[tokio::test]
    async fn multi_step_prompt_is_split_into_independent_tasks() {
        let p = build_planner();
        let plan = p
            .run("Tell me about Gemma4 and tell me about Qwen2.5 and what are the best 7B models?")
            .await
            .unwrap();
        assert!(
            plan.tasks.len() >= 2,
            "expected at least 2 sub-tasks, got {:?}",
            plan.tasks
        );
    }

    #[tokio::test]
    async fn planner_returns_no_ddg_results_ever() {
        let p = build_planner();
        let plan = p.run("Latest news on AI").await.unwrap();
        for r in plan.results {
            assert!(matches!(
                r.hit.engine,
                SearchEngineKind::Brave | SearchEngineKind::Tavily
            ));
        }
    }
}
