//! SearchResultRanker — migration §8.
//!
//! Composes five sub-scores into a single weighted ranking value:
//!
//! ```text
//! final_score = w_rel * relevance + w_fresh * freshness
//!             + w_auth * authority + w_cit * citation
//!             + w_qual * quality
//! ```
//!
//! # Invariants
//!
//! - All sub-scores are in `[0.0, 1.0]`. The composer clamps before
//!   weighting to avoid blowing up under buggy engines.
//! - The ranker is **pure** — no I/O. Real production builds may layer
//!   an LLM-driven re-ranker on top via the bridge crate.

use crate::search::trust::{SourceTrust, TrustClassifier};
use crate::search::SearchHit;

/// Sub-scores per hit. Kept separate so the response builder can render
/// "this source has high freshness but low authority" diagnostics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoreBreakdown {
    pub relevance: f32,
    pub freshness: f32,
    pub authority: f32,
    pub citation: f32,
    pub quality: f32,
    pub final_score: f32,
}

/// Ranked hit returned by [`SearchResultRanker::rank`].
#[derive(Clone, Debug)]
pub struct RankedResult {
    pub hit: SearchHit,
    pub trust: SourceTrust,
    pub scores: ScoreBreakdown,
}

/// Pure ranking engine.
pub struct SearchResultRanker {
    pub w_relevance: f32,
    pub w_freshness: f32,
    pub w_authority: f32,
    pub w_citation: f32,
    pub w_quality: f32,
    pub classifier: TrustClassifier,
}

impl Default for SearchResultRanker {
    fn default() -> Self {
        Self {
            // Weights sum to 1.0 for interpretability.
            w_relevance: 0.40,
            w_freshness: 0.15,
            w_authority: 0.25,
            w_citation: 0.10,
            w_quality: 0.10,
            classifier: TrustClassifier::new(),
        }
    }
}

