//! Intent / Task analysis — migration §4 + §5.
//!
//! # Invariants
//!
//! - The classifier is **deterministic** for a given query string. The
//!   LLM-guided variant lives in the bridge crate and reuses this
//!   module's enums.
//! - `TaskSplitter::split` returns at least one task — never an empty
//!   vector — even on degenerate input.

use serde::{Deserialize, Serialize};

/// Closed enum of task classes (migration §5).
#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// Single factual question with a known short answer.
    Fact,
    /// Open-ended research / explainer / multi-source synthesis.
    Research,
    /// Side-by-side comparison of N entities.
    Compare,
    /// Time-sensitive news / current events.
    News,
    /// Academic / scientific literature lookup.
    Academic,
    /// Shopping / product / price comparison.
    Shopping,
    /// Local listings / places.
    Local,
    /// Multi-part request that must be split before further routing.
    MultiStep,
}

impl TaskKind {
    /// Stable tag for cache keys + FFI snapshots.
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Research => "research",
            Self::Compare => "compare",
            Self::News => "news",
            Self::Academic => "academic",
            Self::Shopping => "shopping",
            Self::Local => "local",
            Self::MultiStep => "multi_step",
        }
    }
}

/// First-pass intent analyser — decides whether the prompt is a single
/// task or needs splitting.
pub struct IntentAnalyzer;

impl IntentAnalyzer {
    /// Classify the entire prompt as a single [`TaskKind`]. The bridge
    /// crate may override this with an LLM-driven variant; the
    /// deterministic heuristic here is a safety net.
    pub fn analyze(query: &str) -> TaskKind {
        let lower = query.to_lowercase();
        if Self::looks_like_multi_step(&lower) {
            return TaskKind::MultiStep;
        }
        TaskClassifier::classify(query)
    }

    fn looks_like_multi_step(lower: &str) -> bool {
        // Heuristic: long prompts with 2+ separators that look like
        // independent questions almost always need a split. Conservative
        // — false positives just trigger an extra split pass that
        // collapses on a single-task result.
        let separators = [
            " and ", " aur ",  // Hinglish
            " also ", // common in chat
            " plus ", ";", "?",
        ];
        let mut hits = 0;
        for sep in separators {
            hits += lower.matches(sep).count();
        }
        hits >= 2 && lower.len() > 40
    }
}

/// Splits a multi-part prompt into independent sub-queries
/// (migration §4).
pub struct TaskSplitter;

impl TaskSplitter {
    /// Conservative deterministic splitter. Real production builds layer
    /// an LLM-driven splitter on top through the bridge crate; this
    /// fallback ensures the planner is functional in sandbox / tests.
    ///
    /// Splits on `?`, `;`, ` and `, ` aur `, ` plus `, ` also ` —
    /// always returns at least one sub-task (the trimmed original).
    pub fn split(query: &str) -> Vec<String> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return vec![trimmed.to_string()];
        }
        // Apply splits in order. We rebuild on the lowercase to find
        // separator offsets, then slice the original to preserve case.
        let mut parts: Vec<String> = vec![trimmed.to_string()];
        for sep in [";", "?", " and ", " aur ", " plus ", " also "] {
            parts = parts
                .into_iter()
                .flat_map(|p| {
                    let lower = p.to_lowercase();
                    if lower.contains(sep) {
                        let mut sub = Vec::new();
                        let mut last = 0usize;
                        let mut search_from = 0usize;
                        while let Some(pos) = lower[search_from..].find(sep) {
                            let abs = search_from + pos;
                            let piece = p[last..abs].trim();
                            if !piece.is_empty() {
                                sub.push(piece.to_string());
                            }
                            last = abs + sep.len();
                            search_from = last;
                        }
                        let tail = p[last..].trim();
                        if !tail.is_empty() {
                            sub.push(tail.to_string());
                        }
                        if sub.is_empty() {
                            sub.push(p);
                        }
                        sub
                    } else {
                        vec![p]
                    }
                })
                .collect();
        }
        // Drop very short fragments that are clearly not standalone
        // queries (e.g. "ok", "yes"). When the splitter produced exactly
        // one part we keep it regardless of length so a single-word
        // query still returns a task.
        if parts.len() > 1 {
            parts.retain(|p| p.split_whitespace().count() >= 2);
        }
        if parts.is_empty() {
            parts.push(trimmed.to_string());
        }
        parts
    }
}

/// Per-task classifier (migration §5).
pub struct TaskClassifier;

impl TaskClassifier {
    /// Deterministic per-task heuristic.
    pub fn classify(query: &str) -> TaskKind {
        let lower = query.to_lowercase();

        if Self::matches_any(
            &lower,
            &["latest", "today", "news", "breaking", "this week", "recent"],
        ) {
            return TaskKind::News;
        }

        if Self::matches_any(
            &lower,
            &[
                "compare",
                "vs",
                "versus",
                "difference between",
                "side by side",
                " or ",
            ],
        ) {
            return TaskKind::Compare;
        }

        if Self::matches_any(
            &lower,
            &[
                "best ",
                "explain",
                "how does",
                "architecture",
                "overview",
                "deep dive",
                "guide to",
                "what are",
                "tutorial",
                "models",
            ],
        ) {
            return TaskKind::Research;
        }

        if Self::matches_any(&lower, &["paper", "doi", "arxiv", "preprint", "citation"]) {
            return TaskKind::Academic;
        }

        if Self::matches_any(
            &lower,
            &["price of", "buy ", "cheapest", "deal on", "review of"],
        ) {
            return TaskKind::Shopping;
        }

        if Self::matches_any(&lower, &["near me", "nearest", "directions", "open now"]) {
            return TaskKind::Local;
        }

        // Single fact question — short prompts that ask "who/what/when/where".
        if lower.split_whitespace().count() <= 8
            && (lower.starts_with("who ")
                || lower.starts_with("what ")
                || lower.starts_with("when ")
                || lower.starts_with("where ")
                || lower.starts_with("which "))
        {
            return TaskKind::Fact;
        }

        TaskKind::Research
    }

    fn matches_any(haystack: &str, needles: &[&str]) -> bool {
        needles.iter().any(|n| haystack.contains(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifier_routes_factual_questions_to_fact() {
        assert_eq!(
            TaskClassifier::classify("Who is the CEO of Meta?"),
            TaskKind::Fact
        );
        assert_eq!(
            TaskClassifier::classify("Capital of India"),
            TaskKind::Research
        );
    }

    #[test]
    fn classifier_routes_research_to_research() {
        assert_eq!(
            TaskClassifier::classify("Explain the Gemma4 architecture"),
            TaskKind::Research
        );
        assert_eq!(
            TaskClassifier::classify("Best 7B open-source models"),
            TaskKind::Research
        );
    }

    #[test]
    fn classifier_routes_comparison_to_compare() {
        assert_eq!(
            TaskClassifier::classify("Qwen 2.5 vs Llama 3.1"),
            TaskKind::Compare
        );
        assert_eq!(
            TaskClassifier::classify("Difference between Mistral and Mixtral"),
            TaskKind::Compare
        );
    }

    #[test]
    fn classifier_routes_news_to_news() {
        assert_eq!(
            TaskClassifier::classify("Latest news on Apple AI"),
            TaskKind::News
        );
    }

    #[test]
    fn intent_analyzer_detects_multi_step_when_two_separators() {
        let q = "Gemma4 models ke baare mein batao aur Qwen2.5-7B-Instruct ke baare mein bhi aur best 7B models bhi";
        assert_eq!(IntentAnalyzer::analyze(q), TaskKind::MultiStep);
    }

    #[test]
    fn splitter_separates_independent_tasks() {
        let q = "Tell me about Gemma4 and tell me about Qwen2.5 and best 7B models";
        let parts = TaskSplitter::split(q);
        assert!(parts.len() >= 3, "got {:?}", parts);
    }

    #[test]
    fn splitter_returns_single_task_when_no_separators() {
        let q = "Explain attention is all you need";
        let parts = TaskSplitter::split(q);
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn splitter_never_returns_empty() {
        let parts = TaskSplitter::split("");
        assert_eq!(parts.len(), 1);
    }
}