impl SearchResultRanker {
    /// Rank a flat list of hits. Hits classified as
    /// [`SourceTrust::Unsafe`] are dropped BEFORE scoring (migration §9).
    pub fn rank(&self, query: &str, hits: Vec<SearchHit>) -> Vec<RankedResult> {
        let mut scored: Vec<RankedResult> = hits
            .into_iter()
            .filter_map(|hit| {
                let trust = self.classifier.classify(&hit.url);
                if matches!(trust, SourceTrust::Unsafe) {
                    return None;
                }
                let scores = self.score_one(query, &hit, trust);
                Some(RankedResult { hit, trust, scores })
            })
            .collect();
        scored.sort_by(|a, b| {
            b.scores
                .final_score
                .partial_cmp(&a.scores.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored
    }

    fn score_one(&self, query: &str, hit: &SearchHit, trust: SourceTrust) -> ScoreBreakdown {
        let relevance = clamp01(self.score_relevance(query, hit));
        let freshness = clamp01(self.score_freshness(hit));
        let authority = clamp01(self.score_authority(trust));
        let citation = clamp01(self.score_citation(hit));
        let quality = clamp01(self.score_quality(hit));
        let final_score = relevance * self.w_relevance
            + freshness * self.w_freshness
            + authority * self.w_authority
            + citation * self.w_citation
            + quality * self.w_quality;
        ScoreBreakdown {
            relevance,
            freshness,
            authority,
            citation,
            quality,
            final_score,
        }
    }

    fn score_relevance(&self, query: &str, hit: &SearchHit) -> f32 {
        if let Some(score) = hit.engine_score {
            return score;
        }
        // Token overlap on title + snippet against the query.
        let q = lowercase_tokens(query);
        if q.is_empty() {
            return 0.0;
        }
        let body = format!("{} {}", hit.title, hit.snippet).to_lowercase();
        let body_tokens: Vec<&str> = body.split_whitespace().collect();
        if body_tokens.is_empty() {
            return 0.0;
        }
        let overlap = q.iter().filter(|t| body_tokens.contains(&t.as_str())).count() as f32;
        overlap / q.len() as f32
    }

    fn score_freshness(&self, hit: &SearchHit) -> f32 {
        // If the engine returned a `published` field we treat anything
        // < 30 days old as 1.0, < 365 days as 0.6, older as 0.2.
        // Without a `published` field we return 0.5 (neutral).
        match hit.published.as_deref() {
            None => 0.5,
            Some(s) => Self::freshness_from_str(s),
        }
    }

    fn freshness_from_str(s: &str) -> f32 {
        // Try to parse ISO-8601; fall back to keyword heuristics
        // ("yesterday", "2 weeks ago").
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            let now = chrono::Utc::now();
            let age = now.signed_duration_since(dt.with_timezone(&chrono::Utc));
            let days = age.num_days();
            return if days < 0 {
                1.0
            } else if days <= 30 {
                1.0
            } else if days <= 365 {
                0.6
            } else {
                0.2
            };
        }
        let lower = s.to_lowercase();
        if lower.contains("hour") || lower.contains("today") || lower.contains("yesterday") {
            1.0
        } else if lower.contains("day") || lower.contains("week") {
            0.8
        } else if lower.contains("month") {
            0.5
        } else if lower.contains("year") {
            0.2
        } else {
            0.5
        }
    }

    fn score_authority(&self, trust: SourceTrust) -> f32 {
        match trust {
            SourceTrust::Trusted => 1.0,
            SourceTrust::SemiTrusted => 0.7,
            SourceTrust::Untrusted => 0.3,
            // Unsafe is filtered out before this is called.
            SourceTrust::Unsafe => 0.0,
        }
    }

    fn score_citation(&self, hit: &SearchHit) -> f32 {
        // Proxy: HTTPS URL with a recognisable TLD = citation-friendly.
        if hit.url.starts_with("https://") {
            1.0
        } else if hit.url.starts_with("http://") {
            0.5
        } else {
            0.0
        }
    }

    fn score_quality(&self, hit: &SearchHit) -> f32 {
        // Proxy: snippet length normalised into [0, 1] with 240 chars
        // as the saturation point.
        let len = hit.snippet.len() as f32;
        (len / 240.0).min(1.0)
    }
}

fn lowercase_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::engines::SearchEngineKind;

    fn hit(title: &str, url: &str, snippet: &str) -> SearchHit {
        SearchHit::new(title, url, snippet, SearchEngineKind::Brave)
    }

    #[test]
    fn unsafe_hits_are_filtered() {
        let ranker = SearchResultRanker::default();
        let hits = vec![
            hit("Bad", "https://promptinject.example/x", "spam"),
            hit("Good", "https://reuters.com/x", "news"),
        ];
        let ranked = ranker.rank("query", hits);
        assert_eq!(ranked.len(), 1);
        assert!(ranked[0].hit.url.contains("reuters.com"));
    }

    #[test]
    fn trusted_outranks_untrusted_on_same_relevance() {
        let ranker = SearchResultRanker::default();
        let q = "machine learning";
        let hits = vec![
            hit("ML overview", "https://random-blog.example/ml", "machine learning is..."),
            hit("ML overview", "https://reuters.com/ml", "machine learning is..."),
        ];
        let ranked = ranker.rank(q, hits);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].trust, SourceTrust::Trusted);
    }

    #[test]
    fn scores_are_clamped_to_unit_interval() {
        let ranker = SearchResultRanker::default();
        let h = hit("t", "https://example.com/p", "abc");
        let breakdown = ranker.score_one("t", &h, SourceTrust::Trusted);
        for s in [
            breakdown.relevance,
            breakdown.freshness,
            breakdown.authority,
            breakdown.citation,
            breakdown.quality,
            breakdown.final_score,
        ] {
            assert!((0.0..=1.0).contains(&s), "score out of range: {s}");
        }
    }

    #[test]
    fn freshness_string_keyword_heuristic() {
        assert!(SearchResultRanker::freshness_from_str("2 hours ago") > 0.9);
        assert!(SearchResultRanker::freshness_from_str("3 days ago") > 0.5);
        assert!(SearchResultRanker::freshness_from_str("2 years ago") < 0.3);
    }
}
